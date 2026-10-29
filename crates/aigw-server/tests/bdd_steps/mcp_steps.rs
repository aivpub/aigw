//! Step bindings for mcp_server_crud.feature — Stage 141 /admin/mcp/* CRUD
//!
//! Mirrors `websearch_admin_steps.rs`: an in-memory SQLite test DB behind a
//! router built for these endpoints only. Token scenarios hang off
//! `数据库已初始化且已配置 master key` (defined in `proxy_steps.rs`) because
//! sealing an upstream credential needs `aigw_master_key`, and the default
//! `ensure_state()` carries `None`.
//!
//! The probe scenarios cannot use `bdd_support::mock_upstream` — that router is
//! a fixed LLM shape with no `server/discover` / `tools/list`. They start their
//! own JSON-RPC stub on `127.0.0.1:0` instead and register its address as the
//! server's `url`, the same technique the probe unit tests use (ADR-041).

use axum::http::Method;
use axum::Router;
use cucumber::{given, then, when};
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::TestWorld;

fn build_mcp_router(state: aigw_server::routes::keys::SharedState) -> Router {
    use aigw_server::routes::mcp;
    Router::new()
        .route(
            "/admin/mcp/servers",
            axum::routing::get(mcp::list_mcp_servers).post(mcp::create_mcp_server),
        )
        .route(
            "/admin/mcp/servers/all",
            axum::routing::get(mcp::list_all_mcp_servers),
        )
        .route(
            "/admin/mcp/servers/batch-delete",
            axum::routing::post(mcp::batch_delete_mcp_servers),
        )
        .route(
            "/admin/mcp/servers/{id}",
            axum::routing::get(mcp::get_mcp_server)
                .put(mcp::update_mcp_server)
                .delete(mcp::delete_mcp_server),
        )
        .route(
            "/admin/mcp/servers/{id}/toggle",
            axum::routing::post(mcp::toggle_mcp_server),
        )
        .route(
            "/admin/mcp/servers/{id}/probe",
            axum::routing::post(mcp::probe_mcp_server),
        )
        .route(
            "/admin/mcp/servers/{id}/tools",
            axum::routing::get(mcp::list_mcp_server_tools),
        )
        .route(
            "/admin/mcp/servers/{id}/tokens",
            axum::routing::get(mcp::list_mcp_tokens).post(mcp::create_mcp_token),
        )
        .route(
            "/admin/mcp/servers/{id}/tokens/{token_id}",
            axum::routing::delete(mcp::delete_mcp_token),
        )
        .route(
            "/admin/mcp/servers/{id}/tokens/{token_id}/default",
            axum::routing::post(mcp::set_default_mcp_token),
        )
        .route("/admin/mcp/reload", axum::routing::post(mcp::reload_mcp))
        .with_state(state)
}

async fn send_request(
    world: &mut TestWorld,
    method: Method,
    uri: &str,
    auth: Option<&str>,
    body: Option<&str>,
) {
    let state = world.ensure_state().await;
    let app = build_mcp_router(state);
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json");
    if let Some(token) = auth {
        req = req.header("Authorization", format!("Bearer {}", token));
    }
    let req = req
        .body(axum::body::Body::from(body.unwrap_or("").to_string()))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

const LATEST_SERVER: &str = "mcp-server:latest";
const LATEST_TOKEN: &str = "mcp-token:latest";
const STUB_URL: &str = "mcp-stub:url";

fn store_latest_server(world: &mut TestWorld, id: i64) {
    world
        .created_keys
        .insert(LATEST_SERVER.to_string(), id.to_string());
}

fn latest_server_id(world: &TestWorld) -> i64 {
    world
        .created_keys
        .get(LATEST_SERVER)
        .expect("no MCP server created yet")
        .parse()
        .expect("server id")
}

fn latest_token_id(world: &TestWorld) -> i64 {
    world
        .created_keys
        .get(LATEST_TOKEN)
        .expect("no MCP token created yet")
        .parse()
        .expect("token id")
}

fn body(world: &TestWorld) -> &Value {
    world.last_body.as_ref().expect("no response body")
}

fn data_array(world: &TestWorld) -> &Vec<Value> {
    body(world)
        .get("data")
        .and_then(Value::as_array)
        .expect("data array")
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stub upstream
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A JSON-RPC stub that speaks the modern stack: `server/discover` plus a single
/// `tools/list` page with two tools. Lives for the rest of the process; the
/// scenarios that use it never signal a shutdown, and a leaked listener on an
/// ephemeral port costs nothing when the test binary exits.
async fn spawn_stub(tools: &[&'static str]) -> String {
    let tools: Vec<Value> = tools
        .iter()
        .map(|name| {
            json!({
                "name": name,
                "description": format!("{name} tool"),
                "inputSchema": {"type": "object"},
            })
        })
        .collect();
    let envelope = std::sync::Arc::new(tools);
    let app = Router::new().route(
        "/mcp",
        axum::routing::post(move |axum::Json(request): axum::Json<Value>| {
            let envelope = envelope.clone();
            async move {
                let id = request.get("id").cloned().unwrap_or(Value::Null);
                let method = request
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let result = match method {
                    "server/discover" => json!({
                        "supportedVersions": ["2026-07-28"],
                        "ttlMs": 60000,
                        "serverInfo": {"name": "bdd-stub", "version": "1.0"},
                    }),
                    "tools/list" => json!({"tools": envelope.as_ref().clone()}),
                    _ => json!({}),
                };
                axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": result}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stub");
    let addr = listener.local_addr().expect("stub addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}/mcp")
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "已启动提供 2 个工具的 MCP JSON-RPC stub")]
async fn given_jsonrpc_stub(world: &mut TestWorld) {
    let url = spawn_stub(&["search", "read"]).await;
    world.created_keys.insert(STUB_URL.to_string(), url);
}

#[given(expr = "已通过 API 创建指向该 stub 的 MCP server {string}")]
async fn given_server_pointing_at_stub(world: &mut TestWorld, alias: String) {
    let url = world
        .created_keys
        .get(STUB_URL)
        .cloned()
        .expect("stub not started");
    let request = json!({
        "alias": alias,
        "display_name": format!("{alias} server"),
        "url": url,
    })
    .to_string();
    send_request(
        world,
        Method::POST,
        "/admin/mcp/servers",
        Some(&world.master_key.clone()),
        Some(&request),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "create MCP server failed: {:?}",
        world.last_body
    );
    let id = body(world)["id"].as_i64().expect("server id");
    store_latest_server(world, id);
}

#[given(expr = "已通过 API 创建 MCP server {string}")]
async fn given_server(world: &mut TestWorld, alias: String) {
    let request = json!({
        "alias": alias,
        "display_name": format!("{alias} server"),
        "url": "http://127.0.0.1:1/mcp",
    })
    .to_string();
    send_request(
        world,
        Method::POST,
        "/admin/mcp/servers",
        Some(&world.master_key.clone()),
        Some(&request),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "create MCP server failed: {:?}",
        world.last_body
    );
    let id = body(world)["id"].as_i64().expect("server id");
    store_latest_server(world, id);
}

#[given(expr = "已生成普通 key {string}（MCP 场景）")]
async fn given_regular_key(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let raw = format!("sk-{}", uuid::Uuid::new_v4());
    let key = aigw_core::models::VirtualKey {
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
        user_id: Some("mcp-user".to_string()),
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
    };
    state.db.insert_key(&key).await.expect("insert key");
    world.created_keys.insert(alias, raw);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(
    expr = "发送 POST \\/admin\\/mcp\\/servers 请求创建 MCP server {string} 且 enabled 为 {word}"
)]
async fn when_create_server(world: &mut TestWorld, alias: String, enabled: String) {
    let request = json!({
        "alias": alias,
        "display_name": format!("{alias} server"),
        "url": "http://127.0.0.1:1/mcp",
        "enabled": enabled == "true",
    })
    .to_string();
    send_request(
        world,
        Method::POST,
        "/admin/mcp/servers",
        Some(&world.master_key.clone()),
        Some(&request),
    )
    .await;
    if let Some(id) = body(world).get("id").and_then(Value::as_i64) {
        store_latest_server(world, id);
    }
}

#[when(expr = "发送 GET \\/admin\\/mcp\\/servers 请求")]
async fn when_list_servers(world: &mut TestWorld) {
    send_request(
        world,
        Method::GET,
        "/admin/mcp/servers",
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "发送 GET \\/admin\\/mcp\\/servers 且 status=disabled 请求")]
async fn when_list_servers_disabled(world: &mut TestWorld) {
    send_request(
        world,
        Method::GET,
        "/admin/mcp/servers?status=disabled",
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "使用普通 key {string} 发送 GET \\/admin\\/mcp\\/servers 请求")]
async fn when_list_servers_with_key(world: &mut TestWorld, alias: String) {
    let token = world.created_keys.get(&alias).expect("key").clone();
    send_request(world, Method::GET, "/admin/mcp/servers", Some(&token), None).await;
}

#[when(expr = "发送 POST \\/admin\\/mcp\\/servers\\/{word}\\/probe 请求")]
async fn when_probe(world: &mut TestWorld, _server: String) {
    let id = latest_server_id(world);
    send_request(
        world,
        Method::POST,
        &format!("/admin/mcp/servers/{id}/probe"),
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "发送 GET \\/admin\\/mcp\\/servers\\/{word}\\/tools 请求")]
async fn when_get_tools(world: &mut TestWorld, _server: String) {
    let id = latest_server_id(world);
    send_request(
        world,
        Method::GET,
        &format!("/admin/mcp/servers/{id}/tools"),
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "发送 POST \\/admin\\/mcp\\/servers\\/{word}\\/tokens 请求添加 token")]
async fn when_add_token(world: &mut TestWorld, _server: String, step: &cucumber::gherkin::Step) {
    let id = latest_server_id(world);
    let request = step.docstring.as_ref().expect("docstring body").to_string();
    send_request(
        world,
        Method::POST,
        &format!("/admin/mcp/servers/{id}/tokens"),
        Some(&world.master_key.clone()),
        Some(&request),
    )
    .await;
    if let Some(token_id) = body(world).get("id").and_then(Value::as_i64) {
        world
            .created_keys
            .insert(LATEST_TOKEN.to_string(), token_id.to_string());
    }
}

#[when(expr = "发送 POST \\/admin\\/mcp\\/servers\\/{word}\\/tokens\\/{word}\\/default 请求")]
async fn when_set_default_token(world: &mut TestWorld, _server: String, _token: String) {
    let id = latest_server_id(world);
    let token_id = latest_token_id(world);
    send_request(
        world,
        Method::POST,
        &format!("/admin/mcp/servers/{id}/tokens/{token_id}/default"),
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "发送 POST \\/admin\\/mcp\\/servers\\/{word}\\/toggle 请求")]
async fn when_toggle(world: &mut TestWorld, _server: String) {
    let id = latest_server_id(world);
    send_request(
        world,
        Method::POST,
        &format!("/admin/mcp/servers/{id}/toggle"),
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "发送 DELETE \\/admin\\/mcp\\/servers\\/{word} 请求")]
async fn when_delete_server(world: &mut TestWorld, _server: String) {
    let id = latest_server_id(world);
    send_request(
        world,
        Method::DELETE,
        &format!("/admin/mcp/servers/{id}"),
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "响应包含 alias 字段值为 {string}")]
async fn then_alias_is(world: &mut TestWorld, expected: String) {
    let alias = body(world)
        .get("alias")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no alias field: {}", body(world)));
    assert_eq!(alias, expected);
}

#[then(expr = "响应 enabled 为 {word}")]
async fn then_enabled_is(world: &mut TestWorld, expected: String) {
    let enabled = body(world)
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("no enabled field: {}", body(world)));
    assert_eq!(enabled, expected == "true", "body: {}", body(world));
}

#[then(expr = "MCP server 列表 enabled 为 {word}")]
async fn then_first_server_enabled_is(world: &mut TestWorld, expected: String) {
    let first = data_array(world).first().expect("no server rows");
    let enabled = first
        .get("enabled")
        .and_then(Value::as_bool)
        .expect("enabled on row");
    assert_eq!(enabled, expected == "true", "row: {first}");
}

#[then(expr = "MCP server 总数为 {int}")]
async fn then_total_count_is(world: &mut TestWorld, expected: i64) {
    let total = body(world)
        .get("total_count")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("no total_count: {}", body(world)));
    assert_eq!(total, expected, "body: {}", body(world));
}

#[then(expr = "MCP server 列表工具数为 {int}")]
async fn then_first_tool_count_is(world: &mut TestWorld, expected: i64) {
    let first = data_array(world).first().expect("no server rows");
    let count = first
        .get("tool_count")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("no tool_count on row: {first}"));
    assert_eq!(count, expected);
}

#[then(expr = "探测协议版本为 {string}")]
async fn then_protocol_version(world: &mut TestWorld, expected: String) {
    let version = body(world)
        .get("protocol_version")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no protocol_version: {}", body(world)));
    assert_eq!(version, expected);
}

#[then(expr = "探测工具数为 {int}")]
async fn then_probe_tool_count(world: &mut TestWorld, expected: usize) {
    let tools = body(world)
        .get("tools")
        .and_then(Value::as_array)
        .expect("tools array");
    assert_eq!(tools.len(), expected, "body: {}", body(world));
}

#[then(expr = "tools 响应工具数为 {int}")]
async fn then_snapshot_tool_count(world: &mut TestWorld, expected: usize) {
    let tools = body(world)
        .get("tools")
        .and_then(Value::as_array)
        .expect("tools array");
    assert_eq!(tools.len(), expected, "body: {}", body(world));
}

#[then(expr = "tools 响应 fetched_at 非空")]
async fn then_snapshot_has_fetched_at(world: &mut TestWorld) {
    let fetched_at = body(world).get("fetched_at").unwrap_or(&Value::Null);
    assert!(
        fetched_at.is_string(),
        "fetched_at should be an RFC 3339 string, got {fetched_at}"
    );
}

#[then(expr = "token 响应 token_redacted 为 {string}")]
async fn then_token_redacted(world: &mut TestWorld, expected: String) {
    let redacted = body(world)
        .get("token_redacted")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no token_redacted: {}", body(world)));
    assert_eq!(redacted, expected);
}

#[then(expr = "数据库存储的 token 带 v2:gcm: 前缀")]
async fn then_token_is_sealed(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let id = latest_server_id(world);
    let tokens = state.db.list_mcp_tokens(id).await.expect("list tokens");
    let stored = tokens.first().expect("a token row").token_value.clone();
    assert!(
        stored.starts_with("v2:gcm:"),
        "token is not sealed: {stored}"
    );
}

#[then(expr = "token 列表中仅有一条 is_default")]
async fn then_exactly_one_default(world: &mut TestWorld) {
    let id = latest_server_id(world);
    send_request(
        world,
        Method::GET,
        &format!("/admin/mcp/servers/{id}/tokens"),
        Some(&world.master_key.clone()),
        None,
    )
    .await;
    assert_eq!(world.last_status, Some(200), "{:?}", world.last_body);
    let defaults = data_array(world)
        .iter()
        .filter(|t| t.get("is_default").and_then(Value::as_bool) == Some(true))
        .count();
    assert_eq!(defaults, 1, "body: {}", body(world));
}
