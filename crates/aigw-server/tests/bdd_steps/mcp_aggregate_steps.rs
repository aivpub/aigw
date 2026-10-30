//! Step bindings for mcp_aggregate.feature — Stage 144 `POST /mcp`.
//!
//! Each upstream is a JSON-RPC stub bound to `127.0.0.1:0` inside the test
//! process, the same technique as `mcp_passthrough_steps` (ADR-041). The stub
//! records the methods, tool names and `Authorization` it saw, which is the only
//! way to prove a prefixed call really landed on the upstream the prefix names.
//!
//! `catalog_cache` is a process-level static keyed by server id, and every
//! scenario starts a fresh `sqlite::memory:` database whose ids restart at 1. A
//! cache entry left by the previous scenario would answer for this one's server,
//! so the first upstream of each scenario clears it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use axum::http::{HeaderMap, Method, StatusCode};
use axum::Router;
use cucumber::{given, then, when};
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::TestWorld;

const PLAIN_KEY: &str = "mcp-agg:plain-key";
const PLAIN_USER: &str = "agg-user";
const CACHE_ISOLATED: &str = "mcp-agg:cache-isolated";
const LAST_CALLED_ALIAS: &str = "mcp-agg:last-called-alias";

fn stub_log_key(alias: &str) -> String {
    format!("mcp-agg:stub:{alias}")
}

fn server_id_key(alias: &str) -> String {
    format!("mcp-agg:server:{alias}")
}

#[derive(Debug, Default)]
struct StubLog {
    methods: Vec<String>,
    tool_names: Vec<String>,
    authorizations: Vec<Option<String>>,
}

type SharedLog = Arc<Mutex<StubLog>>;

fn registry() -> &'static Mutex<HashMap<String, SharedLog>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, SharedLog>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn log_for(world: &TestWorld, alias: &str) -> SharedLog {
    let id = world
        .created_keys
        .get(&stub_log_key(alias))
        .unwrap_or_else(|| panic!("upstream \"{alias}\" was never started in this scenario"));
    registry()
        .lock()
        .unwrap()
        .get(id)
        .cloned()
        .unwrap_or_else(|| panic!("stub recorder {id} not registered"))
}

fn server_id_of(world: &TestWorld, alias: &str) -> i64 {
    world
        .created_keys
        .get(&server_id_key(alias))
        .unwrap_or_else(|| panic!("server \"{alias}\" was never registered"))
        .parse()
        .expect("server id")
}

fn body(world: &TestWorld) -> &Value {
    world.last_body.as_ref().expect("no response body")
}

fn tools_in_body(world: &TestWorld) -> Vec<String> {
    body(world)
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no result.tools in {}", body(world)))
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn build_router(state: aigw_server::routes::keys::SharedState) -> Router {
    use aigw_server::routes::{mcp, mcp_gateway, mcp_user};
    Router::new()
        .route("/mcp", axum::routing::post(mcp_gateway::post_mcp_aggregate))
        .route(
            "/admin/mcp/servers/{id}/allowlist",
            axum::routing::get(mcp::get_mcp_allowlist).put(mcp::put_mcp_allowlist),
        )
        .route("/admin/mcp/calls", axum::routing::get(mcp::list_mcp_calls))
        .route(
            "/v1/mcp/servers/{id}/my-token",
            axum::routing::get(mcp_user::get_my_token)
                .put(mcp_user::put_my_token)
                .delete(mcp_user::delete_my_token),
        )
        .with_state(state)
}

async fn send(world: &mut TestWorld, method: Method, uri: &str, token: &str, body: Option<Value>) {
    let state = world.ensure_state().await;
    let app = build_router(state);
    let request = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
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

/// A modern-stack upstream: `server/discover` plus one `tools/list` page, and a
/// `tools/call` that echoes the tool name so a wrong route is visible in the
/// body as well as in the recorder.
async fn spawn_stub(tools: Vec<String>) -> (String, SharedLog) {
    let sink: SharedLog = Arc::new(Mutex::new(StubLog::default()));
    let recorder = sink.clone();
    let catalog: Vec<Value> = tools
        .iter()
        .map(|name| {
            json!({
                "name": name,
                "description": format!("{name} tool"),
                "inputSchema": {"type": "object"},
            })
        })
        .collect();
    let app = Router::new().route(
        "/mcp",
        axum::routing::post(
            move |headers: HeaderMap, axum::Json(request): axum::Json<Value>| {
                let recorder = recorder.clone();
                let catalog = catalog.clone();
                async move {
                    let method = request
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let tool = request
                        .pointer("/params/name")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    {
                        let mut guard = recorder.lock().unwrap();
                        guard.methods.push(method.clone());
                        guard.tool_names.extend(tool.iter().map(|t| t.to_string()));
                        guard.authorizations.push(
                            headers
                                .get("authorization")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_string),
                        );
                    }
                    let id = request.get("id").cloned().unwrap_or(Value::Null);
                    let result = match method.as_str() {
                        "server/discover" => json!({
                            "supportedVersions": ["2026-07-28"],
                            "ttlMs": 60000,
                            "serverInfo": {"name": "agg-stub", "version": "1.0"},
                        }),
                        "tools/list" => json!({"tools": catalog}),
                        "tools/call" => json!({
                            "content": [{
                                "type": "text",
                                "text": format!("pong:{}", tool.unwrap_or_default()),
                            }],
                            "resultType": "complete",
                        }),
                        _ => json!({}),
                    };
                    (
                        StatusCode::OK,
                        axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": result})),
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

async fn register_upstream(
    world: &mut TestWorld,
    alias: &str,
    scope: &str,
    tools: Vec<String>,
) -> i64 {
    let state = world.ensure_state().await;
    if world
        .created_keys
        .insert(CACHE_ISOLATED.to_string(), "1".to_string())
        .is_none()
    {
        aigw_core::mcp::catalog_cache::clear();
    }

    let (url, sink) = spawn_stub(tools).await;
    let id = uuid::Uuid::new_v4().to_string();
    registry().lock().unwrap().insert(id.clone(), sink);
    world
        .created_keys
        .insert(stub_log_key(alias), id.to_string());

    let server = aigw_core::models::McpServerRow {
        id: 0,
        alias: alias.to_string(),
        display_name: format!("{alias} server"),
        url,
        transport: "streamable_http".to_string(),
        credential_scope: scope.to_string(),
        enabled: true,
        protocol_version: Some("2026-07-28".to_string()),
        auth_header_name: None,
        auth_header_value: None,
        extra_headers: None,
        tool_count: 0,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    };
    let id = state.db.create_mcp_server(&server).await.expect("server");
    aigw_core::mcp::catalog_cache::invalidate(id);
    world
        .created_keys
        .insert(server_id_key(alias), id.to_string());
    id
}

fn tools_of(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn plain_key(world: &TestWorld) -> String {
    world
        .created_keys
        .get(PLAIN_KEY)
        .cloned()
        .expect("the plain key was never created")
}

fn rpc(method: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": {}})
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "已启动聚合上游 {string} 提供工具 {string}")]
async fn given_upstream(world: &mut TestWorld, alias: String, tools: String) {
    register_upstream(world, &alias, "shared", tools_of(&tools)).await;
}

#[given(expr = "已启动要求个人凭据的聚合上游 {string}")]
async fn given_user_required_upstream(world: &mut TestWorld, alias: String) {
    register_upstream(world, &alias, "user_required", vec!["secret".to_string()]).await;
}

#[given(expr = "已生成聚合场景的普通 key")]
async fn given_plain_key(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let raw = format!("sk-{}", uuid::Uuid::new_v4());
    state
        .db
        .insert_key(&aigw_core::models::VirtualKey {
            token: aigw_core::crypto::hash_token(&raw),
            key_name: Some("agg-plain".to_string()),
            key_alias: Some("agg-plain".to_string()),
            soft_budget_cooldown: "false".to_string(),
            spend: 0.0,
            expires: None,
            models: json!([]),
            aliases: json!({}),
            config: json!({}),
            router_settings: None,
            user_id: Some(PLAIN_USER.to_string()),
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
    world.created_keys.insert(PLAIN_KEY.to_string(), raw);
}

#[given(expr = "管理员把 {string} 的工具 {string} 对当前普通 key 设为禁止")]
async fn given_acl_denial(world: &mut TestWorld, alias: String, tool: String) {
    let id = server_id_of(world, &alias);
    let raw = plain_key(world);
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
        &mk,
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

#[when(expr = "用 master key 向聚合端点发送 tools\\/list")]
async fn when_list_with_master_key(world: &mut TestWorld) {
    let mk = world.master_key.clone();
    send(world, Method::POST, "/mcp", &mk, Some(rpc("tools/list"))).await;
}

#[when(expr = "用普通 key 向聚合端点发送 tools\\/list")]
async fn when_list_with_plain_key(world: &mut TestWorld) {
    let raw = plain_key(world);
    send(world, Method::POST, "/mcp", &raw, Some(rpc("tools/list"))).await;
}

fn tools_call(name: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {"name": name, "arguments": {}},
    })
}

fn remember_prefix(world: &mut TestWorld, name: &str) {
    if let Some((alias, _)) = name.split_once("__") {
        world
            .created_keys
            .insert(LAST_CALLED_ALIAS.to_string(), alias.to_string());
    }
}

#[when(expr = "用 master key 调用聚合工具 {string}")]
async fn when_call_with_master_key(world: &mut TestWorld, name: String) {
    remember_prefix(world, &name);
    let mk = world.master_key.clone();
    send(world, Method::POST, "/mcp", &mk, Some(tools_call(&name))).await;
}

#[when(expr = "用普通 key 调用聚合工具 {string}")]
async fn when_call_with_plain_key(world: &mut TestWorld, name: String) {
    remember_prefix(world, &name);
    let raw = plain_key(world);
    send(world, Method::POST, "/mcp", &raw, Some(tools_call(&name))).await;
}

#[when(expr = "该普通 key 为上游 {string} 设置个人 token")]
async fn when_set_personal_token(world: &mut TestWorld, alias: String) {
    let id = server_id_of(world, &alias);
    let raw = plain_key(world);
    send(
        world,
        Method::PUT,
        &format!("/v1/mcp/servers/{id}/my-token"),
        &raw,
        Some(json!({"token_name": "personal", "token_value": "sk-vault-personal"})),
    )
    .await;
}

#[when(expr = "管理员按 server_alias {string} 查询调用日志")]
async fn when_query_call_log(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let app = build_router(state);
    let mk = world.master_key.clone();
    let request = axum::http::Request::builder()
        .method(Method::GET)
        .uri(format!("/admin/mcp/calls?server_alias={alias}"))
        .header("Authorization", format!("Bearer {mk}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_headers = Some(response.headers().clone());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "聚合目录工具数为 {int}")]
async fn then_tool_count(world: &mut TestWorld, expected: usize) {
    let names = tools_in_body(world);
    assert_eq!(names.len(), expected, "aggregated catalogue: {names:?}");
}

#[then(expr = "聚合目录包含工具 {string}")]
async fn then_catalogue_has(world: &mut TestWorld, name: String) {
    let names = tools_in_body(world);
    assert!(names.contains(&name), "\"{name}\" missing from {names:?}");
}

#[then(expr = "聚合目录不含工具 {string}")]
async fn then_catalogue_lacks(world: &mut TestWorld, name: String) {
    let names = tools_in_body(world);
    assert!(
        !names.contains(&name),
        "\"{name}\" must not be listed: {names:?}"
    );
}

#[then(expr = "上游 {string} 依次收到 MCP 方法 {string}")]
async fn then_upstream_methods(world: &mut TestWorld, alias: String, expected: String) {
    let expected: Vec<&str> = expected.split(',').map(str::trim).collect();
    let actual = log_for(world, &alias).lock().unwrap().methods.clone();
    assert_eq!(
        actual, expected,
        "upstream \"{alias}\" saw the wrong methods"
    );
}

#[then(expr = "上游 {string} 没有收到任何 MCP 请求")]
async fn then_upstream_silent(world: &mut TestWorld, alias: String) {
    let actual = log_for(world, &alias).lock().unwrap().methods.clone();
    assert!(
        actual.is_empty(),
        "upstream \"{alias}\" must not be touched: {actual:?}"
    );
}

#[then(expr = "上游 {string} 没有收到工具调用")]
async fn then_upstream_no_tool_call(world: &mut TestWorld, alias: String) {
    let calls: Vec<String> = log_for(world, &alias)
        .lock()
        .unwrap()
        .methods
        .iter()
        .filter(|m| m.as_str() == "tools/call")
        .cloned()
        .collect();
    assert!(
        calls.is_empty(),
        "a denied call must never reach upstream \"{alias}\": {calls:?}"
    );
}

#[then(expr = "上游 {string} 收到的工具名为 {string}")]
async fn then_upstream_tool_name(world: &mut TestWorld, alias: String, expected: String) {
    let names = log_for(world, &alias).lock().unwrap().tool_names.clone();
    assert_eq!(
        names,
        vec![expected.clone()],
        "the prefix must be stripped before the upstream sees the name"
    );
}

fn last_called_alias(world: &TestWorld) -> String {
    world
        .created_keys
        .get(LAST_CALLED_ALIAS)
        .cloned()
        .expect("no aggregate tool was called in this scenario")
}

#[then(expr = "最近一条聚合调用的 server_alias 为 {string}")]
async fn then_last_log_alias(world: &mut TestWorld, expected: String) {
    let state = world.ensure_state().await;
    let alias = last_called_alias(world);
    let row = state
        .db
        .list_mcp_call_log(Some(&alias), 1, 0)
        .await
        .expect("log rows")
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no mcp_call_log row for {alias}"));
    assert_eq!(row.server_alias, expected);
}

#[then(expr = "最近一条聚合调用的 method 为 {string}")]
async fn then_last_log_method(world: &mut TestWorld, expected: String) {
    let state = world.ensure_state().await;
    let alias = last_called_alias(world);
    let row = state
        .db
        .list_mcp_call_log(Some(&alias), 1, 0)
        .await
        .expect("log rows")
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no mcp_call_log row for {alias}"));
    assert_eq!(row.method, expected);
}

#[then(expr = "调用日志 total_count 为 {int}")]
async fn then_call_log_total(world: &mut TestWorld, expected: i64) {
    assert_eq!(
        body(world)["total_count"].as_i64(),
        Some(expected),
        "{}",
        body(world)
    );
}

#[then(expr = "调用日志第一行 tool_name 为 {string}")]
async fn then_call_log_first_tool(world: &mut TestWorld, expected: String) {
    assert_eq!(
        body(world)["data"][0]["tool_name"].as_str(),
        Some(expected.as_str()),
        "{}",
        body(world)
    );
}
