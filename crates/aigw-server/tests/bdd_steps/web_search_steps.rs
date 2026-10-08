//! Step bindings for web_search.feature — Phase 53 Stage 135 wiring.
//!
//! A dedicated stub stands in for SearXNG rather than reusing `MockUpstream`:
//! search needs its own `/search` route plus a controllable delay, and keeping
//! it local avoids widening the shared mock with knobs only this feature uses.

use cucumber::{given, then, when};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

use crate::TestWorld;

/// A search row always inherits its parent's identity, so tests find it by
/// `call_type` (no query API filters on that column).
async fn search_rows(world: &mut TestWorld) -> Vec<aigw_core::models::SpendLog> {
    let state = world.ensure_state().await;
    state
        .db
        .query_spend_logs(None, Some(200))
        .await
        .expect("query spend logs")
        .into_iter()
        .filter(|l| l.call_type == "search")
        .collect()
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Search stub
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Clone)]
struct StubConfig {
    status: u16,
    body: Value,
    delay_ms: u64,
}

struct SearchStub {
    base_url: String,
    config: Arc<Mutex<StubConfig>>,
    queries: Arc<Mutex<Vec<String>>>,
    _shutdown: tokio::sync::oneshot::Sender<()>,
}

fn searxng_fixture() -> Value {
    json!({
        "query": "rust async",
        "results": [
            {
                "title": "Rust Async Book",
                "url": "https://rust-lang.github.io/async-book/",
                "content": "Asynchronous programming in Rust explained.",
                "publishedDate": null,
                "score": 1.0
            },
            {
                "title": "Tokio Tutorial",
                "url": "https://tokio.rs/tokio/tutorial",
                "content": "Learn tokio from scratch.",
                "publishedDate": "2025-03-01T00:00:00",
                "score": 0.83
            }
        ]
    })
}

impl SearchStub {
    async fn start() -> Self {
        use axum::{extract::State, routing::get, Router};

        let config = Arc::new(Mutex::new(StubConfig {
            status: 200,
            body: searxng_fixture(),
            delay_ms: 0,
        }));
        let queries: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        let app = Router::new()
            .route(
                "/search",
                get(
                    |State((cfg, q)): State<(Arc<Mutex<StubConfig>>, Arc<Mutex<Vec<String>>>)>,
                     uri: axum::http::Uri| async move {
                        q.lock().unwrap().push(uri.to_string());
                        let (status, body, delay) = {
                            let c = cfg.lock().unwrap();
                            (c.status, c.body.clone(), c.delay_ms)
                        };
                        if delay > 0 {
                            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                        }
                        (
                            axum::http::StatusCode::from_u16(status)
                                .unwrap_or(axum::http::StatusCode::OK),
                            axum::Json(body),
                        )
                    },
                ),
            )
            .with_state((config.clone(), queries.clone()));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind search stub");
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });

        SearchStub {
            base_url: format!("http://{}:{}", addr.ip(), addr.port()),
            config,
            queries,
            _shutdown: tx,
        }
    }

    fn set(&self, status: u16, body: Value) {
        let mut c = self.config.lock().unwrap();
        c.status = status;
        c.body = body;
    }

    fn set_delay(&self, ms: u64) {
        self.config.lock().unwrap().delay_ms = ms;
    }

    fn request_count(&self) -> usize {
        self.queries.lock().unwrap().len()
    }

    fn last_query(&self) -> Option<String> {
        self.queries.lock().unwrap().last().cloned()
    }
}

static STUB: std::sync::OnceLock<Arc<tokio::sync::Mutex<Option<Arc<SearchStub>>>>> =
    std::sync::OnceLock::new();

/// Second instance slot — for the multi-instance `api_base` scenario.
static STUB2: std::sync::OnceLock<Arc<tokio::sync::Mutex<Option<Arc<SearchStub>>>>> =
    std::sync::OnceLock::new();

fn stub_slot() -> &'static Arc<tokio::sync::Mutex<Option<Arc<SearchStub>>>> {
    STUB.get_or_init(|| Arc::new(tokio::sync::Mutex::new(None)))
}

fn stub2_slot() -> &'static Arc<tokio::sync::Mutex<Option<Arc<SearchStub>>>> {
    STUB2.get_or_init(|| Arc::new(tokio::sync::Mutex::new(None)))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn install_registry(world: &mut TestWorld, stub: Option<Arc<SearchStub>>, timeout_ms: u64) {
    let urls = stub.map(|s| vec![s.base_url.clone()]);
    install_registry_urls(world, urls, timeout_ms).await;
}

/// [`install_registry`] with an explicit instance list — the multi-instance
/// `api_base` scenario needs two endpoints so the column can be shown to track
/// whichever one actually served the query.
async fn install_registry_urls(world: &mut TestWorld, urls: Option<Vec<String>>, timeout_ms: u64) {
    use aigw_core::websearch::{WebSearchConfig, WebSearchInstanceConfig, WebSearchProviderConfig};

    let cfg = urls.map(|urls| WebSearchConfig {
        enabled: true,
        default_provider: "searxng".to_string(),
        failover_order: vec!["searxng".to_string()],
        max_results: 5,
        timeout_ms,
        retries: 0,
        snippet_max_chars: 2000,
        allowed_domains: vec![],
        blocked_domains: vec![],
        proxy_url: String::new(),
        allowed_fails: 3,
        cooldown_secs: 60.0,
        providers: vec![WebSearchProviderConfig {
            name: "searxng".to_string(),
            kind: "searxng".to_string(),
            cost_per_query: 0.01,
            timeout_ms: None,
            instances: urls
                .into_iter()
                .map(|base_url| WebSearchInstanceConfig {
                    base_url,
                    api_key: None,
                    weight: None,
                    enabled: true,
                })
                .collect(),
        }],
    });

    let reg = aigw_core::config_loader::build_websearch_registry(&cfg, "mk")
        .expect("build web_search registry")
        .map(Arc::new);

    let cur = world.ensure_state().await;
    let next: aigw_server::routes::keys::SharedState =
        Arc::new(aigw_server::routes::keys::AppState {
            web_search: reg,
            ..(*cur).clone()
        });
    world.state = Some(next);
}

#[given(expr = "mock 搜索后端已启动且 web_search 已配置")]
async fn given_search_backend_configured(world: &mut TestWorld) {
    let mut slot = stub_slot().lock().await;
    let stub = match slot.as_ref() {
        Some(s) => s.clone(),
        None => {
            let s = Arc::new(SearchStub::start().await);
            *slot = Some(s.clone());
            s
        }
    };
    drop(slot);
    stub.queries.lock().unwrap().clear();
    stub.set(200, searxng_fixture());
    stub.set_delay(0);
    install_registry(world, Some(stub), 5000).await;
}

#[given(expr = "web_search 未配置")]
async fn given_search_not_configured(world: &mut TestWorld) {
    let slot = stub_slot().lock().await;
    if let Some(s) = slot.as_ref() {
        s.queries.lock().unwrap().clear();
        s.set(200, searxng_fixture());
    }
    drop(slot);
    install_registry(world, None, 5000).await;
}

/// Two instances: the first is a dead port, the second is the live stub. The
/// `api_base` column must show the one that actually answered, not the first
/// configured endpoint.
#[given(expr = "web_search 配有两个实例且仅第二个可用")]
async fn given_two_instances_second_live(world: &mut TestWorld) {
    let mut slot = stub2_slot().lock().await;
    let stub = match slot.as_ref() {
        Some(s) => s.clone(),
        None => {
            let s = Arc::new(SearchStub::start().await);
            *slot = Some(s.clone());
            s
        }
    };
    drop(slot);
    stub.queries.lock().unwrap().clear();
    stub.set(200, searxng_fixture());
    stub.set_delay(0);
    let urls = vec!["http://127.0.0.1:9".to_string(), stub.base_url.clone()];
    install_registry_urls(world, Some(urls), 3000).await;
}

#[then(expr = "搜索行的 api_base 为第二个实例地址")]
async fn then_search_api_base_is_second(world: &mut TestWorld) {
    let slot = stub2_slot().lock().await;
    let expected = slot
        .as_ref()
        .expect("second stub not started")
        .base_url
        .clone();
    drop(slot);
    let rows = search_rows(world).await;
    let last = rows.last().expect("no search row");
    assert_eq!(
        last.api_base.as_deref(),
        Some(expected.as_str()),
        "api_base must be the instance that served the query, not the first configured"
    );
}

#[given(expr = "搜索后端返回状态码 {int}")]
async fn given_search_backend_returns(_world: &mut TestWorld, status: u16) {
    let slot = stub_slot().lock().await;
    let stub = slot.as_ref().expect("search stub not started");
    stub.set(status, json!({"error": "boom"}));
}

#[given(expr = "搜索后端延迟 {int} 毫秒且 web_search 超时为 {int} 毫秒")]
async fn given_search_backend_slow(world: &mut TestWorld, delay: u64, timeout: u64) {
    let mut slot = stub_slot().lock().await;
    let stub = match slot.as_ref() {
        Some(s) => s.clone(),
        None => {
            let s = Arc::new(SearchStub::start().await);
            *slot = Some(s.clone());
            s
        }
    };
    drop(slot);
    stub.set(200, searxng_fixture());
    stub.set_delay(delay);
    install_registry(world, Some(stub), timeout).await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn run_request(world: &mut TestWorld, path: &str, alias: &str, body: Value) {
    use axum::http::Method;
    use axum::Router;
    use tower::util::ServiceExt;

    let state = world.ensure_state().await;
    let handler: axum::routing::MethodRouter<aigw_server::routes::keys::SharedState> = match path {
        "/chat/completions" => axum::routing::post(aigw_server::routes::chat::chat_completions),
        "/v1/messages" => axum::routing::post(aigw_server::routes::v1_messages::messages_handler),
        _ => axum::routing::post(aigw_server::routes::responses::responses_handler),
    };
    let app = Router::new()
        .route(path, handler)
        .layer(tower_http::request_id::PropagateRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-call-id"),
        ))
        .layer(tower_http::request_id::SetRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-request-id"),
            aigw_core::request_id::UuidV7RequestId,
        ))
        .with_state(state);

    let token = world.created_keys.get(alias).expect("key not found");
    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("Content-Type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .header("Authorization", format!("Bearer {}", token))
        .header("x-api-key", token)
        .body(axum::body::Body::from(body.to_string()))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let resp_headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    world.last_status = Some(status);
    world.last_body = serde_json::from_slice(&bytes).ok();
    world.last_headers = Some(resp_headers);
}

#[when(
    expr = "使用 key {string} 发送带 web_search_options 的 POST \\/chat\\/completions 请求用 model {string}"
)]
async fn when_chat_with_search(world: &mut TestWorld, alias: String, model: String) {
    run_request(
        world,
        "/chat/completions",
        &alias,
        json!({
            "model": model,
            "messages": [{"role": "user", "content": "rust async 是什么"}],
            "web_search_options": {"search_context_size": "medium"}
        }),
    )
    .await;
}

#[when(
    expr = "使用 key {string} 发送带 web_search_options 但不带 web_search 配置的 POST \\/chat\\/completions 请求用 model {string}"
)]
async fn when_chat_with_search_unconfigured(world: &mut TestWorld, alias: String, model: String) {
    when_chat_with_search(world, alias, model).await;
}

#[when(
    expr = "使用 key {string} 发送带 web_search tool 的 POST \\/v1\\/responses 请求用 model {string}"
)]
async fn when_responses_with_search(world: &mut TestWorld, alias: String, model: String) {
    run_request(
        world,
        "/v1/responses",
        &alias,
        json!({
            "model": model,
            "input": "rust async 是什么",
            "tools": [{"type": "web_search"}]
        }),
    )
    .await;
}

#[when(
    expr = "使用 key {string} 发送带 web_search_20250305 的 POST \\/v1\\/messages 请求用 model {string}"
)]
async fn when_messages_with_search(world: &mut TestWorld, alias: String, model: String) {
    run_request(
        world,
        "/v1/messages",
        &alias,
        json!({
            "model": model,
            "max_tokens": 64,
            "messages": [{"role": "user", "content": "rust async 是什么"}],
            "tools": [{"type": "web_search_20250305", "name": "web_search", "max_uses": 5}]
        }),
    )
    .await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "搜索后端收到 {int} 次请求")]
async fn then_search_backend_calls(_world: &mut TestWorld, n: usize) {
    let slot = stub_slot().lock().await;
    let stub = slot.as_ref().expect("search stub not started");
    assert_eq!(stub.request_count(), n, "search stub call count");
}

#[then(expr = "搜索后端收到的 query 含 {string}")]
async fn then_search_query_contains(_world: &mut TestWorld, needle: String) {
    let slot = stub_slot().lock().await;
    let stub = slot.as_ref().expect("search stub not started");
    let q = stub.last_query().expect("no search query recorded");
    assert!(
        q.contains(&needle),
        "query {q:?} does not contain {needle:?}"
    );
}

#[then(expr = "响应 aigw.web_search.status 为 {string}")]
async fn then_marker_status(_world: &mut TestWorld, expected: String) {
    let body = _world.last_body.as_ref().expect("no response body");
    let actual = body
        .get("aigw")
        .and_then(|v| v.get("web_search"))
        .and_then(|v| v.get("status"))
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no aigw.web_search.status in {body}"));
    assert_eq!(actual, expected);
}

#[when(
    expr = "使用 key {string} 发送仅含图片消息的 web_search_options POST \\/chat\\/completions 请求用 model {string}"
)]
async fn when_chat_image_only_with_search(world: &mut TestWorld, alias: String, model: String) {
    run_request(
        world,
        "/chat/completions",
        &alias,
        json!({
            "model": model,
            "messages": [{"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgo="}}
            ]}],
            "web_search_options": {}
        }),
    )
    .await;
}

#[then(expr = "mock 上游收到的请求 body 含 {string}")]
async fn then_upstream_body_contains(_world: &mut TestWorld, needle: String) {
    let mu = crate::bdd_steps::e2e_steps::mock_upstream().lock().await;
    let upstream = mu.as_ref().expect("mock upstream not started");
    let reqs = upstream.recorded_requests();
    let last = reqs.last().expect("no upstream request recorded");
    let s = last.body.to_string();
    assert!(s.contains(&needle), "upstream body missing {needle:?}: {s}");
}

#[then(expr = "mock 上游收到的请求 body 不含 {string}")]
async fn then_upstream_body_excludes(_world: &mut TestWorld, needle: String) {
    let mu = crate::bdd_steps::e2e_steps::mock_upstream().lock().await;
    let upstream = mu.as_ref().expect("mock upstream not started");
    let reqs = upstream.recorded_requests();
    let last = reqs.last().expect("no upstream request recorded");
    let s = last.body.to_string();
    assert!(
        !s.contains(&needle),
        "upstream body must not contain {needle:?}: {s}"
    );
}

#[then(expr = "mock 上游收到的 tools 不含 {string}")]
async fn then_upstream_tools_excludes(_world: &mut TestWorld, needle: String) {
    let mu = crate::bdd_steps::e2e_steps::mock_upstream().lock().await;
    let upstream = mu.as_ref().expect("mock upstream not started");
    let reqs = upstream.recorded_requests();
    let last = reqs.last().expect("no upstream request recorded");
    let tools = last.body.get("tools").cloned().unwrap_or(json!([]));
    let s = tools.to_string();
    assert!(
        !s.contains(&needle),
        "upstream tools must not contain {needle:?}: {s}"
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then — Stage 136 search billing
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "spend_logs 中存在 call_type=\"search\" 的行")]
async fn then_search_row_exists(world: &mut TestWorld) {
    let rows = search_rows(world).await;
    assert!(!rows.is_empty(), "no call_type=search row written");
}

#[then(expr = "搜索行的 spend 为 {float}")]
async fn then_search_row_spend(world: &mut TestWorld, expected: f64) {
    let rows = search_rows(world).await;
    let last = rows.last().expect("no search row");
    assert!(
        (last.spend - expected).abs() < 1e-9,
        "search spend {} != {expected}",
        last.spend
    );
}

#[then(expr = "搜索行的 token 列全为 0")]
async fn then_search_row_tokens_zero(world: &mut TestWorld) {
    let rows = search_rows(world).await;
    let last = rows.last().expect("no search row");
    assert_eq!(
        (
            last.prompt_tokens,
            last.completion_tokens,
            last.total_tokens
        ),
        (0, 0, 0),
        "search rows must not carry token usage"
    );
}

#[then(expr = "搜索行的 model 为 {string}")]
async fn then_search_row_model(world: &mut TestWorld, expected: String) {
    let rows = search_rows(world).await;
    let last = rows.last().expect("no search row");
    assert_eq!(last.model, expected);
}

#[then(expr = "搜索行的 api_base 非空")]
async fn then_search_row_api_base_present(world: &mut TestWorld) {
    let rows = search_rows(world).await;
    let last = rows.last().expect("no search row");
    assert!(
        last.api_base.as_deref().is_some_and(|b| !b.is_empty()),
        "search row lost the serving instance's base_url"
    );
}

#[then(expr = "搜索行的 metadata.parent_call_id 与同一请求的模型行相同")]
async fn then_search_parent_linkage(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let logs = state
        .db
        .query_spend_logs(None, Some(200))
        .await
        .expect("query spend logs");
    let search = logs
        .iter()
        .rev()
        .find(|l| l.call_type == "search")
        .expect("no search row");
    let parent_id = search
        .metadata
        .as_ref()
        .and_then(|m| m.get("parent_call_id"))
        .and_then(|v| v.as_str())
        .expect("search row has no metadata.parent_call_id");
    let parent = logs
        .iter()
        .find(|l| l.call_id == parent_id)
        .expect("parent call_id does not resolve to a spend_logs row");
    assert_ne!(parent.call_type, "search", "parent must be the LLM row");
}

#[then(expr = "key {string} 的累计 spend 含搜索费")]
async fn then_key_spend_includes_search(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let search_total: f64 = search_rows(world).await.iter().map(|r| r.spend).sum();
    assert!(search_total > 0.0, "no search spend recorded to verify");
    let raw = world
        .created_keys
        .get(&alias)
        .unwrap_or_else(|| panic!("key '{alias}' not found"))
        .clone();
    let key = state
        .db
        .get_key_by_token(&aigw_core::crypto::hash_token(&raw))
        .await
        .expect("query key")
        .expect("key row missing");
    assert!(
        key.spend >= search_total - 1e-9,
        "key spend {} is below the {search_total} of search cost recorded — the \
         four-level increment did not run",
        key.spend
    );
}

#[then(expr = "响应 usage.server_tool_use.web_search_requests 为 {int}")]
async fn then_usage_echoes_search_count(world: &mut TestWorld, expected: i64) {
    let body = world.last_body.as_ref().expect("no response body");
    let actual = body
        .get("usage")
        .and_then(|u| u.get("server_tool_use"))
        .and_then(|s| s.get("web_search_requests"))
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| panic!("no usage.server_tool_use.web_search_requests in {body}"));
    assert_eq!(actual, expected);
}
