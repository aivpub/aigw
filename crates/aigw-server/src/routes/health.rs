//! Health check and system info endpoints
//!
//! Endpoints:
//! - GET /health            — Simple health check (always ok if running)
//! - GET /health/readiness   — Readiness check (DB connected)
//! - GET /health/liveliness  — Liveliness check (always ok if running)
//! - GET /health/metrics     — Operational metrics (admin only)
//! - GET /system/info        — System information (version, deployment mode, routes)

use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};

use super::spend::{require_admin, SpendAuth};
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
            "/health", "/health/readiness", "/health/liveliness", "/health/metrics",
            "/docs", "/openapi.json",
            "/org/new", "/org/info", "/org/list", "/org/update", "/org/delete",
            "/team/new", "/team/info", "/team/list", "/team/update", "/team/delete",
            "/user/new", "/user/info", "/user/list", "/user/update", "/user/delete",
            "/system/info"
        ]
    }))
}

/// GET /health/metrics — operational metrics (admin only)
pub async fn health_metrics(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let uptime = state.started_at.elapsed().as_secs();

    let (keys, models, orgs, teams, users) = tokio::try_join!(
        state.db.count_virtual_keys(),
        state.db.count_proxy_models(),
        state.db.count_organizations(),
        state.db.count_teams(),
        state.db.count_users(),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    Ok(Json(json!({
        "status": "healthy",
        "uptime_seconds": uptime,
        "db": {
            "connected": true,
            "pool_size": state.db.pool_size(),
            "idle": state.db.pool_idle(),
        },
        "counts": {
            "virtual_keys": keys,
            "proxy_models": models,
            "organizations": orgs,
            "teams": teams,
            "users": users,
        },
        "version": env!("CARGO_PKG_VERSION"),
    })))
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
    use aigw_core::resolver::ModelResolver;
        use aigw_core::db::Database;
        use std::sync::Arc;

        let db = Database::init("sqlite::memory:").await.expect("init");
        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            db,
            master_key: None,
            aigw_master_key: None,
            provider_registry: Default::default(),
            router_state: std::sync::Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            rate_limiter: std::sync::Arc::new(Default::default()),
            deployment_mode: "test".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
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
