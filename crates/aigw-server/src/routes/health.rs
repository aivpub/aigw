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

use aigw_core::db::Database;
use aigw_core::models::HealthCheck;
use axum::{extract::State, http::StatusCode, Json};
use prometheus::{Encoder, TextEncoder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;
use chrono::Utc;

use super::spend::{require_admin, SpendAuth};
use crate::routes::keys::SharedState;

/// GET /health — simple health check
pub async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

/// GET /health/readiness — readiness check (DB ping + accepting traffic)
pub async fn readiness(State(state): State<SharedState>) -> Json<Value> {
    // Actually ping the database, not just return static true.
    // This is critical for graceful updates: docker-compose healthcheck
    // should report "not ready" if the DB is unreachable so the load
    // balancer / orchestrator can stop sending traffic.
    let db_ok = state.db.ping().await.is_ok();
    Json(json!({"status": if db_ok { "ok" } else { "error" }, "ready": db_ok}))
}

/// GET /health/liveliness — liveness check (always ok if running)
pub async fn liveliness() -> Json<Value> {
    Json(json!({"status": "ok", "alive": true}))
}

/// GET /system/info — System information (version, deployment mode, available routes)
pub async fn system_info(State(state): State<SharedState>) -> Json<Value> {
    Json(json!({
        "version": crate::VERSION_INFO,
        "name": "aigw",
        "build_date": crate::BUILD_DATE,
        "commit": crate::GIT_COMMIT_HASH,
        "git_describe": crate::GIT_DESCRIBE,
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

/// POST /model/health-check?model_id=xxx — Trigger async check for a single model.
fn create_checking_record(model_name: &str, model_id: &str) -> HealthCheck {
    let now = Utc::now().to_rfc3339();
    HealthCheck {
        health_check_id: Uuid::new_v4().to_string(),
        model_name: model_name.to_string(),
        model_id: Some(model_id.to_string()),
        status: "checking".to_string(),
        healthy_count: 0,
        unhealthy_count: 0,
        error_message: None,
        response_time_ms: None,
        details: "{}".to_string(),
        checked_by: Some("api".to_string()),
        checked_at: now.clone(),
        created_at: now.clone(),
        updated_at: now,
    }
}

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

    let models = state.db.list_models().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})))
    })?;

    let model = models.iter().find(|m| m.model_id == model_id).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(json!({"error": {"message": format!("model_id '{}' not found", model_id), "type": "not_found"}})))
    })?;

    // Insert "checking" placeholder
    let check = create_checking_record(&model.model_name, &model.model_id);
    let _ = state.db.insert_health_check(&check).await;

    // Spawn async background check
    let resolver = state.resolver.clone();
    let db = state.db.clone();
    let mn = model.model_name.clone();
    let mi = model.model_id.clone();
    tokio::spawn(async move {
        run_and_save_health_check(&resolver, &db, &mn, &mi).await;
    });

    Ok(Json(json!({"status": "checking", "model_name": model.model_name, "model_id": model.model_id})))
}

/// POST /model/health-check/all — Trigger async checks for ALL models, return immediately.
pub async fn model_health_check_all(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let models = state.db.list_models().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})))
    })?;

    // Collect model info before moving into spawn
    let to_check: Vec<(String, String)> = models.iter().map(|m| (m.model_name.clone(), m.model_id.clone())).collect();
    let count = to_check.len();

    // Insert "checking" placeholder for all models
    for (name, mid) in &to_check {
        let check = create_checking_record(name, mid);
        let _ = state.db.insert_health_check(&check).await;
    }

    // Spawn worker tasks: one per model, running concurrently
    let resolver = state.resolver.clone();
    let db = state.db.clone();
    tokio::spawn(async move {
        let mut handles = Vec::new();
        for (mn, mi) in to_check {
            let r = resolver.clone();
            let d = db.clone();
            let mn = mn.clone();
            let mi = mi.clone();
            handles.push(tokio::spawn(async move {
                run_and_save_health_check(&r, &d, &mn, &mi).await;
            }));
        }
        // Await all
        for h in handles {
            let _ = h.await;
        }
    });

    Ok(Json(json!({"status": "dispatched", "models": count})))
}

/// GET /health/latest — Latest health check per model (includes "checking" status)
pub async fn health_latest(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let checks = state.db.get_latest_health_checks().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})))
    })?;

    // Merge with models table: show all models, even those without any check yet
    let models = state.db.list_models().await.unwrap_or_default();
    let mut data = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for model in &models {
        seen.insert(model.model_name.clone());
        if let Some(c) = checks.iter().find(|c| c.model_name == model.model_name) {
            data.push(json!({
                "model_name": c.model_name,
                "model_id": c.model_id,
                "status": c.status,
                "response_time_ms": c.response_time_ms,
                "error_message": c.error_message,
                "checked_at": c.checked_at,
            }));
        } else {
            data.push(json!({
                "model_name": model.model_name,
                "model_id": model.model_id,
                "status": "unknown",
                "response_time_ms": null,
                "error_message": null,
                "checked_at": null,
            }));
        }
    }

    let mut last_success = HashMap::new();
    for c in &checks {
        if c.status == "healthy" {
            let entry = last_success.entry(c.model_name.clone()).or_insert(None);
            if entry.is_none() || c.checked_at > *entry.as_ref().unwrap() {
                *entry = Some(c.checked_at.clone());
            }
        }
    }
    for m in &models {
        last_success.entry(m.model_name.clone()).or_insert(None);
    }

    Ok(Json(json!({ "data": data, "count": data.len(), "last_success": last_success })))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Core health check logic — uses ModelResolver for proper deployment resolution
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn run_and_save_health_check(
    resolver: &aigw_core::resolver::ModelResolver,
    db: &Database,
    model_name: &str,
    model_id: &str,
) {
    // Use ModelResolver to get properly decrypted Deployment (handles encryption + credential refs)
    let (base_url, api_key, upstream_model) = match resolver.resolve(model_name).await {
        Ok(deployments) if !deployments.is_empty() => {
            let d = &deployments[0];
            (d.api_base.clone(), d.api_key.clone(), d.upstream_model.clone())
        }
        _ => {
            save_result(db, model_name, model_id, false, None, Some("model not found in resolver".into())).await;
            return;
        }
    };

    if base_url.is_empty() {
        save_result(db, model_name, model_id, false, None, Some("no api_base configured after resolution".into())).await;
        return;
    }

    let base = base_url.trim_end_matches('/').to_string();
    let chat_url = if base.ends_with("/v1") || base.contains("/v1/") {
        format!("{}/chat/completions", base)
    } else {
        format!("{}/v1/chat/completions", base)
    };

    let test_body = json!({
        "model": upstream_model,
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 1,
        "stream": false
    });

    let start = std::time::Instant::now();
    let mut req = reqwest::Client::new()
        .post(&chat_url)
        .header("Content-Type", "application/json")
        .json(&test_body)
        .timeout(std::time::Duration::from_secs(15));

    if let Some(ref key) = api_key {
        req = req.header("Authorization", format!("Bearer {}", key));
    }

    match req.send().await {
        Ok(resp) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            let code = resp.status().as_u16();
            let healthy = resp.status().is_success() || code == 400 || code == 401 || code == 403 || code == 429 || code == 422;
            if healthy {
                save_result(db, model_name, model_id, true, Some(elapsed), None).await;
            } else {
                let body = resp.text().await.unwrap_or_default();
                save_result(db, model_name, model_id, false, Some(elapsed), Some(format!("HTTP {}: {}", code, body.chars().take(120).collect::<String>()))).await;
            }
        }
        Err(e) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            // Fallback: GET /
            let mut fallback = reqwest::Client::new().get(&base).timeout(std::time::Duration::from_secs(10));
            if let Some(ref key) = api_key {
                fallback = fallback.header("Authorization", format!("Bearer {}", key));
            }
            match fallback.send().await {
                Ok(r2) => {
                    let ok = r2.status().is_success() || r2.status().as_u16() == 401;
                    save_result(db, model_name, model_id, ok, Some(elapsed), if ok { None } else { Some(format!("{:?}", e)) }).await;
                }
                Err(_) => {
                    save_result(db, model_name, model_id, false, Some(elapsed), Some(format!("{:?}", e))).await;
                }
            }
        }
    }
}

async fn save_result(
    db: &Database,
    model_name: &str,
    model_id: &str,
    healthy: bool,
    response_time_ms: Option<f64>,
    error: Option<String>,
) {
    let now = Utc::now().to_rfc3339();
    let check = HealthCheck {
        health_check_id: Uuid::new_v4().to_string(),
        model_name: model_name.to_string(),
        model_id: Some(model_id.to_string()),
        status: if healthy { "healthy".into() } else { "unhealthy".into() },
        healthy_count: if healthy { 1 } else { 0 },
        unhealthy_count: if healthy { 0 } else { 1 },
        error_message: error,
        response_time_ms,
        details: "{}".to_string(),
        checked_by: Some("api".to_string()),
        checked_at: now.clone(),
        created_at: now.clone(),
        updated_at: now,
    };
    let _ = db.insert_health_check(&check).await;
}
/// GET /metrics — Prometheus metrics endpoint (Stage 67)
pub async fn prometheus_metrics() -> axum::response::Response {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        tracing::error!("prometheus encode error: {}", e);
        return axum::response::Response::builder()
            .status(500)
            .body(axum::body::Body::from(format!("encode error: {}", e)))
            .unwrap();
    }
    axum::response::Response::builder()
        .header("Content-Type", "text/plain; version=0.0.4")
        .body(axum::body::Body::from(buffer))
        .unwrap()
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
        "version": crate::VERSION_INFO,
        "build_date": crate::BUILD_DATE,
        "commit": crate::GIT_COMMIT_HASH,
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
        use aigw_core::db::Database;

        let db = Database::init("sqlite::memory:").await.expect("init");
        let state = test_state(db);

        let app = Router::new()
            .route("/health/readiness", axum::routing::get(readiness))
            .with_state(state);

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

    /// Build a minimal `SharedState` for tests that need a real DB handle.
    fn test_state(db: Database) -> SharedState {
        use aigw_core::resolver::ModelResolver;
        use aigw_core::router::Router as AigwRouter;
        std::sync::Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "test"),
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
  otel_active: false,
            body_archiver: None,            metrics: None,
        })
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
  otel_active: false,
            body_archiver: None,            metrics: None,
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
        assert!(
            json_val.get("version").and_then(|v| v.as_str())
                .map(|v| v.starts_with(env!("CARGO_PKG_VERSION")))
                .unwrap_or(false),
            "version should start with CARGO_PKG_VERSION"
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
