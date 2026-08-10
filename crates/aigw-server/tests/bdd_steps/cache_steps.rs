//! Step bindings for cache.feature — Stage 119 exact-match response cache

use cucumber::{given, then, when};
use serde_json::json;

use crate::TestWorld;

// ━━━━ When ━━━━

/// Send a chat request that carries the `cache` request field (OpenAI cache
/// extension). Reuses the shared chat body builder with a `cache` block.
#[when(
    expr = "使用 key {string} 发送 POST \\/chat\\/completions 请求用 model {string} 带 cache no-store"
)]
async fn when_post_chat_with_cache_no_store(world: &mut TestWorld, alias: String, model: String) {
    let state = world.ensure_state().await;
    use axum::http::Method;
    use axum::Router;
    use tower::util::ServiceExt;

    let app = Router::new()
        .route(
            "/chat/completions",
            axum::routing::post(aigw_server::routes::chat::chat_completions),
        )
        // Fresh UUID-v7 call_id per request (else spend_logs.call_id UNIQUE
        // collision on repeated cache requests).
        .layer(tower_http::request_id::PropagateRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-call-id"),
        ))
        .layer(tower_http::request_id::SetRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-request-id"),
            aigw_core::request_id::UuidV7RequestId,
        ))
        .with_state(state);

    let token = world.created_keys.get(&alias).expect("key not found");
    let body = json!({
        "model": model,
        "messages": [{"role": "user", "content": "hi"}],
        "cache": {"no-store": true}
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
    let resp_headers = response.headers().clone();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let json_body: Option<serde_json::Value> = serde_json::from_slice(&body_bytes).ok();
    world.last_status = Some(status);
    world.last_body = json_body;
    world.last_headers = Some(resp_headers);
}

// ━━━━ Then ━━━━

#[then(regex = r#"^响应头包含 X-Cache-Status "(.+)"$"#)]
async fn then_x_cache_status(world: &mut TestWorld, expected: String) {
    let headers = world
        .last_headers
        .as_ref()
        .expect("no captured response headers");
    let actual = headers
        .get("X-Cache-Status")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("(missing)");
    assert_eq!(
        actual, expected,
        "Expected X-Cache-Status={}, got {}",
        expected, actual
    );
}

#[then(expr = "SpendLog 中最近一条记录 spend 为 0")]
async fn then_last_spendlog_spend_zero(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let logs = state
        .db
        .query_spend_logs(None, Some(1))
        .await
        .expect("query spend logs");
    let log = logs.first().expect("no spend log");
    assert_eq!(
        log.spend, 0.0,
        "cache-hit SpendLog must have zero cost, got {}",
        log.spend
    );
}

#[then(expr = "SpendLog 中最近一条记录 cached 为 1")]
async fn then_last_spendlog_cached(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let logs = state
        .db
        .query_spend_logs(None, Some(1))
        .await
        .expect("query spend logs");
    let log = logs.first().expect("no spend log");
    let cached = log
        .metadata
        .as_ref()
        .and_then(|m| m.get("cached"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    assert_eq!(
        cached, 1,
        "cache-hit SpendLog metadata.cached must be 1; got metadata={:?} cache_hit={:?}",
        log.metadata, log.cache_hit
    );
}
