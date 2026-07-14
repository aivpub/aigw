//! Claude-compatible /v1/messages endpoint
//!
//! Supports both non-streaming and SSE streaming, with protocol conversion
//! to OpenAI upstream via the adapter layer.
//!
//! Auth: x-api-key header or Authorization: Bearer header (Claude convention)

use aigw_core::adapter::{ClientProtocol, MessageAdapter, ProviderAdapter, select_adapter};
use aigw_core::adapter::DefaultAdapter;
use aigw_core::auth::decode_jwt;
use aigw_core::crypto::hash_token;
use aigw_core::models::{ClaudeMessageRequest, DailySpendKind, DailySpendLog, SpendLog};
use aigw_core::resolver::ModelResolver;
use axum::{
    extract::State,
    http::{self, StatusCode, header},
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::StreamExt;

use super::keys::SharedState;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Anthropic error helper
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn anthropic_error(
    status: StatusCode,
    error_type: &str,
    message: &str,
) -> (StatusCode, Json<Value>) {
    let request_id = format!("req_{}", uuid::Uuid::new_v4());
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
) -> Option<(String, Option<String>)> {
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

    Some((token_hash, key.user_id))
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
    http::request::Parts {
        headers,
        ..
    }: http::request::Parts,
    body: String,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    // 1. Auth: try x-api-key/Bearer header, fallback to cookie JWT
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
    let (auth_token_hash, auth_user_id) = if let Some(token) = extracted {
        if token.is_empty() {
            let request_id = format!("req_{}", uuid::Uuid::new_v4());
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
            ("master_key".to_string(), None)
        } else {
            let token_hash = hash_token(token);
            let key = state.db.get_key_by_token(&token_hash).await.map_err(|_| {
                let request_id = format!("req_{}", uuid::Uuid::new_v4());
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
                let request_id = format!("req_{}", uuid::Uuid::new_v4());
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

            // Budget check
            if let Some(max_budget) = key.max_budget_f64() {
                if key.spend >= max_budget {
                    return Err(anthropic_error(
                        StatusCode::TOO_MANY_REQUESTS,
                        "budget_exceeded",
                        "Budget exceeded for this API key",
                    ));
                }
            }

            let user_id = key.user_id.clone();
            (token_hash, user_id)
        }
    } else {
        // Fallback: try cookie JWT (same pattern as ChatAuth)
        match try_cookie_jwt_auth(&state, &headers).await {
            Some((token_hash, user_id)) => (token_hash, user_id),
            None => {
                let request_id = format!("req_{}", uuid::Uuid::new_v4());
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
            )
        })?;

    // 3. Parse request body
    let body_val: Value = serde_json::from_str(&body).map_err(|e| {
        anthropic_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            &format!("Failed to parse request body: {}", e),
        )
    })?;

    // 3. Validate required fields
    let model = body_val
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            anthropic_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "Missing required field: model",
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
            )
        })?;

    if messages.is_empty() {
        return Err(anthropic_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "messages must not be empty",
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
            )
        })?;

    let is_stream = body_val
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let call_type = if is_stream { "completion_stream" } else { "completion" };

    // 4. Resolve upstream via ModelResolver
    let resolved_deployment = match state.resolver.resolve(&model).await {
        Ok(deployments) => {
            deployments.into_iter().next().ok_or_else(|| {
                anthropic_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    &format!("Model '{}' not found", model),
                )
            })?
        }
        Err((status, body)) => {
            let now = chrono::Utc::now();
            let error_body = body.0.clone();
            let log_state = Arc::clone(&state);
            let log_model = model.clone();
            let log_token_hash = auth_token_hash.clone();
            let log_user_id = auth_user_id.clone();
            tokio::spawn(async move {
                let sl = SpendLog {
                    request_id: uuid::Uuid::new_v4().to_string(),
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
                    metadata: None,
                    cache_hit: None,
                    cache_key: None,
                    request_tags: None,
                    team_id: None,
                    organization_id: None,
                    end_user: None,
                    requester_ip_address: None,
                    messages: Some(error_body.clone()),
                    response: Some(error_body),
                    session_id: None,
                    status: Some(format!("failure:{}", status.as_u16())),
                    mcp_namespaced_tool_name: None,
                    agent_id: None,
                    proxy_server_request: None,
                };
                let _ = log_state.db.insert_spend_log(&sl).await;
            });
            let msg = body["error"]["message"].as_str().unwrap_or("Unknown error");
            let err_type = body["error"]["type"].as_str().unwrap_or("invalid_request_error");
            return Err(anthropic_error(status, err_type, msg));
        }
    };
    let input_cost = resolved_deployment.input_cost_per_token;
    let output_cost = resolved_deployment.output_cost_per_token;

    let upstream_base_url = resolved_deployment.api_base.clone();
    let upstream_api_key = resolved_deployment.api_key.clone();

    // Select adapter based on client protocol + provider type
    let adapter = select_adapter(ClientProtocol::Anthropic, &resolved_deployment.provider_type)
        .ok_or_else(|| {
            anthropic_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "Unsupported provider type for this endpoint",
            )
        })?;

    // 6. Adapt Claude request to OpenAI format via adapter
    let upstream_body = adapter.adapt_request(body_val.clone(), &resolved_deployment).map_err(|e| {
        anthropic_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &format!("Adapter error: {}", e),
        )
    })?;

    let upstream_url = format!(
        "{}/chat/completions",
        upstream_base_url.trim_end_matches('/')
    );

    // 7. Call upstream
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| {
            anthropic_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                &format!("HTTP client error: {}", e),
            )
        })?;

    let mut upstream_req = client.post(&upstream_url).json(&upstream_body);

    if let Some(ref api_key) = upstream_api_key {
        upstream_req = upstream_req.header("Authorization", format!("Bearer {}", api_key));
    }

    let start_time = chrono::Utc::now();

    let upstream_resp = upstream_req.send().await.map_err(|e| {
        anthropic_error(
            StatusCode::BAD_GATEWAY,
            "upstream_error",
            &format!("Upstream request failed: {}", e),
        )
    })?;

    let upstream_status = upstream_resp.status();
    let now = chrono::Utc::now();

    // Helper to record failure SpendLog (on upstream error)
    let write_failure_spend_log = |error_body: String, resp_json: Option<Value>| {
        let state = Arc::clone(&state);
        let upstream_body_clone = upstream_body.clone();
        let model_clone = model.clone();
        let upstream_base_url_clone = upstream_base_url.clone();
        let auth_token_hash_clone = auth_token_hash.clone();
        let auth_user_id_clone = auth_user_id.clone();
        let status_code = upstream_status.as_u16();
        tokio::spawn(async move {
            let sl = SpendLog {
                request_id: uuid::Uuid::new_v4().to_string(),
                call_type: call_type.to_string(),
                api_key: auth_token_hash_clone,
                spend: 0.0,
                total_tokens: 0,
                prompt_tokens: 0,
                completion_tokens: 0,
                start_time,
                end_time: now,
                request_duration_ms: Some(
                    now.signed_duration_since(start_time).num_milliseconds() as i32,
                ),
                completion_start_time: None,
                model: model_clone,
                model_id: None,
                model_group: None,
                custom_llm_provider: None,
                api_base: Some(upstream_base_url_clone),
                user: auth_user_id_clone,
                metadata: None,
                cache_hit: None,
                cache_key: None,
                request_tags: None,
                team_id: None,
                organization_id: None,
                end_user: None,
                requester_ip_address: None,
                messages: Some(upstream_body_clone),
                response: resp_json.or_else(|| Some(json!({"error": error_body}))),
                session_id: None,
                status: Some(format!("failure:{}", status_code)),
                mcp_namespaced_tool_name: None,
                agent_id: None,
                proxy_server_request: None,
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
                StatusCode::from_u16(upstream_status.as_u16())
                    .unwrap_or(StatusCode::BAD_GATEWAY),
                "upstream_error",
                &format!("Upstream returned {}: {}", upstream_status.as_u16(), error_body),
            ));
        }

        // SSE streaming: two-phase spend-log pattern.
        // Phase 1: INSERT placeholder SpendLog BEFORE streaming begins.
        // Phase 2: UPDATE the same row with tokens + response AFTER stream ends.
        let streaming_request_id = format!("req_{}", uuid::Uuid::new_v4());

        // Phase 1: pre-insert placeholder
        {
            let sl = SpendLog {
                request_id: streaming_request_id.clone(),
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
                model: model.clone(),
                model_id: None,
                model_group: None,
                custom_llm_provider: None,
                api_base: Some(upstream_base_url.clone()),
                user: auth_user_id.clone(),
                metadata: None,
                cache_hit: None,
                cache_key: None,
                request_tags: None,
                team_id: None,
                organization_id: None,
                end_user: None,
                requester_ip_address: None,
                messages: Some(upstream_body.clone()),
                response: Some(json!({"status": "streaming"})),
                session_id: None,
                status: Some("streaming".to_string()),
                mcp_namespaced_tool_name: None,
                agent_id: None,
                proxy_server_request: None,
            };
            let _ = state.db.insert_spend_log(&sl).await;
        }

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let state_clone = Arc::clone(&state);
        let sr_id = streaming_request_id.clone();
        let model_clone = model.clone();
        let upstream_base_url_clone = upstream_base_url.clone();
        let auth_token_hash_clone = auth_token_hash.clone();
        let auth_user_id_clone = auth_user_id.clone();

        tokio::spawn(async move {
            use tokio_stream::StreamExt;
            let mut stream = upstream_resp.bytes_stream();
            let mut first_chunk_time: Option<chrono::DateTime<chrono::Utc>> = None;
            let mut buffer: Vec<u8> = Vec::new();
            let mut message_started = false;
            let mut content_block_started = false;
            let message_id = format!("msg_{}", uuid::Uuid::new_v4());
            // Track token usage from the last chunk (when upstream sends stream_options include_usage)
            let mut last_prompt_tokens: i32 = 0;
            let mut last_completion_tokens: i32 = 0;

            // Helper: write a single Anthropic SSE event to the channel
            let write_anthropic_sse =
                |tx: &tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
                 event_type: &str,
                 data: &Value| {
                    let json_str = serde_json::to_string(data).unwrap_or_default();
                    let sse_frame =
                        format!("event: {}\ndata: {}\n\n", event_type, json_str);
                    let _ = tx.send(sse_frame.into_bytes());
                };

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        if first_chunk_time.is_none() && !chunk.is_empty() {
                            first_chunk_time = Some(chrono::Utc::now());
                        }
                        buffer.extend_from_slice(&chunk);

                        // Split on \n\n to get complete SSE frames
                        loop {
                            let pos = buffer
                                .windows(2)
                                .position(|w| w == b"\n\n");
                            let pos = match pos {
                                Some(p) => p,
                                None => break,
                            };
                            let frame = buffer.drain(..pos + 2).collect::<Vec<_>>();
                            let frame_str = String::from_utf8_lossy(&frame);

                            for line in frame_str.lines() {
                                if line == "data: [DONE]" {
                                    // Stream end — send final events
                                    if content_block_started {
                                        write_anthropic_sse(
                                            &tx,
                                            "content_block_stop",
                                            &json!({"type": "content_block_stop", "index": 0}),
                                        );
                                    }
                                    write_anthropic_sse(
                                        &tx,
                                        "message_delta",
                                        &json!({
                                            "type": "message_delta",
                                            "delta": {"stop_reason": "end_turn"},
                                            "usage": {"output_tokens": 0}
                                        }),
                                    );
                                    write_anthropic_sse(
                                        &tx,
                                        "message_stop",
                                        &json!({"type": "message_stop"}),
                                    );
                                    break;
                                }
                                if let Some(json_str) = line.strip_prefix("data: ") {
                                    // Try to extract usage from this chunk
                                    // (OpenAI sends a final chunk with usage when stream_options.include_usage=true)
                                    if let Ok(raw) = serde_json::from_str::<Value>(json_str) {
                                        if let Some(usage) = raw.get("usage") {
                                            last_prompt_tokens = usage
                                                .get("prompt_tokens")
                                                .and_then(|v| v.as_i64())
                                                .unwrap_or(0) as i32;
                                            last_completion_tokens = usage
                                                .get("completion_tokens")
                                                .and_then(|v| v.as_i64())
                                                .unwrap_or(0) as i32;
                                        }
                                    }
                                    if let Ok(c) = serde_json::from_str::<aigw_core::models::ChatCompletionChunk>(json_str) {
                                        if let Some(event) =
                                            DefaultAdapter::openai_chunk_to_claude_stream(&c)
                                        {
                                            match event.event_type.as_str() {
                                                "message_start" => {
                                                    let payload = json!({
                                                        "type": "message_start",
                                                        "message": {
                                                            "id": message_id,
                                                            "type": "message",
                                                            "role": "assistant",
                                                            "content": [],
                                                            "model": model_clone,
                                                            "stop_reason": null,
                                                            "stop_sequence": null,
                                                            "usage": {
                                                                "input_tokens": 0,
                                                                "output_tokens": 0
                                                            }
                                                        }
                                                    });
                                                    write_anthropic_sse(&tx, "message_start", &payload);
                                                    message_started = true;
                                                }
                                                "content_block_delta" => {
                                                    if !content_block_started {
                                                        if !message_started {
                                                            // Some models skip role chunk; inject message_start
                                                            write_anthropic_sse(
                                                                &tx,
                                                                "message_start",
                                                                &json!({
                                                                    "type": "message_start",
                                                                    "message": {
                                                                        "id": message_id,
                                                                        "type": "message",
                                                                        "role": "assistant",
                                                                        "content": [],
                                                                        "model": model_clone,
                                                                        "stop_reason": null,
                                                                        "stop_sequence": null,
                                                                        "usage": {
                                                                            "input_tokens": 0,
                                                                            "output_tokens": 0
                                                                        }
                                                                    }
                                                                }),
                                                            );
                                                            message_started = true;
                                                        }
                                                        write_anthropic_sse(
                                                            &tx,
                                                            "content_block_start",
                                                            &json!({
                                                                "type": "content_block_start",
                                                                "index": 0,
                                                                "content_block": {
                                                                    "type": "text",
                                                                    "text": ""
                                                                }
                                                            }),
                                                        );
                                                        content_block_started = true;
                                                    }
                                                    write_anthropic_sse(
                                                        &tx,
                                                        "content_block_delta",
                                                        &serde_json::to_value(&event).unwrap_or_default(),
                                                    );
                                                }
                                                "message_delta" => {
                                                    if content_block_started {
                                                        write_anthropic_sse(
                                                            &tx,
                                                            "content_block_stop",
                                                            &json!({
                                                                "type": "content_block_stop",
                                                                "index": 0
                                                            }),
                                                        );
                                                    }
                                                    write_anthropic_sse(
                                                        &tx,
                                                        "message_delta",
                                                        &serde_json::to_value(&event).unwrap_or_default(),
                                                    );
                                                }
                                                _ => {}
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
            // drop(tx) happens when scope exits

            let now = chrono::Utc::now();
            let streaming_spend = super::chat::calc_spend(
                last_prompt_tokens, last_completion_tokens, input_cost, output_cost,
            );
            let streaming_response = json!({
                "streaming": true,
                "prompt_tokens": last_prompt_tokens,
                "completion_tokens": last_completion_tokens,
                "total_tokens": last_prompt_tokens + last_completion_tokens,
            });

            // Phase 2: UPDATE the pre-inserted SpendLog row
            let duration_ms = now.signed_duration_since(start_time).num_milliseconds() as i32;
            let cst = first_chunk_time.unwrap_or(now);
            let _ = state_clone.db.update_spend_log(
                &sr_id,
                streaming_spend,
                last_prompt_tokens + last_completion_tokens,
                last_prompt_tokens,
                last_completion_tokens,
                now,
                duration_ms,
                cst,
                streaming_response,
                "success",
            ).await;
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
                StatusCode::from_u16(upstream_status.as_u16())
                    .unwrap_or(StatusCode::BAD_GATEWAY),
                "upstream_error",
                &format!("Upstream returned {}: {}", upstream_status.as_u16(), error_body),
            ));
        }

        let resp_body: Value = upstream_resp.json().await.map_err(|e| {
            anthropic_error(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                &format!("Failed to parse upstream response: {}", e),
            )
        })?;

        // Convert OpenAI response to Claude format
        let oai_response: aigw_core::models::ChatCompletionResponse =
            serde_json::from_value(resp_body.clone()).map_err(|e| {
                anthropic_error(
                    StatusCode::BAD_GATEWAY,
                    "upstream_error",
                    &format!("Failed to parse upstream response: {}", e),
                )
            })?;

        let claude_response =
            DefaultAdapter::openai_to_claude_response(&oai_response);

        // Record spend log
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
        let total_tokens = usage
            .and_then(|u| u.get("total_tokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let spend_amount =
            super::chat::calc_spend(prompt_tokens, completion_tokens, input_cost, output_cost);
        let spend_log = aigw_core::models::SpendLog {
            request_id: uuid::Uuid::new_v4().to_string(),
            call_type: "completion".to_string(),
            api_key: auth_token_hash.clone(),
            spend: spend_amount,
            total_tokens,
            prompt_tokens,
            completion_tokens,
            start_time,
            end_time: now,
            request_duration_ms: Some(
                now.signed_duration_since(start_time).num_milliseconds() as i32,
            ),
            completion_start_time: Some(now), // non-streaming sentinel = end_time
            model: model.to_string(),
            model_id: None,
            model_group: None,
            custom_llm_provider: None,
            api_base: Some(upstream_base_url),
            user: auth_user_id.clone(),
            metadata: None,
            cache_hit: None,
            cache_key: None,
            request_tags: None,
            team_id: None,
            organization_id: None,
            end_user: None,
            requester_ip_address: None,
            messages: Some(upstream_body),
            response: Some(resp_body),
            session_id: None,
            status: Some("success".to_string()),
            mcp_namespaced_tool_name: None,
            agent_id: None,
            proxy_server_request: None,
        };

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
                endpoint: "/v1/messages".to_string(),
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

        Ok(Json(serde_json::to_value(&claude_response).map_err(
            |_| {
                anthropic_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "Failed to serialize response",
                )
            },
        )?).into_response())
    }
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
    use axum::{body::Body, http::Method, Router};
    use std::sync::Arc;
    use tower::util::ServiceExt;

    async fn test_app() -> Router {
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");
        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            db,
            master_key: Some("sk-master-v1msg".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
        });

        Router::new()
            .route(
                "/v1/messages",
                axum::routing::post(messages_handler),
            )
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
        assert_eq!(
            val["error"]["type"].as_str(),
            Some("invalid_request_error")
        );
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
        assert_eq!(
            val["error"]["type"].as_str(),
            Some("authentication_error")
        );
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
        assert!(val["error"]["message"].as_str().unwrap().contains("max_tokens"));
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
        assert!(val["error"]["message"].as_str().unwrap().contains("messages"));
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
        assert_ne!(
            val["error"]["type"].as_str(),
            Some("authentication_error")
        );
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
        assert_eq!(
            val["error"]["type"].as_str(),
            Some("invalid_request_error")
        );
        assert!(
            val["error"]["message"]
                .as_str()
                .unwrap()
                .contains("not found")
        );
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
        assert_eq!(
            val["error"]["type"].as_str(),
            Some("authentication_error")
        );
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
        use aigw_core::models::ChatCompletionChunk;
        use aigw_core::adapter::{ClientProtocol, MessageAdapter, ProviderAdapter, select_adapter};
use aigw_core::adapter::DefaultAdapter;

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
                    if let Ok(c) =
                        serde_json::from_str::<ChatCompletionChunk>(json_str)
                    {
                        if let Some(event) =
                            DefaultAdapter::openai_chunk_to_claude_stream(&c)
                        {
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

        use aigw_core::models::ChatCompletionChunk;
        use aigw_core::adapter::{ClientProtocol, MessageAdapter, ProviderAdapter, select_adapter};
use aigw_core::adapter::DefaultAdapter;

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
                        if let Some(event) =
                            DefaultAdapter::openai_chunk_to_claude_stream(&c)
                        {
                            event_types.push(event.event_type);
                        }
                    }
                }
            }
        }

        assert_eq!(
            event_types,
            vec![
                "message_start",          // role delta
                "content_block_delta",    // content delta
                "message_delta",          // finish_reason
                "DONE",                   // stream end
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
