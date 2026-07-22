//! Common BDD helpers — axum Router builder and HTTP request utilities

use aigw_server::routes::keys::SharedState;
use axum::{
    body::Body,
    http::{header, Method, Request},
    Router,
};
use tower::util::ServiceExt;

/// Build an axum Router with the given SharedState and all key routes
pub fn build_key_router(state: SharedState) -> Router {
    Router::new()
        .route(
            "/key/generate",
            axum::routing::post(aigw_server::routes::keys::generate_key),
        )
        .route(
            "/key/info",
            axum::routing::get(aigw_server::routes::keys::key_info),
        )
        .route(
            "/key/list",
            axum::routing::get(aigw_server::routes::keys::key_list),
        )
        .route(
            "/key/update",
            axum::routing::put(aigw_server::routes::keys::key_update),
        )
        .route(
            "/key/delete",
            axum::routing::delete(aigw_server::routes::keys::key_delete),
        )
        .route(
            "/key/regenerate",
            axum::routing::post(aigw_server::routes::keys::key_regenerate),
        )
        .with_state(state)
}

/// Build an axum Router for health check routes
pub fn build_health_router(state: SharedState) -> Router {
    Router::new()
        .route("/health", axum::routing::get(aigw_server::routes::health::health))
        .route(
            "/health/readiness",
            axum::routing::get(aigw_server::routes::health::readiness),
        )
        .route(
            "/health/liveliness",
            axum::routing::get(aigw_server::routes::health::liveliness),
        )
        .with_state(state)
}

/// Build an axum Router for spend routes with the given SharedState
pub fn build_spend_router(state: SharedState) -> Router {
    Router::new()
        .route(
            "/spend/logs",
            axum::routing::get(aigw_server::routes::spend::spend_logs),
        )
        .route(
            "/spend/keys",
            axum::routing::get(aigw_server::routes::spend::spend_keys),
        )
        .route(
            "/spend/users",
            axum::routing::get(aigw_server::routes::spend::spend_users),
        )
        .route(
            "/spend/tags",
            axum::routing::get(aigw_server::routes::spend::spend_tags),
        )
        .route(
            "/global/spend",
            axum::routing::get(aigw_server::routes::spend::global_spend),
        )
        .route(
            "/global/spend/logs",
            axum::routing::get(aigw_server::routes::spend::global_spend_logs),
        )
        .route(
            "/global/spend/keys",
            axum::routing::get(aigw_server::routes::spend::global_spend_keys),
        )
        .route(
            "/spend/models",
            axum::routing::get(aigw_server::routes::spend::spend_models),
        )
        .route(
            "/spend/providers",
            axum::routing::get(aigw_server::routes::spend::spend_providers),
        )
        .route(
            "/global/spend/models",
            axum::routing::get(aigw_server::routes::spend::global_spend_models),
        )
        .route(
            "/global/spend/providers",
            axum::routing::get(aigw_server::routes::spend::global_spend_providers),
        )
        .route(
            "/global/spend/keys/rankings",
            axum::routing::get(aigw_server::routes::spend::global_spend_keys_rankings),
        )
        .route(
            "/global/spend/activity",
            axum::routing::get(aigw_server::routes::spend::global_spend_activity),
        )
        .with_state(state)
}

/// Helper: send an HTTP request and return (status, json_body)
pub async fn make_request(
    app: &Router,
    method: Method,
    uri: &str,
    auth: Option<&str>,
    body: Option<&str>,
) -> (u16, Option<serde_json::Value>) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");

    if let Some(token) = auth {
        req = req.header(header::AUTHORIZATION, format!("Bearer {}", token));
    }

    let req_body = body
        .map(|b| Body::from(b.to_string()))
        .unwrap_or(Body::empty());

    let request = req.body(req_body).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status().as_u16();

    let json_body = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
        Ok(bytes) => serde_json::from_slice(&bytes).ok(),
        Err(_) => None,
    };

    (status, json_body)
}
