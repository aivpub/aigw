//! Key management endpoints — litellm-compatible /key/* routes
//!
//! Endpoints:
//! - POST   /key/generate     — Generate new API key
//! - GET    /key/info         — Get key info by token
//! - GET    /key/list         — List all keys
//! - PUT    /key/update       — Update key
//! - DELETE /key/delete       — Delete key
//! - POST   /key/regenerate   — Regenerate key (new token, copy config)

use aigw_core::crypto::hash_token;
use aigw_core::daily_spend_queue::DailySpendQueue;
use aigw_core::db::Database;
use aigw_core::body_archive::BodyArchiver;
use aigw_core::metrics::MetricsRecorder;
use aigw_core::models::{GenerateKeyRequest, VirtualKey};
use aigw_core::provider::ProviderRegistry;
use aigw_core::rate_limiter::RateLimiter;
use aigw_core::resolver::ModelResolver;
use aigw_core::router::{Router as AigwRouter, RouterState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;


use super::spend::{require_admin, SpendAuth};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// AppState
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone)]
pub struct AppState {
    /// Model resolver — model_name → Vec<Deployment>
    pub resolver: ModelResolver,
    /// Phase 23 Router — picks deployment + retry loop
    pub router: AigwRouter,
    pub db: Database,
    pub master_key: Option<String>,
    pub aigw_master_key: Option<String>, // for decrypting litellm_params at runtime
    /// Prometheus metrics recorder (global registry, initialized at startup, None in tests)
    #[allow(dead_code)]
    pub metrics: Option<Arc<MetricsRecorder>>,
    #[allow(dead_code)]
    pub provider_registry: ProviderRegistry,
    #[allow(dead_code)]
    pub router_state: RouterState,
    #[allow(dead_code)]
    pub rate_limiter: Arc<RateLimiter>,
    pub deployment_mode: String, // "onprem" or "saas"
    pub started_at: std::time::Instant,
    #[allow(dead_code)]
    pub daily_spend_queue: Option<Arc<DailySpendQueue>>,
    /// OTEL tracing active flag — true when OTLP exporter is configured and running.
    /// Handler code gates traceparent extract/inject on this field.
    pub otel_active: bool,
    /// Body archiver — archive spend_logs body fields to Parquet cold storage.
    pub body_archiver: Option<Arc<BodyArchiver>>,
}

impl AppState {
    /// Create a test AppState with defaults for provider_registry, router_state,
    /// rate_limiter, and resolver (empty, no DB dependency for non-chat tests).
    #[allow(dead_code)]
    pub fn for_test(
        db: Database,
        master_key: Option<String>,
        aigw_master_key: Option<String>,
        deployment_mode: String,
    ) -> Self {
        Self {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key,
            aigw_master_key,
            metrics: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode,
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
        }
    }
}

pub type SharedState = Arc<AppState>;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Request/Response types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Query parameters for /key/info
#[derive(Debug, Deserialize)]
pub struct KeyInfoQuery {
    pub key: Option<String>,
    pub key_alias: Option<String>,
    #[allow(dead_code)]
    pub user_id: Option<String>,
    #[allow(dead_code)]
    pub team_id: Option<String>,
}

/// Query parameters for /key/list
#[derive(Debug, Deserialize)]
pub struct KeyListQuery {
    pub team_id: Option<String>,
    pub user_id: Option<String>,
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

/// Query parameters for /key/delete
#[derive(Debug, Deserialize)]
pub struct KeyDeleteQuery {
    pub key: Option<String>,
    pub key_aliases: Option<String>,
}

/// Key generation response (litellm-compatible)
#[derive(Debug, Serialize)]
pub struct GenerateKeyResponse {
    pub key: String,
    pub key_name: Option<String>,
    pub key_alias: Option<String>,
    pub token: Option<String>, // hashed token, matches litellm field
    pub user_id: Option<String>,
    pub team_id: Option<String>,
    pub organization_id: Option<String>,
    pub project_id: Option<String>,
    pub models: Value,
    pub max_budget: Option<f64>,
    pub budget_duration: Option<String>,
    pub budget_reset_at: Option<String>,
    pub tpm_limit: Option<i64>,
    pub rpm_limit: Option<i64>,
    pub max_parallel_requests: Option<i32>,
    pub spend: f64,
    pub expires: Option<String>,
    pub blocked: Option<bool>,
    pub metadata: Value,
    pub permissions: Value,
    pub auto_rotate: Option<bool>,
    pub rotation_interval: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helper functions
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Generate a litellm-compatible key token: sk- + 22 base64url chars
pub fn generate_key_token() -> String {
    let mut buf = [0u8; 16];
    for b in &mut buf {
        *b = fastrand::u8(..);
    }
    let encoded = base64url_encode(&buf);
    format!("sk-{}", &encoded[..22])
}

/// URL-safe base64 encode (no padding)
fn base64url_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = chunk.len();
        let b0 = chunk[0] as u32;
        let b1 = if n > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if n > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(char::from(CHARS[((triple >> 18) & 0x3F) as usize]));
        out.push(char::from(CHARS[((triple >> 12) & 0x3F) as usize]));
        if n > 1 {
            out.push(char::from(CHARS[((triple >> 6) & 0x3F) as usize]));
        }
        if n > 2 {
            out.push(char::from(CHARS[(triple & 0x3F) as usize]));
        }
    }
    out
}

/// Parse an RFC3339 string to `DateTime<Utc>`.
pub fn parse_rfc3339(s: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helper: build VirtualKey from GenerateKeyRequest
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn build_virtual_key(hash: &str, req: &GenerateKeyRequest) -> VirtualKey {
    let now = Utc::now();
    VirtualKey {
        token: hash.to_string(),
        key_name: req.key_name.clone().or_else(|| req.key_alias.clone()),
        key_alias: req.key_alias.clone(),
        soft_budget_cooldown: "false".to_string(),
        spend: 0.0,
        expires: req.expires,
        models: req
            .models
            .as_ref()
            .map(|m| json!(m))
            .unwrap_or_else(|| json!([])),
        aliases: json!({}),
        config: json!({}),
        router_settings: None,
        user_id: req.user_id.clone(),
        team_id: req.team_id.clone(),
        agent_id: None,
        project_id: req.project_id.clone(),
        permissions: req.permissions.clone().unwrap_or_else(|| json!({})),
        max_parallel_requests: req.max_parallel_requests.map(|v| v.to_string()),
        metadata: req.metadata.clone().unwrap_or_else(|| json!({})),
        blocked: None,
        tpm_limit: req.tpm_limit.map(|v| v.to_string()),
        rpm_limit: req.rpm_limit.map(|v| v.to_string()),
        max_budget: req.max_budget.map(|v| v.to_string()),
        budget_duration: req.budget_duration.clone(),
        budget_reset_at: req.budget_reset_at,
        allowed_cache_controls: json!([]),
        allowed_routes: json!([]),
        policies: json!([]),
        access_group_ids: json!([]),
        model_spend: json!({}),
        model_max_budget: json!({}),
        budget_id: None,
        organization_id: req.organization_id.clone(),
        object_permission_id: None,
        created_at: Some(now),
        created_by: None,
        updated_at: Some(now),
        updated_by: None,
        last_active: None,
        rotation_count: None,
        auto_rotate: req.auto_rotate.map(|v| v.to_string()),
        rotation_interval: req.rotation_interval.clone(),
        last_rotation_at: None,
        key_rotation_at: None,
        budget_limits: None,
        user_email: None,
        user_alias: None,
    }
}

/// Convert a VirtualKey to a GenerateKeyResponse
fn key_to_response(raw_token: &str, key: &VirtualKey) -> GenerateKeyResponse {
    GenerateKeyResponse {
        key: raw_token.to_string(),
        key_name: key.key_name.clone(),
        key_alias: key.key_alias.clone(),
        token: Some(key.token.clone()),
        user_id: key.user_id.clone(),
        team_id: key.team_id.clone(),
        organization_id: key.organization_id.clone(),
        project_id: key.project_id.clone(),
        models: key.models.clone(),
        max_budget: key.max_budget_f64(),
        budget_duration: key.budget_duration.clone(),
        budget_reset_at: key.budget_reset_at.map(|dt| dt.to_rfc3339()),
        tpm_limit: key.tpm_limit_i64(),
        rpm_limit: key.rpm_limit_i64(),
        max_parallel_requests: key.max_parallel_requests_i32(),
        spend: key.spend,
        expires: key.expires.map(|dt| dt.to_rfc3339()),
        blocked: key.blocked,
        metadata: key.metadata.clone(),
        permissions: key.permissions.clone(),
        auto_rotate: key.auto_rotate_bool(),
        rotation_interval: key.rotation_interval.clone(),
        created_at: key.created_at.map(|dt| dt.to_rfc3339()),
        updated_at: key.updated_at.map(|dt| dt.to_rfc3339()),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Handlers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// POST /key/generate — Generate a new virtual API key (admin only)
pub async fn generate_key(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(req): Json<GenerateKeyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let raw_token = req
        .key
        .clone()
        .unwrap_or_else(|| generate_key_token());
    let hash = hash_token(&raw_token);

    // Check if key already exists
    if let Ok(Some(_)) = state.db.get_key_by_token(&hash).await {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": {"message": "Key already exists", "type": "duplicate_key"}})),
        ));
    }

    let vkey = build_virtual_key(&hash, &req);

    state.db.insert_key(&vkey).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    let response = key_to_response(&raw_token, &vkey);
    Ok(Json(json!(response)))
}

/// GET /key/info — Get key info by token (admin only)
pub async fn key_info(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<KeyInfoQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let token = query
        .key
        .as_deref()
        .or(query.key_alias.as_deref())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": {"message": "Missing key or key_alias parameter", "type": "bad_request"}})),
            )
        })?;

    // For key_alias lookups, we need to search; for key hashes we do direct lookup
    let hash = hash_token(token);
    let key = state.db.get_key_by_token(&hash).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    match key {
        Some(k) => {
            let response = key_to_response(token, &k);
            Ok(Json(json!(response)))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "Key not found", "type": "not_found"}})),
        )),
    }
}

/// GET /key/list — List all keys with optional filters (admin only)
/// Server-side paginated (page/page_size), mirrors /global/spend/logs.
pub async fn key_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<KeyListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).max(1).min(100);
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;

    let (keys, total_count) = tokio::try_join!(
        state.db.list_keys_paged(
            query.team_id.as_deref(),
            query.user_id.as_deref(),
            limit,
            offset,
        ),
        state.db.count_keys(query.team_id.as_deref(), query.user_id.as_deref()),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    let data: Vec<Value> = keys
        .iter()
        .map(|k| {
            json!({
                "token": k.token,
                "key_name": k.key_name,
                "key_alias": k.key_alias,
                "user_id": k.user_id,
                "user_email": k.user_email,
                "user_alias": k.user_alias,
                "team_id": k.team_id,
                "spend": k.spend,
                "max_budget": k.max_budget_f64(),
                "max_parallel_requests": k.max_parallel_requests_i32(),
                "tpm_limit": k.tpm_limit_i64(),
                "rpm_limit": k.rpm_limit_i64(),
                "blocked": k.blocked,
                "expires": k.expires.map(|e| e.to_rfc3339()),
                "models": k.models,
                "metadata": k.metadata,
                "created_at": k.created_at.map(|e| e.to_rfc3339()),
            })
        })
        .collect();

    let total_pages = if total_count > 0 {
        ((total_count as f64) / (page_size as f64)).ceil() as i64
    } else {
        0
    };

    Ok(Json(json!({
        "keys": data,
        "data": data,
        "count": data.len(),
        "total_count": total_count,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

/// PUT /key/update — Update a key (admin only)
pub async fn key_update(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let token_param = body
        .get("key")
        .and_then(|v| v.as_str())
        .or_else(|| body.get("key_alias").and_then(|v| v.as_str()))
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": {"message": "Missing 'key' field in request body", "type": "bad_request"}})),
            )
        })?;

    let hash = hash_token(token_param);

    // Fetch existing key
    let _existing = state.db.get_key_by_token(&hash).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    // Build an updated VirtualKey from the request body
    let now = Utc::now();
    let updated = VirtualKey {
        token: hash.clone(),
        key_name: body
            .get("key_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        key_alias: body
            .get("key_alias")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        soft_budget_cooldown: body
            .get("soft_budget_cooldown")
            .and_then(|v| v.as_bool())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "false".to_string()),
        spend: body.get("spend").and_then(|v| v.as_f64()).unwrap_or(0.0),
        expires: body
            .get("expires")
            .and_then(|v| v.as_str())
            .and_then(parse_rfc3339),
        models: body.get("models").cloned().unwrap_or_else(|| json!([])),
        aliases: body.get("aliases").cloned().unwrap_or_else(|| json!({})),
        config: body.get("config").cloned().unwrap_or_else(|| json!({})),
        router_settings: body.get("router_settings").cloned(),
        user_id: body
            .get("user_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        team_id: body
            .get("team_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        agent_id: body
            .get("agent_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        project_id: body
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        permissions: body
            .get("permissions")
            .cloned()
            .unwrap_or_else(|| json!({})),
        max_parallel_requests: body
            .get("max_parallel_requests")
            .and_then(|v| v.as_i64())
            .map(|v| v.to_string()),
        metadata: body.get("metadata").cloned().unwrap_or_else(|| json!({})),
        blocked: body.get("blocked").and_then(|v| v.as_bool()),
        tpm_limit: body.get("tpm_limit").and_then(|v| v.as_i64()).map(|v| v.to_string()),
        rpm_limit: body.get("rpm_limit").and_then(|v| v.as_i64()).map(|v| v.to_string()),
        max_budget: body.get("max_budget").and_then(|v| v.as_f64()).map(|v| v.to_string()),
        budget_duration: body
            .get("budget_duration")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        budget_reset_at: body
            .get("budget_reset_at")
            .and_then(|v| v.as_str())
            .and_then(parse_rfc3339),
        allowed_cache_controls: body
            .get("allowed_cache_controls")
            .cloned()
            .unwrap_or_else(|| json!([])),
        allowed_routes: body
            .get("allowed_routes")
            .cloned()
            .unwrap_or_else(|| json!([])),
        policies: body.get("policies").cloned().unwrap_or_else(|| json!([])),
        access_group_ids: body
            .get("access_group_ids")
            .cloned()
            .unwrap_or_else(|| json!([])),
        model_spend: body
            .get("model_spend")
            .cloned()
            .unwrap_or_else(|| json!({})),
        model_max_budget: body
            .get("model_max_budget")
            .cloned()
            .unwrap_or_else(|| json!({})),
        budget_id: body
            .get("budget_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        organization_id: body
            .get("organization_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        object_permission_id: body
            .get("object_permission_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        created_at: None, // preserve original
        created_by: None,
        updated_at: Some(now),
        updated_by: body
            .get("updated_by")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        last_active: None,
        rotation_count: body
            .get("rotation_count")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32),
        auto_rotate: body.get("auto_rotate").and_then(|v| v.as_bool()).map(|v| v.to_string()),
        rotation_interval: body
            .get("rotation_interval")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        last_rotation_at: None,
        key_rotation_at: None,
        budget_limits: body.get("budget_limits").cloned(),
        user_email: None,
        user_alias: None,
    };

    state.db.update_key(&hash, &updated).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    Ok(Json(
        json!({"status": "ok", "message": "Key updated successfully"}),
    ))
}

/// DELETE /key/delete — Delete (soft-delete) a key (admin only)
pub async fn key_delete(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<KeyDeleteQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let token_param = query
        .key
        .as_deref()
        .or(query.key_aliases.as_deref())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": {"message": "Missing key or key_aliases parameter", "type": "bad_request"}})),
            )
        })?;

    let hash = hash_token(token_param);

    // Check if key exists
    let exists = state.db.get_key_by_token(&hash).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    if exists.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "Key not found", "type": "not_found"}})),
        ));
    }

    state.db.delete_key(&hash).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    Ok(Json(
        json!({"status": "ok", "message": "Key deleted successfully"}),
    ))
}

/// GET /key/deleted — list archived (soft-deleted) keys (paginated)
pub async fn key_deleted_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<KeyListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).max(1).min(100);
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;

    let (keys, total_count) = tokio::try_join!(
        state.db.list_deleted_keys_paged(limit, offset),
        state.db.count_deleted_keys(),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("{}", e)})),
        )
    })?;

    let data: Vec<Value> = serde_json::to_value(&keys).unwrap_or(json!([]))
        .as_array().cloned().unwrap_or_default();
    let total_pages = if total_count > 0 {
        ((total_count as f64) / (page_size as f64)).ceil() as i64
    } else {
        0
    };

    Ok(Json(json!({
        "keys": data,
        "data": data,
        "count": data.len(),
        "total_count": total_count,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

/// POST /key/regenerate — Regenerate a key (new token, copy config) (admin only)
pub async fn key_regenerate(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let old_token = body
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": {"message": "Missing 'key' field in request body", "type": "bad_request"}})),
            )
        })?;

    let old_hash = hash_token(old_token);

    // Fetch existing key
    let existing = state
        .db
        .get_key_by_token(&old_hash)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": {"message": "Key not found", "type": "not_found"}})),
            )
        })?;

    // Generate new token
    let new_raw = generate_key_token();
    let new_hash = hash_token(&new_raw);

    // Copy existing key with new token
    let now = Utc::now();
    let mut new_key = existing.clone();
    new_key.token = new_hash.clone();
    new_key.created_at = Some(now);
    new_key.updated_at = Some(now);
    new_key.expires = body
        .get("new_expiry")
        .and_then(|v| v.as_str())
        .and_then(parse_rfc3339); // Retain key_alias from body if provided
    if let Some(alias) = body.get("key_alias").and_then(|v| v.as_str()) {
        new_key.key_alias = Some(alias.to_string());
    }

    // Insert new key
    state.db.insert_key(&new_key).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    // Delete old key
    let _ = state.db.delete_key(&old_hash).await;

    let response = key_to_response(&new_raw, &new_key);
    Ok(Json(json!(response)))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Integration tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use aigw_core::db::Database;
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
            master_key: Some("sk-master-test".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: RouterState::default(),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            metrics: None,
            otel_active: false,
            body_archiver: None,
        });
        Router::new()
            .route("/key/generate", axum::routing::post(generate_key))
            .route("/key/info", axum::routing::get(key_info))
            .route("/key/list", axum::routing::get(key_list))
            .route("/key/update", axum::routing::put(key_update))
            .route("/key/delete", axum::routing::delete(key_delete))
            .route("/key/regenerate", axum::routing::post(key_regenerate))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_key_generate_requires_auth() {
        let app = test_app().await;
        let body = json!({"key_alias": "my-test-key", "models": ["gpt-4"]});
        let request = Request::builder()
            .method(Method::POST)
            .uri("/key/generate")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_key_generate_endpoint() {
        let app = test_app().await;

        let body = json!({
            "key_alias": "my-test-key",
            "models": ["gpt-4"],
            "max_budget": 50.0,
            "tpm_limit": 1000,
            "rpm_limit": 200,
        });

        let request = Request::builder()
            .method(Method::POST)
            .uri("/key/generate")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer sk-master-test")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json_val: Value = serde_json::from_slice(&body_bytes).unwrap();

        assert!(json_val.get("key").is_some());
        assert!(json_val
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap()
            .starts_with("sk-"));
        assert_eq!(
            json_val.get("key_alias").and_then(|v| v.as_str()),
            Some("my-test-key")
        );
        assert_eq!(
            json_val.get("max_budget").and_then(|v| v.as_f64()),
            Some(50.0)
        );
        assert_eq!(
            json_val.get("tpm_limit").and_then(|v| v.as_i64()),
            Some(1000)
        );
    }

    #[tokio::test]
    async fn test_key_info_endpoint() {
        let app = test_app().await;

        // First generate a key
        let body = json!({
            "key_alias": "info-test",
            "models": ["gpt-3.5-turbo"],
        });
        let request = Request::builder()
            .method(Method::POST)
            .uri("/key/generate")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer sk-master-test")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let gen_resp: Value = serde_json::from_slice(&body_bytes).unwrap();
        let raw_key = gen_resp
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string();

        // Now query info
        let request = Request::builder()
            .method(Method::GET)
            .uri(format!("/key/info?key={}", raw_key))
            .header(header::AUTHORIZATION, "Bearer sk-master-test")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let info: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            info.get("key_alias").and_then(|v| v.as_str()),
            Some("info-test")
        );
    }

    #[tokio::test]
    async fn test_key_list_endpoint() {
        let app = test_app().await;

        // Generate two keys
        for alias in &["list-a", "list-b"] {
            let body = json!({"key_alias": alias, "models": ["gpt-4"]});
            let request = Request::builder()
                .method(Method::POST)
                .uri("/key/generate")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-master-test")
                .body(Body::from(body.to_string()))
                .unwrap();
            let _ = app.clone().oneshot(request).await.unwrap();
        }

        // List keys
        let request = Request::builder()
            .method(Method::GET)
            .uri("/key/list")
            .header(header::AUTHORIZATION, "Bearer sk-master-test")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: Value = serde_json::from_slice(&body_bytes).unwrap();
        let keys = list.get("keys").and_then(|v| v.as_array()).unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[tokio::test]
    async fn test_key_delete_endpoint() {
        let app = test_app().await;

        // Generate a key
        let body = json!({"key_alias": "delete-me"});
        let request = Request::builder()
            .method(Method::POST)
            .uri("/key/generate")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer sk-master-test")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let gen_resp: Value = serde_json::from_slice(&body_bytes).unwrap();
        let raw_key = gen_resp
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string();

        // Delete it
        let request = Request::builder()
            .method(Method::DELETE)
            .uri(format!("/key/delete?key={}", raw_key))
            .header(header::AUTHORIZATION, "Bearer sk-master-test")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        // Verify gone
        let request = Request::builder()
            .method(Method::GET)
            .uri(format!("/key/info?key={}", raw_key))
            .header(header::AUTHORIZATION, "Bearer sk-master-test")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 404);
    }

    #[tokio::test]
    async fn test_key_regenerate_endpoint() {
        let app = test_app().await;

        // Generate original key
        let body = json!({"key_alias": "regen-orig", "models": ["gpt-4"]});
        let request = Request::builder()
            .method(Method::POST)
            .uri("/key/generate")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer sk-master-test")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let gen_resp: Value = serde_json::from_slice(&body_bytes).unwrap();
        let old_key = gen_resp
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string();

        // Regenerate
        let regen_body = json!({"key": old_key, "key_alias": "regen-new"});
        let request = Request::builder()
            .method(Method::POST)
            .uri("/key/regenerate")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer sk-master-test")
            .body(Body::from(regen_body.to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let regen_resp: Value = serde_json::from_slice(&body_bytes).unwrap();
        let new_key = regen_resp
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string();
        assert_ne!(new_key, old_key);
        assert_eq!(
            regen_resp.get("key_alias").and_then(|v| v.as_str()),
            Some("regen-new")
        );

        // Old key should be gone
        let request = Request::builder()
            .method(Method::GET)
            .uri(format!("/key/info?key={}", old_key))
            .header(header::AUTHORIZATION, "Bearer sk-master-test")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 404);
    }
}
