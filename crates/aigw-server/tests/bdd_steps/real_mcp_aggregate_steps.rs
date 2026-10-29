//! Step bindings for real/mcp_aggregate_real.feature — Stage 144 real BDD
//!
//! Drives the running aigw server over HTTP so the PostgreSQL / MySQL branches of
//! [`aigw_core::db`] actually execute: `replace_mcp_tool_allowlist` is a
//! DELETE-then-INSERT inside one transaction (`?` rebinding is repeated on
//! MySQL, `$n` on PostgreSQL, and `allowed` decodes from INTEGER / TINYINT(1) /
//! BOOLEAN), and `count_mcp_call_log` builds a five-predicate WHERE whose
//! placeholders differ per dialect. None of that is visible from the SQLite mock
//! run.
//!
//! A read-only probe against `AIGW_TEST_DB_URL` is what proves the storage facts
//! the HTTP layer cannot show: that "whole-set replacement" really removes the
//! previous rule, and that the failed/true decoding of `allowed` survives.

use cucumber::{given, then, when};
use serde_json::{json, Value};

use super::real_api_steps::{base_url, client, real_api_enabled, set_skip_pass};
use crate::TestWorld;

const STUB_URL: &str = "mcp-agg-real:stub";
const LAST_RULE: &str = "mcp-agg-real:last-rule";

fn server_id_key(alias: &str) -> String {
    format!("mcp-agg-real:id:{}", alias)
}

fn latest_alias(world: &TestWorld) -> String {
    world
        .created_keys
        .get("mcp-agg-real:latest")
        .cloned()
        .expect("no aggregate server created yet")
}

fn latest_server_id(world: &TestWorld) -> i64 {
    let alias = latest_alias(world);
    world
        .created_keys
        .get(&server_id_key(&alias))
        .unwrap_or_else(|| panic!("aggregate server {alias} id missing"))
        .parse()
        .expect("server id")
}

fn test_db_url() -> String {
    std::env::var("AIGW_TEST_DB_URL").expect("AIGW_TEST_DB_URL must be set by the BDD harness")
}

async fn row_count(sql: &str) -> usize {
    let pool = aigw_migrate::native::SourcePool::connect(&test_db_url())
        .await
        .expect("connect to test db");
    pool.read_rows_sql(sql).await.expect("read rows").len()
}

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

async fn put_json(world: &mut TestWorld, path: &str, body: Value) {
    let mk = world.master_key.clone();
    let resp = client()
        .put(format!("{}{}", base_url(), path))
        .header("Authorization", format!("Bearer {}", mk))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("PUT request");
    world.last_status = Some(resp.status().as_u16());
    world.last_headers = Some(resp.headers().clone());
    world.last_body = resp.json().await.ok();
}

fn body(world: &TestWorld) -> &Value {
    world.last_body.as_ref().expect("no response body")
}

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
                    "serverInfo": {"name": "real-agg-stub", "version": "1.0"},
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

#[given(expr = "通过 API 创建聚合场景的 MCP server {string}")]
async fn given_plain_server(world: &mut TestWorld, alias: String) {
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
        .insert(server_id_key(&alias), id.to_string());
    world
        .created_keys
        .insert("mcp-agg-real:latest".to_string(), alias.clone());
}

#[given(expr = "已启动 real 聚合 stub，提供工具 {string}")]
async fn given_stub(world: &mut TestWorld, tool: String) {
    if !real_api_enabled() {
        return;
    }
    assert_eq!(tool, "search", "the stub publishes one tool named search");
    world
        .created_keys
        .insert(STUB_URL.to_string(), spawn_stub().await);
}

#[given(expr = "通过 API 创建指向该 stub 的聚合 server {string}")]
async fn given_server_at_stub(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        return;
    }
    let url = world
        .created_keys
        .get(STUB_URL)
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
    world
        .created_keys
        .insert(server_id_key(&alias), id.to_string());
    world
        .created_keys
        .insert("mcp-agg-real:latest".to_string(), alias.clone());
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn replace_allowlist(world: &mut TestWorld, rules: Value) {
    let id = latest_server_id(world);
    put_json(
        world,
        &format!("/admin/mcp/servers/{id}/allowlist"),
        json!({"rules": rules}),
    )
    .await;
}

#[when(expr = "通过 API 把该 server 的 allowlist 整表替换为一条禁止 key 规则")]
async fn when_deny_key(world: &mut TestWorld) {
    world
        .created_keys
        .insert(LAST_RULE.to_string(), "key".to_string());
    if !real_api_enabled() {
        set_skip_pass(
            world,
            200,
            json!({"object": "list", "count": 1, "data": [{"scope_type": "key", "allowed": false}]}),
        );
        return;
    }
    replace_allowlist(
        world,
        json!([{
            "tool_name": "search",
            "scope_type": "key",
            "scope_id": "hash-of-a-key",
            "allowed": false,
        }]),
    )
    .await;
}

#[when(expr = "通过 API 把该 server 的 allowlist 整表替换为一条团队允许规则")]
async fn when_allow_team(world: &mut TestWorld) {
    world
        .created_keys
        .insert(LAST_RULE.to_string(), "team".to_string());
    if !real_api_enabled() {
        set_skip_pass(
            world,
            200,
            json!({"object": "list", "count": 1, "data": [{"scope_type": "team", "allowed": true}]}),
        );
        return;
    }
    replace_allowlist(
        world,
        json!([{
            "tool_name": "search",
            "scope_type": "team",
            "scope_id": "team-1",
            "allowed": true,
        }]),
    )
    .await;
}

#[when(expr = "通过 API 读取该 server 的 allowlist")]
async fn when_read_allowlist(world: &mut TestWorld) {
    if !real_api_enabled() {
        let rule = if world.created_keys.get(LAST_RULE).map(String::as_str) == Some("team") {
            json!({"scope_type": "team", "allowed": true})
        } else {
            json!({"scope_type": "key", "allowed": false})
        };
        set_skip_pass(
            world,
            200,
            json!({"object": "list", "count": 1, "data": [rule]}),
        );
        return;
    }
    let id = latest_server_id(world);
    let mk = world.master_key.clone();
    let resp = client()
        .get(format!("{}/admin/mcp/servers/{}/allowlist", base_url(), id))
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("GET allowlist");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "通过 API 经聚合端点调用 {string} 的工具 {string}")]
async fn when_aggregate_call(world: &mut TestWorld, alias: String, tool: String) {
    world
        .created_keys
        .insert("mcp-agg-real:latest".to_string(), alias.clone());
    if !real_api_enabled() {
        set_skip_pass(world, 200, json!({"result": {"content": []}}));
        return;
    }
    let mk = world.master_key.clone();
    let resp = client()
        .post(format!("{}/mcp", base_url()))
        .header("Authorization", format!("Bearer {}", mk))
        .header("Content-Type", "application/json")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": format!("{alias}__{tool}"), "arguments": {}},
        }))
        .send()
        .await
        .expect("POST /mcp");
    world.last_status = Some(resp.status().as_u16());
    world.last_headers = Some(resp.headers().clone());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "按该 alias 查询 real 调用日志")]
async fn when_query_calls(world: &mut TestWorld) {
    let alias = latest_alias(world);
    if !real_api_enabled() {
        set_skip_pass(
            world,
            200,
            json!({
                "object": "list",
                "total_count": 1,
                "data": [{"server_alias": alias, "method": "tools/call"}],
            }),
        );
        return;
    }
    let mk = world.master_key.clone();
    let resp = client()
        .get(format!("{}/admin/mcp/calls", base_url()))
        .query(&[("server_alias", alias.as_str())])
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("GET /admin/mcp/calls");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn server_id_sql(world: &TestWorld) -> i64 {
    if real_api_enabled() {
        latest_server_id(world)
    } else {
        1
    }
}

#[then(expr = "真库该 server 的 allowlist 行数为 {int}")]
async fn then_allowlist_rows(world: &mut TestWorld, expected: usize) {
    if !real_api_enabled() {
        return;
    }
    let id = server_id_sql(world).await;
    let rows = row_count(&format!(
        "SELECT tool_name FROM mcp_tool_allowlist WHERE server_id = {}",
        id
    ))
    .await;
    assert_eq!(rows, expected, "mcp_tool_allowlist rows for server {id}");
}

/// `allowed` is INTEGER on SQLite, TINYINT(1) on MySQL and BOOLEAN on
/// PostgreSQL. The shared coercion decodes a zero/one bigint as a bool, so the
/// assertion compares the rendered scalar rather than the raw value.
#[then(expr = "真库该 server 的 allowlist 首行 allowed 为 {int}")]
async fn then_allowlist_allowed(world: &mut TestWorld, expected: i64) {
    if !real_api_enabled() {
        return;
    }
    let id = server_id_sql(world).await;
    let raw = scalar(&format!(
        "SELECT allowed FROM mcp_tool_allowlist WHERE server_id = {}",
        id
    ))
    .await;
    let normalized = if raw == "true" {
        1
    } else if raw == "false" {
        0
    } else {
        raw.parse::<i64>()
            .unwrap_or_else(|_| panic!("allowed did not decode as a flag: {raw:?}"))
    };
    assert_eq!(normalized, expected);
}

#[then(expr = "allowlist 响应 count 为 {int}")]
async fn then_allowlist_count(world: &mut TestWorld, expected: i64) {
    assert_eq!(
        body(world)["count"].as_i64(),
        Some(expected),
        "{}",
        body(world)
    );
}

#[then(expr = "allowlist 响应首行 scope_type 为 {string}")]
async fn then_allowlist_scope(world: &mut TestWorld, expected: String) {
    assert_eq!(
        body(world)["data"][0]["scope_type"].as_str(),
        Some(expected.as_str()),
        "{}",
        body(world)
    );
}

#[then(expr = "allowlist 响应首行 allowed 为 {word}")]
async fn then_allowlist_allowed_wire(world: &mut TestWorld, expected: String) {
    assert_eq!(
        body(world)["data"][0]["allowed"].as_bool(),
        Some(expected == "true"),
        "{}",
        body(world)
    );
}

#[then(expr = "真库该 alias 的 mcp_call_log 行数为 {int}")]
async fn then_call_rows(world: &mut TestWorld, expected: usize) {
    if !real_api_enabled() {
        return;
    }
    let alias = latest_alias(world);
    let rows = row_count(&format!(
        "SELECT call_id FROM mcp_call_log WHERE server_alias = '{}'",
        alias
    ))
    .await;
    assert_eq!(rows, expected, "mcp_call_log rows for {alias}");
}

#[then(expr = "真库该 alias 最近一行 method 为 {string}")]
async fn then_call_method(world: &mut TestWorld, expected: String) {
    if !real_api_enabled() {
        return;
    }
    let alias = latest_alias(world);
    let method = scalar(&format!(
        "SELECT method FROM mcp_call_log WHERE server_alias = '{}' ORDER BY created_at DESC, call_id DESC",
        alias
    ))
    .await;
    assert_eq!(method, expected);
}

#[then(expr = "real 调用日志 total_count 为 {int}")]
async fn then_calls_total(world: &mut TestWorld, expected: i64) {
    assert_eq!(
        body(world)["total_count"].as_i64(),
        Some(expected),
        "{}",
        body(world)
    );
}

#[then(expr = "real 调用日志首行 server_alias 为 {string}")]
async fn then_calls_alias(world: &mut TestWorld, expected: String) {
    assert_eq!(
        body(world)["data"][0]["server_alias"].as_str(),
        Some(expected.as_str()),
        "{}",
        body(world)
    );
}
