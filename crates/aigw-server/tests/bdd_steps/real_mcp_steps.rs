//! Step bindings for real/mcp_server_crud_real.feature — Stage 141 real BDD
//!
//! Drives the running aigw server over HTTP so the PostgreSQL / MySQL branches of
//! [`aigw_core::db`] actually execute — `replace_mcp_tool_catalog`,
//! `list_mcp_tool_catalog`, `list_mcp_servers_paged` and `count_mcp_servers` all
//! differ per dialect (repeated `?` binds on MySQL, `COALESCE(COUNT(...), 0)`,
//! JSON columns on both). The mock scenarios run over SQLite and cannot see any
//! of that.
//!
//! A read-only probe against `AIGW_TEST_DB_URL` is what proves the storage facts
//! the HTTP layer cannot show: the row counts after a delete, and (in the last
//! scenario) that the table-level foreign key really cascades — MySQL ignores a
//! column-level `REFERENCES`, so a direct `DELETE FROM mcp_servers` is the only
//! way to make that constraint speak.

use cucumber::{given, then, when};
use serde_json::{json, Value};

use super::real_api_steps::{base_url, client, real_api_enabled};
use crate::TestWorld;

fn server_id_key(alias: &str) -> String {
    format!("mcp-real:{}", alias)
}

fn store_server_id(world: &mut TestWorld, alias: &str, id: i64) {
    world
        .created_keys
        .insert(server_id_key(alias), id.to_string());
    world
        .created_keys
        .insert("mcp-real:latest".to_string(), id.to_string());
}

fn server_id(world: &TestWorld, alias: &str) -> i64 {
    world
        .created_keys
        .get(&server_id_key(alias))
        .unwrap_or_else(|| panic!("MCP server {} not created", alias))
        .parse()
        .expect("server id")
}

fn latest_server_id(world: &TestWorld) -> i64 {
    world
        .created_keys
        .get("mcp-real:latest")
        .expect("no MCP server created yet")
        .parse()
        .expect("server id")
}

fn token_id(world: &TestWorld, name: &str) -> i64 {
    world
        .created_keys
        .get(&format!("mcp-real:token:{}", name))
        .unwrap_or_else(|| panic!("token {} not created", name))
        .parse()
        .expect("token id")
}

/// One tool as the stub publishes it. Unicode in the schema property and in the
/// annotations is deliberate: it is what catches a MySQL connection left on a
/// non-utf8mb4 charset or a JSON column losing a non-ASCII escape.
fn stub_tool() -> Value {
    json!({
        "name": "search",
        "description": "search tool",
        "inputSchema": {
            "type": "object",
            "properties": {"q": {"type": "string", "description": "查询词"}},
            "required": ["q"],
            "additionalProperties": false,
        },
        "annotations": {"readOnlyHint": true, "title": "检索"},
    })
}

fn stub_input_schema() -> Value {
    stub_tool()["inputSchema"].clone()
}

fn stub_annotations() -> Value {
    stub_tool()["annotations"].clone()
}

/// A JSON-RPC stub on an ephemeral port, answering the modern stack
/// (`server/discover` + one `tools/list` page). Leaked for the rest of the
/// process on purpose — an idle listener costs nothing and the binary exits
/// right after the run.
async fn spawn_stub() -> String {
    let tool = stub_tool();
    let app = axum::Router::new().route(
        "/mcp",
        axum::routing::post(move |axum::Json(request): axum::Json<Value>| {
            let tool = tool.clone();
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
                        "serverInfo": {"name": "real-bdd-stub", "version": "1.0"},
                    }),
                    "tools/list" => json!({"tools": [tool.clone()]}),
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

fn test_db_url() -> String {
    std::env::var("AIGW_TEST_DB_URL").expect("AIGW_TEST_DB_URL must be set by the BDD harness")
}

/// Rows a query returns. Counts rows rather than reading `COUNT(*)` for the
/// reason documented in `real_websearch_steps::row_count`: MySQL's shared value
/// coercion decodes a `BIGINT` as `Bool` first, so `COUNT(*)` reads as `false`.
async fn row_count(sql: &str) -> usize {
    let pool = aigw_migrate::native::SourcePool::connect(&test_db_url())
        .await
        .expect("connect to test db");
    pool.read_rows_sql(sql).await.expect("read rows").len()
}

async fn execute(sql: &str) {
    let pool = aigw_migrate::native::SourcePool::connect(&test_db_url())
        .await
        .expect("connect to test db");
    pool.execute_raw(sql).await.expect("execute sql");
}

async fn auth_get(world: &mut TestWorld, path: &str) {
    let mk = world.master_key.clone();
    let resp = client()
        .get(format!("{}{}", base_url(), path))
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("GET request");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

async fn post_json(world: &mut TestWorld, path: &str, body: Value) {
    let mk = world.master_key.clone();
    let resp = client()
        .post(format!("{}{}", base_url(), path))
        .header("Authorization", format!("Bearer {}", mk))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("POST request");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

async fn delete_req(world: &mut TestWorld, path: &str) {
    let mk = world.master_key.clone();
    let resp = client()
        .delete(format!("{}{}", base_url(), path))
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("DELETE request");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

fn body(world: &TestWorld) -> &Value {
    world.last_body.as_ref().expect("no response body")
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "已启动 real MCP JSON-RPC stub，提供工具 {string}")]
async fn given_stub(world: &mut TestWorld, tool_name: String) {
    if !real_api_enabled() {
        return;
    }
    assert_eq!(
        tool_name, "search",
        "the stub publishes one tool named search"
    );
    let url = spawn_stub().await;
    world.created_keys.insert("mcp-real:stub".to_string(), url);
}

#[given(expr = "通过 API 创建指向该 stub 的 MCP server {string}")]
async fn given_server_pointing_at_stub(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        return;
    }
    let url = world
        .created_keys
        .get("mcp-real:stub")
        .cloned()
        .expect("stub not started");
    post_json(
        world,
        "/admin/mcp/servers",
        json!({
            "alias": alias,
            "display_name": format!("{alias} server"),
            "url": url,
        }),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "create MCP server {} failed: {:?}",
        alias,
        world.last_body
    );
    let id = body(world)["id"].as_i64().expect("server id");
    store_server_id(world, &alias, id);
}

#[given(expr = "通过 API 创建 MCP server {string} 且 enabled 为 {word}")]
async fn given_server_with_enabled(world: &mut TestWorld, alias: String, enabled: String) {
    if !real_api_enabled() {
        return;
    }
    post_json(
        world,
        "/admin/mcp/servers",
        json!({
            "alias": alias,
            "display_name": format!("{alias} server"),
            "url": "http://127.0.0.1:1/mcp",
            "enabled": enabled == "true",
        }),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "create MCP server {} failed: {:?}",
        alias,
        world.last_body
    );
    let id = body(world)["id"].as_i64().expect("server id");
    store_server_id(world, &alias, id);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "通过 API 探测该 MCP server")]
async fn when_probe(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    post_json(
        world,
        &format!("/admin/mcp/servers/{}/probe", id),
        json!({}),
    )
    .await;
}

#[when(expr = "通过 API 读取该 MCP server 的 tools")]
async fn when_read_tools(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    auth_get(world, &format!("/admin/mcp/servers/{}/tools", id)).await;
}

#[when(expr = "通过 API 为该 MCP server 添加 token")]
async fn when_add_token(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    add_token(world, "real-shared").await;
}

#[when(expr = "通过 API 为该 MCP server 添加 token {string}")]
async fn when_add_named_token(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    add_token(world, &name).await;
}

async fn add_token(world: &mut TestWorld, name: &str) {
    let id = latest_server_id(world);
    post_json(
        world,
        &format!("/admin/mcp/servers/{}/tokens", id),
        json!({"token_name": name, "token_value": format!("sk-real-{name}")}),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "add token {} failed: {:?}",
        name,
        world.last_body
    );
    let token_id = body(world)["id"].as_i64().expect("token id");
    world
        .created_keys
        .insert(format!("mcp-real:token:{}", name), token_id.to_string());
}

#[when(expr = "通过 API 把 token {string} 设为默认")]
async fn when_set_default_token(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    let server = latest_server_id(world);
    let token = token_id(world, &name);
    post_json(
        world,
        &format!("/admin/mcp/servers/{}/tokens/{}/default", server, token),
        json!({}),
    )
    .await;
}

#[when(expr = "通过 API 删除该 MCP server")]
async fn when_delete(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    delete_req(world, &format!("/admin/mcp/servers/{}", id)).await;
}

#[when(expr = "通过 API 按 search {string} 与 status {word} 与 page_size {int} 查询 MCP server")]
async fn when_list_paged(world: &mut TestWorld, search: String, status: String, page_size: i64) {
    if !real_api_enabled() {
        return;
    }
    let query = format!(
        "/admin/mcp/servers?page=1&page_size={}&status={}&search={}",
        page_size, status, search
    );
    auth_get(world, &query).await;
}

#[when(expr = "真库直删 MCP server 行 {string}")]
async fn when_delete_row_directly(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        return;
    }
    let id = server_id(world, &alias);
    execute(&format!("DELETE FROM mcp_servers WHERE id = {}", id)).await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "real MCP 创建返回 {int}")]
async fn then_create_status(world: &mut TestWorld, expected: u16) {
    if !real_api_enabled() {
        return;
    }
    assert_eq!(
        world.last_status,
        Some(expected),
        "unexpected create status, body {:?}",
        world.last_body
    );
}

#[then(expr = "real MCP 探测返回 {int}")]
async fn then_probe_status(world: &mut TestWorld, expected: u16) {
    if !real_api_enabled() {
        return;
    }
    assert_eq!(
        world.last_status,
        Some(expected),
        "unexpected probe status, body {:?}",
        world.last_body
    );
}

#[then(expr = "real MCP 删除返回 {int}")]
async fn then_delete_status(world: &mut TestWorld, expected: u16) {
    if !real_api_enabled() {
        return;
    }
    assert_eq!(
        world.last_status,
        Some(expected),
        "unexpected delete status, body {:?}",
        world.last_body
    );
}

#[then(expr = "real MCP 设为默认返回 {int}")]
async fn then_set_default_status(world: &mut TestWorld, expected: u16) {
    if !real_api_enabled() {
        return;
    }
    assert_eq!(
        world.last_status,
        Some(expected),
        "unexpected set-default status, body {:?}",
        world.last_body
    );
    assert_eq!(
        body(world).get("is_default").and_then(Value::as_bool),
        Some(true),
        "default token response should be the promoted one: {}",
        body(world)
    );
}

#[then(expr = "real MCP 列表 total_count 为 {int}")]
async fn then_total_count(world: &mut TestWorld, expected: i64) {
    if !real_api_enabled() {
        return;
    }
    let total = body(world)
        .get("total_count")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("no total_count: {}", body(world)));
    assert_eq!(total, expected, "body: {}", body(world));
}

#[then(expr = "real MCP 列表 total_pages 为 {int}")]
async fn then_total_pages(world: &mut TestWorld, expected: i64) {
    if !real_api_enabled() {
        return;
    }
    let pages = body(world)
        .get("total_pages")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("no total_pages: {}", body(world)));
    assert_eq!(pages, expected, "body: {}", body(world));
}

#[then(expr = "real MCP 列表返回行数为 {int}")]
async fn then_row_count(world: &mut TestWorld, expected: usize) {
    if !real_api_enabled() {
        return;
    }
    let data = body(world)["data"]
        .as_array()
        .unwrap_or_else(|| panic!("no data array: {}", body(world)));
    assert_eq!(data.len(), expected, "body: {}", body(world));
}

#[then(expr = "real MCP 列表的第一行 enabled 为 {word}")]
async fn then_first_row_enabled(world: &mut TestWorld, expected: String) {
    if !real_api_enabled() {
        return;
    }
    let first = body(world)["data"]
        .as_array()
        .and_then(|rows| rows.first())
        .unwrap_or_else(|| panic!("no rows: {}", body(world)));
    assert_eq!(
        first.get("enabled").and_then(Value::as_bool),
        Some(expected == "true"),
        "row: {first}"
    );
}

#[then(expr = "real 库中该 server 的 tool catalog 行数为 {int}")]
async fn then_catalog_rows(world: &mut TestWorld, expected: usize) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    let rows = row_count(&format!(
        "SELECT id FROM mcp_tool_catalog WHERE server_id = {}",
        id
    ))
    .await;
    assert_eq!(rows, expected, "mcp_tool_catalog rows for server {id}");
}

#[then(expr = "real 库中该 server 的 token 行数为 {int}")]
async fn then_token_rows(world: &mut TestWorld, expected: usize) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    let rows = row_count(&format!(
        "SELECT id FROM mcp_server_tokens WHERE server_id = {}",
        id
    ))
    .await;
    assert_eq!(rows, expected, "mcp_server_tokens rows for server {id}");
}

#[then(expr = "real 库中该 server 的默认 token 行数为 {int}")]
async fn then_default_token_rows(world: &mut TestWorld, expected: usize) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    let rows = row_count(&format!(
        "SELECT id FROM mcp_server_tokens WHERE server_id = {} AND is_default",
        id
    ))
    .await;
    assert_eq!(rows, expected, "default token rows for server {id}");
}

#[then(expr = "real 库中该 server 的 mcp_servers 行数为 {int}")]
async fn then_server_rows(world: &mut TestWorld, expected: usize) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    let rows = row_count(&format!("SELECT id FROM mcp_servers WHERE id = {}", id)).await;
    assert_eq!(rows, expected, "mcp_servers rows for id {id}");
}

#[then(expr = "real MCP tools 响应工具数为 {int}")]
async fn then_tools_len(world: &mut TestWorld, expected: usize) {
    if !real_api_enabled() {
        return;
    }
    let tools = body(world)["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("no tools array: {}", body(world)));
    assert_eq!(tools.len(), expected, "body: {}", body(world));
}

#[then(expr = "real MCP tools 的第一个工具 description 为 {string}")]
async fn then_first_tool_description(world: &mut TestWorld, expected: String) {
    if !real_api_enabled() {
        return;
    }
    let tool = first_tool(world);
    assert_eq!(
        tool.get("description").and_then(Value::as_str),
        Some(expected.as_str()),
        "tool: {tool}"
    );
}

#[then(expr = "real MCP tools 的第一个工具 input_schema 与 stub 申报的一致")]
async fn then_first_tool_schema(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let tool = first_tool(world);
    assert_eq!(
        tool.get("input_schema"),
        Some(&stub_input_schema()),
        "input_schema did not survive the round trip: {tool}"
    );
}

#[then(expr = "real MCP tools 的第一个工具 annotations 与 stub 申报的一致")]
async fn then_first_tool_annotations(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let tool = first_tool(world);
    assert_eq!(
        tool.get("annotations"),
        Some(&stub_annotations()),
        "annotations did not survive the round trip: {tool}"
    );
}

fn first_tool(world: &TestWorld) -> Value {
    body(world)["tools"]
        .as_array()
        .and_then(|tools| tools.first())
        .cloned()
        .unwrap_or_else(|| panic!("no tools: {}", body(world)))
}
