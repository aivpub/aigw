//! Step bindings for end_to_end.feature

use axum::http::Method;
use cucumber::{given, then, when};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::bdd_support::mock_upstream::MockUpstream;
use crate::TestWorld;

/// Global mock upstream — lives for the test lifetime
static MOCK_UPSTREAM: std::sync::OnceLock<Arc<Mutex<Option<MockUpstream>>>> =
    std::sync::OnceLock::new();

pub fn mock_upstream() -> &'static Arc<Mutex<Option<MockUpstream>>> {
    MOCK_UPSTREAM.get_or_init(|| Arc::new(Mutex::new(None)))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "mock 上游已启动")]
async fn given_mock_upstream_started(_world: &mut TestWorld) {
    let mu = mock_upstream();
    let mut guard = mu.lock().await;
    if guard.is_none() {
        let upstream = MockUpstream::start().await;
        // Set the UPSTREAM_LLM_URL env so the handler uses our mock
        std::env::set_var("UPSTREAM_LLM_URL", format!("{}/v1", upstream.url()));
        *guard = Some(upstream);
    } else {
        // Reset mock responses to defaults for scenario isolation
        guard.as_mut().unwrap().reset_responses();
    }
}

#[given(expr = "已配置 model {string} 且上游 model 为 {string} 指向 mock 上游")]
async fn given_model_with_upstream_model_points_to_mock(
    world: &mut TestWorld,
    proxy_name: String,
    upstream_model: String,
) {
    let state = world.ensure_state().await;
    let mu = mock_upstream().lock().await;
    let mock_base = mu
        .as_ref()
        .expect("mock upstream not started")
        .url()
        .to_string();

    let model = aigw_core::models::ProxyModel {
        model_id: uuid::Uuid::new_v4().to_string(),
        model_name: proxy_name.clone(),
        litellm_params: serde_json::json!({
            "model": upstream_model,
            "api_base": format!("{mock_base}/v1")
        }),
        model_info: serde_json::json!({}),
        created_at: chrono::Utc::now().to_rfc3339(),
        created_by: Some("test".to_string()),
        updated_at: chrono::Utc::now().to_rfc3339(),
        updated_by: Some("test".to_string()),
    };
    state.db.insert_model(&model).await.expect("insert model");
}

#[given(expr = "已配置 model {string} 指向 mock 上游")]
async fn given_model_points_to_mock(world: &mut TestWorld, name: String) {
    let state = world.ensure_state().await;
    let mu = mock_upstream().lock().await;
    let mock_base = mu
        .as_ref()
        .expect("mock upstream not started")
        .url()
        .to_string();

    let model = aigw_core::models::ProxyModel {
        model_id: uuid::Uuid::new_v4().to_string(),
        model_name: name.clone(),
        litellm_params: serde_json::json!({
            "model": name,
            "api_base": format!("{mock_base}/v1")
        }),
        model_info: serde_json::json!({}),
        created_at: chrono::Utc::now().to_rfc3339(),
        created_by: Some("test".to_string()),
        updated_at: chrono::Utc::now().to_rfc3339(),
        updated_by: Some("test".to_string()),
    };
    state.db.insert_model(&model).await.expect("insert model");
}

#[given(expr = "mock 上游 {string} 返回状态码 {int}")]
async fn given_mock_returns_status(_world: &mut TestWorld, path: String, status: u16) {
    let mu = mock_upstream().lock().await;
    let upstream = mu.as_ref().expect("mock upstream not started");
    upstream.set_response(
        &path,
        status,
        serde_json::json!({"error": {"message": "mock error", "type": "server_error"}}),
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 key {string} 发送 POST \\/chat\\/completions 请求")]
async fn when_post_chat_completions(world: &mut TestWorld, alias: String) {
    when_post_chat_completions_with_model(world, alias, "gpt-4-mock".to_string()).await;
}

#[when(expr = "使用 key {string} 发送 POST \\/chat\\/completions 请求用 model {string}")]
async fn when_post_chat_completions_model(world: &mut TestWorld, alias: String, model: String) {
    when_post_chat_completions_with_model(world, alias, model).await;
}

async fn when_post_chat_completions_with_model(
    world: &mut TestWorld,
    alias: String,
    model: String,
) {
    let state = world.ensure_state().await;
    use axum::Router;
    use tower::util::ServiceExt;

    let app = Router::new()
        .route(
            "/chat/completions",
            axum::routing::post(aigw_server::routes::chat::chat_completions),
        )
        .with_state(state);

    let token = world.created_keys.get(&alias).expect("key not found");
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "hi"}]
    })
    .to_string();

    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/chat/completions")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .body(axum::body::Body::from(body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let json_body: Option<serde_json::Value> = serde_json::from_slice(&body_bytes).ok();
    world.last_status = Some(status);
    world.last_body = json_body;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When — Stage 103 multimodal image request
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Send a /chat/completions request whose user message carries an OpenAI
/// multimodal content array (`image_url` with a data URL). Verifies the gateway
/// (OpenAIPassthrough) forwards the image parts unchanged to an OpenAI-compatible
/// upstream.
#[when(expr = "使用 key {string} 发送带图片的 POST \\/chat\\/completions 请求用 model {string}")]
async fn when_post_chat_completions_with_image(
    world: &mut TestWorld,
    alias: String,
    model: String,
) {
    let state = world.ensure_state().await;
    use axum::Router;
    use tower::util::ServiceExt;

    let app = Router::new()
        .route(
            "/chat/completions",
            axum::routing::post(aigw_server::routes::chat::chat_completions),
        )
        .with_state(state);

    let token = world.created_keys.get(&alias).expect("key not found");
    let body = serde_json::json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "what is in this image?"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgo="}}
            ]
        }]
    })
    .to_string();

    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/chat/completions")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .body(axum::body::Body::from(body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let json_body: Option<serde_json::Value> = serde_json::from_slice(&body_bytes).ok();
    world.last_status = Some(status);
    world.last_body = json_body;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then — Stage 103 multimodal assertions
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// The mock upstream's recorded request body must preserve the OpenAI
/// multimodal content array (image_url parts) — the gateway forwards verbatim.
#[then(expr = "mock 上游收到的请求 body 保留 image_url 图片 parts")]
async fn then_mock_received_image_parts(_world: &mut TestWorld) {
    let mu = mock_upstream().lock().await;
    let upstream = mu.as_ref().expect("mock upstream not started");
    let requests = upstream.recorded_requests();
    let req = requests.last().expect("no recorded request");
    let messages = req
        .body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("no messages in upstream body");
    let last = messages.last().expect("no user message");
    let content = last
        .get("content")
        .and_then(|v| v.as_array())
        .expect("content should be an array (multimodal)");
    let image = content
        .iter()
        .find(|p| p.get("type") == Some(&serde_json::json!("image_url")))
        .expect("no image_url part in forwarded content");
    assert_eq!(
        image["image_url"]["url"].as_str(),
        Some("data:image/png;base64,iVBORw0KGgo="),
        "gateway must forward the image_url data URL verbatim"
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "mock 上游收到请求")]
async fn then_mock_received_request(_world: &mut TestWorld) {
    let mu = mock_upstream().lock().await;
    let upstream = mu.as_ref().expect("mock upstream not started");
    let count = upstream.request_count();
    assert!(
        count > 0,
        "Expected mock upstream to receive at least 1 request, got 0"
    );
}

#[then(expr = "响应状态码为 500 或 502")]
async fn then_status_500_or_502(world: &mut TestWorld) {
    let status = world.last_status.expect("no status");
    assert!(
        status == 500 || status == 502,
        "Expected status 500 or 502, got {}",
        status
    );
}

#[then(expr = "响应状态码为 500 或 502 或 503")]
async fn then_status_500_502_503(world: &mut TestWorld) {
    let status = world.last_status.expect("no status");
    assert!(
        status == 500 || status == 502 || status == 503,
        "Expected status 500/502/503, got {}",
        status
    );
}

#[then(expr = "mock 上游收到路径为 {string} 的请求")]
async fn then_mock_received_path(_world: &mut TestWorld, expected_path: String) {
    let mu = mock_upstream().lock().await;
    let upstream = mu.as_ref().expect("mock upstream not started");
    let requests = upstream.recorded_requests();
    let found = requests.iter().any(|r| r.path == expected_path);
    assert!(
        found,
        "Expected mock upstream to receive request to '{}', but got: {:?}",
        expected_path,
        requests.iter().map(|r| &r.path).collect::<Vec<_>>()
    );
}

/// Check spend_logs after a successful API call — verify model field
/// records the upstream model name, not the proxy name.
#[then(expr = "spend_logs 中 model 字段值为 {string}")]
async fn then_spend_logs_model_is(world: &mut TestWorld, expected_model: String) {
    let state = world.ensure_state().await;
    let logs = state
        .db
        .query_spend_logs(None, Some(1))
        .await
        .expect("query spend logs");
    assert!(!logs.is_empty(), "Expected at least one spend_log record");
    let log = &logs[0];
    assert_eq!(
        log.model, expected_model,
        "Expected spend_logs.model='{}', got '{}'",
        expected_model, log.model
    );
}
