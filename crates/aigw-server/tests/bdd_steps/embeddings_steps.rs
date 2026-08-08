//! Step bindings for embeddings.feature
//!
//! Given steps ("mock upstream", "model pointing to mock", "key generated") are
//! reused from e2e_steps.rs / spend_steps.rs / model_steps.rs. This module only
//! defines the Embeddings-specific When steps and the shared SpendLog
//! assertions (re-exported pattern from responses_steps.rs).

use axum::http::Method;
use cucumber::{then, when};
use tower::util::ServiceExt;

use crate::TestWorld;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn send_embeddings_request(
    world: &mut TestWorld,
    alias: &str,
    uri: &str,
    body: serde_json::Value,
) {
    let state = world.ensure_state().await;
    use axum::Router;

    let app = Router::new()
        .route(
            "/v1/embeddings",
            axum::routing::post(aigw_server::routes::embeddings::embeddings_handler),
        )
        .route(
            "/embeddings",
            axum::routing::post(aigw_server::routes::embeddings::embeddings_handler),
        )
        .route(
            "/engines/{model}/embeddings",
            axum::routing::post(aigw_server::routes::embeddings::embeddings_handler_with_path),
        )
        .route(
            "/openai/deployments/{model}/embeddings",
            axum::routing::post(aigw_server::routes::embeddings::embeddings_handler_with_path),
        )
        .with_state(state);

    let token = world
        .created_keys
        .get(alias)
        .unwrap_or_else(|| panic!("key '{}' not found", alias));

    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .body(axum::body::Body::from(body.to_string()))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let resp_headers = response.headers().clone();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();

    let is_json = resp_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.starts_with("application/json"))
        .unwrap_or(false);

    let json_body: Option<serde_json::Value> = if is_json {
        serde_json::from_slice(&body_bytes).ok()
    } else {
        None
    };
    world.last_status = Some(status);
    world.last_body = json_body;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 key {string} 发送 POST \\/v1\\/embeddings 请求")]
async fn when_post_embeddings(world: &mut TestWorld, alias: String) {
    send_embeddings_request(
        world,
        &alias,
        "/v1/embeddings",
        serde_json::json!({
            "model": "text-embedding-3-small",
            "input": "hello from BDD"
        }),
    )
    .await;
}

#[when(expr = "使用 key {string} 发送 input=\"hello\" 的 \\/v1\\/embeddings 请求")]
async fn when_post_embeddings_input_string(world: &mut TestWorld, alias: String) {
    send_embeddings_request(
        world,
        &alias,
        "/v1/embeddings",
        serde_json::json!({
            "model": "text-embedding-3-small",
            "input": "hello"
        }),
    )
    .await;
}

#[when(expr = "使用 key {string} 发送数组 input 的 \\/v1\\/embeddings 请求")]
async fn when_post_embeddings_input_array(world: &mut TestWorld, alias: String) {
    send_embeddings_request(
        world,
        &alias,
        "/v1/embeddings",
        serde_json::json!({
            "model": "text-embedding-3-small",
            "input": ["a", "b"]
        }),
    )
    .await;
}

#[when(expr = "使用 key {string} 发送 POST \\/embeddings 请求")]
async fn when_post_embeddings_unversioned(world: &mut TestWorld, alias: String) {
    send_embeddings_request(
        world,
        &alias,
        "/embeddings",
        serde_json::json!({
            "model": "text-embedding-3-small",
            "input": "hello from alias"
        }),
    )
    .await;
}

#[when(regex = r#"^使用 key "(.+)" 发送 POST /engines/(.+)/embeddings 请求$"#)]
async fn when_post_embeddings_engine(world: &mut TestWorld, alias: String, model: String) {
    let uri = format!("/engines/{}/embeddings", model);
    send_embeddings_request(
        world,
        &alias,
        &uri,
        serde_json::json!({"input": "hello from engine"}),
    )
    .await;
}

#[when(regex = r#"^使用 key "(.+)" 发送 POST /openai/deployments/(.+)/embeddings 请求$"#)]
async fn when_post_embeddings_deployment(world: &mut TestWorld, alias: String, model: String) {
    let uri = format!("/openai/deployments/{}/embeddings", model);
    send_embeddings_request(
        world,
        &alias,
        &uri,
        serde_json::json!({"input": "hello from deployment"}),
    )
    .await;
}

#[when(expr = "使用 key {string} 发送 POST \\/v1\\/embeddings 请求不带 model")]
async fn when_post_embeddings_no_model(world: &mut TestWorld, alias: String) {
    send_embeddings_request(
        world,
        &alias,
        "/v1/embeddings",
        serde_json::json!({"input": "test"}),
    )
    .await;
}

#[when(expr = "使用 key {string} 发送 POST \\/v1\\/embeddings 请求不带 input")]
async fn when_post_embeddings_no_input(world: &mut TestWorld, alias: String) {
    send_embeddings_request(
        world,
        &alias,
        "/v1/embeddings",
        serde_json::json!({"model": "text-embedding-3-small"}),
    )
    .await;
}

#[when(expr = "使用 key {string} 发送 POST \\/v1\\/embeddings 请求带空 input")]
async fn when_post_embeddings_empty_input(world: &mut TestWorld, alias: String) {
    send_embeddings_request(
        world,
        &alias,
        "/v1/embeddings",
        serde_json::json!({"model": "text-embedding-3-small", "input": []}),
    )
    .await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then — SpendLog assertions (mirrors responses_steps.rs)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn get_latest_spend_log(world: &mut TestWorld) -> aigw_core::models::SpendLog {
    let state = world.ensure_state().await;
    let logs = state
        .db
        .query_spend_logs(None, Some(1))
        .await
        .expect("query spend logs");
    logs.into_iter().next().expect("no spend log found")
}

#[then(expr = "SpendLog 中最近一条记录的 call_type 为 {string}")]
async fn then_spendlog_call_type(world: &mut TestWorld, expected: String) {
    let log = get_latest_spend_log(world).await;
    assert_eq!(
        log.call_type, expected,
        "Expected spend_logs.call_type='{}', got '{}'",
        expected, log.call_type
    );
}

// NOTE: "SpendLog 中最近一条记录的 prompt_tokens 大于 0" is defined in
// responses_steps.rs and shared here — do NOT redefine (ambiguous match).

#[then(expr = "SpendLog 中最近一条记录的 completion_tokens 为 0")]
async fn then_spendlog_completion_tokens(world: &mut TestWorld) {
    let log = get_latest_spend_log(world).await;
    assert_eq!(
        log.completion_tokens, 0,
        "completion_tokens should be 0 for embeddings, got {}",
        log.completion_tokens
    );
}

#[then(expr = "SpendLog 中最近一条记录的 total_tokens 大于 0")]
async fn then_spendlog_total_tokens_positive(world: &mut TestWorld) {
    let log = get_latest_spend_log(world).await;
    assert!(
        log.total_tokens > 0,
        "total_tokens should be > 0, got {}",
        log.total_tokens
    );
}
