//! Step bindings for mcp_server_headers.feature — Stage 146 custom outbound
//! auth header and extra request headers.
//!
//! Every scenario that cares about the wire starts its own JSON-RPC stub on
//! `127.0.0.1:0` and records the headers of each request it receives (ADR-041,
//! same technique as `mcp_passthrough_steps.rs`). The stub is the only place
//! that can tell a correct configuration from a plausible-looking one: the
//! admin response shows cached names, and the row shows ciphertext — neither
//! says which header the upstream actually got.
//!
//! `TestWorld.created_keys` only holds strings, so the recorder lives in a
//! process-level registry and the world carries its id.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::Router;
use cucumber::{given, then, when};
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::TestWorld;

const STUB_LOG: &str = "mcp-headers:log";
const STUB_URL: &str = "mcp-headers:stub-url";
const SERVER_ID: &str = "mcp-headers:id";

const TOOL: &str = "ping";
const SHARED_TOKEN: &str = "sk-upstream-shared";

#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    headers: Vec<(String, String)>,
}

type SharedLog = Arc<Mutex<Vec<Recorded>>>;

fn registry() -> &'static Mutex<HashMap<String, SharedLog>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, SharedLog>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register(sink: SharedLog) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    registry().lock().unwrap().insert(id.clone(), sink);
    id
}

fn log(world: &TestWorld) -> SharedLog {
    let id = world
        .created_keys
        .get(STUB_LOG)
        .expect("the scenario must start the recording stub first");
    registry()
        .lock()
        .unwrap()
        .get(id)
        .cloned()
        .unwrap_or_else(|| panic!("stub recorder {id} not registered"))
}

fn recorded(world: &TestWorld) -> Vec<Recorded> {
    log(world).lock().unwrap().clone()
}

/// Headers the gateway writes from protocol/runtime state, plus the ones reqwest
/// adds itself. What is left is exactly the configuration under test.
const TRANSPORT_HEADERS: [&str; 6] = [
    "content-type",
    "accept",
    "mcp-protocol-version",
    "mcp-session-id",
    "host",
    "content-length",
];

fn configured(headers: &[(String, String)]) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = headers
        .iter()
        .filter(|(name, _)| !TRANSPORT_HEADERS.contains(&name.as_str()))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    pairs.sort();
    pairs
}

fn header_of<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let folded = name.to_ascii_lowercase();
    headers
        .iter()
        .find(|(n, _)| *n == folded)
        .map(|(_, v)| v.as_str())
}

fn build_router(state: aigw_server::routes::keys::SharedState) -> Router {
    use aigw_server::routes::{mcp, mcp_rest};
    Router::new()
        .route(
            "/admin/mcp/servers",
            axum::routing::get(mcp::list_mcp_servers).post(mcp::create_mcp_server),
        )
        .route(
            "/admin/mcp/servers/{id}",
            axum::routing::get(mcp::get_mcp_server).put(mcp::update_mcp_server),
        )
        .route(
            "/admin/mcp/servers/{id}/probe",
            axum::routing::post(mcp::probe_mcp_server),
        )
        .route(
            "/admin/mcp/servers/{id}/tokens",
            axum::routing::post(mcp::create_mcp_token),
        )
        .route(
            "/mcp-rest/tools/call",
            axum::routing::post(mcp_rest::tools_call),
        )
        .with_state(state)
}

async fn send(
    world: &mut TestWorld,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let state = world.ensure_state().await;
    let app = build_router(state);
    let token = world.master_key.clone();
    let request = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(axum::body::Body::from(
            body.map(|b| b.to_string()).unwrap_or_default(),
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let value = serde_json::from_slice(&bytes).unwrap_or(json!({}));
    world.last_status = Some(status);
    world.last_body = Some(value.clone());
    (status, value)
}

fn latest_server_id(world: &TestWorld) -> i64 {
    world
        .created_keys
        .get(SERVER_ID)
        .expect("no MCP server created in this scenario")
        .parse()
        .expect("server id")
}

fn body(world: &TestWorld) -> &Value {
    world.last_body.as_ref().expect("no response body")
}

/// A modern JSON-RPC stub that records the headers of everything it receives.
async fn spawn_recording_stub() -> (String, SharedLog) {
    let sink: SharedLog = Arc::new(Mutex::new(Vec::new()));
    let recorder = sink.clone();
    let app = Router::new().route(
        "/mcp",
        axum::routing::post(
            move |headers: HeaderMap, axum::Json(request): axum::Json<Value>| {
                let recorder = recorder.clone();
                async move {
                    let method = request
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let pairs: Vec<(String, String)> = headers
                        .iter()
                        .filter_map(|(name, value)| {
                            value
                                .to_str()
                                .ok()
                                .map(|v| (name.as_str().to_string(), v.to_string()))
                        })
                        .collect();
                    recorder.lock().unwrap().push(Recorded {
                        method: method.clone(),
                        headers: pairs,
                    });

                    let id = request.get("id").cloned().unwrap_or(Value::Null);
                    let result = match method.as_str() {
                        "server/discover" => json!({
                            "supportedVersions": ["2026-07-28"],
                            "ttlMs": 60000,
                            "serverInfo": {"name": "headers-stub", "version": "1.0"},
                        }),
                        "initialize" => json!({
                            "protocolVersion": "2025-06-18",
                            "capabilities": {"tools": {}},
                            "serverInfo": {"name": "headers-stub", "version": "1.0"},
                        }),
                        "tools/list" => json!({"tools": [{
                            "name": TOOL,
                            "description": "ping tool",
                            "inputSchema": {"type": "object"},
                        }]}),
                        "tools/call" => json!({
                            "content": [{"type": "text", "text": "pong"}],
                            "resultType": "complete",
                        }),
                        _ => json!({}),
                    };
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string(),
                    )
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stub");
    let addr = listener.local_addr().expect("stub addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/mcp"), sink)
}

fn parse_fields(step: &cucumber::gherkin::Step) -> Value {
    let raw = step
        .docstring
        .as_ref()
        .expect("the scenario must supply a JSON field block");
    serde_json::from_str(raw.trim()).expect("field block is JSON")
}

fn merge(base: Value, extra: &Value) -> Value {
    let mut merged = base;
    if let (Some(target), Some(fields)) = (merged.as_object_mut(), extra.as_object()) {
        for (key, value) in fields {
            target.insert(key.clone(), value.clone());
        }
    }
    merged
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "已启动记录出站请求头的 MCP stub")]
async fn given_recording_stub(world: &mut TestWorld) {
    let (url, sink) = spawn_recording_stub().await;
    world
        .created_keys
        .insert(STUB_LOG.to_string(), register(sink));
    world.created_keys.insert(STUB_URL.to_string(), url);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "通过 API 创建指向该 stub 的 server {string} 并携带字段")]
async fn when_create_server(world: &mut TestWorld, alias: String, step: &cucumber::gherkin::Step) {
    let url = world
        .created_keys
        .get(STUB_URL)
        .cloned()
        .expect("stub not started");
    let base = json!({
        "alias": alias,
        "display_name": format!("{alias} server"),
        "url": url,
    });
    let payload = merge(base, &parse_fields(step));
    let (status, response) = send(world, Method::POST, "/admin/mcp/servers", Some(payload)).await;
    assert_eq!(status, 200, "create {alias} failed: {response}");
    let id = response["id"].as_i64().expect("server id");
    world
        .created_keys
        .insert(SERVER_ID.to_string(), id.to_string());
}

/// The same create, for a configuration the gateway is expected to reject: the
/// status is recorded for the following `Then` instead of asserted here.
#[when(expr = "通过 API 尝试创建指向该 stub 的 server {string} 并携带字段")]
async fn when_try_create_server(
    world: &mut TestWorld,
    alias: String,
    step: &cucumber::gherkin::Step,
) {
    let url = world
        .created_keys
        .get(STUB_URL)
        .cloned()
        .expect("stub not started");
    let base = json!({
        "alias": alias,
        "display_name": format!("{alias} server"),
        "url": url,
    });
    let payload = merge(base, &parse_fields(step));
    send(world, Method::POST, "/admin/mcp/servers", Some(payload)).await;
}

#[when(expr = "通过 API 为该 server 添加共享 token {string}")]
async fn when_add_shared_token(world: &mut TestWorld, token: String) {
    let id = latest_server_id(world);
    let (status, response) = send(
        world,
        Method::POST,
        &format!("/admin/mcp/servers/{id}/tokens"),
        Some(json!({"token_name": "shared", "token_value": token, "is_default": true})),
    )
    .await;
    assert_eq!(status, 200, "add token failed: {response}");
}

#[when(expr = "通过 API 探测该 server")]
async fn when_probe(world: &mut TestWorld) {
    let id = latest_server_id(world);
    send(
        world,
        Method::POST,
        &format!("/admin/mcp/servers/{id}/probe"),
        None,
    )
    .await;
}

#[when(expr = "通过 API 更新该 server 并携带字段")]
async fn when_update_server(world: &mut TestWorld, step: &cucumber::gherkin::Step) {
    let id = latest_server_id(world);
    let payload = merge(json!({}), &parse_fields(step));
    send(
        world,
        Method::PUT,
        &format!("/admin/mcp/servers/{id}"),
        Some(payload),
    )
    .await;
}

#[when(expr = "通过 \\/mcp-rest 调用 {string} 的工具 {string}")]
async fn when_rest_call(world: &mut TestWorld, alias: String, tool: String) {
    send(
        world,
        Method::POST,
        "/mcp-rest/tools/call",
        Some(json!({"alias": alias, "tool": tool, "arguments": {}})),
    )
    .await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "该 stub 的最后一个请求带 {string} 为 {string}")]
async fn then_last_request_has_header(world: &mut TestWorld, name: String, expected: String) {
    let requests = recorded(world);
    let last = requests
        .last()
        .unwrap_or_else(|| panic!("the stub received no request at all"));
    let actual = header_of(&last.headers, &name)
        .unwrap_or_else(|| panic!("no {name} in {:?}", configured(&last.headers)));
    assert_eq!(actual, expected, "header {name} on the last request");
}

#[then(expr = "该 stub 的最后一个请求不带 {string}")]
async fn then_last_request_lacks_header(world: &mut TestWorld, name: String) {
    let requests = recorded(world);
    let last = requests
        .last()
        .unwrap_or_else(|| panic!("the stub received no request at all"));
    assert!(
        header_of(&last.headers, &name).is_none(),
        "{name} should not be sent, saw {:?}",
        configured(&last.headers)
    );
}

#[then(expr = "该 stub 的最后一个请求只带协议认证头")]
async fn then_last_request_is_bare_bearer(world: &mut TestWorld) {
    let requests = recorded(world);
    let last = requests.last().expect("the stub received no request");
    assert_eq!(
        configured(&last.headers),
        vec![(
            "authorization".to_string(),
            format!("Bearer {SHARED_TOKEN}")
        )],
        "the outbound set must be the bare protocol header"
    );
}

#[then(expr = "探测与调用的出站认证头与额外头一致")]
async fn then_probe_and_call_agree(world: &mut TestWorld) {
    let requests = recorded(world);
    let probe = requests
        .iter()
        .find(|r| r.method == "tools/list")
        .unwrap_or_else(|| panic!("the stub never saw the probe's tools/list"));
    let call = requests
        .iter()
        .rev()
        .find(|r| r.method == "tools/call")
        .unwrap_or_else(|| panic!("the stub never saw tools/call"));
    let probe = configured(&probe.headers);
    let call = configured(&call.headers);
    assert_eq!(
        probe, call,
        "probe and call must send the same configured headers"
    );
    assert!(!probe.is_empty(), "the scenario configured no headers");
}

#[then(expr = "该 stub 没有收到任何请求")]
async fn then_stub_is_untouched(world: &mut TestWorld) {
    let requests = recorded(world);
    assert!(
        requests.is_empty(),
        "a rejected configuration still reached the upstream: {requests:?}"
    );
}

#[then(expr = "创建响应里认证头值脱敏且认证头名可见")]
async fn then_response_redacts(world: &mut TestWorld) {
    let response = body(world);
    assert_eq!(
        response["auth_header_value"],
        json!("***"),
        "value must come back redacted: {response}"
    );
    assert_eq!(
        response["auth_header_name"],
        json!("X-Tapd-Access-Token"),
        "the name is configuration and stays visible: {response}"
    );
    assert_eq!(
        response["extra_headers"][0]["value"],
        json!("***"),
        "extra values must come back redacted: {response}"
    );
}

#[then(expr = "MCP server 列表第一行的认证头值脱敏")]
async fn then_list_redacts(world: &mut TestWorld) {
    let first = body(world)["data"]
        .as_array()
        .and_then(|rows| rows.first())
        .unwrap_or_else(|| panic!("no rows: {}", body(world)));
    assert_eq!(first["auth_header_value"], json!("***"), "row: {first}");
    assert_eq!(
        first["extra_headers"][0]["value"],
        json!("***"),
        "row: {first}"
    );
}

#[then(expr = "该 server 的认证头名仍为 {string}")]
async fn then_auth_name_unchanged(world: &mut TestWorld, expected: String) {
    let state = world.ensure_state().await;
    let id = latest_server_id(world);
    let row = state
        .db
        .get_mcp_server(id)
        .await
        .expect("get server")
        .expect("server row");
    assert_eq!(row.auth_header_name.as_deref(), Some(expected.as_str()));
    let sealed = row
        .auth_header_value
        .expect("the value survived the update");
    let mk = state.aigw_master_key.as_deref().expect("master key");
    assert_eq!(
        aigw_core::mcp::headers::unseal_value(Some(&sealed), mk).as_deref(),
        Some("tapd-secret")
    );
    assert!(row.extra_headers.is_some(), "extras survived the update");
}

#[then(expr = "该 server 的认证头与额外头已清空")]
async fn then_headers_cleared(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let id = latest_server_id(world);
    let row = state
        .db
        .get_mcp_server(id)
        .await
        .expect("get server")
        .expect("server row");
    assert!(row.auth_header_name.is_none(), "name cleared");
    assert!(row.auth_header_value.is_none(), "value cleared");
    assert!(row.extra_headers.is_none(), "extras cleared");
}
