//! Mock upstream server for BDD end-to-end testing.
//!
//! Provides in-memory HTTP servers simulating OpenAI and Claude upstreams.
//! Supports configurable responses, request recording, and SSE streaming.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::net::TcpListener;

/// A recorded upstream request
#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub path: String,
    #[allow(dead_code)]
    pub headers: HashMap<String, String>,
    #[allow(dead_code)]
    pub body: Value,
}

/// Global mock instance for Anthropic Native BDD tests.
#[allow(dead_code)]
static ANTHROPIC_MOCK: OnceLock<Arc<Mutex<Option<MockUpstream>>>> = OnceLock::new();
#[allow(dead_code)]
static ANTHROPIC_MOCK_URL: OnceLock<String> = OnceLock::new();

/// Get or create the global Anthropic mock URL (starts the mock if needed).
#[allow(dead_code)]
pub async fn get_or_create_anthropic_mock_url() -> &'static str {
    if let Some(url) = ANTHROPIC_MOCK_URL.get() {
        return url;
    }
    let mock = MockUpstream::start().await;
    let url = mock.base_url.clone();
    let _ = ANTHROPIC_MOCK_URL.set(url);
    let cell = ANTHROPIC_MOCK.get_or_init(|| Arc::new(Mutex::new(None)));
    *cell.lock().unwrap() = Some(mock);
    ANTHROPIC_MOCK_URL.get().unwrap()
}

/// Get a reference to the global Anthropic mock (if started).
#[allow(dead_code)]
pub fn get_anthropic_mock() -> Option<MockUpstreamRef> {
    let cell = ANTHROPIC_MOCK.get()?;
    let guard = cell.lock().unwrap();
    guard.as_ref().map(|m| MockUpstreamRef {
        state: m.state.clone(),
    })
}

/// A lightweight reference to a mock upstream's state (for assertions).
#[allow(dead_code)]
pub struct MockUpstreamRef {
    state: Arc<MockState>,
}

impl MockUpstreamRef {
    #[allow(dead_code)]
    pub fn request_count(&self) -> usize {
        self.state.request_count()
    }

    #[allow(dead_code)]
    pub fn recorded_requests(&self) -> Vec<RecordedRequest> {
        self.state.recorded_requests()
    }
}

/// Configuration for a mock endpoint
#[derive(Debug, Clone)]
pub struct MockResponse {
    pub status: u16,
    pub body: Value,
    #[allow(dead_code)]
    pub headers: HashMap<String, String>,
    /// Remaining one-shot hits before the response is removed (0 = persistent).
    #[allow(dead_code)]
    pub remaining: usize,
}

impl Default for MockResponse {
    fn default() -> Self {
        Self {
            status: 200,
            body: serde_json::json!({
                "id": "chatcmpl-mock",
                "object": "chat.completion",
                "created": 1700000000,
                "model": "gpt-4",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Mock response from upstream"
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15
                }
            }),
            headers: HashMap::new(),
            remaining: 0,
        }
    }
}

/// Shared state between mock server and test code
pub struct MockState {
    pub requests: Arc<Mutex<Vec<RecordedRequest>>>,
    pub responses: Arc<Mutex<HashMap<String, MockResponse>>>,
}

impl MockState {
    pub fn new() -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn set_response(&self, path: &str, response: MockResponse) {
        self.responses
            .lock()
            .unwrap()
            .insert(path.to_string(), response);
    }

    /// One-shot response: fire for the first N hits, then remove (default used).
    pub fn set_response_first_n(&self, path: &str, status: u16, body: Value, n: usize) {
        let mut map = self.responses.lock().unwrap();
        map.insert(
            path.to_string(),
            MockResponse {
                status,
                body,
                headers: HashMap::new(),
                remaining: n,
            },
        );
    }

    #[allow(dead_code)]
    pub fn recorded_requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }

    pub fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    pub fn reset_responses(&self) {
        self.responses.lock().unwrap().clear();
    }

    #[allow(dead_code)]
    pub fn reset_all(&self) {
        self.responses.lock().unwrap().clear();
        self.requests.lock().unwrap().clear();
    }
}

/// The mock upstream server
pub struct MockUpstream {
    pub base_url: String,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    #[allow(dead_code)]
    state: Arc<MockState>,
}

impl MockUpstream {
    /// Start a mock upstream server on an ephemeral port.
    pub async fn start() -> Self {
        let state = Arc::new(MockState::new());
        let route_state = state.clone();

        let app = Router::new()
            .route("/v1/chat/completions", post(openai_handler))
            .route("/v1/messages", post(claude_handler))
            .route("/v1/responses", post(responses_handler))
            .route("/v1/embeddings", post(embeddings_handler))
            .route("/api/organizations", get(oauth_orgs_handler))
            .route("/v1/oauth/{org}/authorize", post(oauth_authorize_handler))
            .route("/v1/oauth/token", post(oauth_token_handler))
            .with_state(route_state);

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock upstream");
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}:{}", addr.ip(), addr.port());

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let server = axum::serve(listener, app).with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });

        tokio::spawn(async move {
            let _ = server.await;
        });

        MockUpstream {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            state,
        }
    }

    pub fn url(&self) -> &str {
        &self.base_url
    }

    /// Set the response for a specific path
    pub fn set_response(&self, path: &str, status: u16, body: Value) {
        self.state.set_response(
            path,
            MockResponse {
                status,
                body,
                headers: HashMap::new(),
                remaining: 0,
            },
        );
    }

    /// Set a response that fires only for the first N matching requests; after
    /// N hits the configured response is removed (falls back to the default).
    /// Used by the Stage 128 401-refresh-retry scenario: first call 401, the
    /// retry after token refresh gets the default 200.
    pub fn set_response_first_n(&self, path: &str, status: u16, body: Value, n: usize) {
        self.state.set_response_first_n(path, status, body, n);
    }

    /// Get recorded requests
    #[allow(dead_code)]
    pub fn recorded_requests(&self) -> Vec<RecordedRequest> {
        self.state.recorded_requests()
    }

    /// Number of requests received
    pub fn request_count(&self) -> usize {
        self.state.request_count()
    }

    /// Reset mock responses to defaults between scenarios
    pub fn reset_responses(&self) {
        self.state.reset_responses();
    }

    /// Reset both responses AND recorded requests (full scenario isolation).
    pub fn reset_all(&self) {
        self.state.reset_all();
    }
}

impl Drop for MockUpstream {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Handler: /v1/chat/completions (OpenAI)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn openai_handler(
    State(state): State<Arc<MockState>>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Response {
    let body_val: Value = serde_json::from_str(&body).unwrap_or_default();

    // Record the request
    {
        let mut requests = state.requests.lock().unwrap();
        let mut hdrs = HashMap::new();
        for (k, v) in headers.iter() {
            if let Ok(val) = v.to_str() {
                hdrs.insert(k.to_string(), val.to_string());
            }
        }
        requests.push(RecordedRequest {
            path: "/v1/chat/completions".to_string(),
            headers: hdrs,
            body: body_val.clone(),
        });
    }

    // Check if streaming requested
    let is_stream = body_val
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if is_stream {
        // Return a real SSE stream: emit one usage chunk with the final usage
        // (including prompt_tokens_details when configured), then [DONE].
        // This exercises the gateway's streaming two-phase SpendLog path.
        let usage_chunk = state
            .responses
            .lock()
            .unwrap()
            .get("/v1/chat/completions")
            .cloned()
            .map(|m| m.body.get("usage").cloned())
            .flatten()
            .unwrap_or(serde_json::json!({
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "total_tokens": 120
            }));

        let sse = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({
                "id": "chatcmpl-stream-mock",
                "object": "chat.completion.chunk",
                "created": 1700000000,
                "model": "gpt-4o",
                "choices": [{"index": 0, "delta": {"content": "Mock streamed reply"}, "finish_reason": "stop"}],
                "usage": usage_chunk
            })
        );
        let body_stream = tokio_stream::once(Ok::<_, std::convert::Infallible>(sse));
        let response = axum::response::Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from_stream(body_stream))
            .unwrap();
        return response;
    }
    // Return configured response or default
    let resp = state.responses.lock().unwrap();
    let mock = resp
        .get("/v1/chat/completions")
        .cloned()
        .unwrap_or_default();

    (
        StatusCode::from_u16(mock.status).unwrap_or(StatusCode::OK),
        Json(mock.body),
    )
        .into_response()
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Handler: /v1/messages (Claude)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn claude_handler(
    State(state): State<Arc<MockState>>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let body_val: Value = serde_json::from_str(&body).unwrap_or_default();

    // Record the request
    {
        let mut requests = state.requests.lock().unwrap();
        let mut hdrs = HashMap::new();
        for (k, v) in headers.iter() {
            if let Ok(val) = v.to_str() {
                hdrs.insert(k.to_string(), val.to_string());
            }
        }
        requests.push(RecordedRequest {
            path: "/v1/messages".to_string(),
            headers: hdrs,
            body: body_val,
        });
    }

    // Look up the configured /v1/messages response; if it's one-shot
    // (remaining > 0), decrement and remove when exhausted (default response
    // then applies). Used by the Stage 128 401-refresh-retry scenario.
    let mut responses = state.responses.lock().unwrap();
    let mock = {
        let resp = responses.get("/v1/messages");
        match resp {
            Some(m) if m.remaining > 0 => {
                let mut m = m.clone();
                m.remaining -= 1;
                if m.remaining == 0 {
                    responses.remove("/v1/messages");
                } else {
                    responses.insert("/v1/messages".to_string(), m.clone());
                }
                m
            }
            Some(m) => m.clone(),
            None => MockResponse {
                status: 200,
                body: serde_json::json!({
                    "id": "msg_mock",
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Mock Claude response"}],
                    "model": "claude-3",
                    "stop_reason": "end_turn",
                    "stop_sequence": null,
                    "usage": {"input_tokens": 10, "output_tokens": 5}
                }),
                headers: HashMap::new(),
                remaining: 0,
            },
        }
    };
    drop(responses);

    Ok((
        StatusCode::from_u16(mock.status).unwrap_or(StatusCode::OK),
        Json(mock.body),
    ))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Handler: /v1/embeddings (OpenAI Embeddings API)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn embeddings_handler(
    State(state): State<Arc<MockState>>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let body_val: Value = serde_json::from_str(&body).unwrap_or_default();

    // Record the request
    {
        let mut requests = state.requests.lock().unwrap();
        let mut hdrs = HashMap::new();
        for (k, v) in headers.iter() {
            if let Ok(val) = v.to_str() {
                hdrs.insert(k.to_string(), val.to_string());
            }
        }
        requests.push(RecordedRequest {
            path: "/v1/embeddings".to_string(),
            headers: hdrs,
            body: body_val,
        });
    }

    // Return the configured response or a default OpenAI Embeddings shape
    let resp = state.responses.lock().unwrap();
    let mock = resp.get("/v1/embeddings").cloned().unwrap_or(MockResponse {
        status: 200,
        body: serde_json::json!({
            "object": "list",
            "data": [{
                "object": "embedding",
                "embedding": [0.1, 0.2, 0.3],
                "index": 0
            }],
            "model": "text-embedding-3-small",
            "usage": {"prompt_tokens": 10, "total_tokens": 10}
        }),
        headers: HashMap::new(),
        remaining: 0,
    });

    Ok((
        StatusCode::from_u16(mock.status).unwrap_or(StatusCode::OK),
        Json(mock.body),
    ))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Handler: /v1/responses (OpenAI Responses API)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn responses_handler(
    State(state): State<Arc<MockState>>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    let body_val: Value = serde_json::from_str(&body).unwrap_or_default();

    // Record the request
    {
        let mut requests = state.requests.lock().unwrap();
        let mut hdrs = HashMap::new();
        for (k, v) in headers.iter() {
            if let Ok(val) = v.to_str() {
                hdrs.insert(k.to_string(), val.to_string());
            }
        }
        requests.push(RecordedRequest {
            path: "/v1/responses".to_string(),
            headers: hdrs,
            body: body_val.clone(),
        });
    }

    // Streaming request → return a real Chat Completions stream so the
    // gateway's ResponsesToChatCompletionsStream bridge actually converts it.
    // (Phase 41 test gap ②: the stream path now executes the SSE conversion.)
    if body_val
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let usage = state
            .responses
            .lock()
            .unwrap()
            .get("/v1/responses")
            .cloned()
            .map(|m| m.body.get("usage").cloned())
            .flatten()
            .unwrap_or(serde_json::json!({
                "prompt_tokens": 12,
                "completion_tokens": 7,
                "total_tokens": 19
            }));

        // Chat Completions SSE frames: role chunk → content delta → final usage
        let sse = format!(
            "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({
                "id": "chatcmpl-stream-mock",
                "object": "chat.completion.chunk",
                "created": 1700000000,
                "model": "gpt-4o",
                "choices": [{"index": 0, "delta": {"role": "assistant"}, "finish_reason": null}]
            }),
            serde_json::json!({
                "id": "chatcmpl-stream-mock",
                "object": "chat.completion.chunk",
                "created": 1700000000,
                "model": "gpt-4o",
                "choices": [{"index": 0, "delta": {"content": "Hello from mock stream"}, "finish_reason": null}]
            }),
            serde_json::json!({
                "id": "chatcmpl-stream-mock",
                "object": "chat.completion.chunk",
                "created": 1700000000,
                "model": "gpt-4o",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                "usage": usage
            })
        );
        let body_stream = tokio_stream::once(Ok::<_, std::convert::Infallible>(sse));
        let response = axum::response::Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from_stream(body_stream))
            .unwrap();
        return Ok(response);
    }

    // Return default Responses API format response
    let resp = state.responses.lock().unwrap();
    let mock = resp.get("/v1/responses").cloned().unwrap_or(MockResponse {
        status: 200,
        body: serde_json::json!({
            "id": "resp_mock_001",
            "object": "response",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "Mock Responses API response from upstream"
                }]
            }],
            "usage": {
                "input_tokens": 12,
                "output_tokens": 7,
                "total_tokens": 19
            }
        }),
        headers: HashMap::new(),
        remaining: 0,
    });

    let status = StatusCode::from_u16(mock.status).unwrap_or(StatusCode::OK);
    let body = serde_json::to_string(&mock.body).unwrap_or_default();
    let response = axum::response::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body))
        .unwrap();
    Ok(response)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Handler: Claude OAuth 3-step mock (Phase 51, Stage 126)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// GET /api/organizations — cookie `sessionKey=...` → org list (one team org).
///
/// Mirrors claude.ai's step-1 response. The BDD step sets a custom response via
/// `/api/organizations` when it needs a CF challenge / error, otherwise returns
/// the default single-team org list.
async fn oauth_orgs_handler(
    State(state): State<Arc<MockState>>,
    headers: axum::http::HeaderMap,
) -> Response {
    // Record the request (cookie header captured for assertion).
    {
        let mut requests = state.requests.lock().unwrap();
        let mut hdrs = HashMap::new();
        for (k, v) in headers.iter() {
            if let Ok(val) = v.to_str() {
                hdrs.insert(k.to_string(), val.to_string());
            }
        }
        requests.push(RecordedRequest {
            path: "/api/organizations".to_string(),
            headers: hdrs,
            body: serde_json::json!({}),
        });
    }

    let mock = state
        .responses
        .lock()
        .unwrap()
        .get("/api/organizations")
        .cloned();
    if let Some(m) = mock {
        let status = StatusCode::from_u16(m.status).unwrap_or(StatusCode::OK);
        let body = serde_json::to_string(&m.body).unwrap_or_default();
        return axum::response::Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body))
            .unwrap();
    }

    // Default: a single team org.
    let body = serde_json::json!([
        { "uuid": "org-team-1", "name": "Work", "raven_type": "team" },
        { "uuid": "org-personal", "name": "Personal", "raven_type": null }
    ]);
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap()
}

/// POST /v1/oauth/{org}/authorize — cookie + PKCE body → redirect_uri with
/// `code` + `state`. Custom response via `/v1/oauth/authorize` when set (error).
async fn oauth_authorize_handler(
    State(state): State<Arc<MockState>>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Response {
    {
        let mut requests = state.requests.lock().unwrap();
        let mut hdrs = HashMap::new();
        for (k, v) in headers.iter() {
            if let Ok(val) = v.to_str() {
                hdrs.insert(k.to_string(), val.to_string());
            }
        }
        requests.push(RecordedRequest {
            path: "/v1/oauth/authorize".to_string(),
            headers: hdrs,
            body: serde_json::from_str(&body).unwrap_or_default(),
        });
    }

    let mock = state
        .responses
        .lock()
        .unwrap()
        .get("/v1/oauth/authorize")
        .cloned();
    if let Some(m) = mock {
        let status = StatusCode::from_u16(m.status).unwrap_or(StatusCode::OK);
        let body = serde_json::to_string(&m.body).unwrap_or_default();
        return axum::response::Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body))
            .unwrap();
    }

    // Echo back a redirect_uri carrying the client's state + a fixed code.
    let req: Value = serde_json::from_str(&body).unwrap_or_default();
    let state = req.get("state").and_then(|v| v.as_str()).unwrap_or("st");
    let redirect = format!(
        "https://platform.claude.com/oauth/code/callback?code=mock-auth-code&state={}",
        state
    );
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::json!({ "redirect_uri": redirect }).to_string(),
        ))
        .unwrap()
}

/// POST /v1/oauth/token — authorization_code or refresh_token grant → token pair.
/// Custom response via `/v1/oauth/token` when set.
async fn oauth_token_handler(
    State(state): State<Arc<MockState>>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Response {
    {
        let mut requests = state.requests.lock().unwrap();
        let mut hdrs = HashMap::new();
        for (k, v) in headers.iter() {
            if let Ok(val) = v.to_str() {
                hdrs.insert(k.to_string(), val.to_string());
            }
        }
        requests.push(RecordedRequest {
            path: "/v1/oauth/token".to_string(),
            headers: hdrs,
            body: serde_json::from_str(&body).unwrap_or_default(),
        });
    }

    let mock = state
        .responses
        .lock()
        .unwrap()
        .get("/v1/oauth/token")
        .cloned();
    if let Some(m) = mock {
        let status = StatusCode::from_u16(m.status).unwrap_or(StatusCode::OK);
        let body = serde_json::to_string(&m.body).unwrap_or_default();
        return axum::response::Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body))
            .unwrap();
    }

    let req: Value = serde_json::from_str(&body).unwrap_or_default();
    let grant = req
        .get("grant_type")
        .and_then(|v| v.as_str())
        .unwrap_or("authorization_code");
    let (access, refresh) = if grant == "refresh_token" {
        ("sk-ant-access-refreshed", "sk-ant-refresh-refreshed")
    } else {
        ("sk-ant-access-mock", "sk-ant-refresh-mock")
    };
    let token_body = serde_json::json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": 28800,
        "refresh_token": refresh,
        "scope": "user:profile user:inference",
        "organization": { "uuid": "org-team-1" },
        "account": { "uuid": "acc-1", "email_address": "mock@example.com" },
    });
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(token_body.to_string()))
        .unwrap()
}
