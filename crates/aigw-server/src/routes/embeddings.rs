//! OpenAI Embeddings API endpoint — POST /v1/embeddings (and aliases)
//!
//! Passthrough: validates `model` + `input`, resolves the upstream deployment,
//! **hard-selects `OpenAIPassthrough`** (never `select_adapter` — the
//! `OpenAI + AnthropicNative → OpenAIToAnthropic` arm would treat the embedding
//! body as a chat request and corrupt it), and forwards to upstream
//! `{api_base}/embeddings`. Response is passed through verbatim.
//!
//! ## Endpoints (all share this handler — path differs only)
//! - `/v1/embeddings` — OpenAI standard (primary)
//! - `/embeddings` — unversioned alias
//! - `/engines/{model}/embeddings` — Azure legacy alias (model from path)
//! - `/openai/deployments/{model}/embeddings` — Azure alias (model from path)
//!
//! ## Differences from responses.rs
//!
//! | Area | responses.rs | embeddings.rs |
//! |------|--------------|---------------|
//! | Client protocol | `ClientProtocol::Responses` | `OpenAIPassthrough` (hard-coded) |
//! | Request validation | `model` + `input` | `model` + `input` (string\|array) |
//! | Upstream URL path | `responses` / `chat/completions` | `embeddings` |
//! | Response usage fields | input/output dual fallback | `prompt_tokens` / `total_tokens` |
//! | SpendLog call_type | `"responses"` | `"embedding"` |
//! | Stream support | streaming two-phase | non-streaming only |
//! | Billing | prompt + completion | prompt-only (completion = 0) |

use aigw_core::adapter::{MessageAdapter, OpenAIPassthrough};
use aigw_core::metrics::RequestSummary;
use aigw_core::models::{DailySpendKind, DailySpendLog, SpendLog};
use axum::{
    extract::State,
    http::{self, header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use tower_http::request_id::RequestId;

pub use super::chat::ChatAuth;
use super::chat::{
    calc_spend, extract_cache_creation_tokens, extract_cache_read_tokens, resolve_key_model_list,
};
use super::ip_extractor::OptionalClientIp;
use super::keys::SharedState;
use aigw_core::otel_tracing;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Handler: POST /v1/embeddings (shared by all four endpoints)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Shared handler body — `model_path` is None for the plain `/v1/embeddings`
/// and `/embeddings` routes (registered via `embeddings_handler`), and
/// `Some(model)` for the Azure aliases (`/engines/{model}/embeddings` and
/// `/openai/deployments/{model}/embeddings`, registered via
/// `embeddings_handler_with_path`).
#[allow(clippy::too_many_arguments)]
async fn embeddings_handler_inner(
    state: SharedState,
    auth: aigw_core::middleware::KeyIdentity,
    client_ip: Option<axum_client_ip::RightmostXForwardedFor>,
    headers: axum::http::HeaderMap,
    extensions: axum::http::Extensions,
    model_path: Option<String>,
    body: Value,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    let request_id = extensions
        .get::<RequestId>()
        .and_then(|id| id.header_value().to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    if state.otel_active {
        let _otel_ctx = otel_tracing::extract_traceparent(&headers);
    }

    let body = merge_path_model(body, model_path);

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

    // 2. Validate input (string | array, non-empty)
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

    // Root span
    let root_span = tracing::info_span!("embeddings", model = %_model);
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
            // + RPM/TPM + soft_budget alerting. token_estimate=0 (embeddings
            // have no max_tokens; RPM + budget still enforced).
            let limit_result = aigw_core::middleware::rate_limit::check_request_limits(
                &state.db,
                &state.rate_limiter,
                &auth,
                0,
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

    // Stage 128 §2.5: Anthropic OAuth credentials have no embedding endpoint —
    // reject with 400 (Anthropic offers no /v1/embeddings).
    if deployment.oauth.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "message": format!(
                        "Anthropic OAuth 凭证不支持 embeddings — 模型 '{}' 解析到 OAuth 反代部署",
                        _model
                    ),
                    "type": "invalid_request_error",
                    "code": "unsupported_provider"
                }
            })),
        ));
    }

    // ⚠️ Hard-select OpenAIPassthrough. `select_adapter(OpenAI, AnthropicNative)`
    //    returns `OpenAIToAnthropic` which would mangle the embedding body.
    //    Embedding models are inherently OpenAI-compatible; AnthropicNative
    //    deployments are rejected outright.
    if deployment.provider_type == aigw_core::deployment::ProviderType::AnthropicNative {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "message": format!(
                        "Unsupported provider type for Embeddings API with model '{}'",
                        _model
                    ),
                    "type": "invalid_request_error",
                    "code": "unsupported_provider"
                }
            })),
        ));
    }
    let adapter = OpenAIPassthrough;
    drop(_resolve_enter);

    // Adapt request (OpenAIPassthrough only rewrites model + injects stream_options)
    let adapt_span = tracing::info_span!("adapt_request");
    let _adapt_enter = adapt_span.enter();
    let upstream_body_val = adapter
        .adapt_request(body.clone(), &deployment)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("Adapter error: {}", e), "type": "adapter_error"}})),
            )
        })?;

    // Upstream URL — always {api_base}/embeddings (four endpoints converge here)
    let upstream_url = format!("{}/embeddings", deployment.api_base.trim_end_matches('/'));
    drop(_adapt_enter);

    // ── Metadata extraction (same as chat.rs / responses.rs) ──
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
        "url": "/v1/embeddings",
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
        // Embedding deployments are always OpenAI-compatible — Bearer auth.
        upstream_req = upstream_req.header("Authorization", format!("Bearer {}", api_key));
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

    // ── 6. Non-streaming dispatch ──
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
                call_type: "embedding".to_string(),
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

    // Adapt response (OpenAIPassthrough → identity)
    let adapted_resp = adapter.adapt_response(resp_body.clone()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": {"message": format!("Adapter error: {}", e), "type": "adapter_error"}
            })),
        )
    })?;

    // Extract usage — embeddings report prompt_tokens + total_tokens only
    let usage = adapted_resp.get("usage");
    let prompt_tokens = usage
        .map(super::responses::extract_prompt_tokens)
        .unwrap_or(0);
    let completion_tokens = 0; // embeddings have no completion tokens
    let total_tokens = usage
        .map(super::responses::extract_total_tokens)
        .unwrap_or(0);
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

    // ── Insert SpendLog (call_type = "embedding", prompt-only billing) ──
    let spend_log = SpendLog {
        call_id: request_id.clone(),
        request_id: response_upstream_id,
        call_type: "embedding".to_string(),
        api_key: auth.token_hash.clone(),
        spend: spend_amount,
        total_tokens,
        prompt_tokens,
        completion_tokens,
        start_time,
        end_time: now,
        request_duration_ms: Some(duration_ms),
        completion_start_time: Some(now),
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
            endpoint: "/v1/embeddings".to_string(),
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
    response = response.header("x-request-id", &request_id);
    Ok(response
        .body(axum::body::Body::from(
            serde_json::to_string(&adapted_resp).unwrap(),
        ))
        .unwrap())
}

/// Merge an Azure-alias `{model}` path parameter into the request body.
///
/// For `/engines/{model}/embeddings` and `/openai/deployments/{model}/embeddings`
/// the model arrives in the URL path, not the JSON body. When the path carries
/// a model, it takes precedence (OpenAI Azure semantics). When the path is
/// absent (plain `/v1/embeddings` / `/embeddings`), the body is unchanged so the
/// body-level `model` validation runs normally.
/// Public handler for the plain `/v1/embeddings` + `/embeddings` routes
/// (no `{model}` path segment — Path extractor would reject/500).
#[allow(clippy::too_many_arguments)]
pub async fn embeddings_handler(
    State(state): State<SharedState>,
    ChatAuth(auth): ChatAuth,
    OptionalClientIp(client_ip): OptionalClientIp,
    headers: axum::http::HeaderMap,
    http::request::Parts { extensions, .. }: http::request::Parts,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    embeddings_handler_inner(state, auth, client_ip, headers, extensions, None, body).await
}

/// Public handler for the Azure aliases `/engines/{model}/embeddings` and
/// `/openai/deployments/{model}/embeddings` — extracts the `{model}` path param
/// and merges it into the body.
#[allow(clippy::too_many_arguments)]
pub async fn embeddings_handler_with_path(
    State(state): State<SharedState>,
    ChatAuth(auth): ChatAuth,
    OptionalClientIp(client_ip): OptionalClientIp,
    headers: axum::http::HeaderMap,
    http::request::Parts { extensions, .. }: http::request::Parts,
    axum::extract::Path(model_path): axum::extract::Path<String>,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    embeddings_handler_inner(
        state,
        auth,
        client_ip,
        headers,
        extensions,
        Some(model_path),
        body,
    )
    .await
}

fn merge_path_model(mut body: Value, model_path: Option<String>) -> Value {
    if let Some(m) = model_path {
        if !m.is_empty() {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("model".to_string(), json!(m));
            }
        }
    }
    body
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

    /// Insert a key + model pointing at an unreachable upstream (127.0.0.1:19999).
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
            .route("/v1/embeddings", axum::routing::post(embeddings_handler))
            .route("/embeddings", axum::routing::post(embeddings_handler))
            .route(
                "/engines/{model}/embeddings",
                axum::routing::post(embeddings_handler_with_path),
            )
            .route(
                "/openai/deployments/{model}/embeddings",
                axum::routing::post(embeddings_handler_with_path),
            )
            .with_state(state)
    }

    async fn post_embeddings(
        app: &Router,
        uri: &str,
        token: &str,
        body: serde_json::Value,
    ) -> (u16, serde_json::Value) {
        let req = axum::http::Request::builder()
            .method(axum::http::Method::POST)
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&body_bytes).unwrap_or(serde_json::json!({}));
        (status, json)
    }

    // ── UT-1: missing model → 400 ──

    #[tokio::test]
    async fn test_embedding_missing_model() {
        let state = test_state().await;
        let token = seed_key_and_model(&state, "ut1", "text-embedding-3-small").await;
        let app = build_app(state);

        let (status, body) = post_embeddings(
            &app,
            "/v1/embeddings",
            &token,
            serde_json::json!({"input": "hello"}),
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(
            body["error"]["type"].as_str(),
            Some("invalid_request_error")
        );
        assert!(body["error"]["message"].as_str().unwrap().contains("model"));
    }

    // ── UT-2: missing input → 400 ──

    #[tokio::test]
    async fn test_embedding_missing_input() {
        let state = test_state().await;
        let token = seed_key_and_model(&state, "ut2", "text-embedding-3-small").await;
        let app = build_app(state);

        let (status, body) = post_embeddings(
            &app,
            "/v1/embeddings",
            &token,
            serde_json::json!({"model": "text-embedding-3-small"}),
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(
            body["error"]["type"].as_str(),
            Some("invalid_request_error")
        );
        assert!(body["error"]["message"].as_str().unwrap().contains("input"));
    }

    // ── UT-3: empty input string → 400 ──

    #[tokio::test]
    async fn test_embedding_empty_input_string() {
        let state = test_state().await;
        let token = seed_key_and_model(&state, "ut3", "text-embedding-3-small").await;
        let app = build_app(state);

        let (status, body) = post_embeddings(
            &app,
            "/v1/embeddings",
            &token,
            serde_json::json!({"model": "text-embedding-3-small", "input": ""}),
        )
        .await;
        assert_eq!(status, 400);
        assert!(body["error"]["message"].as_str().unwrap().contains("empty"));
    }

    // ── UT-4: empty input array → 400 ──

    #[tokio::test]
    async fn test_embedding_empty_input_array() {
        let state = test_state().await;
        let token = seed_key_and_model(&state, "ut4", "text-embedding-3-small").await;
        let app = build_app(state);

        let (status, body) = post_embeddings(
            &app,
            "/v1/embeddings",
            &token,
            serde_json::json!({"model": "text-embedding-3-small", "input": []}),
        )
        .await;
        assert_eq!(status, 400);
        assert!(body["error"]["message"].as_str().unwrap().contains("empty"));
    }

    // ── UT-5: upstream unreachable → 502/500 ──

    #[tokio::test]
    async fn test_embedding_upstream_unreachable() {
        let state = test_state().await;
        let token = seed_key_and_model(&state, "ut5", "text-embedding-3-small").await;
        let app = build_app(state);

        let (status, body) = post_embeddings(
            &app,
            "/v1/embeddings",
            &token,
            serde_json::json!({"model": "text-embedding-3-small", "input": "hello"}),
        )
        .await;
        assert!(
            status == 502 || status == 500,
            "expected 502 or 500, got {}",
            status
        );
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Upstream"),
            "expected upstream error, got: {}",
            body
        );
    }

    // ── UT-6: extract_prompt_tokens fallback (embeddings usage shape) ──

    #[test]
    fn test_embedding_prompt_tokens_extraction() {
        // Embeddings usage reports prompt_tokens + total_tokens only
        let usage = serde_json::json!({"prompt_tokens": 10, "total_tokens": 10});
        assert_eq!(super::super::responses::extract_prompt_tokens(&usage), 10);
        assert_eq!(super::super::responses::extract_total_tokens(&usage), 10);
    }
}
