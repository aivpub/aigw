//! Spend/usage tracking endpoints — litellm-compatible /spend/* and /global/spend/* routes
//!
//! Endpoints:
//! - GET /spend/logs          — Query spend logs (scoped to authenticated key)
//! - GET /spend/keys          — Get spend per key summary
//! - GET /spend/users         — Get spend per user summary
//! - GET /spend/tags          — Get spend per tag summary
//! - GET /spend/models        — Spend aggregated by model
//! - GET /spend/providers     — Spend aggregated by provider
//! - GET /global/spend        — Get total global spend (admin only)
//! - GET /global/spend/logs   — All spend logs (admin only)
//! - GET /global/spend/keys   — All keys spend (admin only)
//! - GET /global/spend/models — Spend aggregated by model (admin only)
//! - GET /global/spend/providers — Spend aggregated by provider (admin only)

use aigw_core::auth::decode_jwt;
use aigw_core::crypto::{decrypt_litellm_value, hash_token};
use aigw_core::middleware::{AuthError, KeyIdentity};
use axum::{
    extract::{FromRequestParts, Path, Query, State},
    http::{self, request::Parts, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use aigw_core::db::{Database, DbError};
use chrono::NaiveDate;
use std::collections::HashMap;

use super::keys::SharedState;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Query parameters
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SpendLogsQuery {
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub request_id: Option<String>,
    pub session_id: Option<String>,
    pub status: Option<String>,
    pub min_tokens: Option<i32>,
    pub max_tokens: Option<i32>,
    pub limit: Option<i32>,
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct SpendTagQuery {
    pub tag: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SpendModelQuery {
    pub api_key: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SpendProviderQuery {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SpendAuth — newtype wrapper for KeyIdentity to satisfy orphan rules
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Thin newtype around KeyIdentity so we can implement FromRequestParts<SharedState>
/// locally without running into the orphan rule (middleware.rs already has a blanket impl).
pub struct SpendAuth(pub KeyIdentity);

impl std::ops::Deref for SpendAuth {
    type Target = KeyIdentity;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> FromRequestParts<S> for SpendAuth
where
    S: Send + Sync,
    SharedState: axum::extract::FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state: SharedState = axum::extract::FromRef::from_ref(state);
        // 1. Try Authorization header (Bearer token)
        let header_result = Self::try_bearer_token(&state, parts).await;

        if let Ok(auth) = header_result {
            return Ok(auth);
        }

        // 2. Fall back to HttpOnly cookie JWT
        Self::try_cookie_jwt(&state, parts).await
    }
}

impl SpendAuth {
    /// Try Bearer token from Authorization header
    async fn try_bearer_token(state: &SharedState, parts: &Parts) -> Result<SpendAuth, AuthError> {
        let auth_header = parts
            .headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthError::MissingHeader)?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AuthError::InvalidFormat)?;

        if token.is_empty() {
            return Err(AuthError::InvalidFormat);
        }

        // Check master key
        if let Some(ref mk) = state.master_key {
            if token == *mk {
                return Ok(SpendAuth(KeyIdentity {
                    token_hash: "*master*".to_string(),
                    key_alias: Some("master".to_string()),
                    user_id: None,
                    team_id: None,
                    organization_id: None,
                    is_master_key: true,
                    user_role: Some("proxy_admin".to_string()),
                }));
            }
        }

        // SHA256 hash and DB lookup
        let token_hash = hash_token(token);
        let key = state
            .db
            .get_key_by_token(&token_hash)
            .await
            .map_err(|_| AuthError::TokenNotFound)?;

        match key {
            Some(k) => Ok(SpendAuth(KeyIdentity {
                token_hash,
                key_alias: k.key_alias,
                user_id: k.user_id,
                team_id: k.team_id,
                organization_id: k.organization_id,
                is_master_key: false,
                user_role: None,
            })),
            None => Err(AuthError::TokenNotFound),
        }
    }

    /// Try HttpOnly cookie JWT
    async fn try_cookie_jwt(state: &SharedState, parts: &Parts) -> Result<SpendAuth, AuthError> {
        let master_key = state
            .master_key
            .as_ref()
            .ok_or(AuthError::MissingHeader)?;

        // Extract cookie named "token"
        let cookie_value = parts
            .headers
            .get(http::header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                s.split(';')
                    .map(|c| c.trim())
                    .find_map(|c| {
                        let (k, v) = c.split_once('=')?;
                        if k == "token" { Some(v.to_string()) } else { None }
                    })
            })
            .ok_or(AuthError::MissingHeader)?;

        // Decode JWT
        let claims = decode_jwt(&cookie_value, master_key)
            .map_err(|_| AuthError::TokenNotFound)?;

        // Hash the key from JWT claims and look up in DB
        let token_hash = hash_token(&claims.key);
        let key = state
            .db
            .get_key_by_token(&token_hash)
            .await
            .map_err(|_| AuthError::TokenNotFound)?;

        match key {
            Some(k) => Ok(SpendAuth(KeyIdentity {
                token_hash,
                key_alias: k.key_alias,
                user_id: k.user_id,
                team_id: k.team_id,
                organization_id: k.organization_id,
                is_master_key: false,
                user_role: Some(claims.user_role.clone()),
            })),
            None => Err(AuthError::TokenNotFound),
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helper: check admin access
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub fn require_admin(auth: &KeyIdentity) -> Result<(), (StatusCode, Json<Value>)> {
    if auth.is_master_key || auth.user_role.as_deref() == Some("proxy_admin") {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": {"message": "Admin access required", "type": "forbidden"}})),
        ))
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// /spend/* endpoints (scoped to authenticated key)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// GET /spend/logs — Query spend logs for the authenticated key
pub async fn spend_logs(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<SpendLogsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let api_key = query.api_key.unwrap_or_else(|| auth.token_hash.clone());
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).max(1).min(100);
    let offset = (page - 1) * page_size;

    let (logs, total_count) = tokio::try_join!(
        state.db.query_spend_logs_with_status_filter(
            Some(&api_key),
            query.model.as_deref(),
            query.provider.as_deref(),
            query.start_date.as_deref(),
            query.end_date.as_deref(),
            query.request_id.as_deref(),
            query.status.as_deref(),
            query.min_tokens,
            query.max_tokens,
            Some(page_size),
            Some(offset),
        ),
        state.db.query_spend_logs_count(
            Some(&api_key),
            query.model.as_deref(),
            query.start_date.as_deref(),
            query.end_date.as_deref(),
            query.request_id.as_deref(),
        ),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    let total_pages = if total_count > 0 {
        ((total_count as f64) / (page_size as f64)).ceil() as i64
    } else {
        0
    };

    // Resolve key_alias names for display
    let distinct_keys: Vec<String> = logs.iter().map(|l| l.api_key.clone()).collect::<std::collections::HashSet<_>>().into_iter().collect();
    let mut key_map: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
    for key_hash in &distinct_keys {
        if key_hash == "master_key" {
            key_map.insert(key_hash.clone(), Some("master".to_string()));
        } else if let Ok(Some(k)) = state.db.get_key_by_token(key_hash).await {
            key_map.insert(key_hash.clone(), k.key_alias);
        } else {
            key_map.insert(key_hash.clone(), None);
        }
    }

    let data: Vec<Value> = logs
        .iter()
        .map(|log| {
            let ttft_ms = compute_ttft(log);
            let key_name: Option<String> = key_map.get(&log.api_key).cloned().flatten();
            json!({
                "call_id": log.call_id,
                "request_id": log.request_id,
                "call_type": log.call_type,
                "api_key": log.api_key,
                "key_name": key_name,
                "spend": log.spend,
                "total_tokens": log.total_tokens,
                "prompt_tokens": log.prompt_tokens,
                "completion_tokens": log.completion_tokens,
                "start_time": log.start_time.to_rfc3339(),
                "end_time": log.end_time.to_rfc3339(),
                "request_duration_ms": log.request_duration_ms,
                "ttft_ms": ttft_ms,
                "model": log.model,
                "model_id": log.model_id,
                "model_group": log.model_group,
                "custom_llm_provider": log.custom_llm_provider,
                "api_base": log.api_base,
                "user": log.user,
                "team_id": log.team_id,
                "organization_id": log.organization_id,
                "end_user": log.end_user,
                "session_id": log.session_id,
                "request_tags": log.request_tags,
                "metadata": log.metadata,
                "cache_hit": log.cache_hit,
                "cache_key": log.cache_key,
                "status": log.status,
                "mcp_namespaced_tool_name": log.mcp_namespaced_tool_name,
                "requester_ip_address": &log.requester_ip_address,
            })
        })
        .collect();

    Ok(Json(json!({
        "data": data,
        "count": data.len(),
        "total_count": total_count,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

/// GET /global/spend/logs/{call_id} — Get single spend log detail with full body blobs.
pub async fn global_spend_log_detail(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(call_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let log = state
        .db
        .get_spend_log_by_call_id(&call_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
            )
        })?;

    let log = match log {
        Some(l) => l,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error": {"message": "Spend log not found", "type": "not_found"}})),
            ));
        }
    };

    // Resolve key_alias name for display
    let key_name: Option<String> = if log.api_key == "master_key" {
        Some("master".to_string())
    } else {
        state
            .db
            .get_key_by_token(&log.api_key)
            .await
            .ok()
            .flatten()
            .map(|k| k.key_alias)
            .flatten()
    };

    let ttft_ms = compute_ttft(&log);

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Body resolution — decide source and measure latency
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    #[cfg(debug_assertions)]
    let body_resolve_start = std::time::Instant::now();
    #[cfg(debug_assertions)]
    let mut body_source: &str = "db"; // hot path default

    let body_payload = if log.messages.is_none() && log.body_archived {
        // Cold data — body archived to Parquet. Try to retrieve from storage.
        match &state.body_archiver {
            Some(archiver) => {
                let parquet_path = log.parquet_path.as_deref().unwrap_or("");
                if parquet_path.is_empty() {
                    #[cfg(debug_assertions)]
                    {
                        body_source = "no-parquet-path";
                    }
                    tracing::warn!(%call_id, "body_archived=true but parquet_path is empty");
                    aigw_core::body_archive::query::BodyPayload {
                        messages: None,
                        response: None,
                        proxy_server_request: None,
                    }
                } else {
                    match archiver.read_body_from_parquet_path(parquet_path, &call_id).await {
                        Ok(Some(body)) => {
                            #[cfg(debug_assertions)]
                            {
                                body_source = "parquet";
                            }
                            body
                        }
                        Ok(None) => {
                            #[cfg(debug_assertions)]
                            {
                                body_source = "parquet-miss";
                            }
                            tracing::warn!(%call_id, "body_archived=true but cold retrieval returned None — storage or file missing");
                            aigw_core::body_archive::query::BodyPayload {
                                messages: None,
                                response: None,
                                proxy_server_request: None,
                            }
                        }
                        Err(e) => {
                            #[cfg(debug_assertions)]
                            {
                                body_source = "parquet-error";
                            }
                            tracing::error!(%call_id, %e, "cold retrieval error");
                            aigw_core::body_archive::query::BodyPayload {
                                messages: None,
                                response: None,
                                proxy_server_request: None,
                            }
                        }
                    }
                }
            }
            None => {
                #[cfg(debug_assertions)]
                {
                    body_source = "no-archiver";
                }
                tracing::warn!(%call_id, "body_archived=true but body_archiver not configured");
                aigw_core::body_archive::query::BodyPayload {
                    messages: None,
                    response: None,
                    proxy_server_request: None,
                }
            }
        }
    } else {
        // Hot data — body still in DB
        aigw_core::body_archive::query::BodyPayload {
            messages: log.messages.clone(),
            response: log.response.clone(),
            proxy_server_request: log.proxy_server_request.clone(),
        }
    };

    #[cfg(debug_assertions)]
    {
        let elapsed = body_resolve_start.elapsed();
        tracing::trace!(
            %call_id,
            body_source,
            body_resolve_ms = elapsed.as_secs_f64() * 1000.0,
            body_archived = log.body_archived,
            has_messages_in_db = log.messages.is_some(),
            "🍉 body resolution: source={} elapsed={:.3}ms",
            body_source,
            elapsed.as_secs_f64() * 1000.0
        );
    }

    Ok(Json(json!({
        "call_id": log.call_id,
        "request_id": log.request_id,
        "call_type": log.call_type,
        "api_key": log.api_key,
        "key_name": key_name,
        "spend": log.spend,
        "total_tokens": log.total_tokens,
        "prompt_tokens": log.prompt_tokens,
        "completion_tokens": log.completion_tokens,
        "start_time": log.start_time.to_rfc3339(),
        "end_time": log.end_time.to_rfc3339(),
        "request_duration_ms": log.request_duration_ms,
        "ttft_ms": ttft_ms,
        "model": log.model,
        "model_id": log.model_id,
        "model_group": log.model_group,
        "custom_llm_provider": log.custom_llm_provider,
        "api_base": log.api_base,
        "user": log.user,
        "team_id": log.team_id,
        "organization_id": log.organization_id,
        "end_user": log.end_user,
        "session_id": log.session_id,
        "request_tags": log.request_tags,
        "metadata": log.metadata,
        "cache_hit": log.cache_hit,
        "cache_key": log.cache_key,
        "status": log.status,
        "mcp_namespaced_tool_name": log.mcp_namespaced_tool_name,
        "requester_ip_address": &log.requester_ip_address,
        "messages": &body_payload.messages,
        "response": &body_payload.response,
        "proxy_server_request": &body_payload.proxy_server_request,
    })))
}

/// Compute TTFT (time to first token) in milliseconds.
/// Returns Some(ms) for streaming requests where completion_start_time is a real timestamp
/// (not the sentinel end_time value used for non-streaming requests).
/// Returns None for non-streaming requests.
fn compute_ttft(log: &aigw_core::models::SpendLog) -> Option<f64> {
    match log.completion_start_time {
        Some(cst) if cst != log.end_time => {
            let duration = cst.signed_duration_since(log.start_time);
            // Convert to float milliseconds (with microsecond precision)
            Some(duration.num_microseconds().unwrap_or(0) as f64 / 1000.0)
        }
        _ => None,
    }
}

/// GET /spend/keys — Get spend per key summary for the authenticated key
pub async fn spend_keys(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let spend = state
        .db
        .get_spend_by_key(&auth.token_hash)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
            )
        })?;

    Ok(Json(json!({
        "key": auth.token_hash,
        "spend": spend,
    })))
}

/// GET /spend/users — Get spend per user summary for the authenticated user
pub async fn spend_users(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = auth.user_id.unwrap_or_default();

    if user_id.is_empty() {
        return Ok(Json(json!({
            "user_id": "",
            "spend": 0.0,
            "message": "No user_id associated with this key",
        })));
    }

    let spend = state.db.get_spend_by_user(&user_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    Ok(Json(json!({
        "user_id": user_id,
        "spend": spend,
    })))
}

/// GET /spend/tags — Get spend per tag summary
pub async fn spend_tags(
    State(state): State<SharedState>,
    _auth: SpendAuth,
    Query(query): Query<SpendTagQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let tag = query.tag.as_deref().unwrap_or("");

    if tag.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error": {"message": "Missing 'tag' query parameter", "type": "bad_request"}}),
            ),
        ));
    }

    let spend = state.db.get_spend_by_tag(tag).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    Ok(Json(json!({
        "tag": tag,
        "spend": spend,
    })))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// /global/spend/* endpoints (admin only)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// GET /global/spend — Get total global spend
pub async fn global_spend(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let spend = state.db.get_global_spend().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    Ok(Json(json!({ "spend": spend })))
}

/// GET /global/spend/logs — Get all spend logs (admin only)
pub async fn global_spend_logs(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<SpendLogsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).max(1).min(100);
    let offset = (page - 1) * page_size;

    let (logs, total_count) = tokio::try_join!(
        state.db.query_spend_logs_with_status_filter(
            query.api_key.as_deref(),
            query.model.as_deref(),
            query.provider.as_deref(),
            query.start_date.as_deref(),
            query.end_date.as_deref(),
            query.request_id.as_deref(),
            query.status.as_deref(),
            query.min_tokens,
            query.max_tokens,
            Some(page_size),
            Some(offset),
        ),
        state.db.query_spend_logs_count(
            query.api_key.as_deref(),
            query.model.as_deref(),
            query.start_date.as_deref(),
            query.end_date.as_deref(),
            query.request_id.as_deref(),
        ),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    let total_pages = if total_count > 0 {
        ((total_count as f64) / (page_size as f64)).ceil() as i64
    } else {
        0
    };

    // Resolve key_alias names for display
    let distinct_keys: Vec<String> = logs.iter().map(|l| l.api_key.clone()).collect::<std::collections::HashSet<_>>().into_iter().collect();
    let mut key_map: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
    for key_hash in &distinct_keys {
        if key_hash == "master_key" {
            key_map.insert(key_hash.clone(), Some("master".to_string()));
        } else if let Ok(Some(k)) = state.db.get_key_by_token(key_hash).await {
            key_map.insert(key_hash.clone(), k.key_alias);
        } else {
            key_map.insert(key_hash.clone(), None);
        }
    }

    let data: Vec<Value> = logs
        .iter()
        .map(|log| {
            let ttft_ms = compute_ttft(log);
            let key_name: Option<String> = key_map.get(&log.api_key).cloned().flatten();
            json!({
                "call_id": log.call_id,
                "request_id": log.request_id,
                "call_type": log.call_type,
                "api_key": log.api_key,
                "key_name": key_name,
                "spend": log.spend,
                "total_tokens": log.total_tokens,
                "prompt_tokens": log.prompt_tokens,
                "completion_tokens": log.completion_tokens,
                "start_time": log.start_time.to_rfc3339(),
                "end_time": log.end_time.to_rfc3339(),
                "request_duration_ms": log.request_duration_ms,
                "ttft_ms": ttft_ms,
                "model": log.model,
                "model_id": log.model_id,
                "model_group": log.model_group,
                "custom_llm_provider": log.custom_llm_provider,
                "api_base": log.api_base,
                "user": log.user,
                "team_id": log.team_id,
                "organization_id": log.organization_id,
                "end_user": log.end_user,
                "session_id": log.session_id,
                "request_tags": log.request_tags,
                "metadata": log.metadata,
                "cache_hit": log.cache_hit,
                "cache_key": log.cache_key,
                "status": log.status,
                "mcp_namespaced_tool_name": log.mcp_namespaced_tool_name,
                "requester_ip_address": &log.requester_ip_address,
            })
        })
        .collect();

    Ok(Json(json!({
        "data": data,
        "count": data.len(),
        "total_count": total_count,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

/// GET /global/spend/keys — Get spend for all keys (admin only)
pub async fn global_spend_keys(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let logs = state
        .db
        .query_spend_logs(None, Some(10000))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
            )
        })?;

    let mut spend_by_key: HashMap<String, f64> = HashMap::new();
    for log in &logs {
        *spend_by_key.entry(log.api_key.clone()).or_insert(0.0) += log.spend;
    }

    let data: Vec<Value> = spend_by_key
        .into_iter()
        .map(|(key, spend)| json!({ "api_key": key, "spend": spend }))
        .collect();

    Ok(Json(json!({ "data": data })))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// /spend/models — Spend aggregated by model
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// GET /spend/models — Spend by model (scoped to authenticated key or global)
pub async fn spend_models(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<SpendModelQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let api_key = query.api_key.unwrap_or_else(|| auth.token_hash.clone());

    let aggs = state
        .db
        .aggregate_spend_by_model(
            Some(&api_key),
            query.start_date.as_deref(),
            query.end_date.as_deref(),
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
            )
        })?;

    let data: Vec<Value> = aggs
        .iter()
        .map(|a| {
            json!({
                "model": a.model,
                "total_tokens": a.total_tokens,
                "total_spend": a.total_spend,
                "requests": a.requests,
            })
        })
        .collect();

    Ok(Json(json!({ "data": data, "count": data.len() })))
}

/// GET /spend/providers — Spend by provider (from proxy_models litellm_params)
pub async fn spend_providers(
    State(state): State<SharedState>,
    _auth: SpendAuth,
    Query(query): Query<SpendProviderQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    spend_providers_inner(
        &state,
        query.start_date.as_deref(),
        query.end_date.as_deref(),
    ).await
}

/// GET /global/spend/providers — Spend by provider (admin only)
pub async fn global_spend_providers(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<SpendProviderQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    spend_providers_inner(
        &state,
        query.start_date.as_deref(),
        query.end_date.as_deref(),
    ).await
}

/// Shared implementation: aggregate spend by provider, post-process with decrypted
/// proxy_models litellm_params to resolve encrypted model→provider mappings.
async fn spend_providers_inner(
    state: &SharedState,
    start_date: Option<&str>,
    end_date: Option<&str>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let aggs = state
        .db
        .aggregate_spend_by_provider(start_date, end_date)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
            )
        })?;

    // Post-process: build model_name → decrypted provider name map from proxy_models
    let provider_map = build_decrypted_provider_map(state).await;

    let data: Vec<Value> = aggs
        .iter()
        .map(|a| {
            // The raw "provider" from DB joins proxy_models on model_name.
            // If litellm_params is encrypted, json_extract returns NULL and falls back to sl.model.
            // Use the decrypted map to resolve the real provider name.
            let provider = provider_map
                .get(&a.provider)
                .cloned()
                .unwrap_or_else(|| a.provider.clone());
            json!({
                "provider": provider,
                "total_tokens": a.total_tokens,
                "total_spend": a.total_spend,
                "requests": a.requests,
            })
        })
        .collect();

    Ok(Json(json!({ "data": data, "count": data.len() })))
}

/// Build a map from model_name → decrypted provider name by listing proxy_models
/// and decrypting their litellm_params.
async fn build_decrypted_provider_map(
    state: &SharedState,
) -> HashMap<String, String> {
    let mk = match state.aigw_master_key.as_deref() {
        Some(k) => k,
        None => {
            tracing::warn!("AIGW_MASTER_KEY not set — provider names may be raw model names instead of decrypted provider identifiers");
            return HashMap::new();
        }
    };

    let models = match state.db.list_models().await {
        Ok(m) => m,
        Err(_) => return HashMap::new(),
    };

    let mut map = HashMap::new();
    for m in &models {
        // litellm_params is a JSON value; extract the "model" field (provider name)
        let provider = if let Some(s) = m.litellm_params.as_str() {
            // Encrypted string — decrypt first, then parse JSON
            decrypt_litellm_value(s, mk)
                .ok()
                .and_then(|decrypted| {
                    serde_json::from_str::<Value>(&decrypted).ok()
                })
                .and_then(|v| {
                    // litellm_params.custom_llm_provider is the actual provider name
                    // (e.g. "openai", "deepseek"), NOT the model field.
                    v.get("custom_llm_provider")
                        .and_then(|p| p.as_str().map(String::from))
                        .or_else(|| v.get("model").and_then(|mv| mv.as_str().map(String::from)))
                })
        } else if let Some(obj) = m.litellm_params.as_object() {
            obj.get("model").and_then(|v| v.as_str().map(String::from))
        } else {
            None
        };

        if let Some(provider) = provider {
            map.insert(m.model_name.clone(), provider);
        }
    }

    map
}

/// GET /global/spend/models — Spend by model (admin only, all keys)
pub async fn global_spend_models(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<SpendModelQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let aggs = state
        .db
        .aggregate_spend_by_model(
            None,
            query.start_date.as_deref(),
            query.end_date.as_deref(),
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
            )
        })?;

    let data: Vec<Value> = aggs
        .iter()
        .map(|a| {
            json!({
                "model": a.model,
                "total_tokens": a.total_tokens,
                "total_spend": a.total_spend,
                "requests": a.requests,
            })
        })
        .collect();

    Ok(Json(json!({ "data": data, "count": data.len() })))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// /spend/model-groups — Spend aggregated by model_group
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Deserialize)]
pub struct SpendModelGroupQuery {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

/// GET /spend/model-groups — Spend by model_group (scoped to authenticated key)
pub async fn spend_model_groups(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<SpendModelGroupQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let api_key = &auth.token_hash;

    let aggs = state
        .db
        .aggregate_spend_by_model_group(
            Some(api_key),
            query.start_date.as_deref(),
            query.end_date.as_deref(),
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
            )
        })?;

    let data: Vec<Value> = aggs
        .iter()
        .map(|a| {
            json!({
                "model_group": a.model_group,
                "total_tokens": a.total_tokens,
                "total_spend": a.total_spend,
                "requests": a.requests,
            })
        })
        .collect();

    Ok(Json(json!({ "data": data, "count": data.len() })))
}

/// GET /global/spend/model-groups — Spend by model_group (admin only, all keys)
pub async fn global_spend_model_groups(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<SpendModelGroupQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let aggs = state
        .db
        .aggregate_spend_by_model_group(
            None,
            query.start_date.as_deref(),
            query.end_date.as_deref(),
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
            )
        })?;

    let data: Vec<Value> = aggs
        .iter()
        .map(|a| {
            json!({
                "model_group": a.model_group,
                "total_tokens": a.total_tokens,
                "total_spend": a.total_spend,
                "requests": a.requests,
            })
        })
        .collect();

    Ok(Json(json!({ "data": data, "count": data.len() })))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// /global/spend/activity — aggregated overview (Stage 38)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Deserialize)]
pub struct ActivityQuery {
    pub start_date: String,
    pub end_date: String,
    pub user_id: Option<String>,
    pub team_id: Option<String>,
    pub organization_id: Option<String>,
}

/// GET /global/spend/activity?start_date=X&end_date=Y[&user_id=...]
///
/// Returns metadata (summary metrics) + daily (per-day aggregation)
/// for the given time range. Optional filters for user/team/organization.
pub async fn global_spend_activity(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let activity = query_activity(&state.db, &query).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    Ok(Json(activity))
}

#[derive(Debug, Serialize)]
struct ActivityMetadata {
    total_spend: f64,
    total_requests: i64,
    successful_requests: i64,
    failed_requests: i64,
    total_tokens: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    // Cache breakdown (extracted from spend_logs.metadata JSON).
    // regular_input is provider-aware: OpenAI-style prompt_tokens already
    // includes cached tokens (so regular = prompt - cache_read - cache_create),
    // Anthropic-style does not (so regular = prompt). See query_activity_* SQL.
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    regular_input_tokens: i64,
    cache_read_spend: f64,
    cache_create_spend: f64,
}

#[derive(Debug, Serialize)]
struct DailyRow {
    date: String,
    spend: f64,
    tokens: i64,
    requests: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    successful_requests: i64,
    failed_requests: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    regular_input_tokens: i64,
    cache_read_spend: f64,
    cache_create_spend: f64,
}

#[derive(Debug, Serialize)]
struct ActivityResult {
    metadata: ActivityMetadata,
    daily: Vec<DailyRow>,
    /// "hourly" when query range ≤ 3 days, "daily" otherwise
    granularity: String,
}

/// Check whether start_date → end_date is 3 days or less (≤72h).
/// Returns true for hourly aggregation, false for daily.
fn is_hourly_range(start_date: &str, end_date: &str) -> bool {
    if let (Ok(s), Ok(e)) = (NaiveDate::parse_from_str(start_date, "%Y-%m-%d"), NaiveDate::parse_from_str(end_date, "%Y-%m-%d")) {
        let days = (e - s).num_days();
        // 3 days or fewer → hourly
        days <= 3
    } else {
        // If parsing fails (e.g. ISO datetime string from API), fall back to daily.
        // We log a warning but recover gracefully.
        tracing::warn!("Could not parse dates as YYYY-MM-DD: start={}, end={}; falling back to daily", start_date, end_date);
        false
    }
}

async fn query_activity(
    db: &Database,
    query: &ActivityQuery,
) -> Result<Value, DbError> {
    let use_hourly = is_hourly_range(&query.start_date, &query.end_date);

    let (metadata, rows): (
        (f64, i64, i64, i64, i64, i64, i64, i64, i64, i64, f64, f64),
        Vec<(String, f64, i64, i64, i64, i64, i64, i64, i64, i64, i64, f64, f64)>,
    ) = if use_hourly {
        tokio::try_join!(
            db.query_activity_metadata(
                &query.start_date,
                &query.end_date,
                query.user_id.as_deref(),
                query.team_id.as_deref(),
                query.organization_id.as_deref(),
            ),
            db.query_activity_hourly(
                &query.start_date,
                &query.end_date,
                query.user_id.as_deref(),
                query.team_id.as_deref(),
                query.organization_id.as_deref(),
            ),
        )?
    } else {
        tokio::try_join!(
            db.query_activity_metadata(
                &query.start_date,
                &query.end_date,
                query.user_id.as_deref(),
                query.team_id.as_deref(),
                query.organization_id.as_deref(),
            ),
            db.query_activity_daily(
                &query.start_date,
                &query.end_date,
                query.user_id.as_deref(),
                query.team_id.as_deref(),
                query.organization_id.as_deref(),
            ),
        )?
    };

    // SQL column order (see query_activity_metadata / sql_base):
    //   spend, total_tokens, requests, successful_requests, failed_requests,
    //   prompt_tokens, completion_tokens,
    //   cache_read_tokens, cache_creation_tokens, regular_input_tokens,
    //   cache_read_spend, cache_create_spend
    let metadata_val = ActivityMetadata {
        total_spend: metadata.0,
        total_tokens: metadata.1,
        total_requests: metadata.2,
        successful_requests: metadata.3,
        failed_requests: metadata.4,
        prompt_tokens: metadata.5,
        completion_tokens: metadata.6,
        cache_read_tokens: metadata.7,
        cache_creation_tokens: metadata.8,
        regular_input_tokens: metadata.9,
        cache_read_spend: metadata.10,
        cache_create_spend: metadata.11,
    };

    let daily_vals: Vec<DailyRow> = rows
        .iter()
        .map(|(date, spend, tokens, requests, prompt_tokens, completion_tokens, successful_requests, failed_requests, cache_read, cache_creation, regular_input, cache_read_spend, cache_create_spend)| DailyRow {
            date: date.clone(),
            spend: *spend,
            tokens: *tokens,
            requests: *requests,
            prompt_tokens: *prompt_tokens,
            completion_tokens: *completion_tokens,
            successful_requests: *successful_requests,
            failed_requests: *failed_requests,
            cache_read_tokens: *cache_read,
            cache_creation_tokens: *cache_creation,
            regular_input_tokens: *regular_input,
            cache_read_spend: *cache_read_spend,
            cache_create_spend: *cache_create_spend,
        })
        .collect();

    Ok(serde_json::to_value(ActivityResult {
        metadata: metadata_val,
        daily: daily_vals,
        granularity: if use_hourly { "hourly".to_string() } else { "daily".to_string() },
    })
    .unwrap_or(json!({})))
}

/// GET /global/spend/keys/rankings?start_date=X&end_date=Y[&limit=N]
///
/// Returns top keys ranked by total spend within the given date range.
#[derive(Debug, Deserialize)]
pub struct KeyRankingsQuery {
    pub start_date: String,
    pub end_date: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 { 10 }

pub async fn global_spend_keys_rankings(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<KeyRankingsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let rankings = state.db.aggregate_spend_by_keys(
        &query.start_date,
        &query.end_date,
        query.limit,
    ).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    Ok(Json(serde_json::to_value(rankings).unwrap_or(json!([]))))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::keys::AppState;
    use aigw_core::db::Database;
    use aigw_core::provider::ProviderRegistry;
    use aigw_core::rate_limiter::RateLimiter;
    use aigw_core::router::{Router as AigwRouter, RouterState};
use aigw_core::resolver::ModelResolver;
use aigw_core::models::SpendLog;
    use axum::{
        body::Body,
        http::{header, Method, Request},
        Router,
    };
    use std::sync::Arc;
    use tower::util::ServiceExt;

    async fn test_app() -> Router {
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");
        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key: Some("sk-master-test-123".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: RouterState::default(),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
  otel_active: false,
            body_archiver: None,            metrics: None,
        });
        Router::new()
            .route("/spend/logs", axum::routing::get(spend_logs))
            .route("/spend/keys", axum::routing::get(spend_keys))
            .route("/spend/users", axum::routing::get(spend_users))
            .route("/spend/tags", axum::routing::get(spend_tags))
            .route("/global/spend", axum::routing::get(global_spend))
            .route("/global/spend/logs/{call_id}", axum::routing::get(global_spend_log_detail))
            .route("/global/spend/logs", axum::routing::get(global_spend_logs))
            .route("/global/spend/keys", axum::routing::get(global_spend_keys))
            .route("/global/spend/activity", axum::routing::get(global_spend_activity))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_spend_requires_auth() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/spend/logs")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_global_spend_with_master() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/global/spend")
            .header(header::AUTHORIZATION, "Bearer sk-master-test-123")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(val.get("spend").and_then(|v| v.as_f64()), Some(0.0));
    }

    #[tokio::test]
    async fn test_global_spend_without_admin() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/global/spend")
            .header(header::AUTHORIZATION, "Bearer non-admin-key")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        // Should fail auth since key doesn't exist
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_spend_tags_missing_param() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/spend/tags")
            .header(header::AUTHORIZATION, "Bearer sk-master-test-123")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_global_spend_log_detail_requires_admin() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/global/spend/logs/missing-id")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_global_spend_log_detail_not_found() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/global/spend/logs/nonexistent-request-id")
            .header(header::AUTHORIZATION, "Bearer sk-master-test-123")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_global_spend_log_detail_found() {
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");
        // Insert a spend log first
        let log = SpendLog {
            call_id: "test-req-001".to_string(),
            request_id: None,
            call_type: "completion".to_string(),
            api_key: "hashed-key".to_string(),
            spend: 0.05,
            total_tokens: 100,
            prompt_tokens: 60,
            completion_tokens: 40,
            start_time: chrono::Utc::now(),
            end_time: chrono::Utc::now(),
            request_duration_ms: Some(500),
            completion_start_time: None,
            model: "gpt-4".to_string(),
            model_id: None,
            model_group: Some("gpt-4-group".to_string()),
            custom_llm_provider: Some("openai".to_string()),
            api_base: Some("https://api.openai.com/v1".to_string()),
            user: Some("test-user".to_string()),
            metadata: None,
            cache_hit: None,
            cache_key: None,
            request_tags: None,
            team_id: None,
            organization_id: None,
            end_user: None,
            requester_ip_address: None,
            messages: Some(json!([{"role": "user", "content": "hello"}])),
            response: Some(json!({"choices": [{"message": {"content": "hi"}}]})),
            session_id: None,
            status: Some("success".to_string()),
            mcp_namespaced_tool_name: None,
            agent_id: None,
            proxy_server_request: None,
            body_archived: false,
            parquet_path: None,
        };
        db.insert_spend_log(&log).await.expect("insert log");

        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key: Some("sk-master-test-123".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: RouterState::default(),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
  otel_active: false,
            body_archiver: None,            metrics: None,
        });

        let app = Router::new()
            .route("/global/spend/logs/{call_id}", axum::routing::get(global_spend_log_detail))
            .with_state(state);

        let request = Request::builder()
            .method(Method::GET)
            .uri("/global/spend/logs/test-req-001")
            .header(header::AUTHORIZATION, "Bearer sk-master-test-123")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(val.get("call_id").and_then(|v| v.as_str()), Some("test-req-001"));
        assert_eq!(val.get("model").and_then(|v| v.as_str()), Some("gpt-4"));
        assert_eq!(val.get("spend").and_then(|v| v.as_f64()), Some(0.05));
        // Body blobs should be present
        assert!(val.get("messages").is_some());
        assert!(val.get("response").is_some());
    }

    #[tokio::test]
    async fn test_spend_logs_list_excludes_body() {
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");
        // Insert a spend log with body
        let log = SpendLog {
            call_id: "test-req-bodyless".to_string(),
            request_id: None,
            call_type: "completion".to_string(),
            api_key: "master_key".to_string(),
            spend: 0.01,
            total_tokens: 10,
            prompt_tokens: 5,
            completion_tokens: 5,
            start_time: chrono::Utc::now(),
            end_time: chrono::Utc::now(),
            request_duration_ms: Some(100),
            completion_start_time: None,
            model: "gpt-4".to_string(),
            model_id: None,
            model_group: Some("gpt-4".to_string()),
            custom_llm_provider: Some("openai".to_string()),
            api_base: None,
            user: None,
            metadata: None,
            cache_hit: None,
            cache_key: None,
            request_tags: None,
            team_id: None,
            organization_id: None,
            end_user: None,
            requester_ip_address: None,
            messages: Some(json!([{"role": "user", "content": "hello"}])),
            response: Some(json!({"choices": [{"message": {"content": "hi"}}]})),
            session_id: None,
            status: Some("success".to_string()),
            mcp_namespaced_tool_name: None,
            agent_id: None,
            proxy_server_request: None,
            body_archived: false,
            parquet_path: None,
        };
        db.insert_spend_log(&log).await.expect("insert log");

        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key: Some("sk-master-test-123".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: RouterState::default(),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
  otel_active: false,
            body_archiver: None,            metrics: None,
        });

        let app = Router::new()
            .route("/global/spend/logs", axum::routing::get(global_spend_logs))
            .with_state(state);

        let request = Request::builder()
            .method(Method::GET)
            .uri("/global/spend/logs?page_size=30")
            .header(header::AUTHORIZATION, "Bearer sk-master-test-123")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        let data = val.get("data").and_then(|v| v.as_array()).unwrap();
        assert!(!data.is_empty());
        let first = &data[0];
        // messages and response should NOT be present in the list response
        assert!(first.get("messages").is_none(), "List endpoint must not include messages field");
        assert!(first.get("response").is_none(), "List endpoint must not include response field");
    }
}
