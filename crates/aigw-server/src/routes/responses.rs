//! OpenAI Responses API endpoint — POST /v1/responses
//!
//! Passthrough mode: accepts standard Responses API format, validates
//! `model` + `input` fields, resolves upstream deployment, and forwards
//! the request to upstream `{api_base}/responses`. No protocol conversion
//! is performed — the adapter (`OpenAIPassthrough`) only rewrites `model`
//! and injects `stream_options`.
//!
//! ## Differences from chat.rs
//!
//! | Area | chat.rs | responses.rs |
//! |------|---------|-------------|
//! | Client protocol | `ClientProtocol::OpenAI` | `ClientProtocol::Responses` |
//! | Request validation | `model` + `messages` | `model` + `input` |
//! | Upstream URL path | `chat/completions` | `responses` |
//! | Response usage fields | `prompt_tokens` / `completion_tokens` | `input_tokens` / `output_tokens` (with fallback) |
//! | SpendLog call_type | `"completion"` | `"responses"` |
//! | proxy_server_request.url | `"/v1/chat/completions"` | `"/v1/responses"` |

use super::chat::resolve_key_model_list;
pub use super::chat::ChatAuth;

use aigw_core::adapter::{select_adapter, ClientProtocol};
use aigw_core::metrics::RequestSummary;
use aigw_core::models::{DailySpendKind, DailySpendLog, SpendLog};
use axum::{
    extract::State,
    http::{self, header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tower_http::request_id::RequestId;

use super::chat::{calc_spend, extract_cache_creation_tokens, extract_cache_read_tokens};
use super::ip_extractor::OptionalClientIp;
use super::keys::SharedState;
use aigw_core::otel_tracing;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Usage extraction helpers — dual fallback (Responses API + Chat)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Extract prompt tokens from usage JSON.
/// Tries `input_tokens` (Responses API) first, falls back to
/// `prompt_tokens` (Chat Completions). `pub(crate)` so the Embeddings handler
/// can reuse it (embeddings usage also reports `prompt_tokens`).
pub(crate) fn extract_prompt_tokens(usage: &Value) -> i32 {
    usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32
}

/// Extract completion tokens from usage JSON.
/// Tries `output_tokens` (Responses API) first, falls back to
/// `completion_tokens` (Chat Completions).
fn extract_completion_tokens(usage: &Value) -> i32 {
    usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32
}

/// Extract total tokens from usage JSON.
/// `pub(crate)` so the Embeddings handler can reuse it.
pub(crate) fn extract_total_tokens(usage: &Value) -> i32 {
    usage
        .get("total_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Handler: POST /v1/responses
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub async fn responses_handler(
    State(state): State<SharedState>,
    ChatAuth(auth): ChatAuth,
    OptionalClientIp(client_ip): OptionalClientIp,
    headers: axum::http::HeaderMap,
    http::request::Parts { extensions, .. }: http::request::Parts,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    let request_id = extensions
        .get::<RequestId>()
        .and_then(|id| id.header_value().to_str().ok())
        .unwrap_or("unknown")
        .to_string();

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

    let _input = body.get("input").ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "message": "Missing required field 'input'",
                    "type": "invalid_request_error",
                    "code": null
                }
            })),
        )
    })?;

    // 2. Validate input is not empty
    match _input {
        Value::String(ref s) if s.is_empty() => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": "'input' must not be empty",
                        "type": "invalid_request_error",
                        "code": null
                    }
                })),
            ));
        }
        Value::Array(ref arr) if arr.is_empty() => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": "'input' array must not be empty",
                        "type": "invalid_request_error",
                        "code": null
                    }
                })),
            ));
        }
        _ => {} // ok
    }

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Root span
    let root_span = tracing::info_span!("responses", model = %_model, stream = is_stream);
    let _root_enter = root_span.enter();
    // NOTE: `Span::enter` guards are NOT held across `await` points (tracing
    // sharded.rs assertion risk on the tokio multi-thread runtime). Each guard
    // below is dropped before the next `await`-span boundary.

    // 3. Auth — reused ChatAuth already extracted; now look up key permissions
    let auth_span = tracing::info_span!("auth_check");
    let _auth_enter = auth_span.enter();

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

            // Stage 117: full multi-level guard — budget (key→user→team→org)
            // + RPM/TPM + soft_budget alerting. token_estimate from
            // max_output_tokens when present, else 0.
            let token_estimate = body
                .get("max_output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                .min(u32::MAX as u64) as u32;
            let limit_result = aigw_core::middleware::rate_limit::check_request_limits(
                &state.db,
                &state.rate_limiter,
                &auth,
                token_estimate,
            )
            .await;
            if let Err(e) = limit_result {
                return Ok(e.into_response());
            }
        }
    }
    drop(_auth_enter);

    // 4. Resolve upstream via ModelResolver + Router
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

    // Stage 128 §2.5: Responses → OAuth reverse-proxy. Any Responses request
    // resolving to an `anthropic_oauth` credential is converted to Anthropic
    // Messages format, billing-injected, and sent through the pipeline (Bearer
    // + proxy egress + 401 refresh-retry).
    if let Some(ref oauth) = deployment.oauth {
        // Drop the resolve span guard before the OAuth branch's long awaits
        // (token pipeline + reqwest) — same sharded.rs cross-thread drop risk
        // as chat/v1_messages.
        drop(_resolve_enter);

        let token_provider = state.token_provider.clone();
        let mk = state.aigw_master_key.clone().unwrap_or_default();
        let is_stream = body
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut upstream_body = match aigw_core::oauth_pipeline::adapt_to_anthropic(
            ClientProtocol::Responses,
            body.clone(),
            &deployment,
        ) {
            Ok(v) => v,
            Err(e) => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": {"message": format!("OAuth adapter error: {}", e), "type": "adapter_error"}
                    })),
                ));
            }
        };
        aigw_core::oauth_pipeline::inject_billing_block(&mut upstream_body, oauth);

        let upstream_resp = aigw_core::oauth_pipeline::send(
            &token_provider,
            &state.db,
            &mk,
            oauth,
            upstream_body,
            aigw_core::oauth_pipeline::OauthTarget::Messages,
        )
        .await;

        let upstream_resp = match upstream_resp {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("OAuth pipeline error: {}", e);
                tracing::error!(%err_msg, model = %_model, "oauth responses reverse-proxy failed");
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(json!({
                        "error": {"message": err_msg, "type": "upstream_error", "code": null}
                    })),
                ));
            }
        };
        let upstream_status = upstream_resp.status();
        let _upstream_req_id = upstream_resp
            .headers()
            .get("x-request-id")
            .or_else(|| upstream_resp.headers().get("request-id"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // ── Metadata extraction (same as the non-OAuth path) ──
        let end_user = body
            .get("metadata")
            .and_then(|m| m.get("user_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let session_id = end_user.as_ref().and_then(|eu| {
            serde_json::from_str::<Value>(eu).ok().and_then(|v| {
                v.get("session_id")
                    .and_then(|id| id.as_str())
                    .map(|s| s.to_string())
            })
        });
        let requester_ip: Option<String> = client_ip.map(|cip| cip.0.to_string());
        let start_time = chrono::Utc::now();
        let upstream_model_cl = deployment.upstream_model.clone();
        let model_id_cl = deployment.model_id.clone();
        let model_group_cl = deployment.model_group.clone();
        let custom_llm_provider_cl = deployment.custom_llm_provider.clone();

        // Streaming: the adapted body is Anthropic Messages — passthrough the
        // upstream SSE unchanged (the upstream Anthropic response is native).
        if is_stream {
            if !upstream_status.is_success() {
                let error_body = upstream_resp.text().await.unwrap_or_default();
                return Err((
                    StatusCode::from_u16(upstream_status.as_u16())
                        .unwrap_or(StatusCode::BAD_GATEWAY),
                    Json(json!({
                        "error": {
                            "message": format!("Upstream returned {}: {}", upstream_status.as_u16(), error_body),
                            "type": "upstream_error",
                            "code": null
                        }
                    })),
                ));
            }
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
            let state_clone = state.clone();
            let request_id_cl = request_id.clone();

            // Phase 1 placeholder SpendLog (streaming: INSERT then UPDATE).
            {
                let sl = SpendLog {
                    call_id: request_id.clone(),
                    request_id: None,
                    call_type: "responses".to_string(),
                    api_key: auth.token_hash.clone(),
                    spend: 0.0,
                    total_tokens: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    start_time,
                    end_time: chrono::Utc::now(),
                    request_duration_ms: None,
                    completion_start_time: None,
                    model: upstream_model_cl.clone(),
                    model_id: model_id_cl.clone(),
                    model_group: model_group_cl.clone(),
                    custom_llm_provider: custom_llm_provider_cl.clone(),
                    api_base: Some("oauth:anthropic".to_string()),
                    user: auth.user_id.clone(),
                    metadata: Some(json!({"oauth": true})),
                    cache_hit: None,
                    cache_key: None,
                    request_tags: None,
                    team_id: auth.team_id.clone(),
                    organization_id: auth.organization_id.clone(),
                    end_user: end_user.clone(),
                    requester_ip_address: requester_ip.clone(),
                    messages: Some(body.clone()),
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
                let mut stream_prompt_tokens: i32 = 0;
                let mut stream_completion_tokens: i32 = 0;
                let mut upstream_id: Option<String> = None;
                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => {
                            if let Ok(text) = std::str::from_utf8(&chunk) {
                                for line in text.lines() {
                                    if let Some(data) = line.strip_prefix("data: ") {
                                        if data != "[DONE]" {
                                            if let Ok(val) = serde_json::from_str::<Value>(data) {
                                                if upstream_id.is_none() {
                                                    if let Some(id) =
                                                        val.get("id").and_then(|v| v.as_str())
                                                    {
                                                        upstream_id = Some(id.to_string());
                                                    }
                                                }
                                                if let Some(usage) = val.get("usage") {
                                                    stream_prompt_tokens = usage
                                                        .get("input_tokens")
                                                        .or_else(|| usage.get("prompt_tokens"))
                                                        .and_then(|v| v.as_i64())
                                                        .unwrap_or(0)
                                                        as i32;
                                                    stream_completion_tokens = usage
                                                        .get("output_tokens")
                                                        .or_else(|| usage.get("completion_tokens"))
                                                        .and_then(|v| v.as_i64())
                                                        .unwrap_or(0)
                                                        as i32;
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
                        Err(_) => break,
                    }
                }
                let now = chrono::Utc::now();
                // Phase 2: UPDATE the pre-inserted placeholder row (same
                // call_id) — matching chat/v1_messages OAuth streaming.
                let duration_ms = now.signed_duration_since(start_time).num_milliseconds() as i32;
                let _ = state_clone
                    .db
                    .update_spend_log(
                        &request_id_cl,
                        upstream_id.as_deref(),
                        0.0,
                        stream_prompt_tokens + stream_completion_tokens,
                        stream_prompt_tokens,
                        stream_completion_tokens,
                        now,
                        duration_ms,
                        now,
                        serde_json::json!({"status": "streaming"}),
                        "success",
                        Some(json!({"oauth": true})),
                        None,
                    )
                    .await;
            });
            let sse_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
                .map(|data: Vec<u8>| Ok::<_, Infallible>(data));
            return Ok(axum::response::Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .header(header::CONNECTION, "keep-alive")
                .body(axum::body::Body::from_stream(sse_stream))
                .unwrap());
        }

        // Non-streaming: return the native Anthropic response (the inbound
        // protocol was Responses; the client receives the Anthropic body).
        if !upstream_status.is_success() {
            let error_body = upstream_resp.text().await.unwrap_or_default();
            return Err((
                StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                Json(json!({
                    "error": {
                        "message": format!("Upstream returned {}: {}", upstream_status.as_u16(), error_body),
                        "type": "upstream_error",
                        "code": null
                    }
                })),
            ));
        }
        let resp_body: Value = upstream_resp.json().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": {"message": format!("Failed to parse upstream response: {}", e), "type": "upstream_error"}
                })),
            )
        })?;
        let now = chrono::Utc::now();
        let usage = resp_body.get("usage");
        let prompt_tokens = usage
            .and_then(|u| u.get("input_tokens").or_else(|| u.get("prompt_tokens")))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let completion_tokens = usage
            .and_then(|u| {
                u.get("output_tokens")
                    .or_else(|| u.get("completion_tokens"))
            })
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let cache_read = usage
            .map(super::chat::extract_cache_read_tokens)
            .unwrap_or(0);
        let cache_create = usage
            .map(super::chat::extract_cache_creation_tokens)
            .unwrap_or(0);
        let spend_amount = super::chat::calc_spend(
            prompt_tokens,
            completion_tokens,
            deployment.input_cost_per_token,
            deployment.output_cost_per_token,
            cache_read,
            cache_create,
            deployment.cache_read_input_token_cost,
            deployment.cache_creation_input_token_cost,
        );
        let sl = SpendLog {
            call_id: request_id.clone(),
            request_id: resp_body
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            call_type: "responses".to_string(),
            api_key: auth.token_hash.clone(),
            spend: spend_amount,
            total_tokens: prompt_tokens + completion_tokens,
            prompt_tokens,
            completion_tokens,
            start_time,
            end_time: now,
            request_duration_ms: Some(
                now.signed_duration_since(start_time).num_milliseconds() as i32
            ),
            completion_start_time: None,
            model: upstream_model_cl.clone(),
            model_id: model_id_cl.clone(),
            model_group: model_group_cl.clone(),
            custom_llm_provider: custom_llm_provider_cl.clone(),
            api_base: Some("oauth:anthropic".to_string()),
            user: auth.user_id.clone(),
            metadata: Some(json!({"oauth": true})),
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
            image_tokens: None,
        };
        let _ = state.db.insert_spend_log(&sl).await;
        return Ok(axum::response::Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(
                serde_json::to_string(&resp_body).unwrap(),
            ))
            .unwrap());
    }

    let adapter =
        select_adapter(ClientProtocol::Responses, &deployment.provider_type).ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": format!(
                            "Unsupported provider type for Responses API with model '{}'",
                            _model
                        ),
                        "type": "invalid_request_error",
                        "code": "unsupported_provider"
                    }
                })),
            )
        })?;
    drop(_resolve_enter);

    // Adapt request
    let adapt_span = tracing::info_span!("adapt_request");
    let _adapt_enter = adapt_span.enter();
    let upstream_body_val = adapter
        .adapt_request(body.clone(), &deployment)
        .map_err(|e| {
            let (status, err_type) = match &e {
                aigw_core::adapter::AdapterError::Unsupported(_) => {
                    (StatusCode::BAD_REQUEST, "invalid_request_error")
                }
                aigw_core::adapter::AdapterError::Parse(_) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, "adapter_error")
                }
            };
            (
                status,
                Json(json!({"error": {"message": format!("{}", e), "type": err_type}})),
            )
        })?;

    // Upstream URL — Stage 102: bridge converts to Chat Completions, so use chat/completions path
    let upstream_path = match deployment.provider_type {
        aigw_core::deployment::ProviderType::AnthropicNative => "messages",
        _ => "chat/completions",
    };
    let upstream_url = format!(
        "{}/{}",
        deployment.api_base.trim_end_matches('/'),
        upstream_path
    );
    drop(_adapt_enter);

    // ── Metadata extraction (same as chat.rs) ──
    let end_user = body
        .get("metadata")
        .and_then(|m| m.get("user_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let session_id = end_user.as_ref().and_then(|eu| {
        serde_json::from_str::<Value>(eu).ok().and_then(|v| {
            v.get("session_id")
                .and_then(|id| id.as_str())
                .map(|s| s.to_string())
        })
    });

    let requester_ip: Option<String> = client_ip.map(|cip| cip.0.to_string());

    let user_agent: Option<String> = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let device_id: Option<String> = end_user.as_ref().and_then(|eu| {
        serde_json::from_str::<Value>(eu).ok().and_then(|v| {
            v.get("device_id")
                .and_then(|id| id.as_str())
                .map(|s| s.to_string())
        })
    });

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

    // ── Build proxy_server_request ──
    use std::time::{SystemTime, UNIX_EPOCH};
    let arrival_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let proxy_server_request = Some(json!({
        "url": "/v1/responses",
        "method": "POST",
        "headers": {
            "user-agent": user_agent.clone().unwrap_or_default(),
            "x-forwarded-for": requester_ip.as_deref().unwrap_or(""),
        },
        "arrival_time": arrival_time,
    }));

    let start_time = chrono::Utc::now();

    // ── 5. Build and send upstream request ──
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
    upstream_req = upstream_req.header("x-request-id", &request_id);

    if state.otel_active {
        let mut upstream_headers = axum::http::HeaderMap::new();
        otel_tracing::inject_traceparent(&mut upstream_headers);
        for (key, value) in upstream_headers.iter() {
            upstream_req = upstream_req.header(key, value);
        }
    }

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
                "upstream request TIMEOUT for model '{}', upstream_url={}",
                _model,
                upstream_url
            );
            "timeout_error"
        } else {
            tracing::error!("upstream request failed for model '{}': {}", _model, e);
            "upstream_error"
        };
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
                    call_type: "responses".to_string(),
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

    upstream_span.record("upstream_status", upstream_status.as_u16() as i64);
    upstream_span.record("upstream_latency_ms", upstream_latency_ms);
    drop(_upstream_enter);
    drop(upstream_span);

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Streaming dispatch
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    if is_stream {
        if !upstream_status.is_success() {
            let error_body = upstream_resp.text().await.unwrap_or_default();
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
            let fail_upstream_id = serde_json::from_str::<Value>(&error_body)
                .ok()
                .and_then(|v| v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
                .or_else(|| upstream_req_id.clone());
            let auth_team_id = auth.team_id.clone();
            let auth_org_id = auth.organization_id.clone();
            tokio::spawn(async move {
                let sl = SpendLog {
                    call_id: fail_request_id,
                    request_id: fail_upstream_id,
                    call_type: "responses".to_string(),
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
                        "message": format!("Upstream returned {}: {}", upstream_status.as_u16(), error_body),
                        "type": "upstream_error",
                        "code": null
                    }
                })),
            ));
        }

        // Two-phase spend-log pattern
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let state_clone = Arc::clone(&state);
        let model = deployment.upstream_model.clone();
        let api_base = deployment.api_base.clone();
        let request_body = body.clone();
        let stream_metrics = state.metrics.clone();
        let stream_auth_user = auth.user_id.clone();
        let stream_model = deployment.upstream_model.clone();
        // Owned copy of the call id for the SSE response header (the closure
        // below moves `request_id` into the spend-log task).
        let stream_request_id = request_id.clone();

        // Phase 1: placeholder INSERT
        {
            let sl = SpendLog {
                call_id: request_id.clone(),
                request_id: None,
                call_type: "responses".to_string(),
                api_key: auth.token_hash.clone(),
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
                user: auth.user_id.clone(),
                metadata: metadata.clone(),
                cache_hit: None,
                cache_key: None,
                request_tags: None,
                team_id: auth.team_id.clone(),
                organization_id: auth.organization_id.clone(),
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
            let mut stream = upstream_resp.bytes_stream();
            let mut first_chunk_time: Option<chrono::DateTime<chrono::Utc>> = None;
            let mut chunk_jsons: Vec<Value> = Vec::new();
            let mut stream_prompt_tokens: i32 = 0;
            let mut stream_completion_tokens: i32 = 0;
            let mut stream_total_tokens: i32 = 0;
            let mut stream_cache_read: i32 = 0;
            let mut stream_cache_creation: i32 = 0;
            let mut failure: Option<(u16, String)> = None;
            let mut upstream_id: Option<String> = None;

            // Stage 102: the bridge adapter converts upstream Chat Completions
            // SSE deltas → Responses API SSE events. Instantiating the stream
            // adapter here closes Phase 41 test gap ② — the stream path now
            // actually executes the ResponsesToChatCompletionsStream conversion
            // instead of forwarding raw upstream bytes.
            let mut stream_adapter = adapter
                .stream_adapter()
                .expect("ResponsesToChatCompletions must provide a stream adapter");
            let mut pending_chunk: Vec<u8> = Vec::new();

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
                                            if upstream_id.is_none() {
                                                if let Some(id) =
                                                    val.get("id").and_then(|v| v.as_str())
                                                {
                                                    upstream_id = Some(id.to_string());
                                                }
                                            }
                                            if let Some(usage) = val.get("usage") {
                                                stream_prompt_tokens = extract_prompt_tokens(usage);
                                                stream_completion_tokens =
                                                    extract_completion_tokens(usage);
                                                stream_total_tokens = extract_total_tokens(usage);
                                                stream_cache_read =
                                                    extract_cache_read_tokens(usage);
                                                stream_cache_creation =
                                                    extract_cache_creation_tokens(usage);
                                            }
                                            chunk_jsons.push(val);
                                        }
                                    }
                                }
                            }
                        }
                        // Convert the upstream chunk (Chat SSE) → Responses SSE
                        // events via the bridge stream adapter.
                        pending_chunk.extend_from_slice(&chunk);
                        while let Some(transformed) = stream_adapter.next(&pending_chunk) {
                            if tx.send(transformed).is_err() {
                                break;
                            }
                        }
                        pending_chunk.clear();
                    }
                    Err(e) => {
                        failure = Some((0, e.to_string()));
                        break;
                    }
                }
            }
            // Flush any final SSE events (response.completed + [DONE])
            if let Some(final_event) = stream_adapter.finish() {
                let _ = tx.send(final_event);
            }

            let now = chrono::Utc::now();
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

            let assembled_response = if chunk_jsons.is_empty() {
                json!({
                    "streaming": true,
                    "prompt_tokens": stream_prompt_tokens,
                    "completion_tokens": stream_completion_tokens,
                    "total_tokens": stream_total_tokens,
                    "cache_read_input_tokens": stream_cache_read,
                    "cache_creation_input_tokens": stream_cache_creation,
                })
            } else {
                let last = chunk_jsons.last().unwrap();
                json!({
                    "streaming": true,
                    "response": last,
                    "usage": {
                        "input_tokens": stream_prompt_tokens,
                        "output_tokens": stream_completion_tokens,
                        "total_tokens": stream_total_tokens,
                    }
                })
            };

            let duration_ms = now.signed_duration_since(start_time).num_milliseconds() as i32;
            let cst = first_chunk_time.unwrap_or(now);

            // Phase 2: UPDATE
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
                    let cache_metadata = if stream_cache_read > 0 || stream_cache_creation > 0 {
                        let mut m = serde_json::Map::new();
                        m.insert("cache_read_tokens".to_string(), json!(stream_cache_read));
                        m.insert(
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
                            m.insert(
                                "cache_read_spend".to_string(),
                                json!((cache_read_spend * 10000.0).round() / 10000.0),
                            );
                            m.insert(
                                "cache_create_spend".to_string(),
                                json!((cache_create_spend * 10000.0).round() / 10000.0),
                            );
                        }
                        Some(Value::Object(m))
                    } else {
                        None
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
                            None,
                        )
                        .await;

                    // Increment entity spends (async)
                    {
                        let inc_db = state_clone.db.clone();
                        let inc_th = auth.token_hash.clone();
                        let inc_uid = auth.user_id.clone();
                        let inc_tid = auth.team_id.clone();
                        let inc_oid = auth.organization_id.clone();
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

                    // Queue daily spend
                    if let Some(ref queue) = state_clone.daily_spend_queue {
                        let date = now.format("%Y-%m-%d").to_string();
                        let ds_log = DailySpendLog {
                            entity_id: auth.user_id.clone().unwrap_or_default(),
                            date,
                            api_key: auth.token_hash.clone(),
                            model: model.clone(),
                            model_group: deployment.model_group.clone().unwrap_or_default(),
                            custom_llm_provider: deployment
                                .custom_llm_provider
                                .clone()
                                .unwrap_or_default(),
                            mcp_namespaced_tool_name: String::new(),
                            endpoint: "/v1/responses".to_string(),
                            prompt_tokens: stream_prompt_tokens as i64,
                            completion_tokens: stream_completion_tokens as i64,
                            cache_read_input_tokens: stream_cache_read as i64,
                            cache_creation_input_tokens: stream_cache_creation as i64,
                            image_tokens: 0,
                            spend: streaming_spend,
                            api_requests: 1,
                            successful_requests: 1,
                            failed_requests: 0,
                            kind: DailySpendKind::User,
                        };
                        queue.queue(ds_log.clone());

                        if let Some(ref tid) = auth.team_id {
                            let mut ds_team = ds_log.clone();
                            ds_team.entity_id = tid.clone();
                            ds_team.kind = DailySpendKind::Team;
                            queue.queue(ds_team);
                        }
                        if let Some(ref oid) = auth.organization_id {
                            let mut ds_org = ds_log.clone();
                            ds_org.entity_id = oid.clone();
                            ds_org.kind = DailySpendKind::Organization;
                            queue.queue(ds_org);
                        }
                        if let Some(ref euid) = end_user {
                            let mut ds_eu = ds_log.clone();
                            ds_eu.entity_id = euid.clone();
                            ds_eu.kind = DailySpendKind::EndUser;
                            queue.queue(ds_eu);
                        }
                    }

                    if let Some(ref m) = stream_metrics {
                        m.record_request(&RequestSummary {
                            model: stream_model.clone(),
                            user: stream_auth_user.clone().unwrap_or_default(),
                            status_code: "200".to_string(),
                            success: true,
                            latency_secs: duration_ms as f64 / 1000.0,
                            upstream_latency_secs: 0.0,
                            ttft_secs: Some(
                                cst.signed_duration_since(start_time).num_milliseconds() as f64
                                    / 1000.0,
                            ),
                            queue_time_secs: None,
                            spend: streaming_spend,
                            prompt_tokens: stream_prompt_tokens,
                            completion_tokens: stream_completion_tokens,
                            total_tokens: stream_total_tokens,
                            error_type: String::new(),
                            api_base: Some(api_base),
                        });
                    }
                }
            }
        });

        let sse_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
            .map(|data: Vec<u8>| Ok::<_, Infallible>(data));

        let body = axum::body::Body::from_stream(sse_stream);
        // TD-006: streaming responses must carry the same x-call-id reconciliation
        // header as the non-streaming path (== SpendLog.call_id).
        let response = axum::response::Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::CONNECTION, "keep-alive")
            .header("x-call-id", &stream_request_id)
            .body(body)
            .unwrap();
        Ok(response)
    } else {
        // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
        // Non-streaming dispatch
        // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
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
                    call_type: "responses".to_string(),
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
                Json(json!({
                    "error": {
                        "message": format!(
                            "Upstream returned {}: {}",
                            upstream_status.as_u16(),
                            resp_body
                                .get("error")
                                .and_then(|e| e.get("message"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown error")
                        ),
                        "type": "upstream_error",
                        "code": null
                    }
                })),
            ));
        }

        // Adapt response (passthrough)
        let adapted_resp = adapter.adapt_response(resp_body.clone()).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {"message": format!("Adapter error: {}", e), "type": "adapter_error"}
                })),
            )
        })?;

        // Extract usage — dual fallback
        let usage = adapted_resp.get("usage");
        let prompt_tokens = usage.map(extract_prompt_tokens).unwrap_or(0);
        let completion_tokens = usage.map(extract_completion_tokens).unwrap_or(0);
        let total_tokens = usage.map(extract_total_tokens).unwrap_or(0);
        let cache_read = usage.map(extract_cache_read_tokens).unwrap_or(0);
        let cache_create = usage.map(extract_cache_creation_tokens).unwrap_or(0);

        let spend_amount = calc_spend(
            prompt_tokens,
            completion_tokens,
            deployment.input_cost_per_token,
            deployment.output_cost_per_token,
            cache_read,
            cache_create,
            deployment.cache_read_input_token_cost,
            deployment.cache_creation_input_token_cost,
        );

        let now = chrono::Utc::now();
        let duration_ms = now.signed_duration_since(start_time).num_milliseconds() as i32;

        let response_upstream_id = adapted_resp
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| upstream_req_id.clone());

        // ── Insert SpendLog ──
        let spend_log = SpendLog {
            call_id: request_id.clone(),
            request_id: response_upstream_id,
            call_type: "responses".to_string(),
            api_key: auth.token_hash.clone(),
            spend: spend_amount,
            total_tokens,
            prompt_tokens,
            completion_tokens,
            start_time,
            end_time: now,
            request_duration_ms: Some(duration_ms),
            completion_start_time: None,
            model: deployment.upstream_model.clone(),
            model_id: deployment.model_id.clone(),
            model_group: deployment.model_group.clone(),
            custom_llm_provider: deployment.custom_llm_provider.clone(),
            api_base: Some(deployment.api_base.clone()),
            user: auth.user_id.clone(),
            metadata: metadata.clone(),
            cache_hit: None,
            cache_key: None,
            request_tags: None,
            team_id: auth.team_id.clone(),
            organization_id: auth.organization_id.clone(),
            end_user: end_user.clone(),
            requester_ip_address: requester_ip.clone(),
            messages: Some(body.clone()),
            response: Some(adapted_resp.clone()),
            session_id: session_id.clone(),
            status: Some("success".to_string()),
            mcp_namespaced_tool_name: None,
            agent_id: None,
            proxy_server_request: proxy_server_request.clone(),
            body_archived: false,
            parquet_path: None,
            image_tokens: None,
        };
        let _ = state.db.insert_spend_log(&spend_log).await;

        // Increment entity spends (async)
        {
            let inc_db = state.db.clone();
            let inc_th = auth.token_hash.clone();
            let inc_uid = auth.user_id.clone();
            let inc_tid = auth.team_id.clone();
            let inc_oid = auth.organization_id.clone();
            let inc_cost = spend_amount;
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

        // Queue daily spend
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
                endpoint: "/v1/responses".to_string(),
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
            if let Some(ref aid) = spend_log.agent_id {
                let mut ds_agent = ds_log.clone();
                ds_agent.entity_id = aid.clone();
                ds_agent.kind = DailySpendKind::Agent;
                queue.queue(ds_agent);
            }
        }

        if let Some(ref m) = state.metrics {
            m.record_request(&RequestSummary {
                model: deployment.upstream_model,
                user: auth.user_id.clone().unwrap_or_default(),
                status_code: "200".to_string(),
                success: true,
                latency_secs: duration_ms as f64 / 1000.0,
                upstream_latency_secs: 0.0,
                ttft_secs: None,
                queue_time_secs: None,
                spend: spend_amount,
                prompt_tokens: spend_log.prompt_tokens,
                completion_tokens: spend_log.completion_tokens,
                total_tokens: spend_log.total_tokens,
                error_type: String::new(),
                api_base: Some(deployment.api_base),
            });
        }

        let mut response = axum::response::Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json");
        // TD-006: write the gateway call id (== SpendLog.call_id) to the response
        // so clients can reconcile directly from the header without a DB lookup.
        response = response.header("x-call-id", &request_id);
        Ok(response
            .body(axum::body::Body::from(
                serde_json::to_string(&adapted_resp).unwrap(),
            ))
            .unwrap())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Unit tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::keys::DEFAULT_KEY_TOKEN_LEN;
    use axum::Router;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    async fn test_state() -> SharedState {
        use aigw_core::db::Database;
        let db = Database::init("sqlite::memory:").await.unwrap();
        let mk = "sk-master-test".to_string();
        Arc::new(super::super::keys::AppState {
            resolver: aigw_core::resolver::ModelResolver::new(db.clone(), None, "onprem"),
            router: aigw_core::router::Router::default(),
            db,
            master_key: Some(mk.clone()),
            aigw_master_key: None,
            key_generate_length: DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: false,
            provider_registry: aigw_core::provider::ProviderRegistry::new(),
            router_state: aigw_core::router::RouterState::default(),
            rate_limiter: Arc::new(aigw_core::rate_limiter::RateLimiter::new()),
            deployment_mode: "test".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            token_provider: std::sync::Arc::new(aigw_core::claude_token::TokenProvider::new()),
            metrics: None,
        })
    }

    /// Insert a key + model and return the raw token.
    async fn seed_key_and_model(state: &SharedState, key_alias: &str, model_name: &str) -> String {
        let raw_token = format!("sk-{}", uuid::Uuid::new_v4());
        let token_hash = aigw_core::crypto::hash_token(&raw_token);
        let now = chrono::Utc::now();
        let key = aigw_core::models::VirtualKey {
            token: token_hash,
            key_name: Some(key_alias.to_string()),
            key_alias: Some(key_alias.to_string()),
            soft_budget_cooldown: "false".to_string(),
            spend: 0.0,
            expires: None,
            models: serde_json::json!([model_name]),
            aliases: serde_json::json!({}),
            config: serde_json::json!({}),
            router_settings: None,
            user_id: Some("test-user".to_string()),
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
            created_at: Some(now),
            created_by: Some("test".to_string()),
            updated_at: Some(now),
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
        state.db.insert_key(&key).await.expect("insert key");

        // Also insert a proxy_model so the resolver can find it
        let model = aigw_core::models::ProxyModel {
            model_id: uuid::Uuid::new_v4().to_string(),
            model_name: model_name.to_string(),
            litellm_params: serde_json::json!({
                "model": model_name,
                "api_base": "http://127.0.0.1:19999/v1",
                "input_cost_per_token": 0.00001,
                "output_cost_per_token": 0.00003,
            }),
            model_info: serde_json::json!({
                "input_cost_per_token": 0.00001,
                "output_cost_per_token": 0.00003,
            }),
            created_at: chrono::Utc::now().to_rfc3339(),
            created_by: Some("test".to_string()),
            updated_at: chrono::Utc::now().to_rfc3339(),
            updated_by: Some("test".to_string()),
            enabled: true,
        };
        state.db.insert_model(&model).await.expect("insert model");
        raw_token
    }

    fn build_app(state: SharedState) -> Router {
        Router::new()
            .route("/v1/responses", axum::routing::post(responses_handler))
            .with_state(state)
    }

    // ── UT-1: missing model → 400 ──

    #[tokio::test]
    async fn test_responses_missing_model() {
        let state = test_state().await;
        let token = seed_key_and_model(&state, "ut1", "gpt-4o").await;
        let app = build_app(state);

        let body = serde_json::json!({"input": "hello"});
        let req = axum::http::Request::builder()
            .method(axum::http::Method::POST)
            .uri("/v1/responses")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 400);
        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            json["error"]["type"].as_str(),
            Some("invalid_request_error")
        );
        assert!(json["error"]["message"].as_str().unwrap().contains("model"));
    }

    // ── UT-2: missing input → 400 ──

    #[tokio::test]
    async fn test_responses_missing_input() {
        let state = test_state().await;
        let token = seed_key_and_model(&state, "ut2", "gpt-4o").await;
        let app = build_app(state);

        let body = serde_json::json!({"model": "gpt-4o"});
        let req = axum::http::Request::builder()
            .method(axum::http::Method::POST)
            .uri("/v1/responses")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 400);
        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            json["error"]["type"].as_str(),
            Some("invalid_request_error")
        );
        assert!(json["error"]["message"].as_str().unwrap().contains("input"));
    }

    // ── UT-3: empty input string → 400 ──

    #[tokio::test]
    async fn test_responses_empty_input_string() {
        let state = test_state().await;
        let token = seed_key_and_model(&state, "ut3", "gpt-4o").await;
        let app = build_app(state);

        let body = serde_json::json!({"model": "gpt-4o", "input": ""});
        let req = axum::http::Request::builder()
            .method(axum::http::Method::POST)
            .uri("/v1/responses")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 400);
        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(json["error"]["message"].as_str().unwrap().contains("empty"));
    }

    // ── UT-4: empty input array → 400 ──

    #[tokio::test]
    async fn test_responses_empty_input_array() {
        let state = test_state().await;
        let token = seed_key_and_model(&state, "ut4", "gpt-4o").await;
        let app = build_app(state);

        let body = serde_json::json!({"model": "gpt-4o", "input": []});
        let req = axum::http::Request::builder()
            .method(axum::http::Method::POST)
            .uri("/v1/responses")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 400);
        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(json["error"]["message"].as_str().unwrap().contains("empty"));
    }

    // ── UT-5: empty input string → 400 ──

    #[tokio::test]
    async fn test_responses_upstream_unreachable() {
        let state = test_state().await;
        let token = seed_key_and_model(&state, "ut5", "gpt-4o").await;
        let app = build_app(state);

        let body = serde_json::json!({"model": "gpt-4o", "input": "hello"});
        let req = axum::http::Request::builder()
            .method(axum::http::Method::POST)
            .uri("/v1/responses")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        // Should be 502 (bad gateway) when upstream is unreachable
        assert!(
            status == 502 || status == 500,
            "expected 502 or 500, got {}",
            status
        );
        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(
            json["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Upstream"),
            "expected upstream error, got: {}",
            json
        );
    }

    // ── UT-6: extract_prompt_tokens dual fallback ──

    #[test]
    fn test_extract_prompt_tokens_input_fallback() {
        // Responses API format
        let usage = serde_json::json!({"input_tokens": 42, "output_tokens": 7, "total_tokens": 49});
        assert_eq!(extract_prompt_tokens(&usage), 42);
        assert_eq!(extract_completion_tokens(&usage), 7);
        assert_eq!(extract_total_tokens(&usage), 49);

        // Chat Completions format (fallback)
        let usage2 =
            serde_json::json!({"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15});
        assert_eq!(extract_prompt_tokens(&usage2), 10);
        assert_eq!(extract_completion_tokens(&usage2), 5);
        assert_eq!(extract_total_tokens(&usage2), 15);

        // Missing all → 0
        let usage3 = serde_json::json!({});
        assert_eq!(extract_prompt_tokens(&usage3), 0);
        assert_eq!(extract_completion_tokens(&usage3), 0);
        assert_eq!(extract_total_tokens(&usage3), 0);
    }
}
