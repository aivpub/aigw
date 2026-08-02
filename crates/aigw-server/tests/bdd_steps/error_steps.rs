//! Step bindings for error_handling.feature and auth.feature

use axum::http::Method;
use cucumber::when;

use crate::TestWorld;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Error-handling chat request variants
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 key {string} 发送 POST \\/chat\\/completions 缺少 model")]
async fn when_chat_missing_model(world: &mut TestWorld, alias: String) {
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

#[when(expr = "使用 key {string} 发送 POST \\/chat\\/completions 缺少 messages")]
async fn when_chat_missing_messages(world: &mut TestWorld, alias: String) {
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
        "model": "gpt-4"
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

#[when(expr = "使用 key {string} 发送 POST \\/chat\\/completions messages 为空")]
async fn when_chat_empty_messages(world: &mut TestWorld, alias: String) {
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
        "model": "gpt-4",
        "messages": []
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

#[when(expr = "使用 key {string} 发送 POST \\/chat\\/completions 无效 JSON")]
async fn when_chat_invalid_json(world: &mut TestWorld, alias: String) {
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
    let body = "this is not valid json".to_string();

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
// Auth step bindings
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 invalid key 发送 GET \\/key\\/list 请求")]
async fn when_invalid_key_list(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = super::common::build_key_router(state);
    let (status, body) = super::common::make_request(
        &router,
        Method::GET,
        "/key/list",
        Some("invalid-key-12345"),
        None,
    )
    .await;
    world.last_status = Some(status);
    world.last_body = body;
}

#[when(expr = "使用 invalid key 发送 GET \\/key\\/info 请求")]
async fn when_invalid_key_info(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = super::common::build_key_router(state);
    let (status, body) = super::common::make_request(
        &router,
        Method::GET,
        "/key/info",
        Some("invalid-key-12345"),
        None,
    )
    .await;
    world.last_status = Some(status);
    world.last_body = body;
}

#[when(expr = "不携带 Authorization 发送 GET \\/key\\/list 请求")]
async fn when_no_auth_list(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = super::common::build_key_router(state);
    let (status, body) =
        super::common::make_request(&router, Method::GET, "/key/list", None, None).await;
    world.last_status = Some(status);
    world.last_body = body;
}

#[when(expr = "不携带 Authorization 发送 GET \\/key\\/info 请求")]
async fn when_no_auth_info(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = super::common::build_key_router(state);
    let (status, body) =
        super::common::make_request(&router, Method::GET, "/key/info", None, None).await;
    world.last_status = Some(status);
    world.last_body = body;
}

#[when(expr = "使用 key {string} 发送 GET \\/key\\/list 请求")]
async fn when_key_list_get(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let token = world.created_keys.get(&alias).expect("key not found");
    let router = super::common::build_key_router(state);
    let (status, body) =
        super::common::make_request(&router, Method::GET, "/key/list", Some(token), None).await;
    world.last_status = Some(status);
    world.last_body = body;
}

#[when(expr = "使用 key {string} 发送 GET \\/key\\/info 请求")]
async fn when_key_info_get(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let token = world.created_keys.get(&alias).expect("key not found");
    let router = super::common::build_key_router(state);
    let (status, body) =
        super::common::make_request(&router, Method::GET, "/key/info", Some(token), None).await;
    world.last_status = Some(status);
    world.last_body = body;
}

#[when(expr = "使用 master-key 发送 GET \\/key\\/list 请求")]
async fn when_master_key_list_get(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = super::common::build_key_router(state);
    let (status, body) = super::common::make_request(
        &router,
        Method::GET,
        "/key/list",
        Some(&world.master_key),
        None,
    )
    .await;
    world.last_status = Some(status);
    world.last_body = body;
}
