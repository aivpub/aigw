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

        // CARGO_BIN_EXE_aigw is set by cargo when running integration tests
        // that depend on the aigw binary. It points to the pre-built binary.
        let aigw_bin = std::env::var("CARGO_BIN_EXE_aigw").unwrap_or_else(|_| {
            eprintln!("WARN: CARGO_BIN_EXE_aigw not set, falling back to 'cargo run' (may fail due to lock)");
            "cargo".to_string()
        });

        let (mut cmd, _is_cargo_run): (tokio::process::Command, bool) = if aigw_bin == "cargo" {
            // Fallback: cargo run --bin aigw (will fail if workspace lock held)
            let workspace_root = find_workspace_root();
            let mut c = tokio::process::Command::new("cargo");
            c.args(["run", "--bin", "aigw", "--"])
                .current_dir(&workspace_root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            (c, true)
        } else {
            let mut c = tokio::process::Command::new(&aigw_bin);
            c.stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            (c, false)
        };

        // Forward provider/upstream env vars to the child process
        // so real API scenarios (end_to_end_real, compatibility_real) work.
        for (key, val) in std::env::vars() {
            let upper = key.to_uppercase();
            if upper.starts_with("OPENAI_")
                || upper.starts_with("OPENAPI_")
                || upper.starts_with("AIGW_UPSTREAM_")
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
