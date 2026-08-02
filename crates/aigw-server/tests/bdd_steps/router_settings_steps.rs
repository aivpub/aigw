//! Step bindings for router_settings BDD scenarios (auth.feature)

use axum::http::Method;
use cucumber::when;
use tower::ServiceExt;

use crate::TestWorld;

/// Build an axum Router for router_settings endpoints
fn build_router_settings_router(state: aigw_server::routes::keys::SharedState) -> axum::Router {
    axum::Router::new()
        .route(
            "/router/settings",
            axum::routing::get(aigw_server::routes::router_settings::get_global)
                .put(aigw_server::routes::router_settings::put_global),
        )
        .route(
            "/key/{token}/router/settings",
            axum::routing::patch(aigw_server::routes::router_settings::patch_key),
        )
        .route(
            "/team/{id}/router/settings",
            axum::routing::patch(aigw_server::routes::router_settings::patch_team),
        )
        .with_state(state)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When: router/settings no-auth requests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "不携带 Authorization 发送 GET \\/router\\/settings 请求")]
async fn when_get_router_settings_noauth(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_router_settings_router(state);
    let req = axum::http::Request::builder()
        .method(Method::GET)
        .uri("/router/settings")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When: use key to call router/settings endpoints
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 key {string} 发送 PUT \\/router\\/settings 请求")]
async fn when_key_put_router_settings(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let app = build_router_settings_router(state);
    let token = world.created_keys.get(&alias).expect("key not found");
    let req = axum::http::Request::builder()
        .method(Method::PUT)
        .uri("/router/settings")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .body(axum::body::Body::from(
            r#"{"routing_strategy":"least_latency"}"#,
        ))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

#[when(expr = "使用 master-key 发送 GET \\/router\\/settings 请求")]
async fn when_master_get_router_settings(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_router_settings_router(state);
    let mk = world.master_key.clone();
    let req = axum::http::Request::builder()
        .method(Method::GET)
        .uri("/router/settings")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", mk))
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

#[when(expr = "使用 master-key 带有效 body 发送 PUT \\/router\\/settings 请求")]
async fn when_master_put_router_settings(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_router_settings_router(state);
    let mk = world.master_key.clone();
    let req = axum::http::Request::builder()
        .method(Method::PUT)
        .uri("/router/settings")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", mk))
        .body(axum::body::Body::from(
            r#"{"routing_strategy":"least_latency","num_retries":2}"#,
        ))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}
