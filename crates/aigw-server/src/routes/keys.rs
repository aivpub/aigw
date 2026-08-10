//! Key management endpoints — litellm-compatible /key/* routes
//!
//! Endpoints:
//! - POST   /key/generate     — Generate new API key
//! - GET    /key/info         — Get key info by token
//! - GET    /key/list         — List all keys
//! - PUT    /key/update       — Update key
//! - DELETE /key/delete       — Delete key
//! - POST   /key/regenerate   — Regenerate key (new token, copy config)

use aigw_core::body_archive::BodyArchiver;
use aigw_core::crypto::hash_token;
use aigw_core::daily_spend_queue::DailySpendQueue;
use aigw_core::db::Database;
use aigw_core::metrics::MetricsRecorder;
use aigw_core::middleware::KeyIdentity;
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
    /// litellm `general_settings.custom_key_generate_length` — payload length
    /// for `/key/generate` tokens. Default 22.
    pub key_generate_length: usize,
    /// litellm `general_settings.disable_custom_api_keys` — when true, only the
    /// master key may create new keys.
    pub disable_custom_api_keys: bool,
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
            key_generate_length: DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: false,
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

/// Validate key budget against parent user/team quotas.
/// Called before key create/update when max_budget is set.
async fn validate_key_budget(
    db: &Database,
    key_max_budget: f64,
    user_id: Option<&str>,
    team_id: Option<&str>,
) -> Result<(), (StatusCode, Json<Value>)> {
    // Check against user budget
    if let Some(uid) = user_id {
        if let Ok(Some(user)) = db.get_user_by_id(uid).await {
            if let Some(user_max) = user.max_budget_f64() {
                if user_max > 0.0 && key_max_budget > user_max {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(
                            json!({"error": {"message": "Key budget cannot exceed user budget", "type": "budget_violation"}}),
                        ),
                    ));
                }
            }
        }
    }

    // Check against team budget
    if let Some(tid) = team_id {
        if let Ok(Some(team)) = db.get_team_by_id(tid).await {
            if let Some(team_max) = team.max_budget_f64() {
                if team_max > 0.0 && key_max_budget > team_max {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(
                            json!({"error": {"message": "Key budget cannot exceed team budget", "type": "budget_violation"}}),
                        ),
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Generate a litellm-compatible key token: `sk-` + base64url chars.
///
/// Default length is 22 chars (litellm's default). `custom_key_generate_length`
/// from `general_settings` can override via `generate_key_token_with_len`.
pub fn generate_key_token() -> String {
    generate_key_token_with_len(DEFAULT_KEY_TOKEN_LEN)
}

/// Default number of base64url chars after `sk-` in a generated key token.
pub const DEFAULT_KEY_TOKEN_LEN: usize = 22;

/// Generate a key token with a configurable payload length (litellm
/// `custom_key_generate_length`). The length is clamped to a sane range so a
/// misconfigured value cannot produce degenerate tokens.
pub fn generate_key_token_with_len(len: usize) -> String {
    let n = len.clamp(MIN_KEY_TOKEN_LEN, MAX_KEY_TOKEN_LEN);
    // 3 bytes encode to exactly 4 base64url chars; round up so we always have
    // enough bytes to slice `n` chars.
    let byte_count = (n * 3).div_ceil(4);
    let mut buf = vec![0u8; byte_count];
    for b in &mut buf {
        *b = fastrand::u8(..);
    }
    let encoded = base64url_encode(&buf);
    format!("sk-{}", &encoded[..n])
}

/// Minimum generated payload length (16 chars) — prevents absurdly short keys.
pub const MIN_KEY_TOKEN_LEN: usize = 16;
/// Maximum generated payload length (64 chars) — prevents runaway memory from a
/// config typo (e.g. `custom_key_generate_length: 1000000`).
pub const MAX_KEY_TOKEN_LEN: usize = 64;

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
        soft_budget: None,
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
/// Check if the auth identity has admin privileges.
fn is_admin(auth: &KeyIdentity) -> bool {
    auth.is_master_key || auth.user_role.as_deref() == Some("proxy_admin")
}

/// Check key ownership: admin sees all, non-admin only sees own keys.
/// Returns true if the authenticated user is allowed to access this key.
fn check_key_ownership(auth: &KeyIdentity, key_user_id: Option<&str>) -> bool {
    if is_admin(auth) {
        return true;
    }
    // Non-admin can only access keys belonging to themselves
    key_user_id == auth.user_id.as_deref()
}

// Handlers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// POST /key/generate — Generate a new virtual API key
///
/// Admin users can specify any `user_id` to create keys for others.
/// Non-admin users automatically get keys assigned to their own user_id.
pub async fn generate_key(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(mut req): Json<GenerateKeyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Custom api keys can be disabled via general_settings.disable_custom_api_keys
    // (litellm semantics): only the master key may mint keys when enabled.
    if state.disable_custom_api_keys && !auth.is_master_key {
        return Err((
            StatusCode::FORBIDDEN,
            Json(
                json!({"error": {"message": "Custom API keys are disabled", "type": "forbidden"}}),
            ),
        ));
    }

    // Admin can assign keys to anyone; non-admin gets their own user_id forced
    if !is_admin(&auth) {
        // Non-admin: if user_id is set and different from auth user_id, reject
        if let Some(ref uid) = req.user_id {
            if Some(uid.as_str()) != auth.user_id.as_deref() {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(
                        json!({"error": {"message": "Cannot create keys for other users", "type": "forbidden"}}),
                    ),
                ));
            }
        }
        // Auto-assign to the authenticated user
        if req.user_id.is_none() {
            req.user_id = auth.user_id.clone();
        }
    }

    let raw_token = req
        .key
        .clone()
        .unwrap_or_else(|| generate_key_token_with_len(state.key_generate_length));
    let hash = hash_token(&raw_token);

    // Check if key already exists
    if let Ok(Some(_)) = state.db.get_key_by_token(&hash).await {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": {"message": "Key already exists", "type": "duplicate_key"}})),
        ));
    }

    let vkey = build_virtual_key(&hash, &req);

    // Validate budget hierarchy: key budget cannot exceed parent user/team budget
    if let Some(kb) = vkey.max_budget_f64() {
        if kb > 0.0 {
            validate_key_budget(
                &state.db,
                kb,
                vkey.user_id.as_deref(),
                vkey.team_id.as_deref(),
            )
            .await?;
        }
    }

    state.db.insert_key(&vkey).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    let response = key_to_response(&raw_token, &vkey);
    Ok(Json(json!(response)))
}

/// GET /key/info — Get key info by token
///
/// Admin users can look up any key. Non-admin users can only look up their own keys.
pub async fn key_info(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<KeyInfoQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
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
            if !check_key_ownership(&auth, k.user_id.as_deref()) {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(json!({"error": {"message": "Access denied", "type": "forbidden"}})),
                ));
            }
            let response = key_to_response(token, &k);
            Ok(Json(json!(response)))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "Key not found", "type": "not_found"}})),
        )),
    }
}

/// GET /key/list — List keys with optional filters
///
/// Admin users see all keys. Non-admin users only see their own keys.
/// Server-side paginated (page/page_size), mirrors /global/spend/logs.
pub async fn key_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<KeyListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Non-admin: force user_id filter to their own
    let effective_user_id = if is_admin(&auth) {
        query.user_id.clone()
    } else {
        auth.user_id.clone()
    };
    let effective_team_id = if is_admin(&auth) {
        query.team_id.clone()
    } else {
        None
    };

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).clamp(1, 100);
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;

    let (keys, total_count) = tokio::try_join!(
        state.db.list_keys_paged(
            effective_team_id.as_deref(),
            effective_user_id.as_deref(),
            limit,
            offset,
        ),
        state
            .db
            .count_keys(effective_team_id.as_deref(), effective_user_id.as_deref()),
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
                "budget_duration": k.budget_duration,
                "soft_budget": k.soft_budget.as_ref().and_then(|s| s.parse::<f64>().ok()),
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

/// PUT /key/update — Update a key
///
/// Admin users can update any key. Non-admin users can only update their own keys.
pub async fn key_update(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
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

    // If the token is already a 64-char hex SHA256 hash (e.g. from /key/list response),
    // use it directly; otherwise hash the raw token.
    let already_hashed =
        token_param.len() == 64 && token_param.chars().all(|c| c.is_ascii_hexdigit());
    let hash = if already_hashed {
        token_param.to_string()
    } else {
        hash_token(token_param)
    };

    // Fetch existing key
    let existing = state.db.get_key_by_token(&hash).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    let Some(existing) = existing else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "Key not found", "type": "not_found"}})),
        ));
    };

    // Check ownership for non-admin users
    if !check_key_ownership(&auth, existing.user_id.as_deref()) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": {"message": "Access denied", "type": "forbidden"}})),
        ));
    }

    // Non-admin cannot change user_id to a different user
    if !is_admin(&auth) {
        if let Some(new_uid) = body.get("user_id").and_then(|v| v.as_str()) {
            if new_uid != auth.user_id.as_deref().unwrap_or("") {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(
                        json!({"error": {"message": "Cannot reassign key to another user", "type": "forbidden"}}),
                    ),
                ));
            }
        }
    }

    // Build an updated VirtualKey from the request body
    // IMPORTANT: only include fields explicitly sent by the client.
    // Use existing values as defaults so that partial updates (like
    // setting only budget_duration) don't wipe unrelated columns.
    let now = Utc::now();
    let updated = VirtualKey {
        token: hash.clone(),
        key_name: body
            .get("key_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| existing.key_name.clone()),
        key_alias: body
            .get("key_alias")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| existing.key_alias.clone()),
        soft_budget_cooldown: body
            .get("soft_budget_cooldown")
            .and_then(|v| v.as_bool())
            .map(|v| v.to_string())
            .unwrap_or_else(|| existing.soft_budget_cooldown.clone()),
        spend: body
            .get("spend")
            .and_then(|v| v.as_f64())
            .unwrap_or(existing.spend),
        expires: body
            .get("expires")
            .and_then(|v| v.as_str())
            .and_then(parse_rfc3339)
            .or(existing.expires),
        models: body
            .get("models")
            .cloned()
            .unwrap_or_else(|| existing.models.clone()),
        aliases: body
            .get("aliases")
            .cloned()
            .unwrap_or_else(|| existing.aliases.clone()),
        config: body
            .get("config")
            .cloned()
            .unwrap_or_else(|| existing.config.clone()),
        router_settings: body
            .get("router_settings")
            .cloned()
            .or_else(|| existing.router_settings.clone()),
        user_id: body
            .get("user_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| existing.user_id.clone()),
        team_id: body
            .get("team_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| existing.team_id.clone()),
        agent_id: body
            .get("agent_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| existing.agent_id.clone()),
        project_id: body
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| existing.project_id.clone()),
        permissions: body
            .get("permissions")
            .cloned()
            .unwrap_or_else(|| existing.permissions.clone()),
        max_parallel_requests: body
            .get("max_parallel_requests")
            .and_then(|v| v.as_i64())
            .map(|v| v.to_string())
            .or_else(|| existing.max_parallel_requests.clone()),
        metadata: body
            .get("metadata")
            .cloned()
            .unwrap_or_else(|| existing.metadata.clone()),
        blocked: body
            .get("blocked")
            .and_then(|v| v.as_bool())
            .or(existing.blocked),
        tpm_limit: body
            .get("tpm_limit")
            .and_then(|v| v.as_i64())
            .map(|v| v.to_string())
            .or_else(|| existing.tpm_limit.clone()),
        rpm_limit: body
            .get("rpm_limit")
            .and_then(|v| v.as_i64())
            .map(|v| v.to_string())
            .or_else(|| existing.rpm_limit.clone()),
        max_budget: body
            .get("max_budget")
            .and_then(|v| v.as_f64())
            .map(|v| v.to_string())
            .or_else(|| existing.max_budget.clone()),
        soft_budget: body
            .get("soft_budget")
            .and_then(|v| v.as_f64())
            .map(|v| v.to_string())
            .or_else(|| existing.soft_budget.clone()),
        // budget_duration and budget_reset_at: only override when the key is
        // present in the body, allowing explicit clear (empty string → None).
        budget_duration: {
            let key_present = body.get("budget_duration").is_some();
            if key_present {
                body.get("budget_duration")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
            } else {
                existing.budget_duration.clone()
            }
        },
        budget_reset_at: body
            .get("budget_reset_at")
            .and_then(|v| v.as_str())
            .and_then(parse_rfc3339)
            .or(existing.budget_reset_at),
        allowed_cache_controls: body
            .get("allowed_cache_controls")
            .cloned()
            .unwrap_or_else(|| existing.allowed_cache_controls.clone()),
        allowed_routes: body
            .get("allowed_routes")
            .cloned()
            .unwrap_or_else(|| existing.allowed_routes.clone()),
        policies: body
            .get("policies")
            .cloned()
            .unwrap_or_else(|| existing.policies.clone()),
        access_group_ids: body
            .get("access_group_ids")
            .cloned()
            .unwrap_or_else(|| existing.access_group_ids.clone()),
        model_spend: body
            .get("model_spend")
            .cloned()
            .unwrap_or_else(|| existing.model_spend.clone()),
        model_max_budget: body
            .get("model_max_budget")
            .cloned()
            .unwrap_or_else(|| existing.model_max_budget.clone()),
        budget_id: body
            .get("budget_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| existing.budget_id.clone()),
        organization_id: body
            .get("organization_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| existing.organization_id.clone()),
        object_permission_id: body
            .get("object_permission_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| existing.object_permission_id.clone()),
        created_at: existing.created_at, // preserve original
        created_by: existing.created_by.clone(),
        updated_at: Some(now),
        updated_by: body
            .get("updated_by")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        last_active: existing.last_active,
        rotation_count: body
            .get("rotation_count")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32)
            .or(existing.rotation_count),
        auto_rotate: body
            .get("auto_rotate")
            .and_then(|v| v.as_bool())
            .map(|v| v.to_string())
            .or_else(|| existing.auto_rotate.clone()),
        rotation_interval: body
            .get("rotation_interval")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| existing.rotation_interval.clone()),
        last_rotation_at: existing.last_rotation_at,
        key_rotation_at: existing.key_rotation_at,
        budget_limits: body
            .get("budget_limits")
            .cloned()
            .or_else(|| existing.budget_limits.clone()),
        user_email: existing.user_email.clone(),
        user_alias: existing.user_alias.clone(),
    };

    // Validate budget hierarchy: key budget cannot exceed parent user/team budget
    if let Some(kb) = updated.max_budget_f64() {
        if kb > 0.0 {
            validate_key_budget(
                &state.db,
                kb,
                updated.user_id.as_deref(),
                updated.team_id.as_deref(),
            )
            .await?;
        }
    }

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

/// DELETE /key/delete — Delete (soft-delete) a key
///
/// Admin users can delete any key. Non-admin users can only delete their own keys.
pub async fn key_delete(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<KeyDeleteQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
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

    let Some(key) = exists else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "Key not found", "type": "not_found"}})),
        ));
    };

    // Check ownership for non-admin users
    if !check_key_ownership(&auth, key.user_id.as_deref()) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": {"message": "Access denied", "type": "forbidden"}})),
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

/// GET /key/deleted — list archived (soft-deleted) keys (paginated, admin only)
pub async fn key_deleted_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<KeyListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).clamp(1, 100);
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

    let data: Vec<Value> = serde_json::to_value(&keys)
        .unwrap_or(json!([]))
        .as_array()
        .cloned()
        .unwrap_or_default();
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

/// POST /key/regenerate — Regenerate a key (new token, copy config)
///
/// Admin users can regenerate any key. Non-admin users can only regenerate their own keys.
pub async fn key_regenerate(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
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

    // Check ownership for non-admin users
    if !check_key_ownership(&auth, existing.user_id.as_deref()) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": {"message": "Access denied", "type": "forbidden"}})),
        ));
    }

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
            key_generate_length: DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: false,
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
    async fn test_key_generate_honors_custom_length() {
        // Build a state with a custom key_generate_length and verify the minted
        // token carries `sk-` + that many base64url chars.
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");
        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key: Some("sk-master-test".to_string()),
            aigw_master_key: None,
            key_generate_length: 32,
            disable_custom_api_keys: false,
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
        let app = Router::new()
            .route("/key/generate", axum::routing::post(generate_key))
            .with_state(state);
        let body = json!({"key_alias": "custom-len-key"});
        let request = Request::builder()
            .method(Method::POST)
            .uri("/key/generate")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer sk-master-test")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: Value = serde_json::from_slice(&bytes).unwrap();
        let key = resp.get("key").and_then(|v| v.as_str()).unwrap();
        assert!(key.starts_with("sk-"));
        // payload = 32 chars, token = "sk-" + 32
        assert_eq!(key.len(), 3 + 32);
    }

    #[tokio::test]
    async fn test_generate_key_token_with_len_clamps() {
        // Clamped to [MIN, MAX] so a config typo cannot produce degenerate keys.
        let too_short = generate_key_token_with_len(2);
        assert_eq!(too_short.len(), 3 + MIN_KEY_TOKEN_LEN);
        let too_long = generate_key_token_with_len(10_000);
        assert_eq!(too_long.len(), 3 + MAX_KEY_TOKEN_LEN);
        let default = generate_key_token_with_len(DEFAULT_KEY_TOKEN_LEN);
        assert_eq!(default.len(), 3 + DEFAULT_KEY_TOKEN_LEN);
        let _ = generate_key_token(); // smoke: default wrapper still works
    }

    #[tokio::test]
    async fn test_key_generate_disabled_custom_keys_rejects_non_master() {
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");
        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key: Some("sk-master-test".to_string()),
            aigw_master_key: None,
            key_generate_length: DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: true,
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
        let app = Router::new()
            .route("/key/generate", axum::routing::post(generate_key))
            .with_state(state.clone());
        // A non-master user key (role "user") tries to mint a key -> forbidden.
        // Seed a user + session key via the same path the BDD steps use.
        let user_id = "non-master-user";
        let hashed = aigw_core::password::hash_password("pw").expect("hash");
        let session_token = generate_key_token();
        let session_hash = hash_token(&session_token);
        let user = aigw_core::models::User {
            user_id: user_id.to_string(),
            user_alias: None,
            team_id: None,
            sso_user_id: None,
            organization_id: None,
            object_permission_id: None,
            password: Some(hashed),
            teams: serde_json::json!([]),
            user_role: Some("user".to_string()),
            max_budget: None,
            spend: 0.0,
            user_email: Some("user@example.com".to_string()),
            models: serde_json::json!([]),
            metadata: serde_json::json!({}),
            max_parallel_requests: None,
            tpm_limit: None,
            rpm_limit: None,
            budget_duration: None,
            budget_reset_at: None,
            allowed_cache_controls: serde_json::json!([]),
            policies: serde_json::json!([]),
            model_spend: serde_json::json!({}),
            model_max_budget: serde_json::json!({}),
            virtual_keys_count: None,
            created_at: Some(chrono::Utc::now()),
            updated_at: Some(chrono::Utc::now()),
        };
        state.db.insert_user(&user).await.expect("insert user");
        let session_key = aigw_core::models::VirtualKey {
            token: session_hash.clone(),
            key_name: None,
            key_alias: Some("non-master-session".to_string()),
            soft_budget_cooldown: String::new(),
            spend: 0.0,
            expires: None,
            models: serde_json::json!([]),
            aliases: serde_json::json!({}),
            config: serde_json::json!({}),
            router_settings: None,
            user_id: Some(user_id.to_string()),
            team_id: None,
            agent_id: None,
            project_id: None,
            permissions: serde_json::json!({}),
            max_parallel_requests: None,
            metadata: serde_json::json!({}),
            blocked: None,
            tpm_limit: None,
            rpm_limit: None,
            max_budget: None,
            soft_budget: None,
            budget_duration: None,
            budget_reset_at: None,
            allowed_cache_controls: serde_json::json!([]),
            allowed_routes: serde_json::json!([]),
            policies: serde_json::json!([]),
            access_group_ids: serde_json::json!([]),
            model_spend: serde_json::json!({}),
            model_max_budget: serde_json::json!({}),
            budget_id: None,
            organization_id: None,
            object_permission_id: None,
            created_at: Some(chrono::Utc::now()),
            created_by: None,
            updated_at: Some(chrono::Utc::now()),
            updated_by: None,
            last_active: None,
            rotation_count: None,
            auto_rotate: None,
            rotation_interval: None,
            last_rotation_at: None,
            key_rotation_at: None,
            budget_limits: None,
            user_email: None,
            user_alias: None,
        };
        state
            .db
            .insert_key(&session_key)
            .await
            .expect("insert session key");
        // JWT cookie so the SpendAuth middleware resolves a non-master identity.
        let claims = aigw_core::auth::JwtClaims {
            user_id: user_id.to_string(),
            key: session_token,
            user_email: Some("user@example.com".to_string()),
            user_role: "user".to_string(),
            login_method: "username_password".to_string(),
        };
        let jwt = aigw_core::auth::encode_jwt(&claims, "sk-master-test").expect("jwt");
        let cookie = format!("token={}; HttpOnly; SameSite=Lax; Path=/", jwt);

        let body = json!({"key_alias": "non-master-key"});
        let request = Request::builder()
            .method(Method::POST)
            .uri("/key/generate")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::COOKIE, cookie)
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_key_generate_disabled_custom_keys_allows_master() {
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");
        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key: Some("sk-master-test".to_string()),
            aigw_master_key: None,
            key_generate_length: DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: true,
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
        let app = Router::new()
            .route("/key/generate", axum::routing::post(generate_key))
            .with_state(state);
        let body = json!({"key_alias": "master-key"});
        let request = Request::builder()
            .method(Method::POST)
            .uri("/key/generate")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer sk-master-test")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
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
