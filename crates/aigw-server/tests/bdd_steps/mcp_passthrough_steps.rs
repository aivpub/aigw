//! Step bindings for mcp_passthrough.feature — Stage 143 `/mcp/{alias}` +
//! `/mcp-rest/*`.
//!
//! The upstream is a JSON-RPC stub bound to `127.0.0.1:0` inside the test
//! process (ADR-041). It records every method, `Authorization` and
//! `Mcp-Session-Id` it sees, which is the only way to assert the two properties
//! that matter most here: the client's aigw key never leaves the gateway, and a
//! legacy upstream gets its own `initialize` while the client hears the modern
//! stack.
//!
//! The router is built locally like `mcp_steps.rs` rather than going through
//! `bdd_support::mock_upstream`, which is a fixed LLM shape with no JSON-RPC
//! methods.
//!
//! `TestWorld.created_keys` only holds strings, so the stub's recorder lives in
//! a process-level registry and the world carries its id.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::Router;
use cucumber::{given, then, when};
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::TestWorld;

const STUB_LOG: &str = "mcp-pass:log";
const SERVER_ID: &str = "mcp-pass:server-id";
const SERVER_ALIAS: &str = "mcp-pass:alias";
const CLIENT_KEY: &str = "mcp-pass:key";

const TOOL: &str = "ping";

#[derive(Debug, Default)]
struct StubLog {
    methods: Vec<String>,
    authorizations: Vec<Option<String>>,
    session_ids: Vec<Option<String>>,
}

type SharedLog = Arc<Mutex<StubLog>>;

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
        .expect("stub not started for this scenario");
    registry()
        .lock()
        .unwrap()
        .get(id)
        .cloned()
        .unwrap_or_else(|| panic!("stub recorder {id} not registered"))
}

fn methods(world: &TestWorld) -> Vec<String> {
    log(world).lock().unwrap().methods.clone()
}

fn build_router(state: aigw_server::routes::keys::SharedState) -> Router {
    use aigw_server::routes::{mcp, mcp_gateway, mcp_rest};
    Router::new()
        .route(
            "/mcp/{alias}",
            axum::routing::get(mcp_gateway::get_mcp)
                .post(mcp_gateway::post_mcp)
                .delete(mcp_gateway::delete_mcp),
        )
        .route(
            "/admin/mcp/servers/{id}/allowlist",
            axum::routing::get(mcp::get_mcp_allowlist).put(mcp::put_mcp_allowlist),
        )
        .route(
            "/mcp-rest/tools/list",
            axum::routing::post(mcp_rest::tools_list),
        )
        .route(
            "/mcp-rest/tools/call",
            axum::routing::post(mcp_rest::tools_call),
        )
        .route("/mcp-rest/health", axum::routing::get(mcp_rest::health))
        .with_state(state)
}

/// A legacy upstream answers `server/discover` with 405 — the same signal the
/// real probe reads as "this server speaks `2025-06-18`".
async fn spawn_stub(legacy: bool) -> (String, SharedLog) {
    let sink: SharedLog = Arc::new(Mutex::new(StubLog::default()));
    let recorder = sink.clone();
    let app = Router::new().route(
        "/mcp",
        axum::routing::post(
            move |headers: HeaderMap, axum::Json(body): axum::Json<Value>| {
                let recorder = recorder.clone();
                async move {
                    let method = body
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    {
                        let mut guard = recorder.lock().unwrap();
                        guard.methods.push(method.clone());
                        guard.authorizations.push(
                            headers
                                .get("authorization")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_string),
                        );
                        guard.session_ids.push(
                            headers
                                .get("mcp-session-id")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_string),
                        );
                    }
                    let id = body.get("id").cloned().unwrap_or(Value::Null);
                    if legacy && method == "server/discover" {
                        return (
                            StatusCode::METHOD_NOT_ALLOWED,
                            [(header::CONTENT_TYPE, "application/json")],
                            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"no"}}"#
                                .to_string(),
                        );
                    }
                    let result = match method.as_str() {
                        "server/discover" => json!({
                            "supportedVersions": ["2026-07-28"],
                            "ttlMs": 60000,
                            "serverInfo": {"name": "pass-stub", "version": "1.0"},
                        }),
                        "initialize" => json!({
                            "protocolVersion": "2025-06-18",
                            "capabilities": {"tools": {}},
                            "serverInfo": {"name": "pass-stub", "version": "1.0"},
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
    let app = if legacy {
        app.layer(axum::middleware::from_fn(
            |request: axum::extract::Request, next: axum::middleware::Next| async move {
                let mut response = next.run(request).await;
                response.headers_mut().insert(
                    header::HeaderName::from_static("mcp-session-id"),
                    header::HeaderValue::from_static("legacy-session-1"),
                );
                response
            },
        ))
    } else {
        app
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stub");
    let addr = listener.local_addr().expect("stub addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/mcp"), sink)
}

async fn register_server(world: &mut TestWorld, alias: &str, legacy: bool, protocol: Option<&str>) {
    let state = world.ensure_state().await;
    let (url, sink) = spawn_stub(legacy).await;
    world
        .created_keys
        .insert(STUB_LOG.to_string(), register(sink));

    let server = aigw_core::models::McpServerRow {
        id: 0,
        alias: alias.to_string(),
        display_name: format!("{alias} server"),
        url,
        transport: "streamable_http".to_string(),
        credential_scope: "shared".to_string(),
        enabled: true,
        protocol_version: protocol.map(str::to_string),
        auth_header_name: None,
        auth_header_value: None,
        extra_headers: None,
        tool_count: 0,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    };
    let id = state.db.create_mcp_server(&server).await.expect("server");
    add_server_token(&state, id, "sk-upstream-shared").await;
    aigw_core::mcp::catalog_cache::invalidate(id);

    world
        .created_keys
        .insert(SERVER_ID.to_string(), id.to_string());
    world
        .created_keys
        .insert(SERVER_ALIAS.to_string(), alias.to_string());
}

async fn add_server_token(
    state: &aigw_server::routes::keys::SharedState,
    server_id: i64,
    plaintext: &str,
) {
    use aigw_core::models::{McpServerTokenRow, MCP_OWNER_SERVER};
    let mk = state
        .aigw_master_key
        .as_deref()
        .expect("the scenario must configure a master key first");
    let sealed = aigw_core::mcp::credential::seal_token(plaintext, mk).expect("seal");
    let token_id = state
        .db
        .create_mcp_token(&McpServerTokenRow {
            id: 0,
            server_id,
            owner_type: MCP_OWNER_SERVER.to_string(),
            owner_id: String::new(),
            token_name: "shared".to_string(),
            token_value: sealed,
            is_default: false,
            enabled: true,
            expires_at: None,
            created_by: "bdd".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        })
        .await
        .expect("token");
    state
        .db
        .set_default_mcp_token(server_id, MCP_OWNER_SERVER, "", token_id)
        .await
        .expect("default token");
}

async fn send(
    world: &mut TestWorld,
    method: Method,
    uri: &str,
    token: Option<&str>,
    headers: &[(&str, &str)],
    body: Option<Value>,
) {
    let state = world.ensure_state().await;
    let app = build_router(state);
    let mut builder = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json");
    if let Some(token) = token {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = builder
        .body(axum::body::Body::from(
            body.map(|v| v.to_string()).unwrap_or_default(),
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_headers = Some(response.headers().clone());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

fn discover() -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {}})
}

fn tools_call() -> Value {
    json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": {"name": TOOL, "arguments": {}}})
}

fn body(world: &TestWorld) -> &Value {
    world.last_body.as_ref().expect("no response body")
}

/// Walk a dotted path. A numeric segment indexes an array — `Value::get(&str)`
/// never matches an array element, so `tools.0.tool_name` would otherwise
/// resolve to nothing.
fn resolve_path<'a>(value: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(value, |acc, seg| {
        let found = match seg.parse::<usize>() {
            Ok(index) => acc.get(index),
            Err(_) => acc.get(seg),
        };
        found.unwrap_or_else(|| panic!("path \"{path}\" has no segment \"{seg}\" in {value}"))
    })
}

fn alias_of(world: &TestWorld) -> String {
    world
        .created_keys
        .get(SERVER_ALIAS)
        .cloned()
        .expect("no MCP server registered in this scenario")
}

async fn latest_log(world: &mut TestWorld) -> aigw_core::models::McpCallLogRow {
    let state = world.ensure_state().await;
    let alias = alias_of(world);
    state
        .db
        .list_mcp_call_log(Some(&alias), 50, 0)
        .await
        .expect("log rows")
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no mcp_call_log row for {alias}"))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "已启动现代协议的 MCP 透传 stub 并注册为 server {string}")]
async fn given_modern_stub(world: &mut TestWorld, alias: String) {
    register_server(world, &alias, false, Some("2026-07-28")).await;
}

#[given(expr = "已启动旧版协议的 MCP 透传 stub 并注册为未探测的 server {string}")]
async fn given_legacy_stub(world: &mut TestWorld, alias: String) {
    register_server(world, &alias, true, None).await;
}

#[given(expr = "MCP 透传场景以未注册的 alias {string} 为准")]
async fn given_unknown_alias(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    assert!(
        state
            .db
            .get_mcp_server_by_alias(&alias)
            .await
            .expect("lookup")
            .is_none(),
        "the alias must not exist yet"
    );
    world.created_keys.insert(SERVER_ALIAS.to_string(), alias);
}

#[given(expr = "已为该 MCP server 缓存一个工具目录")]
async fn given_catalog(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let id: i64 = world
        .created_keys
        .get(SERVER_ID)
        .expect("server not registered")
        .parse()
        .expect("server id");
    state
        .db
        .replace_mcp_tool_catalog(
            id,
            &[aigw_core::models::McpToolRow {
                id: 0,
                server_id: id,
                tool_name: TOOL.to_string(),
                description: "ping tool".to_string(),
                input_schema: json!({"type": "object"}),
                annotations: None,
                has_conflict: false,
                fetched_at: "2026-01-01T00:00:00Z".to_string(),
            }],
        )
        .await
        .expect("catalog");
}

#[given(expr = "已生成用于 MCP 透传的普通 key {string}")]
async fn given_plain_key(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let raw = format!("sk-{}", uuid::Uuid::new_v4());
    state
        .db
        .insert_key(&aigw_core::models::VirtualKey {
            token: aigw_core::crypto::hash_token(&raw),
            key_name: Some(alias.clone()),
            key_alias: Some(alias.clone()),
            soft_budget_cooldown: "false".to_string(),
            spend: 0.0,
            expires: None,
            models: json!([]),
            aliases: json!({}),
            config: json!({}),
            router_settings: None,
            user_id: Some("mcp-pass-user".to_string()),
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
            created_by: Some("bdd".to_string()),
            updated_at: Some(chrono::Utc::now()),
            updated_by: Some("bdd".to_string()),
            last_active: None,
            rotation_count: None,
            auto_rotate: None,
            rotation_interval: None,
            last_rotation_at: None,
            key_rotation_at: None,
            budget_limits: None,
            user_email: None,
            user_alias: None,
        })
        .await
        .expect("insert key");
    world.created_keys.insert(CLIENT_KEY.to_string(), raw);
}

#[given(expr = "管理员把透传 server {string} 的工具 {string} 对普通 key 设为禁止")]
async fn given_acl_denial(world: &mut TestWorld, alias: String, tool: String) {
    let id: i64 = world
        .created_keys
        .get(SERVER_ID)
        .expect("server not registered")
        .parse()
        .expect("server id");
    assert_eq!(
        alias_of(world),
        alias,
        "the scenario registered another alias"
    );
    let raw = world
        .created_keys
        .get(CLIENT_KEY)
        .cloned()
        .expect("plain key not created");
    let rules = json!({"rules": [{
        "tool_name": tool,
        "scope_type": "key",
        "scope_id": aigw_core::crypto::hash_token(&raw),
        "allowed": false,
    }]});
    let mk = world.master_key.clone();
    send(
        world,
        Method::PUT,
        &format!("/admin/mcp/servers/{id}/allowlist"),
        Some(&mk),
        &[],
        Some(rules),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "writing the allowlist failed: {:?}",
        world.last_body
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn request_body(method: &str) -> Value {
    match method {
        "server/discover" => discover(),
        "tools/call" => tools_call(),
        other => panic!("scenario asked for unsupported MCP method: {other}"),
    }
}

#[when(expr = "用 master key 对 {string} 发送 MCP 透传请求 {string}")]
async fn when_send(world: &mut TestWorld, alias: String, method: String) {
    let mk = world.master_key.clone();
    send(
        world,
        Method::POST,
        &format!("/mcp/{alias}"),
        Some(&mk),
        &[],
        Some(request_body(&method)),
    )
    .await;
}

#[when(expr = "客户端带会话头对 {string} 发送 MCP 透传请求 {string}")]
async fn when_send_with_client_session(world: &mut TestWorld, alias: String, method: String) {
    let mk = world.master_key.clone();
    send(
        world,
        Method::POST,
        &format!("/mcp/{alias}"),
        Some(&mk),
        &[("Mcp-Session-Id", "client-session-from-elsewhere")],
        Some(request_body(&method)),
    )
    .await;
}

#[when(expr = "用普通 key 对 {string} 发送 MCP 透传请求 {string}")]
async fn when_send_with_plain_key(world: &mut TestWorld, alias: String, method: String) {
    let token = world
        .created_keys
        .get(CLIENT_KEY)
        .cloned()
        .expect("plain key not created");
    send(
        world,
        Method::POST,
        &format!("/mcp/{alias}"),
        Some(&token),
        &[],
        Some(request_body(&method)),
    )
    .await;
}

#[when(expr = "对 {string} 发送旧版 initialize 请求")]
async fn when_initialize(world: &mut TestWorld, alias: String) {
    let mk = world.master_key.clone();
    let body = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "bdd", "version": "1"}},
    });
    send(
        world,
        Method::POST,
        &format!("/mcp/{alias}"),
        Some(&mk),
        &[],
        Some(body),
    )
    .await;
}

#[when(expr = "对 {string} 发送 MCP GET 请求")]
async fn when_get(world: &mut TestWorld, alias: String) {
    let mk = world.master_key.clone();
    send(
        world,
        Method::GET,
        &format!("/mcp/{alias}"),
        Some(&mk),
        &[],
        None,
    )
    .await;
}

#[when(expr = "对 {string} 发送 MCP 请求且 Mcp-Name 头为 {string} 而 body 工具名为 {string}")]
async fn when_mismatched_name(
    world: &mut TestWorld,
    alias: String,
    header_name: String,
    body_name: String,
) {
    let mk = world.master_key.clone();
    let body =
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": body_name}});
    send(
        world,
        Method::POST,
        &format!("/mcp/{alias}"),
        Some(&mk),
        &[("Mcp-Name", header_name.as_str())],
        Some(body),
    )
    .await;
}

#[when(expr = "发送 \\/mcp-rest\\/tools\\/list 请求且 alias 为 {string}")]
async fn when_rest_list(world: &mut TestWorld, alias: String) {
    let mk = world.master_key.clone();
    send(
        world,
        Method::POST,
        "/mcp-rest/tools/list",
        Some(&mk),
        &[],
        Some(json!({"alias": alias})),
    )
    .await;
}

#[when(expr = "发送 \\/mcp-rest\\/tools\\/call 请求且 alias 为 {string} 工具为 {string}")]
async fn when_rest_call(world: &mut TestWorld, alias: String, tool: String) {
    let mk = world.master_key.clone();
    send(
        world,
        Method::POST,
        "/mcp-rest/tools/call",
        Some(&mk),
        &[],
        Some(json!({"alias": alias, "tool": tool, "arguments": {"q": "hello"}})),
    )
    .await;
}

#[when(
    expr = "用普通 key 发送 \\/mcp-rest\\/tools\\/call 请求且 alias 为 {string} 工具为 {string}"
)]
async fn when_rest_call_with_plain_key(world: &mut TestWorld, alias: String, tool: String) {
    let token = world
        .created_keys
        .get(CLIENT_KEY)
        .cloned()
        .expect("plain key not created");
    send(
        world,
        Method::POST,
        "/mcp-rest/tools/call",
        Some(&token),
        &[],
        Some(json!({"alias": alias, "tool": tool, "arguments": {"q": "hello"}})),
    )
    .await;
}

#[when(expr = "发送未认证的 MCP 请求到 {string}")]
async fn when_unauthenticated(world: &mut TestWorld, alias: String) {
    send(
        world,
        Method::POST,
        &format!("/mcp/{alias}"),
        None,
        &[],
        Some(discover()),
    )
    .await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "响应 JSON 路径 {string} 为字符串 {string}")]
async fn then_json_path_string(world: &mut TestWorld, path: String, expected: String) {
    let actual = resolve_path(body(world), &path);
    assert_eq!(actual.as_str(), Some(expected.as_str()), "path {path}");
}

#[then(expr = "响应 JSON 路径 {string} 为数字 {int}")]
async fn then_json_path_int(world: &mut TestWorld, path: String, expected: i64) {
    let actual = resolve_path(body(world), &path);
    assert_eq!(actual.as_i64(), Some(expected), "path {path}");
}

/// A regex step rather than a `{int}` one: MCP error codes are negative, and
/// the built-in `int` parameter does not promise a sign.
#[then(regex = r#"^响应 JSON 路径 "(.+)" 的 JSON 数字为 (-?\d+)$"#)]
async fn then_json_path_signed_int(world: &mut TestWorld, path: String, expected: i64) {
    let actual = resolve_path(body(world), &path);
    assert_eq!(actual.as_i64(), Some(expected), "path {path}");
}

#[then(expr = "响应 JSON 路径 {string} 声明支持的协议版本")]
async fn then_json_path_versions(world: &mut TestWorld, path: String) {
    let actual = resolve_path(body(world), &path);
    assert_eq!(actual, &json!(["2026-07-28"]), "path {path}");
}

#[then(expr = "MCP 响应头包含 Mcp-Session-Id")]
async fn then_session_header(world: &mut TestWorld) {
    let headers = world.last_headers.as_ref().expect("no headers");
    let session = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(session.len(), 36, "a UUID v7 session id, got {session:?}");
}

#[then(expr = "MCP 响应头不包含 Mcp-Session-Id")]
async fn then_no_session_header(world: &mut TestWorld) {
    let headers = world.last_headers.as_ref().expect("no headers");
    assert!(
        headers.get("mcp-session-id").is_none(),
        "the modern stack has no session, got {:?}",
        headers.get("mcp-session-id")
    );
}

#[then(expr = "MCP 响应头 X-Accel-Buffering 为 {string}")]
async fn then_accel_header(world: &mut TestWorld, expected: String) {
    let headers = world.last_headers.as_ref().expect("no headers");
    let actual = headers
        .get("x-accel-buffering")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("(missing)");
    assert_eq!(actual, expected);
}

#[then(expr = "上游依次收到 MCP 方法 {string}")]
async fn then_upstream_methods(world: &mut TestWorld, expected: String) {
    let expected: Vec<&str> = expected.split(',').map(str::trim).collect();
    let actual = methods(world);
    assert_eq!(
        actual, expected,
        "the upstream must see exactly its own negotiation"
    );
}

#[then(expr = "上游没有收到任何 MCP 请求")]
async fn then_upstream_silent(world: &mut TestWorld) {
    let actual = methods(world);
    assert!(
        actual.is_empty(),
        "answered locally, no upstream traffic: {actual:?}"
    );
}

#[then(expr = "上游收到的 Authorization 均为 {string}")]
async fn then_upstream_authorization(world: &mut TestWorld, expected: String) {
    let expected = Some(format!("Bearer {expected}"));
    let actual = log(world).lock().unwrap().authorizations.clone();
    assert!(!actual.is_empty(), "the upstream saw no traffic at all");
    for seen in &actual {
        assert_eq!(
            seen, &expected,
            "the client's aigw key must never leave the gateway"
        );
    }
}

#[then(expr = "上游收到的 Mcp-Session-Id 只可能是它自己签发的 {string}")]
async fn then_only_upstream_session(world: &mut TestWorld, expected: String) {
    let sessions = log(world).lock().unwrap().session_ids.clone();
    assert!(!sessions.is_empty(), "the upstream saw no traffic at all");
    for seen in sessions.iter().flatten() {
        assert_eq!(
            seen, &expected,
            "a client session names a session the upstream never created: {sessions:?}"
        );
    }
}

#[then(expr = "该 server 的 mcp_call_log 行数为 {int}")]
async fn then_call_log_rows(world: &mut TestWorld, expected: usize) {
    let state = world.ensure_state().await;
    let alias = alias_of(world);
    let rows = state
        .db
        .list_mcp_call_log(Some(&alias), 50, 0)
        .await
        .expect("log rows");
    assert_eq!(rows.len(), expected, "{alias}: {rows:?}");
}

#[then(expr = "最近一条 mcp_call_log 的 method 为 {string}")]
async fn then_last_log_method(world: &mut TestWorld, expected: String) {
    assert_eq!(latest_log(world).await.method, expected);
}

#[then(expr = "最近一条 mcp_call_log 的 status 为 {string}")]
async fn then_last_log_status(world: &mut TestWorld, expected: String) {
    assert_eq!(latest_log(world).await.status, expected);
}

#[then(expr = "最近一条 mcp_call_log 的 token_owner 为 {string}")]
async fn then_last_log_owner(world: &mut TestWorld, expected: String) {
    assert_eq!(latest_log(world).await.token_owner, expected);
}

#[then(expr = "最近一条 mcp_call_log 的 request_bytes 大于 0")]
async fn then_last_log_request_bytes(world: &mut TestWorld) {
    let row = latest_log(world).await;
    assert!(
        row.request_bytes > 0,
        "the arguments must be measured, not zeroed: {row:?}"
    );
}

#[then(expr = "MCP 响应头 x-call-id 与最近一条 mcp_call_log 的 call_id 一致")]
async fn then_call_id_header_matches_the_log(world: &mut TestWorld) {
    let header = world
        .last_headers
        .as_ref()
        .and_then(|h| h.get("x-call-id"))
        .and_then(|v| v.to_str().ok())
        .expect("no x-call-id on the response")
        .to_string();
    assert!(!header.is_empty(), "the header must name the row");
    assert_eq!(
        latest_log(world).await.call_id,
        header,
        "a client holding the body must be able to find its audit row"
    );
}
