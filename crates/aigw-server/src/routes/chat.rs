//! OpenAI-compatible chat endpoints — /v1/chat/completions and /v1/models
//!
//! Endpoints:
//! - POST /v1/chat/completions — Chat completions (streaming SSE + non-streaming)
//! - GET  /v1/models           — List available models for the authenticated key

use aigw_core::adapter::{ClientProtocol, select_adapter};
use aigw_core::auth::decode_jwt;
use aigw_core::crypto::{decrypt_json_fields, decrypt_litellm_value, hash_token};
use aigw_core::middleware::KeyIdentity;
use aigw_core::models::{DailySpendKind, DailySpendLog, SpendLog, Team, VirtualKey};
use aigw_core::resolver::ModelResolver;
use axum::{
    extract::State,
    http::{self, StatusCode, header},
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::StreamExt;

use super::keys::SharedState;

/// Resolved upstream routing parameters from proxy_models + credentials lookup.
pub(crate) struct ResolvedUpstream {
    pub(crate) api_base: String,
    pub(crate) api_key: Option<String>,
    pub(crate) model_name: String,
    /// USD per input token (from model_info JSON)
    pub(crate) input_cost_per_token: Option<f64>,
    /// USD per output token (from model_info JSON)
    pub(crate) output_cost_per_token: Option<f64>,
    /// proxy_models UUID (model_id)
    pub(crate) model_id: Option<String>,
    /// litellm_params.model — upstream model name for model_group
    pub(crate) model_group: Option<String>,
    /// litellm_params.custom_llm_provider — e.g. "openai", "anthropic"
    pub(crate) custom_llm_provider: Option<String>,
}

/// Extract pricing — primary from model_info (litellm-standard cost calculator source),
/// fallback to decrypted litellm_params (where custom pricing is defined in proxy config).
///
/// In litellm, pricing is mirrored between columns (see docs/litellm-cost-tracing.md):
///   - model_info is the authoritative cost lookup location
///   - litellm_params is where users set pricing; Deployment.__init__ mirrors it to model_info
fn extract_pricing(model_info: &Value, params_json: &Value) -> (Option<f64>, Option<f64>) {
    let input = model_info
        .get("input_cost_per_token")
        .and_then(|v| v.as_f64())
        .or_else(|| params_json.get("input_cost_per_token").and_then(|v| v.as_f64()));
    let output = model_info
        .get("output_cost_per_token")
        .and_then(|v| v.as_f64())
        .or_else(|| params_json.get("output_cost_per_token").and_then(|v| v.as_f64()));
    (input, output)
}

/// Calculate spend from token counts and per-token pricing.
/// Returns 0.0 if no pricing data is available.
pub(crate) fn calc_spend(prompt_tokens: i32, completion_tokens: i32, input_cost: Option<f64>, output_cost: Option<f64>) -> f64 {
    let input = prompt_tokens as f64 * input_cost.unwrap_or(0.0);
    let output = completion_tokens as f64 * output_cost.unwrap_or(0.0);
    input + output
}

/// Look up a model by name in proxy_models, decrypt litellm_params if encrypted,
/// and resolve credential references. Falls back to env vars if model not found.
pub(crate) async fn resolve_upstream_params(
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
                params_json.clone()
            };

            // Extract pricing: model_info is the primary source (litellm-standard),
            // decrypted litellm_params is the fallback for deployments where pricing
            // was set only in proxy config and not mirrored to model_info.
            let (input_cost, output_cost) = extract_pricing(&m.model_info, &params_json);

            // Extract model_group / custom_llm_provider from proxy_models for SpendLog
            let model_id = Some(m.model_id.clone());
            let model_group = params_json
                .get("model")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let custom_llm_provider = params_json
                .get("custom_llm_provider")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

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
                    input_cost_per_token: input_cost,
                    output_cost_per_token: output_cost,
                    model_id,
                    model_group: model_group.clone(),
                    custom_llm_provider: custom_llm_provider.clone(),
                })
            } else {
                tracing::warn!(%model_name, "resolve_upstream_params: NO credential reference, using litellm_params directly");
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

                tracing::warn!(%model_name, %api_base, ?api_key, %upstream_model, "resolve_upstream_params: DIRECT PARAMS RESOLVED");
                Ok(ResolvedUpstream {
                    api_base,
                    api_key,
                    model_name: upstream_model,
                    input_cost_per_token: input_cost,
                    output_cost_per_token: output_cost,
                    model_id,
                    model_group,
                    custom_llm_provider,
                })
            }
        }
        None => {
            // Fallback to env vars when model is not in proxy_models.
            // Only in non-test deployment modes; test mode (BDD mock) must fail
            // fast with model_not_found so assertions on error types are stable.
            if state.deployment_mode != "test" {
                let api_key_env = std::env::var("OPENAI_API_KEY").ok()
                    .or_else(|| std::env::var("OPENAPI_KEY").ok());
                let api_base_env = std::env::var("OPENAI_BASE_URL")
                    .or_else(|_| std::env::var("OPENAPI_BASE_URL"))
                    .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

                if api_key_env.is_some() {
                    tracing::info!(
                        %model_name,
                        api_base = %api_base_env,
                        "Model not in proxy_models, falling back to env vars"
                    );
                    return Ok(ResolvedUpstream {
                        api_base: api_base_env,
                        api_key: api_key_env,
                        model_name: model_name.to_string(),
                        input_cost_per_token: None,
                        output_cost_per_token: None,
                        model_id: None,
                        model_group: None,
                        custom_llm_provider: None,
                    });
                }
            }
            Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": format!("Model '{}' not found. Add it to proxy_models or check model_name spelling.", model_name),
                        "type": "invalid_request_error",
                        "code": "model_not_found"
                    }
                })),
            ))
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
    headers: axum::http::HeaderMap,
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

    // 4. Resolve upstream via ModelResolver + select adapter
    let deployments = state.resolver.resolve(_model).await?;
    let deployment = deployments.into_iter().next().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "message": format!("Model '{}' not found", _model),
                    "type": "invalid_request_error",
                    "code": "model_not_found"
                }
            })),
        )
    })?;
    let adapter = select_adapter(ClientProtocol::OpenAI, &deployment.provider_type)
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": format!("Unsupported provider type for model '{}'", _model),
                        "type": "invalid_request_error",
                        "code": "unsupported_provider"
                    }
                })),
            )
        })?;

    // Build upstream request body via adapter
    let upstream_body_val = adapter.adapt_request(body.clone(), &deployment).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("Adapter error: {}", e), "type": "adapter_error"}})),
        )
    })?;
    let upstream_url = format!(
        "{}/chat/completions",
        deployment.api_base.trim_end_matches('/')
    );

    // Extract end_user from metadata.user_id (Anthropic protocol convention)
    let end_user = body
        .get("metadata")
        .and_then(|m| m.get("user_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Try to parse session_id from JSON blob (Claude Code convention)
    let session_id = end_user.as_ref().and_then(|eu| {
        serde_json::from_str::<Value>(eu)
            .ok()
            .and_then(|v| {
                v.get("session_id")
                    .and_then(|id| id.as_str())
                    .map(|s| s.to_string())
            })
    });

    let requester_ip: Option<String> = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty());

    // Extract User-Agent from HTTP header (align with litellm: store in metadata.user_agent)
    let user_agent: Option<String> = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Extract device_id from metadata.user_id JSON (Claude Code convention)
    let device_id: Option<String> = end_user.as_ref().and_then(|eu| {
        serde_json::from_str::<Value>(eu)
            .ok()
            .and_then(|v| {
                v.get("device_id")
                    .and_then(|id| id.as_str())
                    .map(|s| s.to_string())
            })
    });

    // Build metadata JSON with user_agent and device_id (align with litellm)
    let metadata: Option<Value> = if user_agent.is_some() || device_id.is_some() {
        let mut meta_map = serde_json::Map::new();
        if let Some(ref ua) = user_agent {
            meta_map.insert("user_agent".to_string(), json!(ua));
        }
        if let Some(ref did) = device_id {
            meta_map.insert("device_id".to_string(), json!(did));
        }
        Some(Value::Object(meta_map))
    } else {
        None
    };

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

    let mut upstream_req = client.post(&upstream_url).json(&upstream_body_val);

    if let Some(ref api_key) = deployment.api_key {
        upstream_req = upstream_req.header("Authorization", format!("Bearer {}", api_key));
    }

    // Build proxy_server_request (align with litellm: url/method/headers/arrival_time)
    use std::time::{SystemTime, UNIX_EPOCH};
    let arrival_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let proxy_server_request = Some(json!({
        "url": "/v1/chat/completions",
        "method": "POST",
        "headers": {
            "user-agent": user_agent.clone().unwrap_or_default(),
            "x-forwarded-for": requester_ip.as_deref().unwrap_or(""),
        },
        "arrival_time": arrival_time,
    }));

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
            // Record failure spend log before returning error
            let fail_upstream_body = upstream_body_val.clone();
            let fail_model = deployment.upstream_model.clone();
            let fail_api_base = deployment.api_base.clone();
            let fail_model_id = deployment.model_id.clone();
            let fail_model_group = deployment.model_group.clone();
            let fail_custom_llm_provider = deployment.custom_llm_provider.clone();
            let fail_token_hash = auth.token_hash.clone();
            let fail_user_id = auth.user_id.clone();
            let fail_status = upstream_status.as_u16();
            let err_body_clone = error_body.clone();
            let fail_end_user = end_user.clone();
            let fail_session_id = session_id.clone();
            let fail_requester_ip = requester_ip.clone();
            tokio::spawn(async move {
                let sl = SpendLog {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    call_type: "completion".to_string(),
                    api_key: fail_token_hash,
                    spend: 0.0,
                    total_tokens: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    start_time,
                    end_time: chrono::Utc::now(),
                    request_duration_ms: Some(
                        (chrono::Utc::now() - start_time).num_milliseconds() as i32,
                    ),
                    completion_start_time: None,
                    model: fail_model,
                    model_id: fail_model_id,
                    model_group: fail_model_group,
                    custom_llm_provider: fail_custom_llm_provider,
                    api_base: Some(fail_api_base),
                    user: fail_user_id,
                    metadata: metadata.clone(),
                    cache_hit: None,
                    cache_key: None,
                    request_tags: None,
                    team_id: None,
                    organization_id: None,
                    end_user: fail_end_user,
                    requester_ip_address: fail_requester_ip,
                    messages: Some(fail_upstream_body),
                    response: Some(json!({"error": err_body_clone})),
                    session_id: fail_session_id,
                    status: Some(format!("failure:{}", fail_status)),
                    mcp_namespaced_tool_name: None,
                    agent_id: None,
                    proxy_server_request: proxy_server_request.clone(),
                };
                let _ = state.db.insert_spend_log(&sl).await;
            });
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

        // SSE streaming proxy: two-phase spend-log pattern.
        // Phase 1: INSERT placeholder SpendLog (request_id, api_key, model, messages) BEFORE streaming.
        // Phase 2: UPDATE the same row with tokens, spend, end_time, and full response AFTER stream ends.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let state_clone = Arc::clone(&state);
        let model = _model.to_string();
        let api_base = deployment.api_base.clone();
        let token_hash = auth.token_hash.clone();
        let user_id = auth.user_id.clone();
        let team_id = auth.team_id.clone();
        let organization_id = auth.organization_id.clone();
        let request_id = uuid::Uuid::new_v4().to_string();
        let request_body = body.clone();

        // Phase 1: pre-insert placeholder SpendLog
        {
            let sl = SpendLog {
                request_id: request_id.clone(),
                call_type: "completion".to_string(),
                api_key: token_hash.clone(),
                spend: 0.0,
                total_tokens: 0,
                prompt_tokens: 0,
                completion_tokens: 0,
                start_time,
                end_time: start_time,
                request_duration_ms: None,
                completion_start_time: None,
                model: model.clone(),
                model_id: deployment.model_id.clone(),
                model_group: deployment.model_group.clone(),
                custom_llm_provider: deployment.custom_llm_provider.clone(),
                api_base: Some(api_base.clone()),
                user: user_id.clone(),
                metadata: metadata.clone(),
                cache_hit: None,
                cache_key: None,
                request_tags: None,
                team_id: team_id.clone(),
                organization_id: organization_id.clone(),
                end_user: end_user.clone(),
                requester_ip_address: requester_ip.clone(),
                messages: Some(request_body.clone()),
                response: Some(json!({"status": "streaming"})),
                session_id: session_id.clone(),
                status: Some("streaming".to_string()),
                mcp_namespaced_tool_name: None,
                agent_id: None,
                proxy_server_request: None,
            };
            let _ = state.db.insert_spend_log(&sl).await;
        }

        tokio::spawn(async move {
            use tokio_stream::StreamExt;
            let mut stream = upstream_resp.bytes_stream();
            let mut first_chunk_time: Option<chrono::DateTime<chrono::Utc>> = None;
            // Collect chunk choices for reconstructing a completion-style response
            let mut chunk_jsons: Vec<Value> = Vec::new();
            let mut stream_prompt_tokens: i32 = 0;
            let mut stream_completion_tokens: i32 = 0;
            let mut stream_total_tokens: i32 = 0;
            let mut failure: Option<(u16, String)> = None;

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        if first_chunk_time.is_none() && !chunk.is_empty() {
                            first_chunk_time = Some(chrono::Utc::now());
                        }

                        if let Ok(text) = std::str::from_utf8(&chunk) {
                            for line in text.lines() {
                                if let Some(data) = line.strip_prefix("data: ") {
                                    if data != "[DONE]" {
                                        if let Ok(val) = serde_json::from_str::<Value>(data) {
                                            if let Some(usage) = val.get("usage") {
                                                stream_prompt_tokens = usage
                                                    .get("prompt_tokens")
                                                    .and_then(|v| v.as_i64())
                                                    .unwrap_or(0) as i32;
                                                stream_completion_tokens = usage
                                                    .get("completion_tokens")
                                                    .and_then(|v| v.as_i64())
                                                    .unwrap_or(0) as i32;
                                                stream_total_tokens = usage
                                                    .get("total_tokens")
                                                    .and_then(|v| v.as_i64())
                                                    .unwrap_or(0) as i32;
                                            }
                                            if let Some(choices) = val.get("choices") {
                                                if !choices.as_array().map(|a| a.is_empty()).unwrap_or(true) {
                                                    chunk_jsons.push(val);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if tx.send(chunk.to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        failure = Some((0, e.to_string()));
                        break;
                    }
                }
            }

            let now = chrono::Utc::now();
            let streaming_spend = calc_spend(stream_prompt_tokens, stream_completion_tokens, deployment.input_cost_per_token, deployment.output_cost_per_token);

            // Build a completion-style response JSON from collected chunks
            let assembled_response = if chunk_jsons.is_empty() {
                json!({"streaming": true, "prompt_tokens": stream_prompt_tokens, "completion_tokens": stream_completion_tokens, "total_tokens": stream_total_tokens})
            } else {
                // Merge choice contents from chunks into a single choices array
                let mut merged_content = String::new();
                let mut finish_reason: Option<String> = None;
                // Accumulate streamed tool_calls by index
                let mut tool_calls: Vec<Value> = Vec::new();
                for c in &chunk_jsons {
                    if let Some(choices) = c["choices"].as_array() {
                        for choice in choices {
                            if let Some(content) = choice["delta"]["content"].as_str() {
                                if !content.is_empty() {
                                    merged_content.push_str(content);
                                }
                            }
                            if let Some(fr) = choice["finish_reason"].as_str() {
                                finish_reason = Some(fr.to_string());
                            }
                            // Accumulate streamed tool_calls
                            if let Some(delta_tcs) = choice["delta"]["tool_calls"].as_array() {
                                for tc in delta_tcs {
                                    let idx = tc.get("index").and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                                    while tool_calls.len() <= idx {
                                        tool_calls.push(json!({"id": "", "type": "function", "function": {"name": "", "arguments": ""}}));
                                    }
                                    if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                                        if !id.is_empty() {
                                            tool_calls[idx]["id"] = json!(id);
                                        }
                                    }
                                    if let Some(fn_name) = tc.get("function").and_then(|v| v.get("name")).and_then(|v| v.as_str()) {
                                        if !fn_name.is_empty() {
                                            tool_calls[idx]["function"]["name"] = json!(fn_name);
                                        }
                                    }
                                    if let Some(args) = tc.get("function").and_then(|v| v.get("arguments")).and_then(|v| v.as_str()) {
                                        tool_calls[idx]["function"]["arguments"] = json!(format!("{}{}",
                                            tool_calls[idx]["function"]["arguments"].as_str().unwrap_or(""),
                                            args
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                let message = if tool_calls.is_empty() {
                    json!({"role": "assistant", "content": merged_content})
                } else {
                    json!({"role": "assistant", "content": if merged_content.is_empty() { Value::Null } else { json!(merged_content) }, "tool_calls": tool_calls})
                };
                json!({
                    "streaming": true,
                    "choices": [{
                        "index": 0,
                        "message": message,
                        "finish_reason": finish_reason
                    }],
                    "usage": {
                        "prompt_tokens": stream_prompt_tokens,
                        "completion_tokens": stream_completion_tokens,
                        "total_tokens": stream_total_tokens
                    }
                })
            };

            let duration_ms = now.signed_duration_since(start_time).num_milliseconds() as i32;
            let cst = first_chunk_time.unwrap_or(now);

            // Phase 2: UPDATE the pre-inserted SpendLog row
            match failure {
                Some((status_code, err)) => {
                    let _ = state_clone.db.update_spend_log(
                        &request_id, 0.0, 0, 0, 0,
                        now, duration_ms, cst,
                        json!({"error": err, "status_code": status_code}),
                        &format!("failure:{}", status_code),
                    ).await;
                }
                None => {
                    let _ = state_clone.db.update_spend_log(
                        &request_id, streaming_spend, stream_total_tokens, stream_prompt_tokens, stream_completion_tokens,
                        now, duration_ms, cst, assembled_response, "success",
                    ).await;
                }
            }

            // Queue daily_spend update
            if let Some(ref queue) = state_clone.daily_spend_queue {
                let date = now.format("%Y-%m-%d").to_string();
                let ds_log = DailySpendLog {
                    entity_id: user_id.unwrap_or_default(),
                    date,
                    api_key: token_hash,
                    model,
                    model_group: String::new(),
                    custom_llm_provider: String::new(),
                    mcp_namespaced_tool_name: String::new(),
                    endpoint: "/v1/chat/completions".to_string(),
                    prompt_tokens: stream_prompt_tokens as i64,
                    completion_tokens: stream_completion_tokens as i64,
                    spend: streaming_spend,
                    api_requests: 1,
                    successful_requests: 1,
                    failed_requests: 0,
                    kind: DailySpendKind::User,
                };
                queue.queue(ds_log);
            }
        });

        let sse_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
            .map(|data: Vec<u8>| Ok::<_, Infallible>(data));

        let body = axum::body::Body::from_stream(sse_stream);
        let response = axum::response::Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::CONNECTION, "keep-alive")
            .body(body)
            .unwrap();
        Ok(response)
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
            // Record failure spend log
            let fail_upstream_body = upstream_body_val.clone();
            let fail_model = deployment.upstream_model.clone();
            let fail_api_base = deployment.api_base.clone();
            let fail_model_id = deployment.model_id.clone();
            let fail_model_group = deployment.model_group.clone();
            let fail_custom_llm_provider = deployment.custom_llm_provider.clone();
            let fail_token_hash = auth.token_hash.clone();
            let fail_user_id = auth.user_id.clone();
            let fail_status = upstream_status.as_u16();
            let fail_resp = resp_body.clone();
            let fail_end_user2 = end_user.clone();
            let fail_session_id2 = session_id.clone();
            let fail_requester_ip2 = requester_ip.clone();
            tokio::spawn(async move {
                let sl = SpendLog {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    call_type: "completion".to_string(),
                    api_key: fail_token_hash,
                    spend: 0.0,
                    total_tokens: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    start_time,
                    end_time: chrono::Utc::now(),
                    request_duration_ms: Some(
                        (chrono::Utc::now() - start_time).num_milliseconds() as i32,
                    ),
                    completion_start_time: None,
                    model: fail_model,
                    model_id: fail_model_id,
                    model_group: fail_model_group,
                    custom_llm_provider: fail_custom_llm_provider,
                    api_base: Some(fail_api_base),
                    user: fail_user_id,
                    metadata: metadata.clone(),
                    cache_hit: None,
                    cache_key: None,
                    request_tags: None,
                    team_id: None,
                    organization_id: None,
                    end_user: fail_end_user2,
                    requester_ip_address: fail_requester_ip2,
                    messages: Some(fail_upstream_body),
                    response: Some(fail_resp),
                    session_id: fail_session_id2,
                    status: Some(format!("failure:{}", fail_status)),
                    mcp_namespaced_tool_name: None,
                    agent_id: None,
                    proxy_server_request: proxy_server_request.clone(),
                };
                let _ = state.db.insert_spend_log(&sl).await;
            });
            return Err((
                StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                Json(resp_body),
            ));
        }

        // 6. Record spend log
        let now = chrono::Utc::now();
        let usage = resp_body.get("usage");
        let spend_amount = calc_spend(
            usage.and_then(|u| u.get("prompt_tokens")).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            usage.and_then(|u| u.get("completion_tokens")).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            deployment.input_cost_per_token,
            deployment.output_cost_per_token,
        );

        let spend_log = aigw_core::models::SpendLog {
            request_id: uuid::Uuid::new_v4().to_string(),
            call_type: "completion".to_string(),
            api_key: auth.token_hash.clone(),
            spend: spend_amount,
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
            model_id: deployment.model_id.clone(),
            model_group: deployment.model_group.clone(),
            custom_llm_provider: deployment.custom_llm_provider.clone(),
            api_base: Some(deployment.api_base.clone()),
            user: auth.user_id.clone(),
            metadata: None,
            cache_hit: None,
            cache_key: None,
            request_tags: None,
            team_id: auth.team_id.clone(),
            organization_id: auth.organization_id.clone(),
            end_user: end_user.clone(),
            requester_ip_address: requester_ip.clone(),
            messages: Some(body.clone()),
            response: Some(resp_body.clone()),
            session_id: session_id.clone(),
            status: Some("success".to_string()),
            mcp_namespaced_tool_name: None,
            agent_id: None,
            proxy_server_request: None,
        };

        // Record spend log (don't fail the request if logging fails)
        let _ = state.db.insert_spend_log(&spend_log).await;

        // Queue daily_spend update
        if let Some(ref queue) = state.daily_spend_queue {
            let date = now.format("%Y-%m-%d").to_string();
            let is_success = spend_log
                .status
                .as_deref()
                .unwrap_or("success")
                == "success";
            let ds_log = DailySpendLog {
                entity_id: spend_log.user.clone().unwrap_or_default(),
                date,
                api_key: spend_log.api_key.clone(),
                model: spend_log.model.clone(),
                model_group: spend_log.model_group.clone().unwrap_or_default(),
                custom_llm_provider: spend_log
                    .custom_llm_provider
                    .clone()
                    .unwrap_or_default(),
                mcp_namespaced_tool_name: spend_log
                    .mcp_namespaced_tool_name
                    .clone()
                    .unwrap_or_default(),
                endpoint: "/v1/chat/completions".to_string(),
                prompt_tokens: spend_log.prompt_tokens as i64,
                completion_tokens: spend_log.completion_tokens as i64,
                spend: spend_log.spend,
                api_requests: 1,
                successful_requests: if is_success { 1 } else { 0 },
                failed_requests: if is_success { 0 } else { 1 },
                kind: DailySpendKind::User,
            };
            queue.queue(ds_log);
        }

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
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            db,
            master_key: Some("sk-master-chat-test".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
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
    async fn test_chat_completions_unsupported_model() {
        let app = test_app().await;
        // Model not in proxy_models, no env fallback → should get 400
        let body = json!({
            "model": "nonexistent-model-xyz",
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
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "Expected 400 for unsupported model"
        );

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            val["error"]["type"].as_str(),
            Some("invalid_request_error")
        );
        assert!(val["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not found"));
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
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            db,
            master_key: Some("sk-master-chat-test".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
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
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            db,
            master_key: None,
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
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
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            db,
            master_key: Some("sk-master-test".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
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
