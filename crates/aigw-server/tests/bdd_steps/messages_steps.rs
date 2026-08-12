//! Step bindings for messages.feature

use crate::TestWorld;
use aigw_core::models::ProxyModel;
use axum::http::Method;
use axum::Router;
use cucumber::gherkin::Step;
use cucumber::{given, then, when};
use tower::util::ServiceExt;

/// Build a router with /v1/messages route only
fn build_messages_router(state: aigw_server::routes::keys::SharedState) -> Router {
    Router::new()
        .route(
            "/v1/messages",
            axum::routing::post(aigw_server::routes::v1_messages::messages_handler),
        )
        .with_state(state)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "已配置 model {string} 在数据库中")]
async fn given_model_in_db(world: &mut TestWorld, name: String) {
    let state = world.ensure_state().await;
    let model = ProxyModel {
        model_id: uuid::Uuid::new_v4().to_string(),
        model_name: name.clone(),
        litellm_params: serde_json::json!({"model": name, "api_base": "http://localhost:9999"}),
        model_info: serde_json::json!({}),
        created_at: chrono::Utc::now().to_rfc3339(),
        created_by: Some("test".to_string()),
        updated_at: chrono::Utc::now().to_rfc3339(),
        updated_by: Some("test".to_string()),
        enabled: true,
    };
    state.db.insert_model(&model).await.expect("insert model");
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "发送 POST \\/v1\\/messages 请求未带 anthropic-version header")]
async fn when_post_messages_no_version(world: &mut TestWorld, step: &Step) {
    let state = world.ensure_state().await;
    let router = build_messages_router(state);
    let body = step.docstring.as_ref().expect("docstring body").to_string();

    let mk = world.master_key.clone();
    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/v1/messages")
        .header("Content-Type", "application/json")
        .header("x-api-key", &mk)
        .body(axum::body::Body::from(body))
        .unwrap();

    let response = router.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let json_body: Option<serde_json::Value> = serde_json::from_slice(&body_bytes).ok();
    world.last_status = Some(status);
    world.last_body = json_body;
}

#[when(expr = "发送 POST \\/v1\\/messages 请求")]
async fn when_post_messages(world: &mut TestWorld, step: &Step) {
    let state = world.ensure_state().await;
    let router = build_messages_router(state);
    let body = step.docstring.as_ref().expect("docstring body").to_string();

    let mk = world.master_key.clone();
    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/v1/messages")
        .header("Content-Type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .header("x-api-key", &mk)
        .body(axum::body::Body::from(body))
        .unwrap();

    let response = router.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let json_body: Option<serde_json::Value> = serde_json::from_slice(&body_bytes).ok();
    world.last_status = Some(status);
    world.last_body = json_body;
}

#[when(expr = "发送 POST \\/v1\\/messages 请求未带认证")]
async fn when_post_messages_noauth(world: &mut TestWorld, step: &Step) {
    let state = world.ensure_state().await;
    let router = build_messages_router(state);
    let body = step.docstring.as_ref().expect("docstring body").to_string();

    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/v1/messages")
        .header("Content-Type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .body(axum::body::Body::from(body))
        .unwrap();

    let response = router.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let json_body: Option<serde_json::Value> = serde_json::from_slice(&body_bytes).ok();
    world.last_status = Some(status);
    world.last_body = json_body;
}

#[when(expr = "发送 POST \\/v1\\/messages 请求带 x-api-key 认证")]
async fn when_post_messages_xapikey(world: &mut TestWorld, step: &Step) {
    let state = world.ensure_state().await;
    let router = build_messages_router(state);
    let body = step.docstring.as_ref().expect("docstring body").to_string();

    let mk = world.master_key.clone();
    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/v1/messages")
        .header("Content-Type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .header("x-api-key", &mk)
        .body(axum::body::Body::from(body))
        .unwrap();

    let response = router.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let json_body: Option<serde_json::Value> = serde_json::from_slice(&body_bytes).ok();
    world.last_status = Some(status);
    world.last_body = json_body;
}

#[when(expr = "发送 POST \\/v1\\/messages 请求带 Bearer 认证")]
async fn when_post_messages_bearer(world: &mut TestWorld, step: &Step) {
    let state = world.ensure_state().await;
    let router = build_messages_router(state);
    let body = step.docstring.as_ref().expect("docstring body").to_string();

    let mk = world.master_key.clone();
    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/v1/messages")
        .header("Content-Type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .header("Authorization", format!("Bearer {}", mk))
        .body(axum::body::Body::from(body))
        .unwrap();

    let response = router.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let json_body: Option<serde_json::Value> = serde_json::from_slice(&body_bytes).ok();
    world.last_status = Some(status);
    world.last_body = json_body;
}

#[when(expr = "发送 POST \\/v1\\/messages 请求带认证 model={string}")]
#[allow(unused_variables)]
async fn when_post_messages_with_model(world: &mut TestWorld, step: &Step, model_name: String) {
    let state = world.ensure_state().await;
    let router = build_messages_router(state);
    let mk = world.master_key.clone();

    let body = serde_json::json!({
        "model": model_name,
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 100
    })
    .to_string();

    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/v1/messages")
        .header("Content-Type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .header("x-api-key", &mk)
        .body(axum::body::Body::from(body))
        .unwrap();

    let response = router.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let json_body: Option<serde_json::Value> = serde_json::from_slice(&body_bytes).ok();
    world.last_status = Some(status);
    world.last_body = json_body;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(regex = "^错误 type 为 \"(.+)\"$")]
async fn then_error_type_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let err_type = body
        .get("error")
        .and_then(|e| e.get("type"))
        .and_then(|v| v.as_str())
        .expect("no error.type in response");
    assert_eq!(
        err_type, expected,
        "Expected error.type '{}', got '{}'",
        expected, err_type
    );
}

#[then(regex = "^错误信息包含 \"(.+)\"$")]
async fn then_error_contains(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let message = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        message.contains(&expected),
        "Expected message to contain '{}', got '{}'",
        expected,
        message
    );
}

#[then(expr = "响应体为 Anthropic 错误格式")]
async fn then_anthropic_error_format(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    assert_eq!(body.get("type").and_then(|v| v.as_str()), Some("error"));
    assert!(body.get("error").and_then(|v| v.get("type")).is_some());
    assert!(body.get("error").and_then(|v| v.get("message")).is_some());
    assert!(body.get("request_id").is_some());
}

#[then(expr = "响应状态码不为 401")]
async fn then_status_not_401(world: &mut TestWorld) {
    let status = world.last_status.expect("no status");
    assert_ne!(status, 401, "Expected status not 401, got {}", status);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 103: /v1/messages with Claude image block → OpenAI upstream
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Send a /v1/messages request whose user message carries a Claude `image`
/// content block. The AnthropicToOpenAI adapter must convert it to OpenAI
/// content-parts (`image_url` with `data:{media_type};base64,{data}`) when
/// forwarding to an OpenAI-compatible upstream.
#[when(expr = "使用 key {string} 发送带图片的 POST \\/v1\\/messages 请求用 model {string}")]
async fn when_post_messages_with_image(world: &mut TestWorld, alias: String, model: String) {
    let state = world.ensure_state().await;
    let router = build_messages_router(state);
    let token = world.created_keys.get(&alias).expect("key not found");
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 256,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "what is in this image?"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="}}
            ]
        }]
    })
    .to_string();

    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/v1/messages")
        .header("Content-Type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .header("x-api-key", token)
        .body(axum::body::Body::from(body))
        .unwrap();

    let response = router.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let json_body: Option<serde_json::Value> = serde_json::from_slice(&body_bytes).ok();
    world.last_status = Some(status);
    world.last_body = json_body;
}

/// The mock upstream's recorded request body must carry the Claude image block
/// converted to OpenAI `image_url` parts (data URL reconstructed).
#[then(expr = "mock 上游收到的 \\/v1\\/messages 请求 body 含 image_url 图片 parts")]
async fn then_mock_received_messages_image_parts(_world: &mut TestWorld) {
    let mu = crate::bdd_steps::e2e_steps::mock_upstream().lock().await;
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
        "AnthropicToOpenAI must reconstruct the data URL (data:{{media_type}};base64,{{data}})"
    );
}
