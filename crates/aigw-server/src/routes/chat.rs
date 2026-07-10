//! OpenAI-compatible chat endpoints — /v1/chat/completions and /v1/models
//!
//! Endpoints:
//! - POST /v1/chat/completions — Chat completions (streaming SSE + non-streaming)
//! - GET  /v1/models           — List available models for the authenticated key

use aigw_core::auth::decode_jwt;
use aigw_core::crypto::{decrypt_json_fields, decrypt_litellm_value, hash_token};
use aigw_core::middleware::KeyIdentity;
use aigw_core::models::{SpendLog, Team, VirtualKey};
use axum::{
    extract::State,
    http::{self, StatusCode},
    response::{sse::{Event, Sse}, IntoResponse},
    Json,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::StreamExt;

use super::keys::SharedState;

/// Resolved upstream routing parameters from proxy_models + credentials lookup.
struct ResolvedUpstream {
    api_base: String,
    api_key: Option<String>,
    model_name: String,
}

/// Look up a model by name in proxy_models, decrypt litellm_params if encrypted,
/// and resolve credential references. Falls back to env vars if model not found.
async fn resolve_upstream_params(
    state: &SharedState,
    model_name: &str,
) -> Result<ResolvedUpstream, (StatusCode, Json<Value>)> {
    // Try to look up the model in proxy_models
    let model = state.db.get_model_by_name(model_name).await.map_err(|e| {
        tracing::warn!("Failed to look up model '{}': {}", model_name, e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("DB error: {}", e), "type": "db_error"}})),
        )
    })?;

    match model {
        Some(m) => {
            // Use as_str() for string values to avoid JSON quoting from to_string()
            let litellm_params_str = m.litellm_params.as_str().map(String::from).unwrap_or_else(|| m.litellm_params.to_string());

            // Detect whether litellm_params is encrypted (base64) or plaintext JSON
            let params_json: Value = if litellm_params_str.starts_with('{') {
                // Plaintext JSON — parse directly
                m.litellm_params.clone()
            } else {
                // Encrypted — decrypt with aigw_master_key
                let key = state.aigw_master_key.as_deref().ok_or_else(|| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": {
                                "message": "Model has encrypted params but AIGW_MASTER_KEY is not configured",
                                "type": "config_error"
                            }
                        })),
                    )
                })?;

                let decrypted = decrypt_litellm_value(&litellm_params_str, key).map_err(|e| {
                    tracing::error!("Failed to decrypt litellm_params for model '{}': {}", model_name, e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": {
                                "message": format!("Failed to decrypt model params: {}", e),
                                "type": "crypto_error"
                            }
                        })),
                    )
                })?;

                serde_json::from_str(&decrypted).unwrap_or_else(|_| json!({}))
            };

            // Decrypt individually encrypted fields inside the JSON object
            // (e.g. api_key, api_base, litellm_credential_name, model).
            let params_json = if let Some(key) = state.aigw_master_key.as_deref() {
                decrypt_json_fields(&params_json, key)
            } else {
                params_json
            };

            // Resolve credential reference if present
            if let Some(cred_name) = params_json
                .get("litellm_credential_name")
                .and_then(|v| v.as_str())
            {
                let cred = state.db.get_credential_by_name(cred_name).await.map_err(|e| {
                    tracing::error!("Failed to look up credential '{}': {}", cred_name, e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": {
                                "message": format!("Credential '{}' not found", cred_name),
                                "type": "not_found"
                            }
                        })),
                    )
                })?;

                let cred = cred.ok_or_else(|| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": {
                                "message": format!("Credential '{}' not found", cred_name),
                                "type": "not_found"
                            }
                        })),
                    )
                })?;

                let cred_values_str = cred.credential_values.to_string();
                let cred_values: Value = if cred_values_str.starts_with('{') {
                    cred.credential_values.clone()
                } else {
                    let key = state.aigw_master_key.as_deref().ok_or_else(|| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({
                                "error": {
                                    "message": "Credential is encrypted but AIGW_MASTER_KEY is not configured",
                                    "type": "config_error"
                                }
                            })),
                        )
                    })?;
                    let decrypted =
                        decrypt_litellm_value(&cred_values_str, key).map_err(|e| {
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(json!({
                                    "error": {
                                        "message": format!("Failed to decrypt credential: {}", e),
                                        "type": "crypto_error"
                                    }
                                })),
                            )
                        })?;
                    serde_json::from_str(&decrypted).unwrap_or_else(|_| json!({}))
                };

                // Decrypt individually encrypted fields inside credential_values
                let cred_values = if let Some(key) = state.aigw_master_key.as_deref() {
                    decrypt_json_fields(&cred_values, key)
                } else {
                    cred_values
                };

                // Merge: credential values take precedence for api_key/api_base,
                // but params_json fields take precedence if already set
                let mut merged = cred_values;
                if let Some(obj) = merged.as_object_mut() {
                    for (k, v) in params_json.as_object().into_iter().flat_map(|o| o.iter()) {
                        if !obj.contains_key(k) {
                            obj.insert(k.clone(), v.clone());
                        }
                    }
                }
                let api_base = merged
                    .get("api_base")
                    .and_then(|v| v.as_str())
                    .unwrap_or("https://api.openai.com/v1")
                    .to_string();
                let api_key = merged.get("api_key").and_then(|v| v.as_str()).map(|s| s.to_string());
                let upstream_model = merged
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or(model_name)
                    .to_string();

                Ok(ResolvedUpstream {
                    api_base,
                    api_key,
                    model_name: upstream_model,
                })
            } else {
                let api_base = params_json
                    .get("api_base")
                    .and_then(|v| v.as_str())
                    .unwrap_or("https://api.openai.com/v1")
                    .to_string();
                let api_key = params_json
                    .get("api_key")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let upstream_model = params_json
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or(model_name)
                    .to_string();

                Ok(ResolvedUpstream {
                    api_base,
                    api_key,
                    model_name: upstream_model,
                })
            }
        }
        None => {
            // Model not found in proxy_models — fall back to env vars
            let api_base = std::env::var("UPSTREAM_LLM_URL")
                .or_else(|_| std::env::var("OPENAI_BASE_URL"))
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
            let api_key = std::env::var("UPSTREAM_API_KEY")
                .or_else(|_| std::env::var("OPENAI_API_KEY"))
                .ok();

            Ok(ResolvedUpstream {
                api_base,
                api_key,
                model_name: model_name.to_string(),
            })
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Auth extraction (reuses the same pattern as spend.rs)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Thin newtype around KeyIdentity for the chat endpoints.
/// Mirrors the SpendAuth pattern from spend.rs to satisfy orphan rules.
pub struct ChatAuth(pub KeyIdentity);

impl std::ops::Deref for ChatAuth {
    type Target = KeyIdentity;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> axum::extract::FromRequestParts<S> for ChatAuth
where
    S: Send + Sync,
    SharedState: axum::extract::FromRef<S>,
{
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let shared_state: SharedState = axum::extract::FromRef::from_ref(state);

        // 1. Try Bearer token from Authorization header
        let bearer_result = Self::try_bearer_token(&shared_state, parts).await;

        if let Ok(auth) = bearer_result {
            return Ok(auth);
        }

        // 2. Fall back to HttpOnly cookie JWT
        Self::try_cookie_jwt(&shared_state, parts).await
    }
}

impl ChatAuth {
    /// Try Bearer token from Authorization header
    async fn try_bearer_token(
        state: &SharedState,
        parts: &http::request::Parts,
    ) -> Result<ChatAuth, (StatusCode, Json<Value>)> {
        let auth_header = parts
            .headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": {"message": "Missing Authorization header", "type": "auth_error"}})),
                )
            })?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": {"message": "Invalid Authorization format. Expected: Bearer <token>", "type": "auth_error"}})),
                )
            })?;

        if token.is_empty() {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(
                    json!({"error": {"message": "Invalid Authorization format", "type": "auth_error"}}),
                ),
            ));
        }

        // Check master key
        if let Some(ref mk) = state.master_key {
            if token == *mk {
                return Ok(ChatAuth(KeyIdentity {
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

        // Hash and DB lookup
        let token_hash = hash_token(token);
        let key = state
            .db
            .get_key_by_token(&token_hash)
            .await
            .map_err(|_| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": {"message": "Invalid API key", "type": "auth_error"}})),
                )
            })?;

        match key {
            Some(k) => Ok(ChatAuth(KeyIdentity {
                token_hash,
                key_alias: k.key_alias,
                user_id: k.user_id,
                team_id: k.team_id,
                organization_id: k.organization_id,
                is_master_key: false,
                user_role: None,
            })),
            None => Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": {"message": "Invalid API key", "type": "auth_error"}})),
            )),
        }
    }

    /// Try HttpOnly cookie JWT
    async fn try_cookie_jwt(
        state: &SharedState,
        parts: &http::request::Parts,
    ) -> Result<ChatAuth, (StatusCode, Json<Value>)> {
        let master_key = state.master_key.as_ref().ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": {"message": "Missing Authorization header", "type": "auth_error"}})),
            )
        })?;

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
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": {"message": "Missing Authorization header", "type": "auth_error"}})),
                )
            })?;

        // Decode JWT
        let claims = decode_jwt(&cookie_value, master_key).map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": {"message": "Invalid API key", "type": "auth_error"}})),
            )
        })?;

        // Hash the key from JWT claims and look up in DB
        let token_hash = hash_token(&claims.key);
        let key = state
            .db
            .get_key_by_token(&token_hash)
            .await
            .map_err(|_| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": {"message": "Invalid API key", "type": "auth_error"}})),
                )
            })?;

        let is_admin = claims.user_role == "proxy_admin";
        match key {
            Some(k) => Ok(ChatAuth(KeyIdentity {
                token_hash,
                key_alias: k.key_alias,
                user_id: k.user_id,
                team_id: k.team_id,
                organization_id: k.organization_id,
                is_master_key: is_admin,
                user_role: Some(claims.user_role.clone()),
            })),
            None => Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": {"message": "Invalid API key", "type": "auth_error"}})),
            )),
        }
    }
}

/// Model entry returned by /v1/models
#[derive(Debug, Serialize)]
pub struct ModelEntry {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Sentinels — litellm-compatible model-list expansion
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const SENTINEL_ALL_TEAM_MODELS: &str = "all-team-models";
const SENTINEL_ALL_PROXY_MODELS: &str = "all-proxy-models";

/// Resolve a key's model allow-list, expanding litellm-compatible sentinel values.
///
/// Returns:
/// - `Ok(None)` — allow all models (null/empty list or `all-proxy-models` sentinel)
/// - `Ok(Some(list))` — restrict to these model names
/// - `Err(...)` — sentinel expansion failed (missing team, etc.)
async fn resolve_key_model_list(
    state: &SharedState,
    key: &VirtualKey,
) -> Result<Option<Vec<String>>, (StatusCode, Json<Value>)> {
    let models = &key.models;
    if models.is_null() {
        return Ok(None);
    }
    let model_list = match models.as_array() {
        Some(a) => a,
        None => return Ok(None),
    };
    if model_list.is_empty() {
        return Ok(None);
    }

    // "all-proxy-models" sentinel → allow everything
    if model_list
        .iter()
        .any(|m| m.as_str() == Some(SENTINEL_ALL_PROXY_MODELS))
    {
        return Ok(None);
    }

    // "all-team-models" sentinel → expand from team
    if model_list
        .iter()
        .any(|m| m.as_str() == Some(SENTINEL_ALL_TEAM_MODELS))
    {
        let team_id = key.team_id.as_deref().ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": {
                        "message": "all-team-models requires team_id",
                        "type": "auth_error"
                    }
                })),
            )
        })?;
        let team = state
            .db
            .get_team_by_id(team_id)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": {
                            "message": format!("Team lookup failed: {}", e),
                            "type": "db_error"
                        }
                    })),
                )
            })?
            .ok_or_else(|| {
                (
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": {
                            "message": "Team not found",
                            "type": "auth_error"
                        }
                    })),
                )
            })?;

        // Recursive: team.models may also contain sentinels
        return resolve_team_model_list(state, &team).await;
    }

    // Literal model list
    Ok(Some(
        model_list
            .iter()
            .filter_map(|m| m.as_str().map(String::from))
            .collect(),
    ))
}

/// Resolve team-level model list with sentinel expansion (recursive).
async fn resolve_team_model_list(
    _state: &SharedState,
    team: &Team,
) -> Result<Option<Vec<String>>, (StatusCode, Json<Value>)> {
    let models = &team.models;
    if models.is_null() {
        return Ok(None);
    }
    let model_list = match models.as_array() {
        Some(a) => a,
        None => return Ok(None),
    };
    if model_list.is_empty() {
        return Ok(None);
    }

    if model_list
        .iter()
        .any(|m| m.as_str() == Some(SENTINEL_ALL_PROXY_MODELS))
        || model_list
            .iter()
            .any(|m| m.as_str() == Some(SENTINEL_ALL_TEAM_MODELS))
    {
        // Sentinel in team.models → expand to all proxy models
        return Ok(None);
    }

    Ok(Some(
        model_list
            .iter()
            .filter_map(|m| m.as_str().map(String::from))
            .collect(),
    ))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Chat completions handler
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// POST /v1/chat/completions
///
/// OpenAI-compatible chat completions endpoint.
/// Validates the request, looks up key permissions, and proxies to the upstream LLM provider.
pub async fn chat_completions(
    State(state): State<SharedState>,
    ChatAuth(auth): ChatAuth,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    // 1. Validate required fields
    let _model = body.get("model").and_then(|v| v.as_str()).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "message": "Missing required field 'model'",
                    "type": "invalid_request_error",
                    "code": null
                }
            })),
        )
    })?;

    let _messages = body
        .get("messages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": "Missing required field 'messages'",
                        "type": "invalid_request_error",
                        "code": null
                    }
                })),
            )
        })?;

    // 2. Validate messages array is not empty
    if _messages.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "message": "'messages' array must not be empty",
                    "type": "invalid_request_error",
                    "code": null
                }
            })),
        ));
    }

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // 3. Look up key permissions from the database
    // Only master key bypasses the model permission check
    if !auth.is_master_key {
        let key_record = state
            .db
            .get_key_by_token(&auth.token_hash)
            .await
            .map_err(|_| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": {"message": "Key lookup failed", "type": "auth_error"}})),
                )
            })?;

        if let Some(key) = key_record {
            // Resolve model list with sentinel expansion
            match resolve_key_model_list(&state, &key).await? {
                Some(allowed_models) => {
                    if !allowed_models.iter().any(|m| m == _model) {
                        return Err((
                            StatusCode::FORBIDDEN,
                            Json(json!({
                                "error": {
                                    "message": format!(
                                        "Model '{}' is not allowed for this API key",
                                        _model
                                    ),
                                    "type": "auth_error",
                                    "code": "model_not_allowed"
                                }
                            })),
                        ));
                    }
                }
                None => { /* allow all models */ }
            }

            // Check budget
            if let Some(max_budget) = key.max_budget_f64() {
                if key.spend >= max_budget {
                    return Err((
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(json!({
                            "error": {
                                "message": "Budget exceeded for this API key",
                                "type": "budget_exceeded",
                                "code": null
                            }
                        })),
                    ));
                }
            }
        }
    }

    // 4. Resolve upstream routing: look up model in proxy_models, decrypt if needed,
    //    resolve credential references. Falls back to env vars if model not found.
    let resolved = resolve_upstream_params(&state, _model).await?;

    // Build upstream request body — inject resolved model name
    let mut upstream_body = body.clone();
    if let Some(obj) = upstream_body.as_object_mut() {
        obj.insert("model".to_string(), json!(resolved.model_name));
    }
    let upstream_url = format!(
        "{}/chat/completions",
        resolved.api_base.trim_end_matches('/')
    );

    // 5. Build and send upstream request
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {
                        "message": format!("HTTP client error: {}", e),
                        "type": "internal_error",
                        "code": null
                    }
                })),
            )
        })?;

    let mut upstream_req = client.post(&upstream_url).json(&upstream_body);

    if let Some(ref api_key) = resolved.api_key {
        upstream_req = upstream_req.header("Authorization", format!("Bearer {}", api_key));
    }

    let start_time = chrono::Utc::now();

    let upstream_resp = upstream_req.send().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": {
                    "message": format!("Upstream request failed: {}", e),
                    "type": "upstream_error",
                    "code": null
                }
            })),
        )
    })?;

    let upstream_status = upstream_resp.status();

    // For streaming, we return the raw SSE response
    // For non-streaming, we parse the JSON and record spend
    if is_stream {
        if !upstream_status.is_success() {
            let error_body = upstream_resp.text().await.unwrap_or_default();
            return Err((
                StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                Json(json!({
                    "error": {
                        "message": format!(
                            "Upstream returned {}: {}",
                            upstream_status.as_u16(),
                            error_body
                        ),
                        "type": "upstream_error",
                        "code": null
                    }
                })),
            ));
        }

        // SSE streaming proxy: forward upstream SSE chunks to client via axum Sse.
        // A background task reads from upstream, captures completion_start_time on the
        // first chunk, and writes a SpendLog after the stream completes.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let state_clone = Arc::clone(&state);
        let model = _model.to_string();
        let api_base = resolved.api_base.clone();
        let token_hash = auth.token_hash.clone();
        let user_id = auth.user_id.clone();
        let team_id = auth.team_id.clone();
        let organization_id = auth.organization_id.clone();
        let request_id = uuid::Uuid::new_v4().to_string();
        let request_body = body.clone();

        tokio::spawn(async move {
            use tokio_stream::StreamExt;
            let mut stream = upstream_resp.bytes_stream();
            let mut first_chunk_time: Option<chrono::DateTime<chrono::Utc>> = None;

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        if first_chunk_time.is_none() && !chunk.is_empty() {
                            first_chunk_time = Some(chrono::Utc::now());
                        }
                        if tx.send(chunk.to_vec()).is_err() {
                            // Client disconnected — stop forwarding
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }

            // Write SpendLog after stream completes
            let now = chrono::Utc::now();
            let spend_log = SpendLog {
                request_id,
                call_type: "completion".to_string(),
                api_key: token_hash,
                spend: 0.0,
                total_tokens: 0,
                prompt_tokens: 0,
                completion_tokens: 0,
                start_time,
                end_time: now,
                request_duration_ms: Some(
                    now.signed_duration_since(start_time).num_milliseconds() as i32
                ),
                completion_start_time: Some(first_chunk_time.unwrap_or(now)),
                model,
                model_id: None,
                model_group: None,
                custom_llm_provider: None,
                api_base: Some(api_base),
                user: user_id,
                metadata: None,
                cache_hit: None,
                cache_key: None,
                request_tags: None,
                team_id,
                organization_id,
                end_user: None,
                requester_ip_address: None,
                messages: Some(request_body),
                response: None, // Streaming — raw chunks not accumulated
                session_id: None,
                status: Some("success".to_string()),
                mcp_namespaced_tool_name: None,
                agent_id: None,
                proxy_server_request: None,
            };
            let _ = state_clone.db.insert_spend_log(&spend_log).await;
        });

        let sse_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
            .map(|data: Vec<u8>| {
                let data_str = String::from_utf8_lossy(&data).to_string();
                Ok::<_, Infallible>(Event::default().data(data_str))
            });

        Ok(Sse::new(sse_stream).into_response())
    } else {
        // Non-streaming: parse upstream response
        let resp_body: Value = upstream_resp.json().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": {
                        "message": format!("Failed to parse upstream response: {}", e),
                        "type": "upstream_error",
                        "code": null
                    }
                })),
            )
        })?;

        if !upstream_status.is_success() {
            return Err((
                StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                Json(resp_body),
            ));
        }

        // 6. Record spend log
        let now = chrono::Utc::now();
        let usage = resp_body.get("usage");

        let spend_log = aigw_core::models::SpendLog {
            request_id: uuid::Uuid::new_v4().to_string(),
            call_type: "completion".to_string(),
            api_key: auth.token_hash.clone(),
            spend: 0.0,
            total_tokens: usage
                .and_then(|u| u.get("total_tokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32,
            prompt_tokens: usage
                .and_then(|u| u.get("prompt_tokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32,
            completion_tokens: usage
                .and_then(|u| u.get("completion_tokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32,
            start_time,
            end_time: now,
            request_duration_ms: Some(
                now.signed_duration_since(start_time).num_milliseconds() as i32
            ),
            completion_start_time: Some(now), // non-streaming sentinel = end_time
            model: _model.to_string(),
            model_id: None,
            model_group: None,
            custom_llm_provider: None,
            api_base: Some(resolved.api_base.clone()),
            user: auth.user_id.clone(),
            metadata: None,
            cache_hit: None,
            cache_key: None,
            request_tags: None,
            team_id: auth.team_id.clone(),
            organization_id: auth.organization_id.clone(),
            end_user: None,
            requester_ip_address: None,
            messages: Some(body.clone()),
            response: Some(resp_body.clone()),
            session_id: None,
            status: Some("success".to_string()),
            mcp_namespaced_tool_name: None,
            agent_id: None,
            proxy_server_request: None,
        };

        // Record spend log (don't fail the request if logging fails)
        let _ = state.db.insert_spend_log(&spend_log).await;

        Ok(Json(resp_body).into_response())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Models list handler
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// GET /v1/models
///
/// Returns the list of models the authenticated key has access to.
/// For the master key, returns all available models.
pub async fn models_list(
    State(state): State<SharedState>,
    ChatAuth(auth): ChatAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Look up the key to get its model permissions
    let mut model_ids: Vec<String> = Vec::new();

    if auth.is_master_key {
        // Master key sees all models registered in proxy_models table
        let models = state
            .db
            .list_models()
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": {"message": "Failed to list models", "type": "db_error"}})),
                )
            })?;
        model_ids = models.into_iter().map(|m| m.model_name).collect();
    } else {
        let key_record = state
            .db
            .get_key_by_token(&auth.token_hash)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(
                        json!({"error": {"message": "Failed to look up key", "type": "db_error"}}),
                    ),
                )
            })?;

        if let Some(key) = key_record {
            if let Some(models) = key.models.as_array() {
                for model in models {
                    if let Some(s) = model.as_str() {
                        model_ids.push(s.to_string());
                    }
                }
            }
        }
    }

    let now_ts = chrono::Utc::now().timestamp();
    let data: Vec<ModelEntry> = model_ids
        .into_iter()
        .map(|id| ModelEntry {
            id: id.clone(),
            object: "model".to_string(),
            created: now_ts,
            owned_by: "aigw".to_string(),
        })
        .collect();

    Ok(Json(json!({
        "object": "list",
        "data": data,
    })))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::keys::AppState;
    use aigw_core::db::Database;
    use aigw_core::models::{ProxyModel, VirtualKey};
    use aigw_core::provider::ProviderRegistry;
    use aigw_core::rate_limiter::RateLimiter;
    use axum::{
        body::Body,
        http::{header, Method, Request},
        Router,
    };
    use std::collections::HashMap;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    async fn test_app() -> Router {
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");
        let state = Arc::new(AppState {
            db,
            master_key: Some("sk-master-chat-test".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
        });

        Router::new()
            .route(
                "/v1/chat/completions",
                axum::routing::post(chat_completions),
            )
            .route("/v1/models", axum::routing::get(models_list))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_chat_completions_missing_model() {
        let app = test_app().await;

        let body = json!({
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer sk-master-chat-test")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        let error_msg = val["error"]["message"].as_str().unwrap();
        assert!(
            error_msg.contains("model"),
            "Expected 'model' error, got: {}",
            error_msg
        );
    }

    #[tokio::test]
    async fn test_chat_completions_missing_messages() {
        let app = test_app().await;

        let body = json!({
            "model": "gpt-4"
        });

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer sk-master-chat-test")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        let error_msg = val["error"]["message"].as_str().unwrap();
        assert!(
            error_msg.contains("messages"),
            "Expected 'messages' error, got: {}",
            error_msg
        );
    }

    #[tokio::test]
    async fn test_chat_completions_requires_auth() {
        let app = test_app().await;

        let body = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_models_list_requires_auth() {
        let app = test_app().await;

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_models_list_with_auth() {
        let db = Database::init("sqlite::memory:").await.expect("init sqlite");

        // Insert a model so the models_list endpoint has data to return
        let model = ProxyModel {
            model_id: uuid::Uuid::new_v4().to_string(),
            model_name: "gpt-4".to_string(),
            litellm_params: json!({"model": "gpt-4"}),
            model_info: json!({}),
            created_at: chrono::Utc::now().to_rfc3339(),
            created_by: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
            updated_by: None,
        };
        db.insert_model(&model).await.expect("insert model");

        let state = Arc::new(AppState {
            db,
            master_key: Some("sk-master-chat-test".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
        });

        let app = Router::new()
            .route(
                "/v1/chat/completions",
                axum::routing::post(chat_completions),
            )
            .route("/v1/models", axum::routing::get(models_list))
            .with_state(state);

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models")
            .header(header::AUTHORIZATION, "Bearer sk-master-chat-test")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(val["object"].as_str(), Some("list"));
        assert!(val["data"].as_array().unwrap().len() > 0);
    }

    #[tokio::test]
    async fn test_chat_completions_empty_messages() {
        let app = test_app().await;

        let body = json!({
            "model": "gpt-4",
            "messages": []
        });

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer sk-master-chat-test")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_chat_completions_invalid_auth() {
        let app = test_app().await;

        let body = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer invalid-key-12345")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_models_list_with_valid_key() {
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");

        // Insert a key with specific model permissions
        let raw_key = "sk-test-models-key";
        let token_hash = aigw_core::crypto::hash_token(raw_key);
        let key = VirtualKey {
            token: token_hash.clone(),
            key_name: Some("test-key".to_string()),
            key_alias: Some("test-alias".to_string()),
            soft_budget_cooldown: "false".to_string(),
            spend: 0.0,
            expires: None,
            models: json!(["gpt-4", "gpt-3.5-turbo"]),
            aliases: json!({}),
            config: json!({}),
            router_settings: None,
            user_id: Some("test-user".to_string()),
            team_id: None,
            agent_id: None,
            project_id: None,
            permissions: json!({}),
            max_parallel_requests: None,
            metadata: json!({}),
            blocked: None,
            tpm_limit: None,
            rpm_limit: None,
            max_budget: None,
            budget_duration: None,
            budget_reset_at: None,
            allowed_cache_controls: json!([]),
            allowed_routes: json!([]),
            policies: json!([]),
            access_group_ids: json!([]),
            model_spend: json!({}),
            model_max_budget: json!({}),
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
        };

        db.insert_key(&key).await.expect("insert key");

        let state = Arc::new(AppState {
            db,
            master_key: None,
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
        });

        let app = Router::new()
            .route("/v1/models", axum::routing::get(models_list))
            .with_state(state);

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models")
            .header(header::AUTHORIZATION, format!("Bearer {}", raw_key))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(val["object"].as_str(), Some("list"));
        let models = val["data"].as_array().unwrap();
        let model_ids: Vec<&str> = models.iter().filter_map(|m| m["id"].as_str()).collect();
        assert!(model_ids.contains(&"gpt-4"));
        assert!(model_ids.contains(&"gpt-3.5-turbo"));
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Sentinel resolution tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    fn make_test_state(db: Database) -> SharedState {
        Arc::new(AppState {
            db,
            master_key: Some("sk-master-test".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
        })
    }

    fn make_key(models_json: Value, team_id: Option<&str>) -> VirtualKey {
        VirtualKey {
            token: "hash".into(),
            key_name: None,
            key_alias: None,
            soft_budget_cooldown: "false".into(),
            spend: 0.0,
            expires: None,
            models: models_json,
            aliases: json!({}),
            config: json!({}),
            router_settings: None,
            user_id: None,
            team_id: team_id.map(String::from),
            agent_id: None,
            project_id: None,
            permissions: json!({}),
            max_parallel_requests: None,
            metadata: json!({}),
            blocked: None,
            tpm_limit: None,
            rpm_limit: None,
            max_budget: None,
            budget_duration: None,
            budget_reset_at: None,
            allowed_cache_controls: json!([]),
            allowed_routes: json!([]),
            policies: json!([]),
            access_group_ids: json!([]),
            model_spend: json!({}),
            model_max_budget: json!({}),
            budget_id: None,
            organization_id: None,
            object_permission_id: None,
            created_at: None,
            created_by: None,
            updated_at: None,
            updated_by: None,
            last_active: None,
            rotation_count: None,
            auto_rotate: None,
            rotation_interval: None,
            last_rotation_at: None,
            key_rotation_at: None,
            budget_limits: None,
        }
    }

    #[tokio::test]
    async fn test_resolve_null_models_returns_none() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let state = make_test_state(db);
        let key = make_key(Value::Null, None);

        let result = resolve_key_model_list(&state, &key).await.unwrap();
        assert!(result.is_none(), "null models should allow all");
    }

    #[tokio::test]
    async fn test_resolve_empty_array_returns_none() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let state = make_test_state(db);
        let key = make_key(json!([]), None);

        let result = resolve_key_model_list(&state, &key).await.unwrap();
        assert!(result.is_none(), "empty array should allow all");
    }

    #[tokio::test]
    async fn test_resolve_literal_list() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let state = make_test_state(db);
        let key = make_key(json!(["gpt-4", "gpt-3.5-turbo"]), None);

        let result = resolve_key_model_list(&state, &key).await.unwrap();
        let list = result.expect("should be Some");
        assert_eq!(list, vec!["gpt-4", "gpt-3.5-turbo"]);
    }

    #[tokio::test]
    async fn test_resolve_all_proxy_models_returns_none() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let state = make_test_state(db);
        let key = make_key(json!(["all-proxy-models"]), None);

        let result = resolve_key_model_list(&state, &key).await.unwrap();
        assert!(result.is_none(), "all-proxy-models should allow all");
    }

    #[tokio::test]
    async fn test_resolve_all_team_models_no_team_id_returns_error() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let state = make_test_state(db);
        let key = make_key(json!(["all-team-models"]), None);

        let result = resolve_key_model_list(&state, &key).await;
        assert!(result.is_err(), "missing team_id should error");
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_resolve_all_team_models_with_valid_team() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let state = make_test_state(db);

        // Insert a team with specific models
        let team = Team {
            team_id: "team-1".into(),
            team_alias: Some("test-team".into()),
            organization_id: None,
            object_permission_id: None,
            admins: json!([]),
            members: json!([]),
            members_with_roles: json!([]),
            metadata: json!({}),
            max_budget: None,
            soft_budget: None,
            spend: 0.0,
            models: json!(["team-model-a", "team-model-b"]),
            max_parallel_requests: None,
            tpm_limit: None,
            rpm_limit: None,
            budget_duration: None,
            budget_reset_at: None,
            blocked: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            model_spend: json!({}),
            model_max_budget: json!({}),
            router_settings: None,
            team_member_permissions: json!([]),
            access_group_ids: json!([]),
            policies: json!([]),
            default_team_member_models: json!([]),
            budget_limits: None,
            model_id: None,
            allow_team_guardrail_config: false,
        };
        state.db.insert_team(&team).await.unwrap();

        let key = make_key(json!(["all-team-models"]), Some("team-1"));
        let result = resolve_key_model_list(&state, &key).await.unwrap();
        let list = result.expect("should be Some");
        assert_eq!(list, vec!["team-model-a", "team-model-b"]);
    }

    #[tokio::test]
    async fn test_resolve_all_team_models_recursive_expansion() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let state = make_test_state(db);

        // Team whose models also contains "all-team-models" sentinel
        let team = Team {
            team_id: "team-2".into(),
            team_alias: Some("recursive-team".into()),
            organization_id: None,
            object_permission_id: None,
            admins: json!([]),
            members: json!([]),
            members_with_roles: json!([]),
            metadata: json!({}),
            max_budget: None,
            soft_budget: None,
            spend: 0.0,
            models: json!(["all-team-models"]),
            max_parallel_requests: None,
            tpm_limit: None,
            rpm_limit: None,
            budget_duration: None,
            budget_reset_at: None,
            blocked: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            model_spend: json!({}),
            model_max_budget: json!({}),
            router_settings: None,
            team_member_permissions: json!([]),
            access_group_ids: json!([]),
            policies: json!([]),
            default_team_member_models: json!([]),
            budget_limits: None,
            model_id: None,
            allow_team_guardrail_config: false,
        };
        state.db.insert_team(&team).await.unwrap();

        let key = make_key(json!(["all-team-models"]), Some("team-2"));
        let result = resolve_key_model_list(&state, &key).await.unwrap();
        // Recursive sentinel in team.models → allow all
        assert!(result.is_none(), "recursive sentinel should allow all");
    }

    #[tokio::test]
    async fn test_resolve_mixed_list_with_sentinel() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let state = make_test_state(db);

        let team = Team {
            team_id: "team-3".into(),
            team_alias: Some("mixed-team".into()),
            organization_id: None,
            object_permission_id: None,
            admins: json!([]),
            members: json!([]),
            members_with_roles: json!([]),
            metadata: json!({}),
            max_budget: None,
            soft_budget: None,
            spend: 0.0,
            models: json!(["team-model-x"]),
            max_parallel_requests: None,
            tpm_limit: None,
            rpm_limit: None,
            budget_duration: None,
            budget_reset_at: None,
            blocked: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            model_spend: json!({}),
            model_max_budget: json!({}),
            router_settings: None,
            team_member_permissions: json!([]),
            access_group_ids: json!([]),
            policies: json!([]),
            default_team_member_models: json!([]),
            budget_limits: None,
            model_id: None,
            allow_team_guardrail_config: false,
        };
        state.db.insert_team(&team).await.unwrap();

        // Key with ["all-team-models", "extra-model"] — sentinel takes priority
        let key = make_key(json!(["all-team-models", "extra-model"]), Some("team-3"));
        let result = resolve_key_model_list(&state, &key).await.unwrap();
        let list = result.expect("should expand from team");
        assert_eq!(list, vec!["team-model-x"]);
    }
}
