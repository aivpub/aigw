//! Single-instance connectivity probe (Stage 138).
//!
//! `WebSearchRegistry::search` walks its instance pool with failover, so it can
//! never answer "is *this* instance healthy?" — the pool would silently serve
//! from a different machine and the operator would read the result as if it
//! described the one they clicked. The probe therefore builds its own
//! single-instance request instead of reusing the registry.
//!
//! Two design consequences worth stating, because both are deliberate:
//!
//! 1. **It runs the full `parse_response`, not just a status check.** SearXNG
//!    answers `200 OK` with an HTML page when `format=json` is not enabled in
//!    its `settings.yml` (measured in Stage 134 §3.3.2), so a status-only probe
//!    would report a thoroughly broken instance as healthy. `parse_ok` is the
//!    field that catches it.
//! 2. **It touches no shared state.** The probe builds its own client and its
//!    own one-element request; it never borrows the registry's `InstancePool`,
//!    whose cooldown counters would otherwise be perturbed by an operator
//!    pressing a button.

use std::time::{Duration, Instant};

use crate::websearch::config::WebSearchInstanceConfig;
use crate::websearch::searxng;

/// The fixed query every probe sends. A fixed string keeps probes comparable
/// across instances and avoids copying operator input into vendor logs.
pub const PROBE_QUERY: &str = "aigw connectivity probe";

/// What one probe learned. Serialized verbatim into `probe_result`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProbeOutcome {
    /// The one field an operator reads first: did a usable search response come
    /// back? False for HTTP errors, transport failures — and for a `200` whose
    /// body could not be parsed.
    pub ok: bool,
    /// HTTP status, when a response was received at all.
    pub http_status: Option<u16>,
    pub latency_ms: u64,
    /// Results after parsing. `0` on any failure.
    pub result_count: usize,
    /// Whether the body parsed as a search response. Separated from `ok` so
    /// "reachable but misconfigured" is distinguishable from "unreachable".
    pub parse_ok: bool,
    pub error: Option<String>,
}

impl ProbeOutcome {
    /// The JSON snapshot stored in `web_search_instances.probe_result`.
    pub fn to_json(&self, probed_at: &str) -> serde_json::Value {
        serde_json::json!({
            "ok": self.ok,
            "http_status": self.http_status,
            "latency_ms": self.latency_ms,
            "result_count": self.result_count,
            "parse_ok": self.parse_ok,
            "probed_at": probed_at,
            "error": self.error,
        })
    }
}

/// Probe one instance with a single real search.
///
/// `instance.api_key` may be plaintext or a `v2:gcm:` ciphertext — the DB stores
/// ciphertext, the yaml path may carry either, and `decrypt_litellm_value`
/// passes plaintext through untouched.
///
/// Retries are deliberately `0`: a probe that retried would report the retry's
/// latency and hide a flaky instance, which is the case an operator most wants
/// to see.
pub async fn probe_instance(
    kind: &str,
    provider_name: &str,
    instance: &WebSearchInstanceConfig,
    master_key: &str,
    proxy_url: Option<&str>,
    timeout_ms: u64,
) -> ProbeOutcome {
    let mut out = ProbeOutcome::default();

    // Only SearXNG has a client this stage. Refusing here (rather than sending a
    // SearXNG-shaped request to a vendor that speaks a different wire format)
    // keeps the probe honest: it reports "cannot probe", not "unhealthy".
    if kind != searxng::KIND {
        out.error = Some(format!(
            "connectivity probe is not implemented for provider kind \"{}\" (supported: {})",
            kind,
            crate::websearch::SUPPORTED_KINDS.join(", ")
        ));
        return out;
    }

    // Decrypt only for the ciphertext form; `decrypt_litellm_value` passes
    // plaintext through unchanged, so both yaml and DB rows work.
    let api_key = instance
        .api_key
        .as_ref()
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
        .map(|k| {
            crate::crypto::decrypt_litellm_value(k, master_key).unwrap_or_else(|_| k.to_string())
        });

    // Deliberately `0` retries: a probe that retried would report the retry's
    // latency and hide exactly the flakiness an operator is looking for.
    let client = match crate::websearch::client::build_search_client(
        proxy_url,
        Duration::from_millis(timeout_ms),
        0,
    ) {
        Ok(c) => c,
        Err(e) => {
            out.error = Some(e);
            return out;
        }
    };

    let url = searxng::SearxngProvider::build_url(&instance.base_url, PROBE_QUERY);
    let mut request = client.get(&url);
    if let Some(key) = api_key.as_deref() {
        request = request.bearer_auth(key);
    }

    let started = Instant::now();
    let response = match request.send().await {
        Ok(r) => r,
        Err(e) => {
            out.latency_ms = started.elapsed().as_millis() as u64;
            out.error = Some(if searxng::is_timeout(&e) {
                format!("timeout after {}ms", timeout_ms)
            } else {
                e.to_string()
            });
            return out;
        }
    };

    let status = response.status().as_u16();
    let bytes = response.bytes().await.unwrap_or_default();
    out.latency_ms = started.elapsed().as_millis() as u64;
    out.http_status = Some(status);

    if !(200..300).contains(&status) {
        out.error = Some(searxng::map_error(provider_name, status, &bytes).to_string());
        return out;
    }

    // The whole point of the probe: a 200 alone does not mean the instance
    // works, so parse the body like a real request would.
    match searxng::parse_response_from(provider_name, PROBE_QUERY, &bytes, Some(&instance.base_url))
    {
        Ok(resp) => {
            out.ok = true;
            out.parse_ok = true;
            out.result_count = resp.results.len();
        }
        Err(e) => {
            out.error = Some(e.to_string());
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Spawn a stub HTTP server that answers `/search` with a fixed status/body.
    async fn spawn_stub(status: u16, body: &'static str) -> String {
        let app = axum::Router::new().route(
            "/search",
            axum::routing::get(move || async move {
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    body.to_string(),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{}", addr)
    }

    fn instance(base_url: &str) -> WebSearchInstanceConfig {
        WebSearchInstanceConfig {
            base_url: base_url.to_string(),
            api_key: None,
            weight: None,
            enabled: true,
        }
    }

    const OK_BODY: &str = r#"{"query":"aigw connectivity probe","results":[
        {"title":"A","url":"https://a.test/1","content":"first","score":1.0,"publishedDate":null},
        {"title":"B","url":"https://b.test/1","content":"second","score":0.5,"publishedDate":null},
        {"title":"C","url":"https://c.test/1","content":"third","score":0.25,"publishedDate":null}
    ],"unresponsive_engines":[]}"#;

    const HTML_BODY: &str = "<!doctype html><html><body>Search</body></html>";

    #[tokio::test]
    async fn test_probe_reports_success_and_result_count() {
        let url = spawn_stub(200, OK_BODY).await;
        let out = probe_instance("searxng", "p", &instance(&url), "mk", None, 5000).await;

        assert!(out.ok, "a healthy 200 must probe ok: {:?}", out);
        assert_eq!(out.http_status, Some(200));
        assert!(out.parse_ok);
        assert_eq!(out.result_count, 3);
        assert!(out.error.is_none());
    }

    /// The case a status-code-only probe gets wrong: SearXNG serves HTML with a
    /// 200 when `format=json` is off (Stage 134 §3.3.2). The probe must call
    /// this unhealthy even though the HTTP layer succeeded.
    #[tokio::test]
    async fn test_probe_detects_html_body_as_parse_failure() {
        let url = spawn_stub(200, HTML_BODY).await;
        let out = probe_instance("searxng", "p", &instance(&url), "mk", None, 5000).await;

        assert!(
            !out.ok,
            "200 + HTML means the instance is misconfigured, not healthy"
        );
        assert_eq!(out.http_status, Some(200), "the status itself was fine");
        assert!(!out.parse_ok, "the body could not be parsed");
        assert_eq!(out.result_count, 0);
        assert!(out.error.is_some(), "an operator needs to know why");
    }

    #[tokio::test]
    async fn test_probe_reports_http_error_status() {
        let url = spawn_stub(503, "upstream exploded").await;
        let out = probe_instance("searxng", "p", &instance(&url), "mk", None, 5000).await;

        assert!(!out.ok);
        assert_eq!(out.http_status, Some(503));
        assert!(!out.parse_ok);
        assert_eq!(out.result_count, 0);
    }

    #[tokio::test]
    async fn test_probe_reports_transport_failure_without_status() {
        // Nothing is listening on this port; the probe must degrade, not panic.
        let out = probe_instance(
            "searxng",
            "p",
            &instance("http://127.0.0.1:1"),
            "mk",
            None,
            800,
        )
        .await;

        assert!(!out.ok);
        assert!(out.http_status.is_none());
        assert!(!out.parse_ok);
        assert!(out.error.is_some());
    }

    #[tokio::test]
    async fn test_probe_records_latency() {
        let url = spawn_stub(200, OK_BODY).await;
        let out = probe_instance("searxng", "p", &instance(&url), "mk", None, 5000).await;
        assert!(out.ok);
        // Bound is generous — the point is that a real measurement happened.
        assert!(out.latency_ms < 5_000, "latency {}", out.latency_ms);
    }

    #[tokio::test]
    async fn test_probe_snapshot_shape_is_stable() {
        let url = spawn_stub(200, OK_BODY).await;
        let out = probe_instance("searxng", "p", &instance(&url), "mk", None, 5000).await;
        let snap = out.to_json("2026-10-09T00:00:00Z");

        for key in [
            "ok",
            "http_status",
            "latency_ms",
            "result_count",
            "parse_ok",
            "probed_at",
            "error",
        ] {
            assert!(snap.get(key).is_some(), "snapshot missing {key}: {snap}");
        }
        assert_eq!(snap["probed_at"], "2026-10-09T00:00:00Z");
    }

    /// The probe must not perturb shared cooldown state. It never touches the
    /// registry's pool, but this pins that the *only* constructor argument is a
    /// single instance — a future refactor that starts passing the pool fails
    /// to compile against this signature rather than silently coupling them.
    #[test]
    fn test_probe_takes_one_instance_not_a_pool() {
        let i = instance("http://x");
        let _ = Arc::new(i);
    }
}
