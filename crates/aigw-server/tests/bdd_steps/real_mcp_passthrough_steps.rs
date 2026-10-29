//! Step bindings for real/mcp_passthrough_real.feature — Stage 143 real BDD
//!
//! Drives the running aigw server over HTTP so the PostgreSQL / MySQL branches of
//! [`aigw_core::db`] actually execute: `insert_mcp_call_log` binds thirteen
//! parameters in three dialects (MySQL has no `?n` numbering and no
//! `RETURNING`), and `list_mcp_call_log` returns BIGINT counters that the shared
//! value coercion in `aigw_migrate::native` would decode as `Bool` if it probed
//! them first. Neither is visible from the SQLite mock run.
//!
//! A read-only probe against `AIGW_TEST_DB_URL` is what proves the storage facts
//! the HTTP layer cannot show: the row that a real call wrote, and that its
//! counters and nullability survived the round trip.

use cucumber::{given, then, when};
use serde_json::{json, Value};

use super::real_api_steps::{base_url, client, real_api_enabled, set_skip_pass};
use crate::TestWorld;

const SERVER_ALIAS: &str = "mcp-pass-real:alias";

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

/// One scalar column as text. Empty string means SQL NULL or an empty value —
/// the call-log columns this is used on are never legitimately empty.
async fn scalar(sql: &str) -> String {
    let pool = aigw_migrate::native::SourcePool::connect(&test_db_url())
        .await
        .expect("connect to test db");
    let rows = pool.read_rows_sql(sql).await.expect("read rows");
    let (_, value) = rows
        .first()
        .and_then(|r| r.first())
        .unwrap_or_else(|| panic!("no row for: {sql}"));
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn alias_of(world: &TestWorld) -> String {
    world
        .created_keys
        .get(SERVER_ALIAS)
        .cloned()
        .expect("no MCP server registered in this scenario")
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
    world.last_headers = Some(resp.headers().clone());
    world.last_body = resp.json().await.ok();
}

fn body(world: &TestWorld) -> &Value {
    world.last_body.as_ref().expect("no response body")
}

/// A JSON-RPC stub on an ephemeral port answering the modern stack. Leaked for
/// the rest of the process on purpose — an idle listener costs nothing and the
/// binary exits right after the run.
async fn spawn_stub() -> String {
    let app = axum::Router::new().route(
        "/mcp",
        axum::routing::post(|axum::Json(request): axum::Json<Value>| async move {
            let id = request.get("id").cloned().unwrap_or(Value::Null);
            let method = request
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let result = match method {
                "server/discover" => json!({
                    "supportedVersions": ["2026-07-28"],
                    "ttlMs": 60000,
                    "serverInfo": {"name": "real-pass-stub", "version": "1.0"},
                }),
                "tools/list" => json!({"tools": [{
                    "name": "search",
                    "description": "search tool",
                    "inputSchema": {"type": "object"},
                }]}),
                "tools/call" => json!({
                    "content": [{"type": "text", "text": "pong"}],
                    "resultType": "complete",
                }),
                _ => json!({}),
            };
            axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": result}))
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

#[given(expr = "通过 API 创建指向 real 透传 stub 的 MCP server {string} 并配置共享 token")]
async fn given_server_with_token(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        return;
    }
    let url = spawn_stub().await;
    post_json(
        world,
        "/admin/mcp/servers",
        json!({
            "alias": alias,
            "display_name": format!("{alias} server"),
            "url": url,
            "credential_scope": "shared",
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
    world
        .created_keys
        .insert(format!("mcp-pass-real:id:{}", alias), id.to_string());

    post_json(
        world,
        &format!("/admin/mcp/servers/{id}/tokens"),
        json!({"token_name": "real-shared", "token_value": "sk-real-upstream", "is_default": true}),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "add token failed: {:?}",
        world.last_body
    );
    world
        .created_keys
        .insert(SERVER_ALIAS.to_string(), alias.clone());
}

#[given(expr = "真库 MCP 透传场景以未注册的 alias {string} 为准")]
async fn given_unknown_alias(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        return;
    }
    let client = client();
    let resp = client
        .get(format!("{}/admin/mcp/servers", base_url()))
        .query(&[("search", alias.as_str()), ("page_size", "10")])
        .header("Authorization", format!("Bearer {}", world.master_key))
        .send()
        .await
        .expect("list MCP servers");
    let listed: Value = resp.json().await.expect("list body");
    assert_eq!(
        listed["total_count"].as_i64(),
        Some(0),
        "the alias must not exist yet: {listed}"
    );
    world.created_keys.insert(SERVER_ALIAS.to_string(), alias);
}

#[given(expr = "真库 mcp_call_log 中该 alias 的行为 0")]
async fn given_no_rows(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let alias = alias_of(world);
    let rows = row_count(&format!(
        "SELECT call_id FROM mcp_call_log WHERE server_alias = '{}'",
        alias
    ))
    .await;
    assert_eq!(rows, 0, "a fresh alias must start with no audit rows");
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(regex = r#"^通过 API 对 "([^"]+)" 调用 MCP tools/call 且工具为 "([^"]+)"$"#)]
async fn when_tools_call(world: &mut TestWorld, alias: String, tool: String) {
    if !real_api_enabled() {
        set_skip_pass(
            world,
            200,
            json!({"result": {"content": [{"type": "text", "text": "pong"}]}}),
        );
        return;
    }
    post_json(
        world,
        "/mcp-rest/tools/call",
        json!({"alias": alias, "tool": tool, "arguments": {"q": "hi"}}),
    )
    .await;
}

#[when(regex = r#"^通过 API 对未注册的 alias "([^"]+)" 调用 MCP tools/call$"#)]
async fn when_tools_call_for_an_unknown_alias(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        set_skip_pass(world, 404, json!({"error": {"code": -32601}}));
        return;
    }
    post_json(
        world,
        "/mcp-rest/tools/call",
        json!({"alias": alias, "tool": "search", "arguments": {}}),
    )
    .await;
}

#[when(expr = "通过 API 对 {string} 发送现代握手")]
async fn when_discover(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        set_skip_pass(
            world,
            200,
            json!({"result": {"supportedVersions": ["2026-07-28"]}}),
        );
        return;
    }
    let mk = world.master_key.clone();
    let resp = client()
        .post(format!("{}/mcp/{}", base_url(), alias))
        .header("Authorization", format!("Bearer {}", mk))
        .header("Content-Type", "application/json")
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {}}))
        .send()
        .await
        .expect("POST /mcp/{alias}");
    world.last_status = Some(resp.status().as_u16());
    world.last_headers = Some(resp.headers().clone());
    world.last_body = resp.json().await.ok();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "真库 mcp_call_log 中该 alias 的行数为 {int}")]
async fn then_row_count(world: &mut TestWorld, expected: usize) {
    if !real_api_enabled() {
        return;
    }
    let alias = alias_of(world);
    let rows = row_count(&format!(
        "SELECT call_id FROM mcp_call_log WHERE server_alias = '{}'",
        alias
    ))
    .await;
    assert_eq!(rows, expected, "mcp_call_log rows for {alias}");
}

#[then(expr = "真库最近一行 mcp_call_log 的 method 为 {string}")]
async fn then_method(world: &mut TestWorld, expected: String) {
    if !real_api_enabled() {
        return;
    }
    let alias = alias_of(world);
    let method = scalar(&format!(
        "SELECT method FROM mcp_call_log WHERE server_alias = '{}' ORDER BY created_at DESC, call_id DESC",
        alias
    ))
    .await;
    assert_eq!(method, expected);
}

#[then(expr = "真库最近一行 mcp_call_log 的 status 为 {string}")]
async fn then_status(world: &mut TestWorld, expected: String) {
    if !real_api_enabled() {
        return;
    }
    let alias = alias_of(world);
    let status = scalar(&format!(
        "SELECT status FROM mcp_call_log WHERE server_alias = '{}' ORDER BY created_at DESC, call_id DESC",
        alias
    ))
    .await;
    assert_eq!(status, expected);
}

/// The counters are BIGINT on every dialect. A shared coercion that probed
/// `bool` first would turn a positive count into `true`; a column declared INT4
/// would fail PG decoding outright. Both show up here as something other than a
/// positive integer.
#[then(expr = "真库最近一行 mcp_call_log 的 request_bytes 为正整数")]
async fn then_request_bytes(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let alias = alias_of(world);
    let raw = scalar(&format!(
        "SELECT request_bytes FROM mcp_call_log WHERE server_alias = '{}' ORDER BY created_at DESC, call_id DESC",
        alias
    ))
    .await;
    let value: i64 = raw
        .parse()
        .unwrap_or_else(|_| panic!("request_bytes did not decode as an integer: {raw:?}"));
    assert!(value > 0, "the arguments must be measured, got {value}");
}

#[then(expr = "真库最近一行 mcp_call_log 的 call_id 与 x-call-id 响应头一致")]
async fn then_call_id_matches(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let alias = alias_of(world);
    let stored = scalar(&format!(
        "SELECT call_id FROM mcp_call_log WHERE server_alias = '{}' ORDER BY created_at DESC, call_id DESC",
        alias
    ))
    .await;
    let header = world
        .last_headers
        .as_ref()
        .and_then(|h| h.get("x-call-id"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert_eq!(stored, header, "the audit row must carry the call's own id");
}
