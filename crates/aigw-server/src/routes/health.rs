//! Health check and system info endpoints
//!
//! Endpoints:
//! - GET /health             — Simple health check (always ok if running)
//! - GET /health/readiness   — Readiness check (DB connected)
//! - GET /health/liveliness  — Liveliness check (always ok if running)
//! - GET /health/metrics     — Operational metrics (admin only)
//! - GET /system/info        — System information (version, deployment mode, routes)
//! - GET /health/latest      — Latest model health check results
//! - POST /model/health-check       — Ping a single model upstream
//! - POST /model/health-check/all   — Ping all models

use aigw_core::models::HealthCheck;
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

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

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Model Health Check endpoints
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Deserialize)]
pub struct ModelHealthCheckQuery {
    pub model_id: Option<String>,
}

/// POST /model/health-check?model_id=xxx — Ping a single model upstream
pub async fn model_health_check(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Query(query): axum::extract::Query<ModelHealthCheckQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let model_id = query.model_id.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": "model_id query parameter required", "type": "bad_request"}})),
        )
    })?;

    // Find the proxy model
    let models = state.db.list_models().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    let model = models.iter().find(|m| m.model_id == model_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": format!("model_id '{}' not found", model_id), "type": "not_found"}})),
        )
    })?;

    let result = ping_model(model).await;
    let now = Utc::now().to_rfc3339();
    let check = HealthCheck {
        health_check_id: Uuid::new_v4().to_string(),
        model_name: model.model_name.clone(),
        model_id: Some(model.model_id.clone()),
        status: if result.healthy { "healthy".into() } else { "unhealthy".into() },
        healthy_count: if result.healthy { 1 } else { 0 },
        unhealthy_count: if result.healthy { 0 } else { 1 },
        error_message: result.error,
        response_time_ms: result.response_time_ms,
        details: "{}".to_string(),
        checked_by: Some("api".to_string()),
        checked_at: now.clone(),
        created_at: now.clone(),
        updated_at: now,
    };

    state.db.insert_health_check(&check).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    Ok(Json(json!({
        "model_name": check.model_name,
        "model_id": check.model_id,
        "status": check.status,
        "response_time_ms": check.response_time_ms,
        "error_message": check.error_message,
        "checked_at": check.checked_at,
    })))
}

/// POST /model/health-check/all — Ping all models
pub async fn model_health_check_all(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let models = state.db.list_models().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    let mut results = Vec::new();
    for model in &models {
        let result = ping_model(model).await;
        let now = chrono::Utc::now().to_rfc3339();
        let check = HealthCheck {
            health_check_id: Uuid::new_v4().to_string(),
            model_name: model.model_name.clone(),
            model_id: Some(model.model_id.clone()),
            status: if result.healthy { "healthy".into() } else { "unhealthy".into() },
            healthy_count: if result.healthy { 1 } else { 0 },
            unhealthy_count: if result.healthy { 0 } else { 1 },
            error_message: result.error.clone(),
            response_time_ms: result.response_time_ms,
            details: "{}".to_string(),
            checked_by: Some("api".to_string()),
            checked_at: now.clone(),
            created_at: now.clone(),
            updated_at: now,
        };

        let _ = state.db.insert_health_check(&check).await;

        results.push(json!({
            "model_name": check.model_name,
            "model_id": check.model_id,
            "status": check.status,
            "response_time_ms": check.response_time_ms,
            "error_message": check.error_message,
            "checked_at": check.checked_at,
        }));
    }

    Ok(Json(json!({
        "checked": results.len(),
        "healthy": results.iter().filter(|r| r["status"] == "healthy").count(),
        "unhealthy": results.iter().filter(|r| r["status"] == "unhealthy").count(),
        "results": results,
    })))
}

/// GET /health/latest — Latest health check per model
pub async fn health_latest(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let checks = state.db.get_latest_health_checks().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    let data: Vec<Value> = checks.iter().map(|c| json!({
        "model_name": c.model_name,
        "model_id": c.model_id,
        "status": c.status,
        "response_time_ms": c.response_time_ms,
        "error_message": c.error_message,
        "checked_at": c.checked_at,
    })).collect();

    Ok(Json(json!({ "data": data, "count": data.len() })))
}

struct PingResult {
    healthy: bool,
    response_time_ms: Option<f64>,
    error: Option<String>,
}

async fn ping_model(model: &aigw_core::models::ProxyModel) -> PingResult {
    let base_url = model.litellm_params
        .get("api_base")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let api_key = model.litellm_params.get("api_key")
        .and_then(|v| v.as_str())
        .or_else(|| model.litellm_params.get("key").and_then(|v| v.as_str()));

    if base_url.is_empty() {
        return PingResult {
            healthy: false,
            response_time_ms: None,
            error: Some("model has no api_base configured".to_string()),
        };
    }

    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let start = std::time::Instant::now();

    let mut req = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(10));

    if let Some(key) = api_key {
        req = req.header("Authorization", format!("Bearer {}", key));
    }

    match req.send().await {
        Ok(resp) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            if resp.status().is_success() {
                PingResult {
                    healthy: true,
                    response_time_ms: Some(elapsed),
                    error: None,
                }
            } else {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                PingResult {
                    healthy: false,
                    response_time_ms: Some(elapsed),
                    error: Some(format!("{} {}", status.as_u16(), body.chars().take(80).collect::<String>())),
                }
            }
        }
        Err(e) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            PingResult {
                healthy: false,
                response_time_ms: Some(elapsed),
                error: Some(format!("{:?}", e)),
            }
        }
    }
}

use chrono::Utc;

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
    use aigw_core::router::Router as AigwRouter;
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
            router: AigwRouter::default(),
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
