//! Server guard — manage aigw-server subprocess for BDD tests.
//!
//! Controlled by env var `AIGW_TEST_START_SERVER=1`.

use std::process::Stdio;
use std::time::Duration;

/// Manages an aigw-server child process.
/// Drops kill the process even on panic (prevent leaked processes).
pub struct ServerGuard {
    child: Option<tokio::process::Child>,
    pub base_url: String,
}

impl ServerGuard {
    /// Start aigw-server with the given database and master key.
    /// Picks a random port (127.0.0.1:0) and waits for /health/liveliness.
    pub async fn start(database_url: &str, master_key: &str) -> Result<Self, String> {
        // Find a free port by binding to 127.0.0.1:0
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("bind random port: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("get local addr: {e}"))?
            .port();
        drop(listener);

        let base_url = format!("http://127.0.0.1:{port}");

        // CARGO_BIN_EXE_aigw is set by cargo for sandboxed test binaries.
        // When not set (e.g. plain `cargo test --test bdd`), target/ is still there
        // from the build step, so use the binary directly instead of `cargo run`
        // which would trigger another full rebuild.
        let aigw_bin = std::env::var("CARGO_BIN_EXE_aigw").unwrap_or_else(|_| {
            let workspace_root = find_workspace_root();
            let target_dir = std::env::var("CARGO_TARGET_DIR")
                .unwrap_or_else(|_| format!("{workspace_root}/target"));
            let profile = if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            };
            format!("{target_dir}/{profile}/aigw")
        });

        let mut cmd = tokio::process::Command::new(&aigw_bin);
        cmd.stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        // Forward provider/upstream env vars to the child process
        // so real API scenarios (end_to_end_real, compatibility_real) work.
        for (key, val) in std::env::vars() {
            let upper = key.to_uppercase();
            if upper.starts_with("OPENAI_")
                || upper.starts_with("OPENAPI_")
                || upper.starts_with("AIGW_UPSTREAM_")
                || upper.starts_with("ANTHROPIC_")
            {
                cmd.env(&key, &val);
            }
        }

        // Common args for both paths.
        // When using cargo run, the "--" separator is already added above.
        // Collapse sqlite://// → sqlite:// to prevent driver-level path divergence.
        let database_url = database_url.replace("sqlite:////", "sqlite://");
        cmd.args([
            "--database-url",
            &database_url,
            "--master-key",
            master_key,
            "--bind",
            &format!("127.0.0.1:{port}"),
        ]);

        // Set AIGW_MASTER_KEY so the server can decrypt credentials/proxy_models
        // that were re-encrypted with this key during migration sync.
        // (--master-key is for admin auth; AIGW_MASTER_KEY is for field-decryption.)
        cmd.env("AIGW_MASTER_KEY", master_key);

        eprintln!("==> Starting aigw server on {base_url} (db={database_url})");
        let child = cmd.spawn().map_err(|e| format!("spawn aigw-server: {e}"))?;

        let mut guard = Self {
            child: Some(child),
            base_url: base_url.clone(),
        };

        // Wait for the server to become ready (poll /health/liveliness).
        let health_url = format!("{}/health/liveliness", guard.base_url);
        let client = reqwest::Client::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);

        loop {
            if tokio::time::Instant::now() > deadline {
                guard.kill().await;
                return Err(format!(
                    "server did not become healthy within 30s at {health_url}"
                ));
            }

            match client.get(&health_url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let body = resp.text().await.unwrap_or_default();
                    if body.contains("\"alive\":true") || body.contains("\"status\":\"ok\"") {
                        eprintln!("==> aigw server ready on {base_url}");
                        break;
                    }
                }
                _ => {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
            }
        }

        Ok(guard)
    }

    /// Kill the server subprocess.
    pub async fn stop(mut self) {
        self.kill().await;
    }

    async fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        if self.child.is_some() {
            // Synchronous kill on drop (panic safety).
            if let Some(mut child) = self.child.take() {
                let _ = child.start_kill();
                // Can't await in drop, but start_kill is fire-and-forget.
                // The process will be reaped by the OS.
                std::mem::drop(child);
            }
        }
    }
}

fn find_workspace_root() -> String {
    let server_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    server_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.as_os_str().to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string())
}
