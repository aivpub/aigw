//! Outbound HTTP client for search providers.
//!
//! Neither existing client fits: `Router::build_retry_client`
//! (`router.rs:535-548`) retries but has no proxy support and hardcodes a 600s
//! timeout, while `probe::build_proxy_client` (`probe.rs:26-33`) proxies but
//! never retries. Search needs all three at once — and a timeout far below 600s,
//! because the search happens *before* the first upstream byte and lands
//! directly on TTFT.
//!
//! The client is built once when the registry is constructed and shared, unlike
//! `build_retry_client`'s per-request construction: connection reuse is directly
//! visible in TTFT here.

use std::time::Duration;

/// Build a search client: optional proxy egress, transient retries, hard timeout.
///
/// `proxy_url` accepts `http`/`https`/`socks5`/`socks5h` (same as
/// `probe::build_proxy_client`; `insecure_skip_verify` is likewise not offered).
/// Retries cover 5xx and network errors only — 4xx is never retried.
pub fn build_search_client(
    proxy_url: Option<&str>,
    timeout: Duration,
    retries: u32,
) -> Result<reqwest_middleware::ClientWithMiddleware, String> {
    use reqwest_middleware::ClientBuilder;
    use reqwest_retry::policies::ExponentialBackoff;
    use reqwest_retry::RetryTransientMiddleware;

    let mut builder = reqwest::Client::builder().timeout(timeout);
    if let Some(url) = proxy_url.map(str::trim).filter(|u| !u.is_empty()) {
        let proxy = reqwest::Proxy::all(url).map_err(|e| format!("invalid proxy URL: {}", e))?;
        builder = builder.proxy(proxy);
    }
    let client = builder
        .build()
        .map_err(|e| format!("failed to build search client: {}", e))?;

    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(retries);
    Ok(ClientBuilder::new(client)
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_search_client_without_proxy() {
        assert!(build_search_client(None, Duration::from_millis(5000), 1).is_ok());
        assert!(
            build_search_client(Some("   "), Duration::from_millis(5000), 1).is_ok(),
            "blank proxy_url means direct connection, not an error"
        );
    }

    #[test]
    fn test_build_search_client_rejects_invalid_proxy() {
        let err = build_search_client(Some("not a proxy"), Duration::from_millis(100), 0)
            .expect_err("malformed proxy URL must fail at build time");
        assert!(err.contains("invalid proxy URL"), "{err}");
    }

    #[test]
    fn test_build_search_client_accepts_socks5() {
        assert!(
            build_search_client(Some("socks5://127.0.0.1:1080"), Duration::from_secs(5), 1).is_ok()
        );
    }
}
