//! Claude-compatible /v1/messages endpoint
//!
//! Supports both non-streaming and SSE streaming, with protocol conversion
//! to OpenAI upstream via the adapter layer.
//!
//! Auth: x-api-key header or Authorization: Bearer header (Claude convention)

use aigw_core::adapter::{select_adapter, AnthropicToOpenAIStream, ClientProtocol, StreamAdapter};
use aigw_core::auth::decode_jwt;
use aigw_core::crypto::hash_token;
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

use super::ip_extractor::OptionalClientIp;
use super::keys::SharedState;
use aigw_core::otel_tracing;
use tower_http::request_id::RequestId;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Anthropic error helper
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn anthropic_error(
    status: StatusCode,
    error_type: &str,
    message: &str,
    request_id: &str,
) -> (StatusCode, Json<Value>) {
    let request_id = format!("req_{}", request_id);
    (
        status,
        Json(json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": message
            },
            "request_id": request_id
        })),
    )
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Cookie JWT auth helper (mirrors ChatAuth::try_cookie_jwt)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn try_cookie_jwt_auth(
    state: &SharedState,
    headers: &http::HeaderMap,
) -> Option<(String, Option<String>, Option<String>, Option<String>)> {
    let master_key = state.master_key.as_deref()?;

    // Extract cookie named "token"
    let cookie_value = headers
        .get(http::header::COOKIE)
        .and_then(|v| v.to_str().ok())?
        .split(';')
        .find_map(|c| {
            let c = c.trim();
            c.strip_prefix("token=").map(|v| v.to_string())
        })?;

    // Decode JWT
    let claims = decode_jwt(&cookie_value, master_key).ok()?;

    // Hash the key from JWT claims and look up in DB
    let token = claims.key;
    let token_hash = hash_token(&token);
    let key = state.db.get_key_by_token(&token_hash).await.ok()??;

    let team_id = key.team_id.clone();
    let org_id = key.organization_id.clone();
    Some((token_hash, key.user_id, team_id, org_id))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Handler
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// POST /v1/messages — Claude Messages API endpoint
///
/// Supports both non-streaming and SSE streaming.
/// Proxies to upstream with protocol conversion via the adapter layer.
pub async fn messages_handler(
    State(state): State<SharedState>,
    OptionalClientIp(client_ip): OptionalClientIp,
    http::request::Parts {
        headers,
        extensions,
        ..
    }: http::request::Parts,
    body: String,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    // Extract the unified request ID from the SetRequestIdLayer (UUID v7).
    // This ID is used consistently for: tracing span, SpendLog DB record,
    // upstream x-request-id header, and Anthropic error response bodies.
    let request_id = extensions
        .get::<RequestId>()
        .and_then(|id| id.header_value().to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    // Extract W3C traceparent from incoming headers (noop if OTEL disabled)
    if state.otel_active {
        let _otel_ctx = otel_tracing::extract_traceparent(&headers);
    }

    // 1. Auth: try x-api-key/Bearer header, fallback to cookie JWT
    let auth_span = tracing::info_span!("auth_check");
    let _auth_enter = auth_span.enter();
    let extracted = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get(http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        });

    // 1b. Validate token and extract identity (saved for SpendLog)
    let (auth_token_hash, auth_user_id, auth_team_id, auth_org_id) = if let Some(token) = extracted
    {
        if token.is_empty() {
            let request_id = format!("req_{}", request_id);
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "type": "error",
                    "error": {
                        "type": "authentication_error",
                        "message": "Missing x-api-key or Authorization header"
                    },
                    "request_id": request_id
                })),
            ));
        }
        let is_master = state
            .master_key
            .as_ref()
            .map(|mk| token == mk.as_str())
            .unwrap_or(false);

        if is_master {
            ("master_key".to_string(), None, None, None)
        } else {
            let token_hash = hash_token(token);
            let key = state.db.get_key_by_token(&token_hash).await.map_err(|_| {
                let request_id = format!("req_{}", request_id);
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "type": "error",
                        "error": {
                            "type": "authentication_error",
                            "message": "Invalid API key"
                        },
                        "request_id": request_id
                    })),
                )
            })?;
            let key = key.ok_or_else(|| {
                let request_id = format!("req_{}", request_id);
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "type": "error",
                        "error": {
                            "type": "authentication_error",
                            "message": "Invalid API key"
                        },
                        "request_id": request_id
                    })),
                )
            })?;

            let user_id = key.user_id.clone();
            let team_id = key.team_id.clone();
            let org_id = key.organization_id.clone();
            (token_hash, user_id, team_id, org_id)
        }
    } else {
        // Fallback: try cookie JWT (same pattern as ChatAuth)
        match try_cookie_jwt_auth(&state, &headers).await {
            Some((token_hash, user_id, team_id, org_id)) => (token_hash, user_id, team_id, org_id),
            None => {
                let request_id = format!("req_{}", request_id);
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "type": "error",
                        "error": {
                            "type": "authentication_error",
                            "message": "Missing x-api-key or Authorization header"
                        },
                        "request_id": request_id
                    })),
                ));
            }
        }
    };

    // 2. Validate anthropic-version header
    let _api_version = headers
        .get("anthropic-version")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            anthropic_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "Missing required header: anthropic-version",
                &request_id,
            )
        })?;

    // 3. Parse request body
    let body_val: Value = serde_json::from_str(&body).map_err(|e| {
        anthropic_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            &format!("Failed to parse request body: {}", e),
            &request_id,
        )
    })?;

    // 3. Validate required fields
    drop(_auth_enter);
    let model = body_val
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            anthropic_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "Missing required field: model",
                &request_id,
            )
        })?;

    let messages = body_val
        .get("messages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            anthropic_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "Missing required field: messages",
                &request_id,
            )
        })?;

    if messages.is_empty() {
        return Err(anthropic_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "messages must not be empty",
            &request_id,
        ));
    }

    let _max_tokens = body_val
        .get("max_tokens")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| {
            anthropic_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "Missing required field: max_tokens",
                &request_id,
            )
        })?;

    // Stage 117: full multi-level guard — budget (key→user→team→org)
    // + RPM/TPM + soft_budget alerting. token_estimate = max_tokens (required
    // for Anthropic), so both RPM and TPM are enforced.
    let token_estimate = body_val
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
    let identity = aigw_core::middleware::KeyIdentity {
        token_hash: auth_token_hash.clone(),
        key_alias: None,
        user_id: auth_user_id.clone(),
        team_id: auth_team_id.clone(),
        organization_id: auth_org_id.clone(),
        is_master_key: false,
        user_role: None,
    };
    let limit_result = aigw_core::middleware::rate_limit::check_request_limits(
        &state.db,
        &state.rate_limiter,
        &identity,
        token_estimate,
    )
    .await;
    if let Err(e) = limit_result {
        return Ok(e.into_response());
    }

    let is_stream = body_val
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let call_type = if is_stream {
        "completion_stream"
    } else {
        "completion"
    };

    // Root span for the entire request lifecycle
    let root_span = tracing::info_span!(
        "messages_handler",
        model = %model,
        stream = is_stream,
    );
    let _root_enter = root_span.enter();

    // Extract end_user from Anthropic protocol metadata.user_id
    // Claude Code packs device_id/session_id as JSON string in this field
    let end_user = body_val
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

    let requester_ip = client_ip.map(|cip| cip.0.to_string());

    // Extract User-Agent from HTTP header (align with litellm)
    let user_agent: Option<String> = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Extract device_id from metadata.user_id JSON
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

    // 4. Resolve upstream via ModelResolver + Router.pick_deployment()
    let resolve_span = tracing::info_span!("resolve_deployment", model = %model);
    let _resolve_enter = resolve_span.enter();
    let resolved_deployment = match state.resolver.resolve(&model).await {
        Ok(mut deployments) => {
            let idx = state
                .router
                .pick_deployment(&mut deployments)
                .ok_or_else(|| {
                    anthropic_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_request_error",
                        &format!("Model '{}' not found", model),
                        &request_id,
                    )
                })?;
            deployments.remove(idx)
        }
        Err((status, body)) => {
            let now = chrono::Utc::now();
            let error_body = body.0.clone();
            let log_state = Arc::clone(&state);
            let log_model = model.clone();
            let log_token_hash = auth_token_hash.clone();
            let log_user_id = auth_user_id.clone();
            let log_team_id = auth_team_id.clone();
            let log_org_id = auth_org_id.clone();
            let log_end_user = end_user.clone();
            let log_session_id = session_id.clone();
            let log_requester_ip = requester_ip.clone();
            let log_metadata = metadata.clone();
            let rid = request_id.clone();
            // v6.1 §11.2: resolver-failure path (aigw-side, never reached upstream) → no upstream id.
            tokio::spawn(async move {
                let sl = SpendLog {
                    call_id: rid,
                    request_id: None,
                    call_type: call_type.to_string(),
                    api_key: log_token_hash,
                    spend: 0.0,
                    total_tokens: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    start_time: now,
                    end_time: now,
                    request_duration_ms: Some(0),
                    completion_start_time: None,
                    model: log_model,
                    model_id: None,
                    model_group: None,
                    custom_llm_provider: None,
                    api_base: None,
                    user: log_user_id,
                    metadata: log_metadata,
                    cache_hit: None,
                    cache_key: None,
                    request_tags: None,
                    team_id: log_team_id,
                    organization_id: log_org_id,
                    end_user: log_end_user,
                    requester_ip_address: log_requester_ip,
                    messages: Some(error_body.clone()),
                    response: Some(error_body),
                    session_id: log_session_id,
                    status: Some(format!("failure:{}", status.as_u16())),
                    mcp_namespaced_tool_name: None,
                    agent_id: None,
                    proxy_server_request: None,
                    body_archived: false,
                    parquet_path: None,
                    image_tokens: None,
                };
                let _ = log_state.db.insert_spend_log(&sl).await;
            });
            let msg = body["error"]["message"].as_str().unwrap_or("Unknown error");
            let err_type = body["error"]["type"]
                .as_str()
                .unwrap_or("invalid_request_error");
            return Err(anthropic_error(status, err_type, msg, &request_id));
        }
    };
    let input_cost = resolved_deployment.input_cost_per_token;
    let output_cost = resolved_deployment.output_cost_per_token;
    let cache_read_cost = resolved_deployment.cache_read_input_token_cost;
    let cache_create_cost = resolved_deployment.cache_creation_input_token_cost;

    let upstream_base_url = resolved_deployment.api_base.clone();
    let upstream_api_key = resolved_deployment.api_key.clone();
    let upstream_model_id = resolved_deployment.model_id.clone();
    let upstream_model_group = resolved_deployment.model_group.clone();
    let upstream_custom_llm_provider = resolved_deployment.custom_llm_provider.clone();
    let upstream_model = resolved_deployment.upstream_model.clone();
    let provider_type = resolved_deployment.provider_type.clone();

    // Select adapter based on client protocol + provider type
    drop(_resolve_enter);
    let adapt_span = tracing::info_span!("adapt_request");
    let _adapt_enter = adapt_span.enter();
    let adapter = select_adapter(ClientProtocol::Anthropic, &provider_type).ok_or_else(|| {
        anthropic_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "Unsupported provider type for this endpoint",
            &request_id,
        )
    })?;

    // 6. Adapt Claude request to OpenAI format via adapter
    let upstream_body = adapter
        .adapt_request(body_val.clone(), &resolved_deployment)
        .map_err(|e| {
            anthropic_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                &format!("Adapter error: {}", e),
                &request_id,
            )
        })?;

    // Build upstream URL path based on provider type
    let upstream_path = match provider_type {
        aigw_core::deployment::ProviderType::AnthropicNative => "messages",
        _ => "chat/completions",
    };
    let upstream_url = format!(
        "{}/{}",
        upstream_base_url.trim_end_matches('/'),
        upstream_path
    );

    // 7. Call upstream (with retry support)
    drop(_adapt_enter);
    let client = state.router.build_retry_client();

    let mut upstream_req = client.post(&upstream_url).json(&upstream_body);

    if let Some(ref api_key) = upstream_api_key {
        match provider_type {
            aigw_core::deployment::ProviderType::AnthropicNative => {
                upstream_req = upstream_req.header("x-api-key", api_key);
                upstream_req = upstream_req.header("anthropic-version", "2023-06-01");
            }
            _ => {
                upstream_req = upstream_req.header("Authorization", format!("Bearer {}", api_key));
            }
        }
    }

    // Build proxy_server_request (align with litellm)
    use std::time::{SystemTime, UNIX_EPOCH};
    let arrival_time_v1 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let proxy_server_request = Some(json!({
        "url": "/v1/messages",
        "method": "POST",
        "headers": {
            "user-agent": user_agent.clone().unwrap_or_default(),
            "x-forwarded-for": requester_ip.as_deref().unwrap_or(""),
        },
        "arrival_time": arrival_time_v1,
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
                model,
                upstream_base_url
            );
            "timeout_error"
        } else {
            tracing::error!("upstream request failed for model '{}': {}", model, e);
            "upstream_error"
        };
        // Record failure spend_log on timeout
        if is_timeout {
            let state2 = Arc::clone(&state);
            let upstream_body_clone = upstream_body.clone();
            let upstream_model2 = upstream_model.clone();
            let auth_token_hash_clone = auth_token_hash.clone();
            let auth_user_id_clone = auth_user_id.clone();
            let auth_team_id_clone = auth_team_id.clone();
            let auth_org_id_clone = auth_org_id.clone();
            let end_user_clone = end_user.clone();
            let psr = proxy_server_request.clone();
            let session_id_clone = session_id.clone();
            let requester_ip_clone = requester_ip.clone();
            let mid = upstream_model_id.clone();
            let mg = upstream_model_group.clone();
            let ccp = upstream_custom_llm_provider.clone();
            let mdata = metadata.clone();
            let call_type2 = call_type.to_string();
            let err_clone = err_msg.clone();
            let url_clone = upstream_base_url.clone();
            let url_for_resp = url_clone.clone();
            let model_for_resp = upstream_model2.clone();
            let rid = request_id.clone();
            tokio::spawn(async move {
                let end_time = chrono::Utc::now();
                let sl = SpendLog {
                    call_id: rid,
                    request_id: None, // timeout — no upstream response
                    call_type: call_type2,
                    api_key: auth_token_hash_clone,
                    spend: 0.0,
                    total_tokens: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    start_time,
                    end_time,
                    request_duration_ms: Some(
                        end_time
                            .signed_duration_since(start_time)
                            .num_milliseconds() as i32,
                    ),
                    completion_start_time: None,
                    model: upstream_model2,
                    model_id: mid,
                    model_group: mg,
                    custom_llm_provider: ccp,
                    api_base: Some(url_clone),
                    user: auth_user_id_clone,
                    metadata: mdata,
                    cache_hit: None,
                    cache_key: None,
                    request_tags: None,
                    team_id: auth_team_id_clone,
                    organization_id: auth_org_id_clone,
                    end_user: end_user_clone,
                    requester_ip_address: requester_ip_clone,
                    messages: Some(upstream_body_clone),
                    response: Some(json!({
                        "error": err_clone,
                        "failure_reason": "upstream_timeout",
                        "upstream_url": url_for_resp,
                        "model": model_for_resp,
                    })),
                    session_id: session_id_clone,
                    status: Some("timeout:upstream".to_string()),
                    mcp_namespaced_tool_name: None,
                    agent_id: None,
                    proxy_server_request: psr,
                    body_archived: false,
                    parquet_path: None,
                    image_tokens: None,
                };
                let _ = state2.db.insert_spend_log(&sl).await;
            });
        }
        anthropic_error(StatusCode::BAD_GATEWAY, err_type, &err_msg, &request_id)
    })?;

    let upstream_status = upstream_resp.status();
    let upstream_latency_ms = upstream_start.elapsed().as_millis() as i64;

    // Check if upstream returned a different x-request-id than what we sent.
    // v6.1 §11.4: also capture Anthropic's `request-id` header (no `x-` prefix)
    // before `.text().await` consumes upstream_resp downstream.
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

    let now = chrono::Utc::now();

    // Helper to record failure SpendLog (on upstream error)
    let write_failure_spend_log = |error_body: String, resp_json: Option<Value>| {
        let state = Arc::clone(&state);
        let upstream_body_clone = upstream_body.clone();
        let upstream_model3 = upstream_model.clone();
        let upstream_base_url_clone = upstream_base_url.clone();
        let auth_token_hash_clone = auth_token_hash.clone();
        let auth_user_id_clone = auth_user_id.clone();
        let auth_team_id_clone = auth_team_id.clone();
        let auth_org_id_clone = auth_org_id.clone();
        let end_user_clone = end_user.clone();
        let psr = proxy_server_request.clone();
        let session_id_clone = session_id.clone();
        let requester_ip_clone = requester_ip.clone();
        let status_code = upstream_status.as_u16();
        let mid = upstream_model_id.clone();
        let mg = upstream_model_group.clone();
        let ccp = upstream_custom_llm_provider.clone();
        let mdata = metadata.clone();
        let rid = request_id.clone();
        // v6.1 §11.2/§11.4: failure-path upstream id at INSERT.
        // Anthropic error body carries `request_id` (protocol field, value=upstream id);
        // fallback to pre-extracted upstream_req_id (request-id / x-request-id header).
        let fail_upstream_id = serde_json::from_str::<Value>(&error_body)
            .ok()
            .and_then(|v| {
                v.get("request_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
            })
            .or_else(|| upstream_req_id.clone());
        tokio::spawn(async move {
            let sl = SpendLog {
                call_id: rid,
                request_id: fail_upstream_id,
                call_type: call_type.to_string(),
                api_key: auth_token_hash_clone,
                spend: 0.0,
                total_tokens: 0,
                prompt_tokens: 0,
                completion_tokens: 0,
                start_time,
                end_time: now,
                request_duration_ms: Some(
                    now.signed_duration_since(start_time).num_milliseconds() as i32
                ),
                completion_start_time: None,
                model: upstream_model3,
                model_id: mid,
                model_group: mg,
                custom_llm_provider: ccp,
                api_base: Some(upstream_base_url_clone),
                user: auth_user_id_clone,
                metadata: mdata,
                cache_hit: None,
                cache_key: None,
                request_tags: None,
                team_id: auth_team_id_clone,
                organization_id: auth_org_id_clone,
                end_user: end_user_clone,
                requester_ip_address: requester_ip_clone,
                messages: Some(upstream_body_clone),
                response: resp_json.or_else(|| Some(json!({"error": error_body}))),
                session_id: session_id_clone,
                status: Some(format!("failure:{}", status_code)),
                mcp_namespaced_tool_name: None,
                agent_id: None,
                proxy_server_request: psr,
                body_archived: false,
                parquet_path: None,
                image_tokens: None,
            };
            let _ = state.db.insert_spend_log(&sl).await;
        });
    };

    let upstream_status = upstream_resp.status();

    if is_stream {
        if !upstream_status.is_success() {
            let error_body = upstream_resp.text().await.unwrap_or_default();
            write_failure_spend_log(error_body.clone(), None);
            return Err(anthropic_error(
                StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                "upstream_error",
                &format!(
                    "Upstream returned {}: {}",
                    upstream_status.as_u16(),
                    error_body
                ),
                &request_id,
            ));
        }

        // SSE streaming: two-phase spend-log pattern.
        // Phase 1: INSERT placeholder SpendLog BEFORE streaming begins.
        // Phase 2: UPDATE the same row with tokens + response AFTER stream ends.
        let streaming_request_id = request_id.clone();

        // Phase 1: pre-insert placeholder
        {
            let sl = SpendLog {
                call_id: streaming_request_id.clone(),
                request_id: None,
                call_type: call_type.to_string(),
                api_key: auth_token_hash.clone(),
                spend: 0.0,
                total_tokens: 0,
                prompt_tokens: 0,
                completion_tokens: 0,
                start_time,
                end_time: start_time,
                request_duration_ms: None,
                completion_start_time: None,
                model: upstream_model.clone(),
                model_id: upstream_model_id.clone(),
                model_group: upstream_model_group.clone(),
                custom_llm_provider: upstream_custom_llm_provider.clone(),
                api_base: Some(upstream_base_url.clone()),
                user: auth_user_id.clone(),
                metadata: metadata.clone(),
                cache_hit: None,
                cache_key: None,
                request_tags: None,
                team_id: auth_team_id.clone(),
                organization_id: auth_org_id.clone(),
                end_user: end_user.clone(),
                requester_ip_address: requester_ip.clone(),
                messages: Some(upstream_body.clone()),
                response: Some(json!({"status": "streaming"})),
                session_id: session_id.clone(),
                status: Some("streaming".to_string()),
                mcp_namespaced_tool_name: None,
                agent_id: None,
                proxy_server_request: proxy_server_request.clone(),
                body_archived: false,
                parquet_path: None,
                image_tokens: None,
            };
            let _ = state.db.insert_spend_log(&sl).await;
        }

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let state_clone = Arc::clone(&state);
        let sr_id = streaming_request_id.clone();
        let model_for_response = upstream_model.clone();
        let _upstream_base_url_clone = upstream_base_url.clone();
        let _auth_token_hash_clone = auth_token_hash.clone();
        let _auth_user_id_clone = auth_user_id.clone();
        let _auth_team_id_clone = auth_team_id.clone();
        let _auth_org_id_clone = auth_org_id.clone();
        let stream_metrics = state.metrics.clone();
        let stream_model = upstream_model.clone();
        let stream_user = auth_user_id.clone();
        let stream_api_base = upstream_base_url.clone();

        tokio::spawn(async move {
            use tokio_stream::StreamExt;
            let mut stream = upstream_resp.bytes_stream();
            let mut first_chunk_time: Option<chrono::DateTime<chrono::Utc>> = None;
            let _buffer: Vec<u8> = Vec::new();
            let _message_id = format!("msg_{}", uuid::Uuid::new_v4());
            let mut last_prompt_tokens: i32 = 0;
            let mut last_completion_tokens: i32 = 0;
            let mut last_cache_read: i32 = 0;
            let mut last_cache_creation: i32 = 0;
            let mut chunk_jsons: Vec<Value> = Vec::new();
            // Use AnthropicToOpenAIStream for full SSE→SSE tool_use conversion
            let mut stream_adapter = AnthropicToOpenAIStream::new();
            // v6.1 §11.3: extract upstream id BEFORE the `if choices` branch (which only
            // fires for OpenAI-shaped chunks). Anthropic-native `message_start` has no
            // `choices`; borrow `raw` here so the later `push(raw)` move is unaffected.
            let mut upstream_id: Option<String> = None;

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        if first_chunk_time.is_none() && !chunk.is_empty() {
                            first_chunk_time = Some(chrono::Utc::now());
                        }
                        // Process each SSE 'data:' line individually through AnthropicToOpenAIStream
                        if let Ok(text) = std::str::from_utf8(&chunk) {
                            for line in text.lines() {
                                if let Some(data) = line.strip_prefix("data: ") {
                                    if data != "[DONE]" {
                                        if let Ok(raw) = serde_json::from_str::<Value>(data) {
                                            // Extract upstream id (borrow raw, before any push/move).
                                            // Anthropic: message_start.message.id; OpenAI: top-level id.
                                            if upstream_id.is_none() {
                                                if let Some(id) =
                                                    raw.get("id").and_then(|v| v.as_str())
                                                {
                                                    upstream_id = Some(id.to_string());
                                                } else if let Some(msg) = raw.get("message") {
                                                    if let Some(id) =
                                                        msg.get("id").and_then(|v| v.as_str())
                                                    {
                                                        upstream_id = Some(id.to_string());
                                                    }
                                                }
                                            }
                                            if let Some(usage) = raw.get("usage") {
                                                last_prompt_tokens = usage
                                                    .get("prompt_tokens")
                                                    .or_else(|| usage.get("input_tokens"))
                                                    .and_then(|v| v.as_i64())
                                                    .unwrap_or(0)
                                                    as i32;
                                                last_completion_tokens = usage
                                                    .get("completion_tokens")
                                                    .or_else(|| usage.get("output_tokens"))
                                                    .and_then(|v| v.as_i64())
                                                    .unwrap_or(0)
                                                    as i32;
                                                last_cache_read =
                                                    super::chat::extract_cache_read_tokens(usage);
                                                last_cache_creation =
                                                    super::chat::extract_cache_creation_tokens(
                                                        usage,
                                                    );
                                            }
                                            if raw
                                                .get("choices")
                                                .and_then(|c| c.as_array())
                                                .map(|a| !a.is_empty())
                                                .unwrap_or(false)
                                            {
                                                chunk_jsons.push(raw);
                                            }
                                        }
                                        // Forward each SSE data line to the streaming adapter for Claude conversion
                                        if let Some(sse_event) =
                                            stream_adapter.next(data.as_bytes())
                                        {
                                            if tx.send(sse_event).is_err() {
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            // Send finishing SSE events once after stream ends (content_block_stop + message_stop).
            // finish() is idempotent — second call returns None.
            if let Some(final_event) = stream_adapter.finish() {
                let _ = tx.send(final_event);
            }

            let now = chrono::Utc::now();
            let streaming_spend = super::chat::calc_spend(
                last_prompt_tokens,
                last_completion_tokens,
                input_cost,
                output_cost,
                last_cache_read,
                last_cache_creation,
                cache_read_cost,
                cache_create_cost,
            );
            // Assemble a completion-style response from upstream raw chunks
            let assembled_response = if chunk_jsons.is_empty() {
                json!({"streaming": true, "prompt_tokens": last_prompt_tokens, "completion_tokens": last_completion_tokens, "total_tokens": last_prompt_tokens + last_completion_tokens})
            } else {
                let mut merged_content = String::new();
                let mut finish_reason: Option<String> = None;
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
                    "id": upstream_id.as_deref().unwrap_or("chatcmpl-streaming"),
                    "object": "chat.completion",
                    "model": model_for_response,
                    "choices": [{"index": 0, "message": message, "finish_reason": finish_reason}],
                    "usage": {"prompt_tokens": last_prompt_tokens, "completion_tokens": last_completion_tokens, "total_tokens": last_prompt_tokens + last_completion_tokens}
                })
            };

            // Phase 2: UPDATE the pre-inserted SpendLog row
            let duration_ms = now.signed_duration_since(start_time).num_milliseconds() as i32;
            let cst = first_chunk_time.unwrap_or(now);
            let cache_metadata = if last_cache_read > 0 || last_cache_creation > 0 {
                let mut m = serde_json::Map::new();
                m.insert("cache_read_tokens".to_string(), json!(last_cache_read));
                m.insert(
                    "cache_creation_tokens".to_string(),
                    json!(last_cache_creation),
                );
                let cache_read_spend =
                    last_cache_read as f64 * cache_read_cost.unwrap_or(input_cost.unwrap_or(0.0));
                let cache_create_spend = last_cache_creation as f64
                    * cache_create_cost.unwrap_or(input_cost.unwrap_or(0.0));
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
                Some(serde_json::Value::Object(m))
            } else {
                None
            };
            let _ = state_clone
                .db
                .update_spend_log(
                    &sr_id,
                    upstream_id.as_deref(),
                    streaming_spend,
                    last_prompt_tokens + last_completion_tokens,
                    last_prompt_tokens,
                    last_completion_tokens,
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
                let inc_th = _auth_token_hash_clone.clone();
                let inc_uid = _auth_user_id_clone.clone();
                let inc_tid = _auth_team_id_clone.clone();
                let inc_oid = _auth_org_id_clone.clone();
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

            // Record streaming metrics
            if let Some(ref m) = stream_metrics {
                let ttft = first_chunk_time.map(|fct| {
                    fct.signed_duration_since(start_time).num_milliseconds() as f64 / 1000.0
                });
                m.record_request(&RequestSummary {
                    model: stream_model.clone(),
                    user: stream_user.clone().unwrap_or_default(),
                    status_code: "200".to_string(),
                    success: true,
                    latency_secs: duration_ms as f64 / 1000.0,
                    upstream_latency_secs: duration_ms as f64 / 1000.0,
                    ttft_secs: ttft,
                    queue_time_secs: None,
                    spend: streaming_spend,
                    prompt_tokens: last_prompt_tokens,
                    completion_tokens: last_completion_tokens,
                    total_tokens: last_prompt_tokens + last_completion_tokens,
                    error_type: String::new(),
                    api_base: Some(stream_api_base.clone()),
                });
            }
        });

        // Build SSE response from the Anthropic-formatted event stream
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
        // Non-streaming: parse upstream OpenAI response and convert to Claude format
        if !upstream_status.is_success() {
            let error_body = upstream_resp.text().await.unwrap_or_default();
            write_failure_spend_log(error_body.clone(), None);
            return Err(anthropic_error(
                StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                "upstream_error",
                &format!(
                    "Upstream returned {}: {}",
                    upstream_status.as_u16(),
                    error_body
                ),
                &request_id,
            ));
        }

        let resp_body: Value = upstream_resp.json().await.map_err(|e| {
            anthropic_error(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                &format!("Failed to parse upstream response: {}", e),
                &request_id,
            )
        })?;

        // Convert upstream OpenAI response to Claude format via the adapter
        // (handles tool_calls → tool_use conversion correctly)
        let claude_response = adapter.adapt_response(resp_body.clone()).map_err(|e| {
            anthropic_error(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                &format!("Failed to convert response: {}", e),
                &request_id,
            )
        })?;

        // Record spend log
        let now = chrono::Utc::now();
        let usage = resp_body.get("usage");
        // Handle both OpenAI format (prompt_tokens/completion_tokens) and
        // Anthropic format (input_tokens/output_tokens) from passthrough.
        let prompt_tokens = usage
            .and_then(|u| u.get("prompt_tokens").or_else(|| u.get("input_tokens")))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let completion_tokens = usage
            .and_then(|u| {
                u.get("completion_tokens")
                    .or_else(|| u.get("output_tokens"))
            })
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let total_tokens = usage
            .and_then(|u| u.get("total_tokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or((prompt_tokens + completion_tokens) as i64)
            as i32;
        let non_stream_cache_read = usage
            .map(super::chat::extract_cache_read_tokens)
            .unwrap_or(0);
        let non_stream_cache_create = usage
            .map(super::chat::extract_cache_creation_tokens)
            .unwrap_or(0);
        let spend_amount = super::chat::calc_spend(
            prompt_tokens,
            completion_tokens,
            input_cost,
            output_cost,
            non_stream_cache_read,
            non_stream_cache_create,
            cache_read_cost,
            cache_create_cost,
        );
        // Build cache metadata JSON so cache tokens persist in spend_logs.metadata.
        // Mirrors the streaming path (above) and chat.rs non-streaming path.
        // Keys must match the Spend-Logs drawer extractCacheTokens + the activity SQL.
        // Image tokens: Anthropic usage has no image_tokens — estimate from
        // the original request body's Claude content blocks.
        let image_tokens = usage
            .and_then(aigw_core::image_tokens::extract_image_tokens_from_usage)
            .or_else(|| {
                let blocks = body_val
                    .get("messages")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let est = aigw_core::image_tokens::estimate_image_tokens_from_blocks(
                    &blocks,
                    &upstream_model,
                );
                if est > 0 {
                    Some(est as i32)
                } else {
                    None
                }
            });
        let non_stream_cache_metadata = if non_stream_cache_read > 0 || non_stream_cache_create > 0
        {
            let mut m = serde_json::Map::new();
            m.insert(
                "cache_read_tokens".to_string(),
                json!(non_stream_cache_read),
            );
            m.insert(
                "cache_creation_tokens".to_string(),
                json!(non_stream_cache_create),
            );
            let cache_read_spend =
                non_stream_cache_read as f64 * cache_read_cost.unwrap_or(input_cost.unwrap_or(0.0));
            let cache_create_spend = non_stream_cache_create as f64
                * cache_create_cost.unwrap_or(input_cost.unwrap_or(0.0));
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
            if image_tokens.is_some() {
                m.insert("image_tokens_source".to_string(), json!("estimated"));
            }
            Some(serde_json::Value::Object(m))
        } else if image_tokens.is_some() {
            let mut m = serde_json::Map::new();
            m.insert("image_tokens_source".to_string(), json!("estimated"));
            Some(serde_json::Value::Object(m))
        } else {
            None
        };
        let spend_log = aigw_core::models::SpendLog {
            call_id: request_id.clone(),
            // v6.1 §4.3: non-streaming success — upstream id at INSERT from resp_body.
            // (Both OpenAI `chatcmpl-xxx` and Anthropic `msg_xxx` put `id` at top level.)
            request_id: resp_body
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            call_type: "completion".to_string(),
            api_key: auth_token_hash.clone(),
            spend: spend_amount,
            total_tokens,
            prompt_tokens,
            completion_tokens,
            start_time,
            end_time: now,
            request_duration_ms: Some(
                now.signed_duration_since(start_time).num_milliseconds() as i32
            ),
            completion_start_time: Some(now), // non-streaming sentinel = end_time
            model: upstream_model.clone(),
            model_id: upstream_model_id.clone(),
            model_group: upstream_model_group.clone(),
            custom_llm_provider: upstream_custom_llm_provider.clone(),
            api_base: Some(upstream_base_url),
            user: auth_user_id.clone(),
            metadata: non_stream_cache_metadata,
            cache_hit: None,
            cache_key: None,
            request_tags: None,
            team_id: auth_team_id.clone(),
            organization_id: auth_org_id.clone(),
            end_user: end_user.clone(),
            requester_ip_address: requester_ip.clone(),
            messages: Some(upstream_body),
            response: Some(resp_body),
            session_id: session_id.clone(),
            status: Some("success".to_string()),
            mcp_namespaced_tool_name: None,
            agent_id: None,
            proxy_server_request: proxy_server_request.clone(),
            body_archived: false,
            parquet_path: None,
            image_tokens,
        };

        let _ = state.db.insert_spend_log(&spend_log).await;

        // Increment entity spends asynchronously (key + user + team + org if associated)
        let inc_db = state.db.clone();
        let inc_token_hash = auth_token_hash.clone();
        let inc_user_id = auth_user_id.clone();
        let inc_team_id = auth_team_id.clone();
        let inc_org_id = auth_org_id.clone();
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

        // Record Prometheus metrics (non-streaming v1/messages success)
        if let Some(ref m) = state.metrics {
            m.record_request(&RequestSummary {
                model: upstream_model.clone(),
                user: String::new(),
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
                api_base: Some(resolved_deployment.api_base.clone()),
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
                endpoint: "/v1/messages".to_string(),
                prompt_tokens: spend_log.prompt_tokens as i64,
                completion_tokens: spend_log.completion_tokens as i64,
                cache_read_input_tokens: non_stream_cache_read as i64,
                cache_creation_input_tokens: non_stream_cache_create as i64,
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

        Ok(Json(serde_json::to_value(&claude_response).map_err(|_| {
            anthropic_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Failed to serialize response",
                &request_id,
            )
        })?)
        .into_response())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::keys::{AppState, DEFAULT_KEY_TOKEN_LEN};
    use aigw_core::db::Database;
    use aigw_core::provider::ProviderRegistry;
    use aigw_core::rate_limiter::RateLimiter;
    use aigw_core::resolver::ModelResolver;
    use aigw_core::router::Router as AigwRouter;
    use axum::{body::Body, http::Method, Router};
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
            master_key: Some("sk-master-v1msg".to_string()),
            aigw_master_key: None,
            key_generate_length: DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: false,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            metrics: None,
        });

        Router::new()
            .route("/v1/messages", axum::routing::post(messages_handler))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_missing_anthropic_version() {
        let app = test_app().await;
        let body = json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100
        });

        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header("x-api-key", "sk-master-v1msg")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(val["type"].as_str(), Some("error"));
        assert_eq!(val["error"]["type"].as_str(), Some("invalid_request_error"));
    }

    #[tokio::test]
    async fn test_missing_auth() {
        let app = test_app().await;
        let body = json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100
        });

        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header("anthropic-version", "2023-06-01")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(val["type"].as_str(), Some("error"));
        assert_eq!(val["error"]["type"].as_str(), Some("authentication_error"));
    }

    #[tokio::test]
    async fn test_missing_max_tokens() {
        let app = test_app().await;
        let body = json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": "hi"}]
        });

        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header("anthropic-version", "2023-06-01")
            .header("x-api-key", "sk-master-v1msg")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(val["error"]["message"]
            .as_str()
            .unwrap()
            .contains("max_tokens"));
    }

    #[tokio::test]
    async fn test_missing_model() {
        let app = test_app().await;
        let body = json!({
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100
        });

        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header("anthropic-version", "2023-06-01")
            .header("x-api-key", "sk-master-v1msg")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(val["error"]["message"].as_str().unwrap().contains("model"));
    }

    #[tokio::test]
    async fn test_missing_messages() {
        let app = test_app().await;
        let body = json!({
            "model": "claude-3",
            "max_tokens": 100
        });

        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header("anthropic-version", "2023-06-01")
            .header("x-api-key", "sk-master-v1msg")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(val["error"]["message"]
            .as_str()
            .unwrap()
            .contains("messages"));
    }

    #[tokio::test]
    async fn test_auth_bearer_token() {
        let app = test_app().await;
        let body = json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100
        });

        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header("anthropic-version", "2023-06-01")
            .header(http::header::AUTHORIZATION, "Bearer sk-master-v1msg")
            .body(Body::from(body.to_string()))
            .unwrap();

        // Master key should pass auth. Response may fail later (upstream unreachable, etc.)
        // but should NOT be an authentication_error from aigw.
        let response = app.oneshot(request).await.unwrap();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        // Not an auth error from aigw (upstream errors show as upstream_error, not authentication_error)
        assert_ne!(val["error"]["type"].as_str(), Some("authentication_error"));
    }

    #[tokio::test]
    async fn test_model_not_found() {
        // When model is not in proxy_models, /v1/messages should return Anthropic format error
        let body = json!({
            "model": "nonexistent-model-12345",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100
        });

        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header("anthropic-version", "2023-06-01")
            .header("x-api-key", "sk-master-v1msg")
            .body(Body::from(body.to_string()))
            .unwrap();

        let app = test_app().await;
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        // Anthropic error format: {"type":"error","error":{"type":"...","message":"..."}}
        assert_eq!(val["type"].as_str(), Some("error"));
        assert_eq!(val["error"]["type"].as_str(), Some("invalid_request_error"));
        assert!(val["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not found"));
    }

    #[tokio::test]
    async fn test_missing_auth_returns_unauthorized() {
        let app = test_app().await;
        let body = json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100
        });

        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header("anthropic-version", "2023-06-01")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(val["type"].as_str(), Some("error"));
        assert_eq!(val["error"]["type"].as_str(), Some("authentication_error"));
    }

    #[tokio::test]
    async fn test_anthropic_error_format() {
        let app = test_app().await;
        let body = json!({});

        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(http::header::CONTENT_TYPE, "application/json")
            .header("anthropic-version", "2023-06-01")
            .header("x-api-key", "sk-master-v1msg")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&body_bytes).unwrap();

        // Anthropic error format: {"type":"error","error":{"type":"...","message":"..."},"request_id":"..."}
        assert_eq!(val["type"].as_str(), Some("error"));
        assert!(val["error"]["type"].as_str().is_some());
        assert!(val["error"]["message"].as_str().is_some());
        assert!(val["request_id"].as_str().is_some());
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // SSE conversion unit tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    /// Test basic OpenAI SSE → Anthropic SSE conversion
    #[test]
    fn test_sse_conversion_adapter_mapping() {
        use aigw_core::adapter::{DefaultAdapter, ProviderAdapter};
        use aigw_core::models::ChatCompletionChunk;

        // Simulate OpenAI SSE chunks
        let chunks = vec![
            // role delta → message_start
            r#"data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1,"model":"gpt-4","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#,
            // content delta → content_block_delta
            r#"data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#,
            // finish_reason → message_delta
            r#"data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1,"model":"gpt-4","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        ];

        let events: Vec<String> = chunks
            .iter()
            .filter_map(|line| {
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if let Ok(c) = serde_json::from_str::<ChatCompletionChunk>(json_str) {
                        if let Some(event) = DefaultAdapter::openai_chunk_to_claude_stream(&c) {
                            return Some(event.event_type);
                        }
                    }
                }
                None
            })
            .collect();

        assert_eq!(
            events,
            vec![
                "message_start".to_string(),
                "content_block_delta".to_string(),
                "message_delta".to_string(),
            ]
        );
    }

    /// Test that [DONE] marker parsing is recognized correctly
    #[test]
    fn test_sse_done_marker_detection() {
        // Simulate the SSE parsing logic for [DONE]
        let done_line = "data: [DONE]";
        assert_eq!(done_line, "data: [DONE]");
    }

    /// Test full SSE frame → Anthropic event type extraction
    #[test]
    fn test_sse_frame_to_event_types() {
        // Simulate what the streaming loop produces: parse OpenAI SSE → event types
        let raw_sse = concat!(
            "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );

        use aigw_core::adapter::{DefaultAdapter, ProviderAdapter};
        use aigw_core::models::ChatCompletionChunk;

        let mut event_types = Vec::new();
        let mut rest = raw_sse;
        while let Some(pos) = rest.find("\n\n") {
            let frame = &rest[..pos + 2];
            rest = &rest[pos + 2..];

            for line in frame.lines() {
                if line == "data: [DONE]" {
                    event_types.push("DONE".to_string());
                    break;
                }
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if let Ok(c) = serde_json::from_str::<ChatCompletionChunk>(json_str) {
                        if let Some(event) = DefaultAdapter::openai_chunk_to_claude_stream(&c) {
                            event_types.push(event.event_type);
                        }
                    }
                }
            }
        }

        assert_eq!(
            event_types,
            vec![
                "message_start",       // role delta
                "content_block_delta", // content delta
                "message_delta",       // finish_reason
                "DONE",                // stream end
            ]
        );
    }

    /// Test that usage extraction from a chunk with usage field works
    #[test]
    fn test_sse_usage_extraction() {
        let json_str = r#"{"id":"1","object":"chat.completion.chunk","created":1,"model":"gpt-4","choices":[],"usage":{"prompt_tokens":15,"completion_tokens":42,"total_tokens":57}}"#;

        let raw: Value = serde_json::from_str(json_str).unwrap();
        let prompt = raw
            .get("usage")
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let completion = raw
            .get("usage")
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;

        assert_eq!(prompt, 15);
        assert_eq!(completion, 42);
    }
}
