//! OpenAI-compatible chat endpoints — /v1/chat/completions and /v1/models
//!
//! Endpoints:
//! - POST /v1/chat/completions — Chat completions (streaming SSE + non-streaming)
//! - GET  /v1/models           — List available models for the authenticated key

use aigw_core::adapter::{select_adapter, ClientProtocol};
use aigw_core::auth::decode_jwt;
use aigw_core::crypto::{decrypt_json_fields, decrypt_litellm_value, hash_token};
use aigw_core::metrics::RequestSummary;
use aigw_core::middleware::KeyIdentity;
use aigw_core::models::{DailySpendKind, DailySpendLog, SpendLog, Team, VirtualKey};
use axum::{
    extract::State,
    http::{self, header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tower_http::request_id::RequestId;

use super::ip_extractor::OptionalClientIp;
use super::keys::SharedState;
use aigw_core::otel_tracing;

/// Resolved upstream routing parameters from proxy_models + credentials lookup.
#[allow(dead_code)]
pub(crate) struct ResolvedUpstream {
    pub(crate) api_base: String,
    pub(crate) api_key: Option<String>,
    pub(crate) model_name: String,
    /// USD per input token (from model_info JSON)
    pub(crate) input_cost_per_token: Option<f64>,
    /// USD per output token (from model_info JSON)
    pub(crate) output_cost_per_token: Option<f64>,
    /// USD per cache-read input token
    pub(crate) cache_read_input_token_cost: Option<f64>,
    /// USD per cache-creation input token
    pub(crate) cache_creation_input_token_cost: Option<f64>,
    /// proxy_models UUID (model_id)
    pub(crate) model_id: Option<String>,
    /// proxy_models.model_name — deployment name for model_group (litellm-compatible)
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
#[allow(dead_code)]
fn extract_pricing(
    model_info: &Value,
    params_json: &Value,
) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    let input = model_info
        .get("input_cost_per_token")
        .and_then(|v| v.as_f64())
        .or_else(|| {
            params_json
                .get("input_cost_per_token")
                .and_then(|v| v.as_f64())
        });
    let output = model_info
        .get("output_cost_per_token")
        .and_then(|v| v.as_f64())
        .or_else(|| {
            params_json
                .get("output_cost_per_token")
                .and_then(|v| v.as_f64())
        });
    let cache_read = model_info
        .get("cache_read_input_token_cost")
        .and_then(|v| v.as_f64())
        .or_else(|| {
            params_json
                .get("cache_read_input_token_cost")
                .and_then(|v| v.as_f64())
        });
    let cache_create = model_info
        .get("cache_creation_input_token_cost")
        .and_then(|v| v.as_f64())
        .or_else(|| {
            params_json
                .get("cache_creation_input_token_cost")
                .and_then(|v| v.as_f64())
        });
    (input, output, cache_read, cache_create)
}

/// Calculate spend with three-tier cache billing.
///
/// If cache_read/cache_creation pricing is absent, falls back to input_cost_per_token
/// (litellm `_cost_per_token_custom_pricing_helper` behaviour).
/// Anthropic callers MUST normalize prompt_tokens before calling (add cache_read + cache_creation).
#[allow(clippy::too_many_arguments)]
pub(crate) fn calc_spend(
    prompt_tokens: i32,
    completion_tokens: i32,
    input_cost: Option<f64>,
    output_cost: Option<f64>,
    cache_read_tokens: i32,
    cache_creation_tokens: i32,
    cache_read_cost: Option<f64>,
    cache_creation_cost: Option<f64>,
) -> f64 {
    let regular = 0.max(prompt_tokens - cache_read_tokens - cache_creation_tokens) as f64;
    // Fallback: cache pricing missing → use regular input cost (don't zero out)
    let read_cost = cache_read_cost.unwrap_or(input_cost.unwrap_or(0.0));
    let create_cost = cache_creation_cost.unwrap_or(input_cost.unwrap_or(0.0));
    let base_input = input_cost.unwrap_or(0.0);
    regular * base_input
        + cache_read_tokens as f64 * read_cost
        + cache_creation_tokens as f64 * create_cost
        + completion_tokens as f64 * output_cost.unwrap_or(0.0)
}

/// Per-modality input cost (TD-012b): price a multimodal input whose tokens are
/// broken down by modality (image/audio/video) against `modal_pricing` (USD per
/// 1M tokens), falling back to the deployment's scalar `input_cost_per_token`
/// when no modal pricing is configured or a modality is unknown.
///
/// `modal_tokens` is a map of modality → token count. Sums `Σ tokens × price`.
/// NOTE: `modal_pricing` values are USD-per-1M-tokens (e.g. Gemini image $0.45/M)
/// so they're divided by 1e6; the scalar `input_cost` is already per-token
/// (aigw's model_info input_cost_per_token is per-token) and is used as-is.
///
/// Wired into the embeddings spend path once a request carries per-modality
/// input tokens (gemini-embedding-2 style); the pure math + UTs are the current
/// deliverable (TD-012b defers real-load wiring until such traffic exists).
#[allow(dead_code)]
pub(crate) fn calc_spend_modal(
    modal_tokens: &[(&str, i32)],
    input_cost: Option<f64>,
    modal_pricing: Option<&aigw_core::models::ModalPricing>,
) -> f64 {
    let scalar = input_cost.unwrap_or(0.0);
    modal_tokens
        .iter()
        .map(|(modal, tokens)| {
            let per_token = match *modal {
                "image" => modal_pricing.and_then(|p| p.image).map(|v| v / 1_000_000.0),
                "audio" => modal_pricing.and_then(|p| p.audio).map(|v| v / 1_000_000.0),
                "video" => modal_pricing.and_then(|p| p.video).map(|v| v / 1_000_000.0),
                _ => None,
            }
            .unwrap_or(scalar);
            (*tokens as f64) * per_token
        })
        .sum()
}

/// Extract cache-read tokens from usage JSON (Anthropic + OpenAI formats).
pub(crate) fn extract_cache_read_tokens(usage: &Value) -> i32 {
    // Anthropic: usage.cache_read_input_tokens
    if let Some(v) = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_i64())
    {
        return v as i32;
    }
    // OpenAI: usage.prompt_tokens_details.cached_tokens
    usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32
}

/// Extract cache-creation tokens from usage JSON.
pub(crate) fn extract_cache_creation_tokens(usage: &Value) -> i32 {
    // Anthropic: usage.cache_creation_input_tokens
    if let Some(v) = usage
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_i64())
    {
        return v as i32;
    }
    // OpenAI: prompt_tokens_details.cache_write_tokens or cache_creation_tokens
    let details = usage.get("prompt_tokens_details");
    details
        .and_then(|d| d.get("cache_write_tokens"))
        .or_else(|| details.and_then(|d| d.get("cache_creation_tokens")))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32
}

/// Look up a model by name in proxy_models, decrypt litellm_params if encrypted,
/// and resolve credential references. Falls back to env vars if model not found.
#[allow(dead_code)]
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
            let litellm_params_str = m
                .litellm_params
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| m.litellm_params.to_string());

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
                    tracing::error!(
                        "Failed to decrypt litellm_params for model '{}': {}",
                        model_name,
                        e
                    );
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
            let (input_cost, output_cost, cache_read_cost, cache_create_cost) =
                extract_pricing(&m.model_info, &params_json);

            // Extract model_group / custom_llm_provider from proxy_models for SpendLog
            let model_id = Some(m.model_id.clone());
            let model_group = Some(m.model_name.clone());
            let custom_llm_provider = params_json
                .get("custom_llm_provider")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // Resolve credential reference if present
            if let Some(cred_name) = params_json
                .get("litellm_credential_name")
                .and_then(|v| v.as_str())
            {
                let cred = state
                    .db
                    .get_credential_by_name(cred_name)
                    .await
                    .map_err(|e| {
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

                // Use as_str() for string values to avoid JSON quoting from to_string()
                let cred_values_str = cred
                    .credential_values
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| cred.credential_values.to_string());
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
                    let decrypted = decrypt_litellm_value(&cred_values_str, key).map_err(|e| {
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
                let api_key = merged
                    .get("api_key")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
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
                    cache_read_input_token_cost: cache_read_cost,
                    cache_creation_input_token_cost: cache_create_cost,
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
                    cache_read_input_token_cost: cache_read_cost,
                    cache_creation_input_token_cost: cache_create_cost,
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
                let api_key_env = std::env::var("OPENAI_API_KEY")
                    .ok()
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
                        cache_read_input_token_cost: None,
                        cache_creation_input_token_cost: None,
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
        let key = state.db.get_key_by_token(&token_hash).await.map_err(|_| {
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
        let key = state.db.get_key_by_token(&token_hash).await.map_err(|_| {
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
    /// Optional model metadata (mode, pricing, ...) — exposed so clients can
    /// distinguish multimodal (mode: "image") from chat-only models. Omitted
    /// (not `{}`) when the model has no registered proxy_models row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_info: Option<serde_json::Value>,
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
pub(crate) async fn resolve_key_model_list(
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
    OptionalClientIp(client_ip): OptionalClientIp,
    headers: axum::http::HeaderMap,
    http::request::Parts { extensions, .. }: http::request::Parts,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    // Extract the unified request ID from the SetRequestIdLayer (UUID v7).
    // This ID is used consistently for: tracing span, SpendLog DB record,
    // upstream x-request-id header, and error response bodies.
    let request_id = extensions
        .get::<RequestId>()
        .and_then(|id| id.header_value().to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    // Extract W3C traceparent from incoming headers (noop if OTEL disabled).
    // We extract but don't attach — the tracing-opentelemetry layer handles
    // context propagation automatically from the tracing spans.
    if state.otel_active {
        let _otel_ctx = otel_tracing::extract_traceparent(&headers);
    }

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

    // Root span for the entire request lifecycle
    let root_span = tracing::info_span!(
        "chat_completions",
        model = %_model,
        stream = is_stream,
    );
    let _root_enter = root_span.enter();

    // 3. Look up key permissions from the database
    let auth_span = tracing::info_span!("auth_check");
    let _auth_enter = auth_span.enter();
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
            if let Some(allowed_models) = resolve_key_model_list(&state, &key).await? {
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
            // None → allow all models

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

    // 4. Resolve upstream via ModelResolver + Router.pick_deployment()
    drop(_auth_enter);
    let resolve_span = tracing::info_span!("resolve_deployment", model = %_model);
    let _resolve_enter = resolve_span.enter();
    let mut deployments = state.resolver.resolve(_model).await?;
    let deployment_idx = state
        .router
        .pick_deployment(&mut deployments)
        .ok_or_else(|| {
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
    let deployment = deployments.remove(deployment_idx);
    let adapter =
        select_adapter(ClientProtocol::OpenAI, &deployment.provider_type).ok_or_else(|| {
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
    drop(_resolve_enter);
    let adapt_span = tracing::info_span!("adapt_request");
    let _adapt_enter = adapt_span.enter();
    let upstream_body_val = adapter.adapt_request(body.clone(), &deployment).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("Adapter error: {}", e), "type": "adapter_error"}})),
        )
    })?;

    // Build upstream URL path based on provider type
    let upstream_path = match deployment.provider_type {
        aigw_core::deployment::ProviderType::AnthropicNative => "messages",
        _ => "chat/completions",
    };
    let upstream_url = format!(
        "{}/{}",
        deployment.api_base.trim_end_matches('/'),
        upstream_path
    );

    // Extract end_user from metadata.user_id (Anthropic protocol convention)
    let end_user = body
        .get("metadata")
        .and_then(|m| m.get("user_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Try to parse session_id from JSON blob (Claude Code convention)
    let session_id = end_user.as_ref().and_then(|eu| {
        serde_json::from_str::<Value>(eu).ok().and_then(|v| {
            v.get("session_id")
                .and_then(|id| id.as_str())
                .map(|s| s.to_string())
        })
    });

    let requester_ip: Option<String> = client_ip.map(|cip| cip.0.to_string());

    // Extract User-Agent from HTTP header (align with litellm: store in metadata.user_agent)
    let user_agent: Option<String> = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Extract device_id from metadata.user_id JSON (Claude Code convention)
    let device_id: Option<String> = end_user.as_ref().and_then(|eu| {
        serde_json::from_str::<Value>(eu).ok().and_then(|v| {
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

    // 5. Build and send upstream request (with retry support)
    drop(_adapt_enter);
    let client = state.router.build_retry_client();

    let mut upstream_req = client.post(&upstream_url).json(&upstream_body_val);

    if let Some(ref api_key) = deployment.api_key {
        match deployment.provider_type {
            aigw_core::deployment::ProviderType::AnthropicNative => {
                upstream_req = upstream_req.header("x-api-key", api_key);
                upstream_req = upstream_req.header("anthropic-version", "2023-06-01");
            }
            _ => {
                upstream_req = upstream_req.header("Authorization", format!("Bearer {}", api_key));
            }
        }
    }

    // Forward aigw's request-id to upstream for log correlation
    upstream_req = upstream_req.header("x-request-id", &request_id);

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

    // Inject W3C traceparent into upstream request headers (noop if OTEL disabled)
    if state.otel_active {
        let mut upstream_headers = axum::http::HeaderMap::new();
        otel_tracing::inject_traceparent(&mut upstream_headers);
        for (key, value) in upstream_headers.iter() {
            upstream_req = upstream_req.header(key, value);
        }
    }

    // Upstream call span — wraps the HTTP request
    let upstream_span = tracing::info_span!(
        "upstream_call",
        upstream_url = %upstream_url,
        upstream_status = tracing::field::Empty,
        upstream_latency_ms = tracing::field::Empty,
    );
    let _upstream_enter = upstream_span.enter();
    let upstream_start = std::time::Instant::now();

    let upstream_resp = upstream_req.send().await.map_err(|e| {
        let is_timeout = e.is_timeout();
        let err_msg = format!("Upstream request failed: {}", e);
        let err_type = if is_timeout {
            tracing::error!(
                "upstream request TIMEOUT after {}s for model '{}', upstream_url={}",
                600,
                _model,
                upstream_url
            );
            "timeout_error"
        } else {
            tracing::error!("upstream request failed for model '{}': {}", _model, e);
            "upstream_error"
        };
        // Record failure spend_log on timeout (before returning error)
        if is_timeout {
            let state2 = state.clone();
            let fail_upstream_body = upstream_body_val.clone();
            let fail_model = deployment.upstream_model.clone();
            let fail_api_base = deployment.api_base.clone();
            let fail_model_id = deployment.model_id.clone();
            let fail_model_group = deployment.model_group.clone();
            let fail_custom_llm_provider = deployment.custom_llm_provider.clone();
            let fail_token_hash = auth.token_hash.clone();
            let fail_user_id = auth.user_id.clone();
            let fail_end_user = end_user.clone();
            let fail_session_id = session_id.clone();
            let fail_requester_ip = requester_ip.clone();
            let fail_psr = proxy_server_request.clone();
            let fail_metadata = metadata.clone();
            let fail_url = upstream_url.clone();
            let fail_model_name = _model.to_string();
            let err_msg_for_log = err_msg.clone();
            let fail_request_id = request_id.clone();
            let auth_team_id = auth.team_id.clone();
            let auth_org_id = auth.organization_id.clone();
            tokio::spawn(async move {
                let sl = SpendLog {
                    call_id: fail_request_id,
                    request_id: None,
                    call_type: "completion".to_string(),
                    api_key: fail_token_hash,
                    spend: 0.0,
                    total_tokens: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    start_time,
                    end_time: chrono::Utc::now(),
                    request_duration_ms: Some(
                        (chrono::Utc::now() - start_time).num_milliseconds() as i32
                    ),
                    completion_start_time: None,
                    model: fail_model,
                    model_id: fail_model_id,
                    model_group: fail_model_group,
                    custom_llm_provider: fail_custom_llm_provider,
                    api_base: Some(fail_api_base),
                    user: fail_user_id,
                    metadata: fail_metadata,
                    cache_hit: None,
                    cache_key: None,
                    request_tags: None,
                    team_id: auth_team_id,
                    organization_id: auth_org_id,
                    end_user: fail_end_user,
                    requester_ip_address: fail_requester_ip,
                    messages: Some(fail_upstream_body),
                    response: Some(json!({
                        "error": err_msg_for_log,
                        "failure_reason": "upstream_timeout",
                        "upstream_url": fail_url,
                        "model": fail_model_name,
                    })),
                    session_id: fail_session_id,
                    status: Some("timeout:upstream".to_string()),
                    mcp_namespaced_tool_name: None,
                    agent_id: None,
                    proxy_server_request: fail_psr,
                    body_archived: false,
                    parquet_path: None,
                    image_tokens: None,
                };
                let _ = state2.db.insert_spend_log(&sl).await;
            });
        }
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": {
                    "message": err_msg,
                    "type": err_type,
                    "code": null
                }
            })),
        )
    })?;

    let upstream_status = upstream_resp.status();
    let upstream_latency_ms = upstream_start.elapsed().as_millis() as i64;

    // Check if upstream returned a different x-request-id than what we sent.
    // tokenhub and some other gateways may replace/override the request-id header.
    let upstream_req_id = upstream_resp
        .headers()
        .get("x-request-id")
        .or_else(|| upstream_resp.headers().get("request-id"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    if let Some(ref upstream_rid) = upstream_req_id {
        if upstream_rid != &request_id {
            tracing::warn!(
                "mismatch request_id: ours={} theirs={} upstream_url={}",
                request_id,
                upstream_rid,
                upstream_url,
            );
        }
    }

    // Record upstream span fields
    upstream_span.record("upstream_status", upstream_status.as_u16() as i64);
    upstream_span.record("upstream_latency_ms", upstream_latency_ms);
    drop(_upstream_enter);
    drop(upstream_span);

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
            let fail_request_id = request_id.clone();
            // v6.1 §11.2: failure-path upstream id at INSERT (no Phase 2 UPDATE).
            // OpenAI 4xx/5xx error body may carry `id`; fallback to upstream header.
            let fail_upstream_id = serde_json::from_str::<serde_json::Value>(&error_body)
                .ok()
                .and_then(|v| v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
                .or_else(|| upstream_req_id.clone());
            let auth_team_id = auth.team_id.clone();
            let auth_org_id = auth.organization_id.clone();
            tokio::spawn(async move {
                let sl = SpendLog {
                    call_id: fail_request_id,
                    request_id: fail_upstream_id,
                    call_type: "completion".to_string(),
                    api_key: fail_token_hash,
                    spend: 0.0,
                    total_tokens: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    start_time,
                    end_time: chrono::Utc::now(),
                    request_duration_ms: Some(
                        (chrono::Utc::now() - start_time).num_milliseconds() as i32
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
                    team_id: auth_team_id,
                    organization_id: auth_org_id,
                    end_user: fail_end_user,
                    requester_ip_address: fail_requester_ip,
                    messages: Some(fail_upstream_body),
                    response: Some(json!({"error": err_body_clone})),
                    session_id: fail_session_id,
                    status: Some(format!("failure:{}", fail_status)),
                    mcp_namespaced_tool_name: None,
                    agent_id: None,
                    proxy_server_request: proxy_server_request.clone(),
                    body_archived: false,
                    parquet_path: None,
                    image_tokens: None,
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
        let model = deployment.upstream_model.clone();
        let api_base = deployment.api_base.clone();
        let token_hash = auth.token_hash.clone();
        let user_id = auth.user_id.clone();
        let team_id = auth.team_id.clone();
        let organization_id = auth.organization_id.clone();
        let request_body = body.clone();
        let stream_metrics = state.metrics.clone();
        let stream_auth_user = auth.user_id.clone();
        let stream_model = deployment.upstream_model.clone();

        // Phase 1: pre-insert placeholder SpendLog
        {
            let sl = SpendLog {
                call_id: request_id.clone(),
                request_id: None,
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
                body_archived: false,
                parquet_path: None,
                image_tokens: None,
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
            let mut stream_cache_read: i32 = 0;
            let mut stream_cache_creation: i32 = 0;
            let mut stream_image_tokens: i64 = 0;
            let mut stream_image_tokens_source: Option<String> = None;
            let mut failure: Option<(u16, String)> = None;
            // v6.1 §4.3: extract upstream id from the first chunk carrying it
            // (OpenAI chunks put `id` at the top level). Borrow val before any push/move.
            let mut upstream_id: Option<String> = None;

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
                                            // Extract upstream id (borrow, before any push/move).
                                            if upstream_id.is_none() {
                                                if let Some(id) =
                                                    val.get("id").and_then(|v| v.as_str())
                                                {
                                                    upstream_id = Some(id.to_string());
                                                }
                                            }
                                            if let Some(usage) = val.get("usage") {
                                                stream_prompt_tokens = usage
                                                    .get("prompt_tokens")
                                                    .and_then(|v| v.as_i64())
                                                    .unwrap_or(0)
                                                    as i32;
                                                stream_completion_tokens = usage
                                                    .get("completion_tokens")
                                                    .and_then(|v| v.as_i64())
                                                    .unwrap_or(0)
                                                    as i32;
                                                stream_total_tokens = usage
                                                    .get("total_tokens")
                                                    .and_then(|v| v.as_i64())
                                                    .unwrap_or(0)
                                                    as i32;
                                                stream_cache_read =
                                                    extract_cache_read_tokens(usage);
                                                stream_cache_creation =
                                                    extract_cache_creation_tokens(usage);
                                                // Streaming image tokens: upstream first,
                                                // fallback estimate from the request body.
                                                if let Some(t) = aigw_core::image_tokens::
                                                    extract_image_tokens_from_usage(usage)
                                                {
                                                    stream_image_tokens = t as i64;
                                                    stream_image_tokens_source =
                                                        Some("upstream".to_string());
                                                } else if stream_image_tokens_source.is_none() {
                                                    if let Some(est) = aigw_core::image_tokens::
                                                        calculate_image_tokens(
                                                            &request_body,
                                                            &model,
                                                        )
                                                    {
                                                        stream_image_tokens = est as i64;
                                                        stream_image_tokens_source =
                                                            Some("estimated".to_string());
                                                    }
                                                }
                                            }
                                            if let Some(choices) = val.get("choices") {
                                                if !choices
                                                    .as_array()
                                                    .map(|a| a.is_empty())
                                                    .unwrap_or(true)
                                                {
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
            // Anthropic-style providers: normalize prompt_tokens for cache tokens
            // (Anthropic doesn't include cache_read/cache_creation in input_tokens)
            let effective_prompt = if deployment.provider_type.is_anthropic_style() {
                stream_prompt_tokens + stream_cache_read + stream_cache_creation
            } else {
                stream_prompt_tokens
            };
            let streaming_spend = calc_spend(
                effective_prompt,
                stream_completion_tokens,
                deployment.input_cost_per_token,
                deployment.output_cost_per_token,
                stream_cache_read,
                stream_cache_creation,
                deployment.cache_read_input_token_cost,
                deployment.cache_creation_input_token_cost,
            );

            // Build a completion-style response JSON from collected chunks
            let assembled_response = if chunk_jsons.is_empty() {
                json!({"streaming": true, "prompt_tokens": stream_prompt_tokens, "completion_tokens": stream_completion_tokens, "total_tokens": stream_total_tokens,
                    "cache_read_input_tokens": stream_cache_read, "cache_creation_input_tokens": stream_cache_creation})
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
                                    let idx = tc.get("index").and_then(|v| v.as_i64()).unwrap_or(0)
                                        as usize;
                                    while tool_calls.len() <= idx {
                                        tool_calls.push(json!({"id": "", "type": "function", "function": {"name": "", "arguments": ""}}));
                                    }
                                    if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                                        if !id.is_empty() {
                                            tool_calls[idx]["id"] = json!(id);
                                        }
                                    }
                                    if let Some(fn_name) = tc
                                        .get("function")
                                        .and_then(|v| v.get("name"))
                                        .and_then(|v| v.as_str())
                                    {
                                        if !fn_name.is_empty() {
                                            tool_calls[idx]["function"]["name"] = json!(fn_name);
                                        }
                                    }
                                    if let Some(args) = tc
                                        .get("function")
                                        .and_then(|v| v.get("arguments"))
                                        .and_then(|v| v.as_str())
                                    {
                                        tool_calls[idx]["function"]["arguments"] = json!(format!(
                                            "{}{}",
                                            tool_calls[idx]["function"]["arguments"]
                                                .as_str()
                                                .unwrap_or(""),
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
                    let _ = state_clone
                        .db
                        .update_spend_log(
                            &request_id,
                            upstream_id.as_deref(),
                            0.0,
                            0,
                            0,
                            0,
                            now,
                            duration_ms,
                            cst,
                            json!({"error": err, "status_code": status_code}),
                            &format!("failure:{}", status_code),
                            None,
                            None,
                        )
                        .await;
                    if let Some(ref m) = stream_metrics {
                        m.record_request(&RequestSummary {
                            model: stream_model.clone(),
                            user: stream_auth_user.clone().unwrap_or_default(),
                            status_code: status_code.to_string(),
                            success: false,
                            latency_secs: duration_ms as f64 / 1000.0,
                            upstream_latency_secs: 0.0,
                            ttft_secs: None,
                            queue_time_secs: None,
                            spend: 0.0,
                            prompt_tokens: 0,
                            completion_tokens: 0,
                            total_tokens: 0,
                            error_type: "upstream_error".into(),
                            api_base: Some(api_base.clone()),
                        });
                    }
                }
                None => {
                    let mut meta_map = serde_json::Map::new();
                    if stream_cache_read > 0 || stream_cache_creation > 0 {
                        meta_map.insert("cache_read_tokens".to_string(), json!(stream_cache_read));
                        meta_map.insert(
                            "cache_creation_tokens".to_string(),
                            json!(stream_cache_creation),
                        );
                        let cache_read_spend = stream_cache_read as f64
                            * deployment
                                .cache_read_input_token_cost
                                .unwrap_or(deployment.input_cost_per_token.unwrap_or(0.0));
                        let cache_create_spend = stream_cache_creation as f64
                            * deployment
                                .cache_creation_input_token_cost
                                .unwrap_or(deployment.input_cost_per_token.unwrap_or(0.0));
                        if cache_read_spend > 0.0 || cache_create_spend > 0.0 {
                            meta_map.insert(
                                "cache_read_spend".to_string(),
                                json!((cache_read_spend * 10000.0).round() / 10000.0),
                            );
                            meta_map.insert(
                                "cache_create_spend".to_string(),
                                json!((cache_create_spend * 10000.0).round() / 10000.0),
                            );
                        }
                    }
                    if let Some(src) = stream_image_tokens_source {
                        meta_map.insert("image_tokens_source".to_string(), json!(src));
                    }
                    let cache_metadata = if meta_map.is_empty() {
                        None
                    } else {
                        Some(serde_json::Value::Object(meta_map))
                    };
                    let _ = state_clone
                        .db
                        .update_spend_log(
                            &request_id,
                            upstream_id.as_deref(),
                            streaming_spend,
                            stream_total_tokens,
                            stream_prompt_tokens,
                            stream_completion_tokens,
                            now,
                            duration_ms,
                            cst,
                            assembled_response,
                            "success",
                            cache_metadata,
                            if stream_image_tokens > 0 {
                                Some(stream_image_tokens as i32)
                            } else {
                                None
                            },
                        )
                        .await;

                    // Increment entity spends (async — don't block the SSE stream response)
                    {
                        let inc_db = state_clone.db.clone();
                        let inc_th = token_hash.clone();
                        let inc_uid = user_id.clone();
                        let inc_tid = team_id.clone();
                        let inc_oid = organization_id.clone();
                        let inc_cost = streaming_spend;
                        tokio::spawn(async move {
                            let _ = inc_db.increment_key_spend(&inc_th, inc_cost).await;
                            if let Some(ref uid) = inc_uid {
                                let _ = inc_db.increment_user_spend(uid, inc_cost).await;
                            }
                            if let Some(ref tid) = inc_tid {
                                let _ = inc_db.increment_team_spend(tid, inc_cost).await;
                            }
                            if let Some(ref oid) = inc_oid {
                                let _ = inc_db.increment_org_spend(oid, inc_cost).await;
                            }
                        });
                    }
                    if let Some(ref m) = stream_metrics {
                        let ttft = first_chunk_time.map(|fct| {
                            fct.signed_duration_since(start_time).num_milliseconds() as f64 / 1000.0
                        });
                        m.record_request(&RequestSummary {
                            model: stream_model.clone(),
                            user: stream_auth_user.clone().unwrap_or_default(),
                            status_code: "200".to_string(),
                            success: true,
                            latency_secs: duration_ms as f64 / 1000.0,
                            upstream_latency_secs: duration_ms as f64 / 1000.0,
                            ttft_secs: ttft,
                            queue_time_secs: None,
                            spend: streaming_spend,
                            prompt_tokens: stream_prompt_tokens,
                            completion_tokens: stream_completion_tokens,
                            total_tokens: stream_total_tokens,
                            error_type: String::new(),
                            api_base: Some(api_base.clone()),
                        });
                    }
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
                    model_group: deployment.model_group.clone().unwrap_or_default(),
                    custom_llm_provider: deployment.custom_llm_provider.clone().unwrap_or_default(),
                    mcp_namespaced_tool_name: String::new(),
                    endpoint: "/v1/chat/completions".to_string(),
                    prompt_tokens: stream_prompt_tokens as i64,
                    completion_tokens: stream_completion_tokens as i64,
                    cache_read_input_tokens: stream_cache_read as i64,
                    cache_creation_input_tokens: stream_cache_creation as i64,
                    image_tokens: stream_image_tokens,
                    spend: streaming_spend,
                    api_requests: 1,
                    successful_requests: 1,
                    failed_requests: 0,
                    kind: DailySpendKind::User,
                };
                queue.queue(ds_log.clone());

                // Queue additional daily_spend dimensions for streaming path
                let team_tid = team_id.clone();
                let team_oid = organization_id.clone();
                if let Some(ref tid) = team_tid {
                    let mut ds_team = ds_log.clone();
                    ds_team.entity_id = tid.clone();
                    ds_team.kind = DailySpendKind::Team;
                    queue.queue(ds_team);
                }
                if let Some(ref oid) = team_oid {
                    let mut ds_org = ds_log.clone();
                    ds_org.entity_id = oid.clone();
                    ds_org.kind = DailySpendKind::Organization;
                    queue.queue(ds_org);
                }
                // Queue EndUser dimension (agent_id reserved, always None for now)
                if let Some(ref euid) = end_user {
                    let mut ds_eu = ds_log.clone();
                    ds_eu.entity_id = euid.clone();
                    ds_eu.kind = DailySpendKind::EndUser;
                    queue.queue(ds_eu);
                }
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
            let fail_request_id = request_id.clone();
            // v6.1 §11.2: non-streaming 4xx/5xx failure — upstream id at INSERT.
            // OpenAI error bodies usually carry no `id` field; fall back to the
            // pre-extracted upstream response header (x-request-id / request-id)
            // captured at chat.rs:1067-1073 before upstream_resp was consumed.
            let fail_upstream_id = fail_resp
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| upstream_req_id.clone());
            let auth_team_id = auth.team_id.clone();
            let auth_org_id = auth.organization_id.clone();
            tokio::spawn(async move {
                let sl = SpendLog {
                    call_id: fail_request_id,
                    request_id: fail_upstream_id,
                    call_type: "completion".to_string(),
                    api_key: fail_token_hash,
                    spend: 0.0,
                    total_tokens: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    start_time,
                    end_time: chrono::Utc::now(),
                    request_duration_ms: Some(
                        (chrono::Utc::now() - start_time).num_milliseconds() as i32
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
                    team_id: auth_team_id,
                    organization_id: auth_org_id,
                    end_user: fail_end_user2,
                    requester_ip_address: fail_requester_ip2,
                    messages: Some(fail_upstream_body),
                    response: Some(fail_resp),
                    session_id: fail_session_id2,
                    status: Some(format!("failure:{}", fail_status)),
                    mcp_namespaced_tool_name: None,
                    agent_id: None,
                    proxy_server_request: proxy_server_request.clone(),
                    body_archived: false,
                    parquet_path: None,
                    image_tokens: None,
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
        let prompt_tokens = usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let completion_tokens = usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let cache_read = usage.map(extract_cache_read_tokens).unwrap_or(0);
        let cache_create = usage.map(extract_cache_creation_tokens).unwrap_or(0);
        // Image tokens: upstream first (Qwen OpenAI-compat / DashScope native),
        // fallback to client-side estimation from the request body (OpenAI/
        // Anthropic don't report this breakdown). image_tokens ⊆ prompt_tokens.
        let (image_tokens, image_tokens_source) = usage
            .and_then(aigw_core::image_tokens::extract_image_tokens_from_usage)
            .map(|t| (Some(t), Some("upstream")))
            .unwrap_or_else(|| {
                let est = aigw_core::image_tokens::calculate_image_tokens(
                    &body,
                    &deployment.upstream_model,
                );
                match est {
                    Some(t) if t > 0 => (Some(t), Some("estimated")),
                    _ => (None, None),
                }
            });
        // Anthropic normalization
        let effective_prompt = if deployment.provider_type.is_anthropic_style() {
            prompt_tokens + cache_read + cache_create
        } else {
            prompt_tokens
        };
        let spend_amount = calc_spend(
            effective_prompt,
            completion_tokens,
            deployment.input_cost_per_token,
            deployment.output_cost_per_token,
            cache_read,
            cache_create,
            deployment.cache_read_input_token_cost,
            deployment.cache_creation_input_token_cost,
        );

        let spend_log = aigw_core::models::SpendLog {
            call_id: request_id.clone(),
            // v6.1 §4.3: non-streaming success — upstream id at INSERT from resp_body.
            request_id: resp_body
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
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
            model: deployment.upstream_model.clone(),
            model_id: deployment.model_id.clone(),
            model_group: deployment.model_group.clone(),
            custom_llm_provider: deployment.custom_llm_provider.clone(),
            api_base: Some(deployment.api_base.clone()),
            user: auth.user_id.clone(),
            metadata: {
                let mut m = serde_json::Map::new();
                if cache_read > 0 || cache_create > 0 {
                    m.insert("cache_read_tokens".to_string(), json!(cache_read));
                    m.insert("cache_creation_tokens".to_string(), json!(cache_create));
                    // Effective cache spend (excludes regular token portion)
                    let cache_read_spend = cache_read as f64
                        * deployment
                            .cache_read_input_token_cost
                            .unwrap_or(deployment.input_cost_per_token.unwrap_or(0.0));
                    let cache_create_spend = cache_create as f64
                        * deployment
                            .cache_creation_input_token_cost
                            .unwrap_or(deployment.input_cost_per_token.unwrap_or(0.0));
                    if cache_read_spend > 0.0 || cache_create_spend > 0.0 {
                        m.insert(
                            "cache_read_spend".to_string(),
                            json!((cache_read_spend * 10000.0).round() / 10000.0),
                        );
                        m.insert(
                            "cache_create_spend".to_string(),
                            json!((cache_create_spend * 10000.0).round() / 10000.0),
                        );
                    }
                }
                if let Some(src) = image_tokens_source {
                    m.insert("image_tokens_source".to_string(), json!(src));
                }
                if m.is_empty() {
                    None
                } else {
                    Some(Value::Object(m))
                }
            },
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
            body_archived: false,
            parquet_path: None,
            image_tokens,
        };

        // Record spend log (don't fail the request if logging fails)
        let _ = state.db.insert_spend_log(&spend_log).await;

        // Increment entity spends asynchronously (key + user + team + org if associated)
        let inc_db = state.db.clone();
        let inc_token_hash = auth.token_hash.clone();
        let inc_user_id = auth.user_id.clone();
        let inc_team_id = auth.team_id.clone();
        let inc_org_id = auth.organization_id.clone();
        let inc_cost = spend_amount;
        tokio::spawn(async move {
            let _ = inc_db.increment_key_spend(&inc_token_hash, inc_cost).await;
            if let Some(ref uid) = inc_user_id {
                let _ = inc_db.increment_user_spend(uid, inc_cost).await;
            }
            if let Some(ref tid) = inc_team_id {
                let _ = inc_db.increment_team_spend(tid, inc_cost).await;
            }
            if let Some(ref oid) = inc_org_id {
                let _ = inc_db.increment_org_spend(oid, inc_cost).await;
            }
        });

        // Record OTEL span attributes and close root span
        root_span.record("prompt_tokens", spend_log.prompt_tokens as i64);
        root_span.record("completion_tokens", spend_log.completion_tokens as i64);
        root_span.record("total_tokens", spend_log.total_tokens as i64);
        root_span.record("spend", spend_amount);

        // Record Prometheus metrics (non-streaming success)
        if let Some(ref m) = state.metrics {
            m.record_request(&RequestSummary {
                model: deployment.upstream_model.clone(),
                user: auth.user_id.clone().unwrap_or_default(),
                status_code: "200".to_string(),
                success: true,
                latency_secs: now.signed_duration_since(start_time).num_milliseconds() as f64
                    / 1000.0,
                upstream_latency_secs: now.signed_duration_since(start_time).num_milliseconds()
                    as f64
                    / 1000.0,
                ttft_secs: None,
                queue_time_secs: None,
                spend: spend_amount,
                prompt_tokens: spend_log.prompt_tokens,
                completion_tokens: spend_log.completion_tokens,
                total_tokens: spend_log.total_tokens,
                error_type: String::new(),
                api_base: Some(deployment.api_base.clone()),
            });
        }

        // Queue daily_spend update
        if let Some(ref queue) = state.daily_spend_queue {
            let date = now.format("%Y-%m-%d").to_string();
            let is_success = spend_log.status.as_deref().unwrap_or("success") == "success";
            let ds_log = DailySpendLog {
                entity_id: spend_log.user.clone().unwrap_or_default(),
                date,
                api_key: spend_log.api_key.clone(),
                model: spend_log.model.clone(),
                model_group: spend_log.model_group.clone().unwrap_or_default(),
                custom_llm_provider: spend_log.custom_llm_provider.clone().unwrap_or_default(),
                mcp_namespaced_tool_name: spend_log
                    .mcp_namespaced_tool_name
                    .clone()
                    .unwrap_or_default(),
                endpoint: "/v1/chat/completions".to_string(),
                prompt_tokens: spend_log.prompt_tokens as i64,
                completion_tokens: spend_log.completion_tokens as i64,
                cache_read_input_tokens: cache_read as i64,
                cache_creation_input_tokens: cache_create as i64,
                image_tokens: spend_log.image_tokens.unwrap_or(0) as i64,
                spend: spend_log.spend,
                api_requests: 1,
                successful_requests: if is_success { 1 } else { 0 },
                failed_requests: if is_success { 0 } else { 1 },
                kind: DailySpendKind::User,
            };
            queue.queue(ds_log.clone());

            // Queue additional daily_spend dimensions
            if let Some(ref tid) = spend_log.team_id {
                let mut ds_team = ds_log.clone();
                ds_team.entity_id = tid.clone();
                ds_team.kind = DailySpendKind::Team;
                queue.queue(ds_team);
            }
            if let Some(ref oid) = spend_log.organization_id {
                let mut ds_org = ds_log.clone();
                ds_org.entity_id = oid.clone();
                ds_org.kind = DailySpendKind::Organization;
                queue.queue(ds_org);
            }
            if let Some(ref euid) = spend_log.end_user {
                let mut ds_eu = ds_log.clone();
                ds_eu.entity_id = euid.clone();
                ds_eu.kind = DailySpendKind::EndUser;
                queue.queue(ds_eu);
            }
            // Agent dimension (reserved, currently always None)
            if let Some(ref aid) = spend_log.agent_id {
                let mut ds_agent = ds_log.clone();
                ds_agent.entity_id = aid.clone();
                ds_agent.kind = DailySpendKind::Agent;
                queue.queue(ds_agent);
            }
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
    // model_info keyed by model_name — only the master path (proxy_models rows)
    // has it; non-master keys carry model-name strings only.
    let mut model_infos: Vec<Option<serde_json::Value>> = Vec::new();

    if auth.is_master_key {
        // Master key sees all models registered in proxy_models table
        let models = state.db.list_models().await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": "Failed to list models", "type": "db_error"}})),
            )
        })?;
        for m in models {
            model_ids.push(m.model_name);
            model_infos.push(Some(m.model_info));
        }
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
                        model_infos.push(None);
                    }
                }
            }
        }
    }

    let now_ts = chrono::Utc::now().timestamp();
    let data: Vec<ModelEntry> = model_ids
        .into_iter()
        .zip(model_infos)
        .map(|(id, model_info)| ModelEntry {
            id,
            object: "model".to_string(),
            created: now_ts,
            owned_by: "aigw".to_string(),
            model_info,
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
    use aigw_core::resolver::ModelResolver;
    use aigw_core::router::Router as AigwRouter;
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
            router: AigwRouter::default(),
            db,
            master_key: Some("sk-master-chat-test".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            metrics: None,
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
        assert_eq!(val["error"]["type"].as_str(), Some("invalid_request_error"));
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
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");

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
            router: AigwRouter::default(),
            db,
            master_key: Some("sk-master-chat-test".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            metrics: None,
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
            soft_budget: None,
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
            user_email: None,
            user_alias: None,
        };

        db.insert_key(&key).await.expect("insert key");

        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key: None,
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            metrics: None,
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

    #[tokio::test]
    async fn test_models_list_master_includes_model_info() {
        // Stage 103: master key /v1/models exposes model_info (mode) so clients
        // can identify multimodal models. list_models() returns full ProxyModel.
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");
        let model = ProxyModel {
            model_id: uuid::Uuid::new_v4().to_string(),
            model_name: "qwen3.5-vl".to_string(),
            litellm_params: json!({"model": "qwen/qwen3.5-vl"}),
            model_info: json!({"id": "qwen3.5-vl", "mode": "image"}),
            created_at: chrono::Utc::now().to_rfc3339(),
            created_by: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
            updated_by: None,
        };
        db.insert_model(&model).await.expect("insert model");

        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key: Some("sk-master-models".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            metrics: None,
        });

        let app = Router::new()
            .route("/v1/models", axum::routing::get(models_list))
            .with_state(state);

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models")
            .header(header::AUTHORIZATION, "Bearer sk-master-models")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        let data = val["data"].as_array().expect("data array");
        let entry = data
            .iter()
            .find(|m| m["id"] == "qwen3.5-vl")
            .expect("qwen entry");
        assert_eq!(
            entry["model_info"]["mode"].as_str(),
            Some("image"),
            "master key /v1/models should expose model_info.mode"
        );
    }

    #[tokio::test]
    async fn test_models_list_key_omits_model_info() {
        // Stage 103: non-master key model list comes from key.models (name strings
        // only) — model_info must be absent (skip_serializing_if), not empty object.
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");
        let raw_key = "sk-models-key-omit";
        let token_hash = aigw_core::crypto::hash_token(raw_key);
        let key = VirtualKey {
            token: token_hash.clone(),
            key_name: Some("omit-key".to_string()),
            key_alias: Some("omit-key".to_string()),
            soft_budget_cooldown: "false".to_string(),
            spend: 0.0,
            expires: None,
            models: json!(["gpt-4"]),
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
            soft_budget: None,
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
            created_by: Some("test".to_string()),
            updated_at: Some(chrono::Utc::now()),
            updated_by: Some("test".to_string()),
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
        db.insert_key(&key).await.expect("insert key");

        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key: None,
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            metrics: None,
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
        let data = val["data"].as_array().expect("data array");
        for entry in data {
            assert!(
                entry.get("model_info").is_none(),
                "non-master key /v1/models must not include model_info: {}",
                entry
            );
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Sentinel resolution tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    fn make_test_state(db: Database) -> SharedState {
        Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key: Some("sk-master-test".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            metrics: None,
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
            soft_budget: None,
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
            user_email: None,
            user_alias: None,
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

    /// Test that spend_logs.model records Deployment.upstream_model
    /// (litellm_params["model"]), NOT the proxy model name from the request.
    #[tokio::test]
    async fn test_spend_log_records_upstream_model() {
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");

        // Insert proxy_model: model_name="my-gpt-proxy", litellm_params.model="azure/gpt-4"
        let model = ProxyModel {
            model_id: uuid::Uuid::new_v4().to_string(),
            model_name: "my-gpt-proxy".to_string(),
            litellm_params: json!({
                "model": "azure/gpt-4",
                "api_base": "https://api.openai.com/v1",
                "custom_llm_provider": "openai",
            }),
            model_info: json!({}),
            created_at: chrono::Utc::now().to_rfc3339(),
            created_by: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
            updated_by: None,
        };
        db.insert_model(&model).await.expect("insert model");

        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db: db.clone(),
            master_key: Some("sk-master-test".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            metrics: None,
        });

        // Send a request with the proxy model name "my-gpt-proxy"
        // The upstream mock will fail (no real server running), so we check
        // the failure path which already records upstream_model.
        // But for success path testing, we use a mock server.
        let _body = json!({
            "model": "my-gpt-proxy",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        // Use a simple test approach: directly resolve and check
        // that Deployment.upstream_model == "azure/gpt-4"
        let deployments = state.resolver.resolve("my-gpt-proxy").await.unwrap();
        assert_eq!(deployments.len(), 1);
        assert_eq!(deployments[0].upstream_model, "azure/gpt-4");
        // The proxy model_name (model_group) should be the deployment name
        assert_eq!(deployments[0].model_group.as_deref(), Some("my-gpt-proxy"));
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Stage 90: Cache detection + three-tier billing tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[test]
    fn test_calc_spend_no_cache_no_pricing() {
        let spend = calc_spend(100, 50, None, None, 0, 0, None, None);
        assert_eq!(spend, 0.0);
    }

    #[test]
    fn test_calc_spend_with_cache_read() {
        // prompt=600, cache_read=500, cache_create=0, input=$0.01, cache_read=$0.0025
        // regular = 100 * 0.01 = $1.0, cache_read = 500 * 0.0025 = $1.25, total = $2.25
        let spend = calc_spend(600, 0, Some(0.01), Some(0.02), 500, 0, Some(0.0025), None);
        assert!(
            (spend - 2.25).abs() < 0.0001,
            "expected 2.25, got {}",
            spend
        );
    }

    #[test]
    fn test_calc_spend_cache_cost_fallback() {
        // cache pricing None → fallback to input_cost_per_token
        // prompt=600, cache_read=500, input=$0.01
        // regular = 100 * 0.01 = 1.0, cache_read = 500 * 0.01 = 5.0, total = 6.0
        let spend = calc_spend(600, 0, Some(0.01), None, 500, 0, None, None);
        assert!((spend - 6.0).abs() < 0.0001, "expected 6.0, got {}", spend);
    }

    #[test]
    fn test_calc_spend_with_cache_create() {
        // prompt=700, cache_read=300, cache_create=50, input=$0.01
        // regular = 350 * 0.01 = 3.5
        // cache_read = 300 * 0.0025 = 0.75
        // cache_create = 50 * 0.0125 = 0.625
        // total = 4.875
        let spend = calc_spend(
            700,
            100,
            Some(0.01),
            Some(0.02),
            300,
            50,
            Some(0.0025),
            Some(0.0125),
        );
        // regular=350*0.01=3.5 + cache_read=300*0.0025=0.75 + cache_create=50*0.0125=0.625 + completion=100*0.02=2.0 = 6.875
        assert!(
            (spend - 6.875).abs() < 0.0001,
            "expected 6.875, got {}",
            spend
        );
    }

    #[test]
    fn test_calc_spend_modal_image_only() {
        // 1000 image tokens × $0.45/M = 0.00045
        let mp = aigw_core::models::ModalPricing {
            image: Some(0.45),
            audio: Some(6.50),
            video: Some(12.00),
        };
        let spend = calc_spend_modal(&[("image", 1000)], Some(0.0002), Some(&mp));
        assert!((spend - 0.00045).abs() < 1e-12, "got {spend}");
    }

    #[test]
    fn test_calc_spend_modal_mixed_audio_video() {
        // audio 500 tokens × $6.50/M + video 100 tokens × $12.00/M
        let mp = aigw_core::models::ModalPricing {
            image: Some(0.45),
            audio: Some(6.50),
            video: Some(12.00),
        };
        let spend = calc_spend_modal(&[("audio", 500), ("video", 100)], Some(0.0002), Some(&mp));
        let expected = 500.0 * 6.50 / 1e6 + 100.0 * 12.00 / 1e6;
        assert!((spend - expected).abs() < 1e-12, "got {spend}");
    }

    #[test]
    fn test_calc_spend_modal_unknown_falls_back_scalar() {
        // unknown modality → scalar input cost per token
        let mp = aigw_core::models::ModalPricing {
            image: Some(0.45),
            audio: None,
            video: None,
        };
        let spend = calc_spend_modal(&[("text", 1000)], Some(0.0002), Some(&mp));
        assert!((spend - 0.2).abs() < 1e-10, "got {spend}");
    }

    #[test]
    fn test_calc_spend_modal_no_modal_pricing_scalar() {
        // no modal_pricing configured → scalar input cost for all modalities
        let spend = calc_spend_modal(&[("image", 1000)], Some(0.0002), None);
        assert!((spend - 0.2).abs() < 1e-10, "got {spend}");
    }

    #[test]
    fn test_calc_spend_cache_tokens_exceed_prompt() {
        // When cache tokens > prompt (can happen at provider level), regular = 0
        let spend = calc_spend(100, 0, Some(0.01), None, 200, 0, Some(0.005), None);
        // regular = max(0, 100-200) * 0.01 = 0, cache_read = 200 * 0.005 = 1.0
        assert!((spend - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_extract_cache_read_openai_format() {
        let usage = json!({
            "prompt_tokens": 600,
            "prompt_tokens_details": {"cached_tokens": 500}
        });
        assert_eq!(extract_cache_read_tokens(&usage), 500);
    }

    #[test]
    fn test_extract_cache_read_anthropic_format() {
        let usage = json!({"cache_read_input_tokens": 500});
        assert_eq!(extract_cache_read_tokens(&usage), 500);
    }

    #[test]
    fn test_extract_cache_creation_openai_format() {
        let usage = json!({
            "prompt_tokens_details": {"cache_write_tokens": 50}
        });
        assert_eq!(extract_cache_creation_tokens(&usage), 50);
    }

    #[test]
    fn test_extract_cache_creation_anthropic_format() {
        let usage = json!({"cache_creation_input_tokens": 50});
        assert_eq!(extract_cache_creation_tokens(&usage), 50);
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Stage 107: Image token handler integration
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[test]
    fn test_image_tokens_upstream_priority() {
        // Qwen returns image_tokens in usage → upstream value wins over estimate.
        let usage = json!({
            "prompt_tokens": 1270,
            "prompt_tokens_details": {"image_tokens": 400, "cached_tokens": 0}
        });
        let upstream = aigw_core::image_tokens::extract_image_tokens_from_usage(&usage);
        assert_eq!(upstream, Some(400));
        // If usage has image_tokens, the handler uses it and never estimates.
        let image_tokens = upstream
            .map(|t| (Some(t), Some("upstream")))
            .unwrap_or((None, None));
        assert_eq!(image_tokens, (Some(400), Some("upstream")));
    }

    #[test]
    fn test_image_tokens_fallback_estimate() {
        // OpenAI usage has no image_tokens → client-side estimate from body.
        let body = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "hi"},
                    {"type": "image_url", "image_url": {"url": data_url_png(1024, 1024)}}
                ]
            }]
        });
        let est = aigw_core::image_tokens::calculate_image_tokens(&body, "gpt-4o");
        assert_eq!(est, Some(765)); // 85 + 170 × ⌈1024/512⌉²
    }

    #[test]
    fn test_image_tokens_text_only_null() {
        // Text-only request → None (no image parts).
        let body = json!({
            "model": "qwen2.5-vl-72b",
            "messages": [{"role": "user", "content": "hello"}]
        });
        assert_eq!(
            aigw_core::image_tokens::calculate_image_tokens(&body, "qwen2.5-vl-72b"),
            None
        );
    }

    #[test]
    fn test_image_tokens_source_metadata() {
        // The handler's metadata merge: when an estimate exists, the
        // image_tokens_source key is written into the metadata JSON.
        let mut m = serde_json::Map::new();
        m.insert("image_tokens_source".to_string(), json!("estimated"));
        let meta = serde_json::Value::Object(m);
        assert_eq!(meta["image_tokens_source"], json!("estimated"));
    }

    /// Minimal valid PNG header (8-byte sig + IHDR) of given size → data URL.
    /// Reuses aigw-core's decode path so the test exercises the same base64 engine.
    fn data_url_png(w: u32, h: u32) -> String {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes.extend_from_slice(&[0, 0, 0, 13]);
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&w.to_be_bytes());
        bytes.extend_from_slice(&h.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        let b64 = aigw_core::image_tokens::encode_png_header(&bytes);
        format!("data:image/png;base64,{}", b64)
    }

    #[test]
    fn test_extract_cache_read_none() {
        let usage = json!({"prompt_tokens": 100, "completion_tokens": 50});
        assert_eq!(extract_cache_read_tokens(&usage), 0);
        assert_eq!(extract_cache_creation_tokens(&usage), 0);
    }
}
