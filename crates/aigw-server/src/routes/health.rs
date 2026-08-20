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
use aigw_core::deployment::ProviderType;
use aigw_core::models::{HealthCheck, SpendLog};
use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use prometheus::{Encoder, TextEncoder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

use super::chat::{calc_spend, extract_cache_creation_tokens, extract_cache_read_tokens};
use super::spend::{require_admin, SpendAuth};
use crate::routes::keys::SharedState;

/// Sentinel api_key recorded in spend_logs for model health-check probes.
/// Real virtual keys are SHA256 hashes; this constant string is used so probes
/// are easily filterable and never collide with a real key hash.
const HEALTH_CHECK_API_KEY: &str = "health_check";
/// call_type recorded in spend_logs for model health-check probes.
const HEALTH_CHECK_CALL_TYPE: &str = "health_check";

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
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
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
    let mm = model.model_info.clone();
    tokio::spawn(async move {
        run_and_save_health_check(&resolver, &db, &mn, &mi, mm).await;
    });

    Ok(Json(
        json!({"status": "checking", "model_name": model.model_name, "model_id": model.model_id}),
    ))
}

/// POST /model/health-check/all — Trigger async checks for ALL models, return immediately.
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

    // Collect model info before moving into spawn
    let to_check: Vec<(String, String)> = models
        .iter()
        .map(|m| (m.model_name.clone(), m.model_id.clone()))
        .collect();
    let count = to_check.len();

    // Insert "checking" placeholder for all models
    for (name, mid) in &to_check {
        let check = create_checking_record(name, mid);
        let _ = state.db.insert_health_check(&check).await;
    }

    // Spawn worker tasks: one per model, running concurrently
    let resolver = state.resolver.clone();
    let db = state.db.clone();
    let model_infos: Vec<Value> = models.iter().map(|m| m.model_info.clone()).collect();
    tokio::spawn(async move {
        let mut handles = Vec::new();
        for ((mn, mi), mm) in to_check.into_iter().zip(model_infos) {
            let r = resolver.clone();
            let d = db.clone();
            handles.push(tokio::spawn(async move {
                run_and_save_health_check(&r, &d, &mn, &mi, mm).await;
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
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
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

    Ok(Json(
        json!({ "data": data, "count": data.len(), "last_success": last_success }),
    ))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Core health check logic — uses ModelResolver for proper deployment resolution
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn run_and_save_health_check(
    resolver: &aigw_core::resolver::ModelResolver,
    db: &Database,
    model_name: &str,
    model_id: &str,
    model_info: Value,
) {
    // Use ModelResolver to get the properly decrypted Deployment (handles
    // encryption + credential references). Distinguish three resolve outcomes:
    //   Ok(non-empty)  -> proceed with deployments[0]
    //   Ok([])          -> model absent / env fallback unavailable -> record real reason
    //   Err((code,j))   -> DB / decryption error -> record the resolver's error body
    let deployment = match resolver.resolve(model_name).await {
        Ok(deployments) if !deployments.is_empty() => deployments.into_iter().next().unwrap(),
        Ok(_) => {
            save_result(
                db,
                model_name,
                model_id,
                None,
                false,
                None,
                Some("model not found in resolver (no deployment resolved)".into()),
                None,
            )
            .await;
            return;
        }
        Err((code, body)) => {
            let msg = body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .map(|s| format!("resolver error (HTTP {}): {}", code, s))
                .unwrap_or_else(|| format!("resolver error (HTTP {})", code));
            save_result(db, model_name, model_id, None, false, None, Some(msg), None).await;
            return;
        }
    };

    if deployment.api_base.is_empty() {
        save_result(
            db,
            model_name,
            model_id,
            Some(&deployment),
            false,
            None,
            Some("no api_base configured after resolution".into()),
            None,
        )
        .await;
        return;
    }

    // ── Build the probe URL + auth headers by provider_type, aligned with
    // chat.rs (chat_completions): OpenAI-compatible -> /chat/completions (or
    // /v1/chat/completions when api_base lacks /v1), Authorization: Bearer;
    // AnthropicNative -> /v1/messages, x-api-key + anthropic-version. This
    // fixes the bug where Anthropic-native models were probed on the wrong
    // path with the wrong auth header and always reported unhealthy.
    //
    // TD-010a: embedding-mode models (model_info.mode = "embed") cannot answer
    // a chat probe (embedding-only upstreams 400 /chat/completions). Branch to
    // POST {api_base}/embeddings with `{model, input:["ping"]}` instead.
    let (probe_path, test_body, is_embed) = build_probe_spec(&deployment, &model_info);
    let probe_url = format!(
        "{}/{}",
        deployment.api_base.trim_end_matches('/'),
        probe_path
    );
    let is_anthropic = !is_embed && deployment.provider_type == ProviderType::AnthropicNative;

    let start = std::time::Instant::now();
    let mut req = reqwest::Client::new()
        .post(&probe_url)
        .header("Content-Type", "application/json")
        .json(&test_body)
        .timeout(std::time::Duration::from_secs(15));
    if let Some(ref key) = deployment.api_key {
        if is_anthropic {
            req = req.header("x-api-key", key);
            req = req.header("anthropic-version", "2023-06-01");
        } else {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
    }

    match req.send().await {
        Ok(resp) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            let code = resp.status().as_u16();
            // Treat transport-level success + 2xx/4xx-auth-style codes as healthy
            // (mirror litellm: a 401/429 still means the upstream is reachable).
            let healthy = resp.status().is_success()
                || code == 400
                || code == 401
                || code == 403
                || code == 429
                || code == 422;
            if healthy {
                // Read the body once; parse usage to record real spend/tokens.
                let body_text = resp.text().await.unwrap_or_default();
                let body_val: Value = serde_json::from_str(&body_text).unwrap_or(json!({}));
                save_result(
                    db,
                    model_name,
                    model_id,
                    Some(&deployment),
                    true,
                    Some(elapsed),
                    None,
                    Some(body_val),
                )
                .await;
            } else {
                let body_text = resp.text().await.unwrap_or_default();
                let snippet = body_text.chars().take(120).collect::<String>();
                let err = format!("HTTP {}: {}", code, snippet);
                let body_val: Value = serde_json::from_str(&body_text).unwrap_or(json!({}));
                save_result(
                    db,
                    model_name,
                    model_id,
                    Some(&deployment),
                    false,
                    Some(elapsed),
                    Some(err),
                    Some(body_val),
                )
                .await;
            }
        }
        Err(e) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            // Fallback: GET api_base — if even that fails, the upstream is unreachable.
            let mut fallback = reqwest::Client::new()
                .get(deployment.api_base.trim_end_matches('/'))
                .timeout(std::time::Duration::from_secs(10));
            if let Some(ref key) = deployment.api_key {
                if is_anthropic {
                    fallback = fallback.header("x-api-key", key);
                    fallback = fallback.header("anthropic-version", "2023-06-01");
                } else {
                    fallback = fallback.header("Authorization", format!("Bearer {}", key));
                }
            }
            match fallback.send().await {
                Ok(r2) => {
                    let ok = r2.status().is_success() || r2.status().as_u16() == 401;
                    save_result(
                        db,
                        model_name,
                        model_id,
                        Some(&deployment),
                        ok,
                        Some(elapsed),
                        if ok { None } else { Some(format!("{:?}", e)) },
                        None,
                    )
                    .await;
                }
                Err(_) => {
                    save_result(
                        db,
                        model_name,
                        model_id,
                        Some(&deployment),
                        false,
                        Some(elapsed),
                        Some(format!("{:?}", e)),
                        None,
                    )
                    .await;
                }
            }
        }
    }
}

/// Build the probe path + body for a health-check ping.
///
/// TD-010a: embedding-mode models (`model_info.mode = "embed"`) cannot answer
/// a chat probe — embedding-only upstreams 400 `/chat/completions`. They are
/// probed with POST `{api_base}/embeddings` and an embeddings-shaped body
/// (`{model, input:["ping"]}`) instead. Non-embed models keep the existing
/// chat/messages path and body.
fn build_probe_spec(
    deployment: &aigw_core::deployment::Deployment,
    model_info: &Value,
) -> (String, Value, bool) {
    let is_embed = model_info.get("mode").and_then(|v| v.as_str()) == Some("embed");
    let base = deployment.api_base.trim_end_matches('/').to_string();
    let probe_path = if is_embed {
        if base.ends_with("/v1") || base.contains("/v1/") {
            "embeddings".to_string()
        } else {
            "v1/embeddings".to_string()
        }
    } else {
        match deployment.provider_type {
            ProviderType::AnthropicNative => "messages".to_string(),
            ProviderType::OpenAICompatible => {
                if base.ends_with("/v1") || base.contains("/v1/") {
                    "chat/completions".to_string()
                } else {
                    "v1/chat/completions".to_string()
                }
            }
        }
    };
    let body = if is_embed {
        json!({
            "model": deployment.upstream_model,
            "input": ["ping"],
            "encoding_format": "float"
        })
    } else {
        json!({
            "model": deployment.upstream_model,
            "messages": [{"role": "user", "content": "ping"}],
            "max_tokens": 1,
            "stream": false
        })
    };
    (probe_path, body, is_embed)
}

/// Insert a health_checks row AND a spend_logs row for the probe.
///
/// `deployment` is Some when the probe actually reached the resolution stage
/// (so pricing/model metadata is available for the spend_log); None when the
/// probe failed at resolution (no spend_log is written — there was no upstream
/// call to bill).
///
/// `response` is the parsed upstream response body (when available) — used to
/// extract real usage tokens and compute spend via calc_spend, mirroring
/// chat.rs's success/failure spend_log paths.
#[allow(clippy::too_many_arguments)]
async fn save_result(
    db: &Database,
    model_name: &str,
    model_id: &str,
    deployment: Option<&aigw_core::deployment::Deployment>,
    healthy: bool,
    response_time_ms: Option<f64>,
    error: Option<String>,
    response: Option<Value>,
) {
    let now = Utc::now();
    let now_rfc = now.to_rfc3339();
    let check = HealthCheck {
        health_check_id: Uuid::new_v4().to_string(),
        model_name: model_name.to_string(),
        model_id: Some(model_id.to_string()),
        status: if healthy {
            "healthy".into()
        } else {
            "unhealthy".into()
        },
        healthy_count: if healthy { 1 } else { 0 },
        unhealthy_count: if healthy { 0 } else { 1 },
        error_message: error.clone(),
        response_time_ms,
        details: "{}".to_string(),
        checked_by: Some("api".to_string()),
        checked_at: now_rfc.clone(),
        created_at: now_rfc.clone(),
        updated_at: now_rfc,
    };
    let _ = db.insert_health_check(&check).await;

    // Only write a spend_log when we actually have a resolved deployment —
    // resolution-stage failures (model not found, decryption error) made no
    // upstream call, so there's nothing to bill.
    let Some(d) = deployment else { return };

    // Extract real usage from the upstream response (if any).
    let usage = response
        .as_ref()
        .and_then(|r| r.get("usage"))
        .cloned()
        .unwrap_or(json!({}));
    let prompt_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let completion_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let cache_read = extract_cache_read_tokens(&usage);
    let cache_create = extract_cache_creation_tokens(&usage);
    let total_tokens = prompt_tokens + completion_tokens + cache_read + cache_create;

    // Compute spend from the deployment's pricing fields (real upstream cost),
    // mirroring chat.rs:1666. Probes use max_tokens=1 so spend is tiny but
    // non-zero when the upstream returns usage.
    let spend = calc_spend(
        prompt_tokens,
        completion_tokens,
        d.input_cost_per_token,
        d.output_cost_per_token,
        cache_read,
        cache_create,
        d.cache_read_input_token_cost,
        d.cache_creation_input_token_cost,
    );

    let start_time = now;
    let end_time = now;
    let sl = SpendLog {
        call_id: Uuid::now_v7().to_string(),
        request_id: response
            .as_ref()
            .and_then(|r| r.get("id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        call_type: HEALTH_CHECK_CALL_TYPE.to_string(),
        api_key: HEALTH_CHECK_API_KEY.to_string(),
        spend,
        total_tokens,
        prompt_tokens,
        completion_tokens,
        start_time,
        end_time,
        request_duration_ms: response_time_ms.map(|ms| ms as i32),
        completion_start_time: Some(end_time),
        model: d.upstream_model.clone(),
        model_id: d.model_id.clone(),
        model_group: d.model_group.clone(),
        custom_llm_provider: d.custom_llm_provider.clone(),
        api_base: Some(d.api_base.clone()),
        user: None,
        metadata: None,
        cache_hit: None,
        cache_key: None,
        request_tags: None,
        team_id: None,
        organization_id: None,
        end_user: None,
        requester_ip_address: None,
        messages: None,
        response: response.or_else(|| {
            error
                .as_ref()
                .map(|e| json!({"error": e, "failure_reason": "health_check"}))
        }),
        session_id: None,
        status: Some(if healthy {
            "success".to_string()
        } else {
            "failure:health_check".to_string()
        }),
        mcp_namespaced_tool_name: None,
        agent_id: None,
        proxy_server_request: None,
        body_archived: false,
        parquet_path: None,
        image_tokens: None,
    };
    let _ = db.insert_spend_log(&sl).await;
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
    use crate::routes::keys::DEFAULT_KEY_TOKEN_LEN;
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
            key_generate_length: DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: false,
            provider_registry: Default::default(),
            router_state: std::sync::Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            rate_limiter: std::sync::Arc::new(Default::default()),
            deployment_mode: "test".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            token_provider: std::sync::Arc::new(aigw_core::claude_token::TokenProvider::new()),
            metrics: None,
        })
    }

    // TD-010a: the probe-URL builder must route an embedding-mode model
    // (model_info.mode="embed") to /embeddings with an embeddings-shaped body,
    // and keep the chat path for a normal OpenAI model.
    #[test]
    fn test_probe_shape_branch_by_embed_mode() {
        let base = "https://upstream.example.com/v1".to_string();

        // Embedding mode → /embeddings + input body.
        let embed_deployment = aigw_core::deployment::Deployment {
            api_base: base.clone(),
            api_key: Some("sk-embed".into()),
            upstream_model: "text-embedding-3-small".into(),
            provider_type: ProviderType::OpenAICompatible,
            input_cost_per_token: None,
            output_cost_per_token: None,
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: serde_json::json!({}),
            model_id: Some("m1".into()),
            model_group: Some("text-embedding-3-small".into()),
            custom_llm_provider: Some("openai".into()),
            chat_template_compat: None,
            modal_pricing: None,
            weight: None,
            rpm: None,
            tpm: None,
            priority: None,
            fail_count: 0,
            cooldown_until: None,
            last_latency_ms: 0.0,
            oauth: None,
        };
        let (probe_path, body, is_embed) =
            build_probe_spec(&embed_deployment, &serde_json::json!({"mode": "embed"}));
        assert!(is_embed, "embed-mode model must be probed as embed");
        assert_eq!(probe_path, "embeddings");
        assert!(
            body.get("input").is_some(),
            "embed probe must carry input, got {:?}",
            body
        );
        assert!(
            body.get("messages").is_none(),
            "embed probe must NOT carry messages"
        );

        // Non-embed OpenAI → chat path.
        let chat_deployment = aigw_core::deployment::Deployment {
            api_base: base.clone(),
            api_key: Some("sk-chat".into()),
            upstream_model: "gpt-4".into(),
            provider_type: ProviderType::OpenAICompatible,
            input_cost_per_token: None,
            output_cost_per_token: None,
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: serde_json::json!({}),
            model_id: Some("m2".into()),
            model_group: Some("gpt-4".into()),
            custom_llm_provider: Some("openai".into()),
            chat_template_compat: None,
            modal_pricing: None,
            weight: None,
            rpm: None,
            tpm: None,
            priority: None,
            fail_count: 0,
            cooldown_until: None,
            last_latency_ms: 0.0,
            oauth: None,
        };
        let (probe_path, body, is_embed) =
            build_probe_spec(&chat_deployment, &serde_json::json!({}));
        assert!(!is_embed, "non-embed model must use chat probe");
        assert_eq!(probe_path, "chat/completions");
        assert!(
            body.get("messages").is_some(),
            "chat probe must carry messages"
        );
        assert!(
            body.get("input").is_none(),
            "chat probe must NOT carry input"
        );
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
        use aigw_core::resolver::ModelResolver;
        use std::sync::Arc;

        let db = Database::init("sqlite::memory:").await.expect("init");
        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key: None,
            aigw_master_key: None,
            key_generate_length: DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: false,
            provider_registry: Default::default(),
            router_state: std::sync::Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            rate_limiter: std::sync::Arc::new(Default::default()),
            deployment_mode: "test".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            token_provider: std::sync::Arc::new(aigw_core::claude_token::TokenProvider::new()),
            metrics: None,
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
            json_val
                .get("version")
                .and_then(|v| v.as_str())
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
