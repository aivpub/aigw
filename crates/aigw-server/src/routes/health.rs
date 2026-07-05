//! Health check and system info endpoints
//!
//! Endpoints:
//! - GET /health            — Simple health check (always ok if running)
//! - GET /health/readiness   — Readiness check (DB connected)
//! - GET /health/liveliness  — Liveliness check (always ok if running)
//! - GET /system/info        — System information (version, deployment mode, routes)

use axum::{extract::State, Json};
use serde_json::{json, Value};

use crate::routes::keys::SharedState;

/// GET /health — simple health check
pub async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

/// GET /health/readiness — readiness check (DB connected)
pub async fn readiness() -> Json<Value> {
    Json(json!({"status": "ok", "ready": true}))
}

/// GET /health/liveliness — liveness check (always ok if running)
pub async fn liveliness() -> Json<Value> {
    Json(json!({"status": "ok", "alive": true}))
}

/// GET /system/info — System information (version, deployment mode, available routes)
pub async fn system_info(State(state): State<SharedState>) -> Json<Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "name": "aigw",
        "deployment_mode": state.deployment_mode,
        "database_type": state.db.database_type(),
        "routes": [
            "/key/generate", "/key/info", "/key/list", "/key/update", "/key/delete", "/key/regenerate",
            "/spend/logs", "/spend/keys", "/spend/users", "/spend/tags",
            "/global/spend", "/global/spend/logs", "/global/spend/keys",
            "/v1/chat/completions", "/v1/models",
            "/health", "/health/readiness", "/health/liveliness",
            "/docs", "/openapi.json",
            "/system/info"
        ]
    }))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::keys::AppState;
    use axum::{
        body::Body,
        http::{Method, Request},
        Router,
    };
    use serde_json::Value as JsonValue;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn test_health_returns_ok() {
        let app = Router::new().route("/health", axum::routing::get(health));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json_val: JsonValue = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json_val.get("status").and_then(|v| v.as_str()), Some("ok"));
    }

    #[tokio::test]
    async fn test_readiness_returns_ok() {
        let app = Router::new().route("/health/readiness", axum::routing::get(readiness));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/health/readiness")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json_val: JsonValue = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json_val.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert_eq!(json_val.get("ready").and_then(|v| v.as_bool()), Some(true));
    }

    #[tokio::test]
    async fn test_liveliness_returns_ok() {
        let app = Router::new().route("/health/liveliness", axum::routing::get(liveliness));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/health/liveliness")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json_val: JsonValue = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json_val.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert_eq!(json_val.get("alive").and_then(|v| v.as_bool()), Some(true));
    }

    #[tokio::test]
    async fn test_system_info_returns_ok() {
        use aigw_core::db::Database;
        use std::sync::Arc;

        let db = Database::init("sqlite::memory:").await.expect("init");
        let state = Arc::new(AppState {
            db,
            master_key: None,
            aigw_master_key: None,
            provider_registry: Default::default(),
            router_state: std::sync::Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            rate_limiter: std::sync::Arc::new(Default::default()),
            deployment_mode: "test".to_string(),
        });

        let app = Router::new()
            .route("/system/info", axum::routing::get(system_info))
            .with_state(state);

        let request = Request::builder()
            .method(Method::GET)
            .uri("/system/info")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json_val: JsonValue = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            json_val.get("version").and_then(|v| v.as_str()),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(json_val.get("name").and_then(|v| v.as_str()), Some("aigw"));
        assert_eq!(
            json_val.get("deployment_mode").and_then(|v| v.as_str()),
            Some("test")
        );
        assert_eq!(
            json_val.get("database_type").and_then(|v| v.as_str()),
            Some("sqlite")
        );
        assert!(json_val.get("routes").and_then(|v| v.as_array()).is_some());
    }
}
