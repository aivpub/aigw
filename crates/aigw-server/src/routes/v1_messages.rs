//! Claude-compatible /v1/messages endpoint
//!
//! Supports both non-streaming and SSE streaming, with protocol conversion
//! to OpenAI upstream via the adapter layer.
//!
//! Auth: x-api-key header or Authorization: Bearer header (Claude convention)

use aigw_core::adapter::{DefaultAdapter, ProviderAdapter};
use aigw_core::crypto::hash_token;
use aigw_core::models::{ClaudeMessageRequest, DailySpendKind, DailySpendLog, SpendLog};
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
    // 1. Auth check (must happen before body parsing to catch missing auth early)
    let extracted = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get(http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        });

    match extracted {
        None | Some("") => {
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
        Some(token) => {
            // Validate token (master key or DB lookup)
            let is_master = state.master_key.as_ref().map(|mk| token == *mk).unwrap_or(false);
            if !is_master {
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
                if key.is_none() {
                    let request_id = format!("req_{}", uuid::Uuid::new_v4());
                    return Err((
                        StatusCode::UNAUTHORIZED,
                        Json(json!({
                            "type": "error",
                            "error": {
                                "type": "authentication_error",
                                "message": "Invalid API key"
                            },
                            "request_id": request_id
                        })),
                    ));
                }
            }
        }
    }

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

    // 4. Check if model exists in proxy_models (by model_name)
    let models = state.db.list_models().await.map_err(|_| {
        anthropic_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Failed to look up model",
        )
    })?;

    let pm = models.iter().find(|m| m.model_name == model).ok_or_else(|| {
        anthropic_error(
            StatusCode::NOT_FOUND,
            "not_found_error",
            &format!("Model '{}' not found", model),
        )
    })?;

    let input_cost = pm.model_info
        .get("input_cost_per_token")
        .and_then(|v| v.as_f64());
    let output_cost = pm.model_info
        .get("output_cost_per_token")
        .and_then(|v| v.as_f64());

    // 5. Determine upstream URL from environment
    let upstream_base_url = std::env::var("UPSTREAM_LLM_URL")
        .or_else(|_| std::env::var("OPENAI_BASE_URL"))
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

    let upstream_api_key = std::env::var("UPSTREAM_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .ok();

    // 6. Parse Claude request and convert to OpenAI format
    let claude_req: ClaudeMessageRequest =
        serde_json::from_value(body_val).map_err(|e| {
            anthropic_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                &format!("Invalid request format: {}", e),
            )
        })?;

    let oai_req = DefaultAdapter::claude_to_openai_request(&claude_req);
    let upstream_body = serde_json::to_value(&oai_req).map_err(|_| {
        anthropic_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Failed to serialize upstream request",
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

    if is_stream {
        if !upstream_status.is_success() {
            let error_body = upstream_resp.text().await.unwrap_or_default();
            return Err(anthropic_error(
                StatusCode::from_u16(upstream_status.as_u16())
                    .unwrap_or(StatusCode::BAD_GATEWAY),
                "upstream_error",
                &format!("Upstream returned {}: {}", upstream_status.as_u16(), error_body),
            ));
        }

        // SSE streaming proxy: forward upstream SSE chunks to client via axum Sse.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let state_clone = Arc::clone(&state);
        let model_clone = model.clone();
        let upstream_base_url_clone = upstream_base_url.clone();

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
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }

            let now = chrono::Utc::now();
            let spend_log = SpendLog {
                request_id: uuid::Uuid::new_v4().to_string(),
                call_type: "completion".to_string(),
                api_key: "claude-message".to_string(),
                spend: 0.0,
                total_tokens: 0,
                prompt_tokens: 0,
                completion_tokens: 0,
                start_time,
                end_time: now,
                request_duration_ms: Some(
                    now.signed_duration_since(start_time).num_milliseconds() as i32,
                ),
                completion_start_time: Some(first_chunk_time.unwrap_or(now)),
                model: model_clone,
                model_id: None,
                model_group: None,
                custom_llm_provider: None,
                api_base: Some(upstream_base_url_clone),
                user: None,
                metadata: None,
                cache_hit: None,
                cache_key: None,
                request_tags: None,
                team_id: None,
                organization_id: None,
                end_user: None,
                requester_ip_address: None,
                messages: Some(upstream_body),
                response: None,
                session_id: None,
                status: Some("success".to_string()),
                mcp_namespaced_tool_name: None,
                agent_id: None,
                proxy_server_request: None,
            };
            let _ = state_clone.db.insert_spend_log(&spend_log).await;
        });

        // Claude streams use "text/event-stream" — we're forwarding raw SSE bytes.
        // The upstream already sends SSE-formatted data.
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
        let spend_amount =
            (usage.and_then(|u| u.get("prompt_tokens")).and_then(|v| v.as_i64()).unwrap_or(0) as f64
                * input_cost.unwrap_or(0.0))
                + (usage.and_then(|u| u.get("completion_tokens")).and_then(|v| v.as_i64())
                    .unwrap_or(0) as f64
                    * output_cost.unwrap_or(0.0));
        let spend_log = aigw_core::models::SpendLog {
            request_id: uuid::Uuid::new_v4().to_string(),
            call_type: "completion".to_string(),
            api_key: "claude-message".to_string(),
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
                now.signed_duration_since(start_time).num_milliseconds() as i32,
            ),
            completion_start_time: Some(now), // non-streaming sentinel = end_time
            model: model.to_string(),
            model_id: None,
            model_group: None,
            custom_llm_provider: None,
            api_base: Some(upstream_base_url),
            user: None,
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

        // Should fail with 404 because model doesn't exist, not 401
        let response = app.oneshot(request).await.unwrap();
        // Without model in DB, it should be a 404, not a 401
        assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
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
}
