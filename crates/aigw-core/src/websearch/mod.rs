//! Built-in web search backend abstraction (Phase 53, Stage 134).
//!
//! Two levels of selection, with deliberately non-overlapping responsibilities:
//!
//! ```text
//! WebSearchRegistry (this file) — provider level
//!   pick the named provider, else config.default_provider
//!   on a retriable failure, walk failover_order to the next provider
//!     │
//!     └─ InstancePool (instance.rs) — instance level
//!          skip disabled, skip cooling, weighted-or-uniform pick
//!          all cooling → earliest recovery (never refuse service)
//!          success resets, failures accumulate into cooldown
//! ```
//!
//! Instance level answers "one machine of this backend is down" (switch
//! machines, semantically identical). Provider level answers "this vendor is
//! unusable" (switch vendors, possibly a quality change).
//!
//! Only SearXNG is implemented in this Stage. Adding a vendor costs one new
//! `websearch/<name>.rs` plus one arm in [`build_provider`] — nothing else in
//! this module, `types.rs`, `provider.rs`, `config.rs` or `client.rs` changes.
//! That claim is checkable: the unknown-kind error enumerates
//! [`config::SUPPORTED_KINDS`], and the `StubProvider` tests below exercise every
//! branch that only a second provider can reach.
//!
//! Nothing here is wired into a request path yet — that is Stage 135.

pub mod client;
pub mod config;
pub mod instance;
pub mod provider;
pub mod searxng;
pub mod types;

pub use config::{
    WebSearchConfig, WebSearchInstanceConfig, WebSearchProviderConfig, SUPPORTED_KINDS,
};
pub use instance::{CooldownPolicy, InstancePool, WebSearchInstanceState};
pub use provider::{SearchError, SearchProvider};
pub use types::{
    normalize, Guardrails, SearchRequest, SearchResponse, SearchResult, DEFAULT_MAX_RESULTS,
    DEFAULT_SNIPPET_MAX_CHARS,
};

use std::sync::Arc;

/// Runtime search layer: the configured providers plus the shared guardrails.
///
/// Built by [`crate::config_loader::build_websearch_registry`]. `None` from that
/// function means the layer is off and costs nothing.
pub struct WebSearchRegistry {
    providers: Vec<Arc<dyn SearchProvider>>,
    /// Provider names in attempt order: default first, then failover_order.
    attempt_order: Vec<String>,
    guardrails: Guardrails,
}

impl std::fmt::Debug for WebSearchRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSearchRegistry")
            .field(
                "providers",
                &self.providers.iter().map(|p| p.name()).collect::<Vec<_>>(),
            )
            .field("attempt_order", &self.attempt_order)
            .field("guardrails", &self.guardrails)
            .finish()
    }
}

impl WebSearchRegistry {
    pub fn new(
        providers: Vec<Arc<dyn SearchProvider>>,
        attempt_order: Vec<String>,
        guardrails: Guardrails,
    ) -> Self {
        Self {
            providers,
            attempt_order,
            guardrails,
        }
    }

    pub fn guardrails(&self) -> &Guardrails {
        &self.guardrails
    }

    pub fn provider_names(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.name()).collect()
    }

    fn get(&self, name: &str) -> Option<&Arc<dyn SearchProvider>> {
        self.providers.iter().find(|p| p.name() == name)
    }

    /// Price of one query against `name` (or the default provider).
    ///
    /// Stage 136 is the first consumer; exposed here so the config value has a
    /// reader and does not rot into another `litellm_settings`.
    pub fn cost_per_query(&self, name: Option<&str>) -> Option<f64> {
        self.resolve_order(name)
            .first()
            .and_then(|n| self.get(n))
            .map(|p| p.cost_per_query())
    }

    /// Attempt order for this call: the requested provider first (when it
    /// exists), then the configured order minus duplicates.
    fn resolve_order(&self, requested: Option<&str>) -> Vec<String> {
        let mut order: Vec<String> = Vec::with_capacity(self.attempt_order.len() + 1);
        if let Some(name) = requested {
            if self.get(name).is_some() {
                order.push(name.to_string());
            }
        }
        for name in &self.attempt_order {
            if !order.contains(name) {
                order.push(name.clone());
            }
        }
        order
    }

    /// Run one search, applying provider-level failover and the guardrails.
    ///
    /// `provider` names a preferred backend; unknown or absent falls back to the
    /// configured default. Failover only follows retriable errors — a 4xx
    /// (bad key, exhausted quota) fails immediately rather than re-spending the
    /// same request against another vendor.
    pub async fn search(
        &self,
        req: &SearchRequest,
        provider: Option<&str>,
    ) -> Result<SearchResponse, SearchError> {
        if self.providers.is_empty() {
            return Err(SearchError::NotConfigured);
        }
        if req.query.trim().is_empty() {
            return Err(SearchError::EmptyQuery);
        }

        let max_results = self.guardrails.clamp_max_results(req.max_results);
        let clamped = SearchRequest {
            query: req.query.clone(),
            max_results: Some(max_results),
        };

        let mut last_err: Option<SearchError> = None;
        for name in self.resolve_order(provider) {
            let Some(p) = self.get(&name) else { continue };
            match p.search(&clamped).await {
                Ok(mut resp) => {
                    normalize(&mut resp, &self.guardrails, max_results);
                    return Ok(resp);
                }
                Err(err) => {
                    if !err.is_retriable() {
                        return Err(err);
                    }
                    tracing::warn!(
                        provider = %name,
                        error = %err,
                        "web search provider failed, trying next in failover order"
                    );
                    last_err = Some(err);
                }
            }
        }

        Err(last_err.unwrap_or(SearchError::NotConfigured))
    }
}

/// Construct the provider for one config entry.
///
/// **This is the single place a new vendor is wired in.** Unknown kinds are
/// rejected with a message listing [`config::SUPPORTED_KINDS`], so the error text
/// stays accurate without being maintained by hand.
pub fn build_provider(
    cfg: &WebSearchProviderConfig,
    instances: Vec<WebSearchInstanceConfig>,
    policy: CooldownPolicy,
    client: reqwest_middleware::ClientWithMiddleware,
    timeout_ms: u64,
) -> Result<Arc<dyn SearchProvider>, String> {
    let pool = InstancePool::new(instances, policy);
    match cfg.kind.as_str() {
        searxng::KIND => Ok(Arc::new(searxng::SearxngProvider::new(
            cfg.name.clone(),
            cfg.cost_per_query,
            pool,
            client,
            timeout_ms,
        ))),
        other => Err(format!(
            "unsupported provider kind \"{}\" for provider \"{}\"; supported kinds: {}",
            other,
            cfg.name,
            SUPPORTED_KINDS.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Test-only second provider.
    ///
    /// Stands in for the vendors deliberately left unimplemented, and is the only
    /// way to reach the branches a single provider cannot: trait object
    /// heterogeneity, provider-level failover ordering, 4xx-does-not-failover,
    /// real consumption of a decrypted `api_key`, and a non-zero
    /// `cost_per_query`. Without it "multi-provider ready" would be unverifiable.
    struct StubProvider {
        name: String,
        cost: f64,
        /// Replayed in order; the last entry repeats once exhausted.
        script: Vec<StubOutcome>,
        calls: AtomicUsize,
        /// Echoed into the response query so a test can assert the key that the
        /// loader decrypted actually reached the provider.
        api_key: Option<String>,
    }

    #[derive(Clone)]
    enum StubOutcome {
        Ok(usize),
        Http(u16),
        Timeout,
        Transport,
    }

    impl StubProvider {
        fn new(name: &str, script: Vec<StubOutcome>) -> Self {
            Self {
                name: name.to_string(),
                cost: 0.0,
                script,
                calls: AtomicUsize::new(0),
                api_key: None,
            }
        }

        fn with_cost(mut self, cost: f64) -> Self {
            self.cost = cost;
            self
        }

        fn with_api_key(mut self, key: Option<String>) -> Self {
            self.api_key = key;
            self
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl SearchProvider for StubProvider {
        fn name(&self) -> &str {
            &self.name
        }

        fn cost_per_query(&self) -> f64 {
            self.cost
        }

        async fn search(&self, req: &SearchRequest) -> Result<SearchResponse, SearchError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            let outcome = self
                .script
                .get(n)
                .or_else(|| self.script.last())
                .cloned()
                .unwrap_or(StubOutcome::Ok(1));
            match outcome {
                StubOutcome::Ok(count) => Ok(SearchResponse {
                    results: (0..count)
                        .map(|i| SearchResult {
                            title: format!("{} hit {}", self.name, i),
                            url: format!("https://{}.test/{}", self.name, i),
                            snippet: "snippet".to_string(),
                            published_date: None,
                            score: Some(1.0 - (i as f32) * 0.1),
                        })
                        .collect(),
                    // Echo the key so a test can prove it survived decryption.
                    query: self.api_key.clone().unwrap_or_else(|| req.query.clone()),
                    provider: self.name.clone(),
                    reported_credits: None,
                }),
                StubOutcome::Http(status) => Err(SearchError::Http {
                    provider: self.name.clone(),
                    status,
                    body: String::new(),
                }),
                StubOutcome::Timeout => Err(SearchError::Timeout {
                    provider: self.name.clone(),
                    ms: 5000,
                }),
                StubOutcome::Transport => Err(SearchError::Transport {
                    provider: self.name.clone(),
                    detail: "connection refused".to_string(),
                }),
            }
        }
    }

    fn registry(providers: Vec<Arc<dyn SearchProvider>>, order: Vec<&str>) -> WebSearchRegistry {
        WebSearchRegistry::new(
            providers,
            order.into_iter().map(str::to_string).collect(),
            Guardrails::default(),
        )
    }

    #[tokio::test]
    async fn test_registry_selects_default_provider() {
        // Two heterogeneous providers behind one `Arc<dyn SearchProvider>` list —
        // this compiling at all is the object-safety proof.
        let primary = Arc::new(StubProvider::new("primary", vec![StubOutcome::Ok(2)]));
        let secondary = Arc::new(StubProvider::new("secondary", vec![StubOutcome::Ok(2)]));
        let reg = registry(
            vec![primary.clone(), secondary.clone()],
            vec!["primary", "secondary"],
        );

        let resp = reg
            .search(&SearchRequest::new("q"), None)
            .await
            .expect("default provider must answer");
        assert_eq!(resp.provider, "primary");
        assert_eq!(secondary.call_count(), 0, "default must be tried alone");
    }

    #[tokio::test]
    async fn test_registry_honours_requested_provider() {
        let primary = Arc::new(StubProvider::new("primary", vec![StubOutcome::Ok(1)]));
        let secondary = Arc::new(StubProvider::new("secondary", vec![StubOutcome::Ok(1)]));
        let reg = registry(
            vec![primary.clone(), secondary.clone()],
            vec!["primary", "secondary"],
        );

        let resp = reg
            .search(&SearchRequest::new("q"), Some("secondary"))
            .await
            .unwrap();
        assert_eq!(resp.provider, "secondary");
        assert_eq!(primary.call_count(), 0);

        // An unknown name falls back to the configured order rather than failing.
        let resp = reg
            .search(&SearchRequest::new("q"), Some("ghost"))
            .await
            .unwrap();
        assert_eq!(resp.provider, "primary");
    }

    #[tokio::test]
    async fn test_registry_failover_on_5xx_and_timeout() {
        let primary = Arc::new(StubProvider::new("primary", vec![StubOutcome::Http(503)]));
        let secondary = Arc::new(StubProvider::new("secondary", vec![StubOutcome::Ok(1)]));
        let reg = registry(
            vec![primary.clone(), secondary.clone()],
            vec!["primary", "secondary"],
        );
        let resp = reg.search(&SearchRequest::new("q"), None).await.unwrap();
        assert_eq!(resp.provider, "secondary");
        assert_eq!(primary.call_count(), 1, "primary tried exactly once");

        let primary = Arc::new(StubProvider::new("primary", vec![StubOutcome::Timeout]));
        let secondary = Arc::new(StubProvider::new("secondary", vec![StubOutcome::Ok(1)]));
        let reg = registry(vec![primary, secondary], vec!["primary", "secondary"]);
        assert_eq!(
            reg.search(&SearchRequest::new("q"), None)
                .await
                .unwrap()
                .provider,
            "secondary"
        );

        let primary = Arc::new(StubProvider::new("primary", vec![StubOutcome::Transport]));
        let secondary = Arc::new(StubProvider::new("secondary", vec![StubOutcome::Ok(1)]));
        let reg = registry(vec![primary, secondary], vec!["primary", "secondary"]);
        assert_eq!(
            reg.search(&SearchRequest::new("q"), None)
                .await
                .unwrap()
                .provider,
            "secondary"
        );
    }

    #[tokio::test]
    async fn test_registry_no_failover_on_4xx() {
        let primary = Arc::new(StubProvider::new("primary", vec![StubOutcome::Http(401)]));
        let secondary = Arc::new(StubProvider::new("secondary", vec![StubOutcome::Ok(1)]));
        let reg = registry(
            vec![primary.clone(), secondary.clone()],
            vec!["primary", "secondary"],
        );

        let err = reg
            .search(&SearchRequest::new("q"), None)
            .await
            .expect_err("4xx must surface, not failover");
        assert_eq!(err.status(), Some(401));
        assert_eq!(
            secondary.call_count(),
            0,
            "a 4xx must not re-spend the query on another vendor"
        );
    }

    #[tokio::test]
    async fn test_registry_all_providers_failed() {
        let primary = Arc::new(StubProvider::new("primary", vec![StubOutcome::Timeout]));
        let secondary = Arc::new(StubProvider::new("secondary", vec![StubOutcome::Timeout]));
        let reg = registry(
            vec![primary.clone(), secondary.clone()],
            vec!["primary", "secondary"],
        );

        let err = reg
            .search(&SearchRequest::new("q"), None)
            .await
            .unwrap_err();
        assert!(matches!(err, SearchError::Timeout { .. }));
        assert_eq!(
            err.provider(),
            Some("secondary"),
            "the surfaced error comes from the last provider tried"
        );
        assert_eq!(primary.call_count(), 1);
        assert_eq!(secondary.call_count(), 1);
    }

    #[tokio::test]
    async fn test_registry_empty_returns_not_configured() {
        let reg = registry(vec![], vec![]);
        assert!(matches!(
            reg.search(&SearchRequest::new("q"), None).await,
            Err(SearchError::NotConfigured)
        ));
    }

    #[tokio::test]
    async fn test_registry_rejects_empty_query_before_dispatch() {
        let p = Arc::new(StubProvider::new("primary", vec![StubOutcome::Ok(1)]));
        let reg = registry(vec![p.clone()], vec!["primary"]);
        assert!(matches!(
            reg.search(&SearchRequest::new("   "), None).await,
            Err(SearchError::EmptyQuery)
        ));
        assert_eq!(p.call_count(), 0, "no round-trip for an empty query");
    }

    #[tokio::test]
    async fn test_registry_applies_guardrails_to_provider_output() {
        // A provider ignoring max_results must still be capped by the registry.
        let p = Arc::new(StubProvider::new("primary", vec![StubOutcome::Ok(20)]));
        let reg = WebSearchRegistry::new(
            vec![p],
            vec!["primary".to_string()],
            Guardrails {
                max_results: 5,
                ..Default::default()
            },
        );
        let mut req = SearchRequest::new("q");
        req.max_results = Some(50);
        let resp = reg.search(&req, None).await.unwrap();
        assert_eq!(resp.results.len(), 5, "caller cannot raise the ceiling");
    }

    #[tokio::test]
    async fn test_provider_api_key_decrypted_from_v2_gcm() {
        // SearXNG needs no auth, so this is the only consumer of the decryption
        // path — see the Stage 134 risk note: a paid provider's first integration
        // must re-verify this end to end over the network.
        let master_key = "sk-master-test-key";
        let cipher = crate::crypto::encrypt_litellm_value_gcm("sk-test-xxx", master_key)
            .expect("gcm encrypt");
        assert!(cipher.starts_with("v2:gcm:"));

        let decrypted = crate::crypto::decrypt_litellm_value(&cipher, master_key).unwrap();
        let plain = crate::crypto::decrypt_litellm_value("plain-key", master_key)
            .unwrap_or_else(|_| "plain-key".to_string());
        assert_eq!(decrypted, "sk-test-xxx");
        assert_eq!(plain, "plain-key", "plaintext keys must pass through");

        // And the decrypted value really reaches the provider.
        let p = Arc::new(
            StubProvider::new("secret", vec![StubOutcome::Ok(1)]).with_api_key(Some(decrypted)),
        );
        let reg = registry(vec![p], vec!["secret"]);
        let resp = reg.search(&SearchRequest::new("q"), None).await.unwrap();
        assert_eq!(resp.query, "sk-test-xxx");
    }

    #[tokio::test]
    async fn test_calc_cost_with_nonzero_cost_per_query_via_stub() {
        let p = Arc::new(StubProvider::new("paid", vec![StubOutcome::Ok(1)]).with_cost(0.008));
        let reg = registry(vec![p], vec!["paid"]);

        assert_eq!(reg.cost_per_query(None), Some(0.008));
        let mut total = 0.0;
        for _ in 0..2 {
            reg.search(&SearchRequest::new("q"), None).await.unwrap();
            total += reg.cost_per_query(Some("paid")).unwrap();
        }
        assert!(
            (total - 0.016).abs() < f64::EPSILON,
            "unit price × query count must be exact, got {total}"
        );
    }

    #[test]
    fn test_build_provider_rejects_unknown_kind() {
        let cfg = WebSearchProviderConfig {
            name: "x".to_string(),
            kind: "tavily".to_string(),
            cost_per_query: 0.008,
            timeout_ms: None,
            instances: vec![],
        };
        let client = client::build_search_client(None, std::time::Duration::from_secs(5), 0)
            .expect("client");
        // `Arc<dyn SearchProvider>` is not Debug, so unwrap the Result by hand
        // instead of via expect_err.
        let err = match build_provider(&cfg, vec![], CooldownPolicy::default(), client, 5000) {
            Ok(_) => panic!("unimplemented kind must fail at construction"),
            Err(e) => e,
        };
        assert!(err.contains("tavily"), "{err}");
        assert!(err.contains("searxng"), "must list supported kinds: {err}");
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Integration: real SearxngProvider against local stub servers.
    // The only tests covering the seam between the IO shell and the pure
    // functions. No network egress — everything binds 127.0.0.1:0.
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    /// Spawn a stub HTTP server. `status` is served as the response code, `body`
    /// as the body, and every request's query string is recorded.
    async fn spawn_stub(status: u16, body: &'static str, delay_ms: u64) -> (String, Arc<Mutexed>) {
        let seen = Arc::new(Mutexed::default());
        let seen_clone = seen.clone();
        let app = axum::Router::new().route(
            "/search",
            axum::routing::get(move |uri: axum::http::Uri| {
                let seen = seen_clone.clone();
                async move {
                    seen.record(uri.query().unwrap_or_default().to_string());
                    if delay_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        body.to_string(),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), seen)
    }

    #[derive(Default)]
    struct Mutexed {
        queries: std::sync::Mutex<Vec<String>>,
    }

    impl Mutexed {
        fn record(&self, q: String) {
            self.queries.lock().unwrap().push(q);
        }
        fn count(&self) -> usize {
            self.queries.lock().unwrap().len()
        }
        fn first(&self) -> Option<String> {
            self.queries.lock().unwrap().first().cloned()
        }
    }

    const STUB_BODY: &str = r#"{"query":"rust async","results":[
        {"title":"A","url":"https://a.test/1","content":"first","score":1.0,"publishedDate":null},
        {"title":"B","url":"https://b.test/1","content":"","score":0.5,"publishedDate":"2026-01-02T00:00:00"}
    ],"unresponsive_engines":[]}"#;

    fn searxng_over(base_urls: &[&str], timeout_ms: u64) -> searxng::SearxngProvider {
        let instances: Vec<WebSearchInstanceConfig> = base_urls
            .iter()
            .map(|u| WebSearchInstanceConfig {
                base_url: (*u).to_string(),
                api_key: None,
                weight: None,
                enabled: true,
            })
            .collect();
        let client =
            client::build_search_client(None, std::time::Duration::from_millis(timeout_ms), 0)
                .expect("client");
        searxng::SearxngProvider::new(
            "searxng",
            0.0002,
            InstancePool::new(instances, CooldownPolicy::default()),
            client,
            timeout_ms,
        )
    }

    #[tokio::test]
    async fn test_searxng_search_end_to_end_against_local_stub() {
        let (url, seen) = spawn_stub(200, STUB_BODY, 0).await;
        let p = searxng_over(&[&url], 5000);

        let resp = p
            .search(&SearchRequest::new("rust async"))
            .await
            .expect("stub must answer");
        assert_eq!(resp.provider, "searxng");
        assert_eq!(resp.results.len(), 2);
        assert_eq!(resp.results[0].snippet, "first");
        assert_eq!(
            resp.results[1].snippet, "",
            "empty snippet survives IO path"
        );

        let q = seen.first().expect("request recorded");
        assert!(q.contains("format=json"), "{q}");
        assert!(q.contains("q=rust+async"), "{q}");
    }

    #[tokio::test]
    async fn test_searxng_multi_instance_failover_against_local_stubs() {
        // Instance pick is randomized, so assert the order-independent property:
        // a dead instance never breaks the query, whichever is tried first.
        let (bad, bad_seen) = spawn_stub(500, "boom", 0).await;
        let (good, good_seen) = spawn_stub(200, STUB_BODY, 0).await;

        for _ in 0..12 {
            // Fresh provider each round so cooldown from the previous round does
            // not pin the choice.
            let p = searxng_over(&[&bad, &good], 5000);
            let resp = p
                .search(&SearchRequest::new("rust async"))
                .await
                .expect("a 500 on one instance must not fail the query");
            assert_eq!(resp.results.len(), 2);
        }
        assert!(
            bad_seen.count() > 0,
            "the failing instance must really be in the pool, else this proves nothing"
        );
        assert_eq!(
            good_seen.count(),
            12,
            "every round must end up served by the healthy instance"
        );
    }

    #[tokio::test]
    async fn test_searxng_all_instances_down_returns_last_error() {
        let (a, a_seen) = spawn_stub(500, "boom", 0).await;
        let (b, b_seen) = spawn_stub(502, "boom", 0).await;
        let p = searxng_over(&[&a, &b], 5000);

        let err = p
            .search(&SearchRequest::new("rust async"))
            .await
            .expect_err("all instances down must fail");
        assert!(matches!(err, SearchError::Http { status, .. } if status >= 500));
        assert_eq!(a_seen.count(), 1);
        assert_eq!(b_seen.count(), 1, "every instance must be attempted");
    }

    #[tokio::test]
    async fn test_searxng_4xx_does_not_walk_remaining_instances() {
        // A 4xx means "this config is wrong", not "this machine is down", so the
        // walk must stop. Pick order is random, so loop until the 403 instance is
        // chosen first and assert the healthy one was left untouched that round.
        let mut saw_403_first = false;
        for _ in 0..24 {
            let (bad, bad_seen) = spawn_stub(403, "forbidden", 0).await;
            let (good, good_seen) = spawn_stub(200, STUB_BODY, 0).await;
            let p = searxng_over(&[&bad, &good], 5000);

            match p.search(&SearchRequest::new("rust async")).await {
                Err(err) => {
                    assert_eq!(err.status(), Some(403));
                    assert!(err.to_string().contains("settings.yml"));
                    assert_eq!(bad_seen.count(), 1);
                    assert_eq!(
                        good_seen.count(),
                        0,
                        "a 403 must not fan out to the other instance"
                    );
                    saw_403_first = true;
                    break;
                }
                // Healthy instance was picked first; nothing to assert this round.
                Ok(_) => continue,
            }
        }
        assert!(
            saw_403_first,
            "the 403 instance was never picked first in 24 rounds — selection looks broken"
        );
    }

    #[tokio::test]
    async fn test_searxng_html_200_is_a_parse_error_then_tries_next() {
        // SearXNG answers 200 + HTML when format=json is unavailable. That is a
        // retriable misconfiguration of one instance, so the query must still be
        // served by a healthy sibling regardless of pick order.
        let (html, html_seen) = spawn_stub(200, "<!DOCTYPE html><html></html>", 0).await;
        let (good, good_seen) = spawn_stub(200, STUB_BODY, 0).await;

        for _ in 0..12 {
            let p = searxng_over(&[&html, &good], 5000);
            let resp = p
                .search(&SearchRequest::new("rust async"))
                .await
                .expect("an HTML-serving instance must not fail the query");
            assert_eq!(resp.results.len(), 2);
        }
        assert!(
            html_seen.count() > 0,
            "the HTML instance must really be in the pool"
        );
        assert_eq!(good_seen.count(), 12);
    }

    #[tokio::test]
    async fn test_search_timeout_returns_timeout_error() {
        let (slow, _) = spawn_stub(200, STUB_BODY, 400).await;
        let p = searxng_over(&[&slow], 80);

        let err = p
            .search(&SearchRequest::new("rust async"))
            .await
            .expect_err("a slow instance must time out, not hang");
        assert!(
            matches!(err, SearchError::Timeout { .. }),
            "expected Timeout, got {err:?}"
        );
    }

    #[tokio::test]
    async fn test_searxng_empty_query_short_circuits_before_io() {
        let (url, seen) = spawn_stub(200, STUB_BODY, 0).await;
        let p = searxng_over(&[&url], 5000);
        assert!(matches!(
            p.search(&SearchRequest::new("  ")).await,
            Err(SearchError::EmptyQuery)
        ));
        assert_eq!(seen.count(), 0, "no round-trip for an empty query");
    }

    #[tokio::test]
    async fn test_registry_over_real_searxng_provider_normalizes() {
        let (url, _) = spawn_stub(200, STUB_BODY, 0).await;
        let reg = WebSearchRegistry::new(
            vec![Arc::new(searxng_over(&[&url], 5000))],
            vec!["searxng".to_string()],
            Guardrails {
                max_results: 1,
                ..Default::default()
            },
        );
        let resp = reg
            .search(&SearchRequest::new("rust async"), None)
            .await
            .unwrap();
        assert_eq!(resp.results.len(), 1, "registry applies the guardrails");
        assert_eq!(resp.results[0].score, Some(1.0), "highest score survives");
        assert_eq!(reg.provider_names(), vec!["searxng"]);
        assert_eq!(reg.cost_per_query(None), Some(0.0002));
    }

    #[test]
    fn test_registry_and_pool_are_send_sync() {
        // The registry is held in AppState across await points (Stage 135), so
        // losing Send+Sync would surface as a confusing handler error later.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WebSearchRegistry>();
        assert_send_sync::<InstancePool>();
    }
}
