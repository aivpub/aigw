//! Step bindings for real/mcp_server_headers_real.feature — Stage 146 real BDD.
//!
//! Drives the running aigw server over HTTP so the PostgreSQL / MySQL branches of
//! [`aigw_core::db`] actually execute: `create_mcp_server` / `update_mcp_server`
//! bind twelve `?` (MySQL) or `$n` (PostgreSQL) parameters in a fixed order, and
//! the UPDATE is a whole-row SET — one missing column in one dialect silently
//! blanks a stored secret on every edit.
//!
//! The HTTP layer cannot show any of that: it only ever returns `***`. So the
//! assertions read the row directly against `AIGW_TEST_DB_URL` (same technique as
//! `real_mcp_steps.rs`) and check the two facts that matter — the columns hold
//! `v2:gcm:` ciphertext with no plaintext, and a partial update leaves them
//! alone.

use cucumber::{given, then, when};
use serde_json::{json, Value};

use super::real_api_steps::{base_url, client, real_api_enabled};
use crate::TestWorld;

const SECRET: &str = "tapd-secret";
const EXTRA_NAME: &str = "X-Project-Id";
const EXTRA_VALUE: &str = "12345";

fn server_id_key(alias: &str) -> String {
    format!("mcp-hdr-real:{}", alias)
}

fn store_server_id(world: &mut TestWorld, alias: &str, id: i64) {
    world
        .created_keys
        .insert(server_id_key(alias), id.to_string());
    world
        .created_keys
        .insert("mcp-hdr-real:latest".to_string(), id.to_string());
}

fn latest_server_id(world: &TestWorld) -> i64 {
    world
        .created_keys
        .get("mcp-hdr-real:latest")
        .expect("no MCP server created yet")
        .parse()
        .expect("server id")
}

fn test_db_url() -> String {
    std::env::var("AIGW_TEST_DB_URL").expect("AIGW_TEST_DB_URL must be set by the BDD harness")
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
    world.last_body = resp.json().await.ok();
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

fn body(world: &TestWorld) -> &Value {
    world.last_body.as_ref().expect("no response body")
}

/// `test-db-up` creates one throwaway database per run and the server is started
/// against it, so the alias can be taken as-is; the scenarios still use distinct
/// aliases so a rerun against a kept DB (`AIGW_TEST_KEEP_DB=1`) cannot collide.
async fn create_custom_header_server(world: &mut TestWorld, alias: &str) {
    post_json(
        world,
        "/admin/mcp/servers",
        json!({
            "alias": alias,
            "display_name": format!("{alias} server"),
            "url": "http://127.0.0.1:1/mcp",
            "auth_header_name": "X-Tapd-Access-Token",
            "auth_header_value": SECRET,
            "extra_headers": [{"name": EXTRA_NAME, "value": EXTRA_VALUE}],
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
    store_server_id(world, alias, id);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "通过 API 创建带自定义头的 MCP server {string}")]
async fn given_custom_header_server(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        return;
    }
    create_custom_header_server(world, &alias).await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "通过 API 创建带自定义头的 MCP server {string}")]
async fn when_create_custom_header_server(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        return;
    }
    create_custom_header_server(world, &alias).await;
}

#[when(expr = "通过 API 只更新该 MCP server 的显示名")]
async fn when_rename(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    put_json(
        world,
        &format!("/admin/mcp/servers/{}", id),
        json!({"display_name": "renamed server"}),
    )
    .await;
}

#[when(expr = "通过 API 用空认证头名与空额外头更新该 MCP server")]
async fn when_clear(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    put_json(
        world,
        &format!("/admin/mcp/servers/{}", id),
        json!({"auth_header_name": "", "extra_headers": []}),
    )
    .await;
}

#[when(expr = "通过 API 按 id 读取该 MCP server")]
async fn when_read(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    auth_get(world, &format!("/admin/mcp/servers/{}", id)).await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "real MCP 更新返回 {int}")]
async fn then_update_status(world: &mut TestWorld, expected: u16) {
    if !real_api_enabled() {
        return;
    }
    assert_eq!(
        world.last_status,
        Some(expected),
        "unexpected update status, body {:?}",
        world.last_body
    );
}

#[then(expr = "real MCP 读取返回 {int}")]
async fn then_read_status(world: &mut TestWorld, expected: u16) {
    if !real_api_enabled() {
        return;
    }
    assert_eq!(
        world.last_status,
        Some(expected),
        "unexpected read status, body {:?}",
        world.last_body
    );
}

#[then(expr = "real 库中该 server 的认证头密文以 v2:gcm: 开头且不含明文")]
async fn then_auth_value_sealed(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    let value = scalar(&format!(
        "SELECT auth_header_value FROM mcp_servers WHERE id = {}",
        id
    ))
    .await;
    assert!(
        value.starts_with("v2:gcm:"),
        "auth_header_value is not sealed: {value}"
    );
    assert!(
        !value.contains(SECRET),
        "plaintext leaked into the column: {value}"
    );
}

#[then(expr = "real 库中该 server 的额外头密文以 v2:gcm: 开头且不含明文")]
async fn then_extras_sealed(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    let value = scalar(&format!(
        "SELECT extra_headers FROM mcp_servers WHERE id = {}",
        id
    ))
    .await;
    assert!(
        value.starts_with("v2:gcm:"),
        "extra_headers is not sealed: {value}"
    );
    assert!(
        !value.contains(EXTRA_VALUE) && !value.contains(EXTRA_NAME),
        "plaintext leaked into the column: {value}"
    );
}

#[then(expr = "real 库中该 server 的认证头值解密后仍为 {string}")]
async fn then_auth_value_survived(world: &mut TestWorld, expected: String) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    let name = scalar(&format!(
        "SELECT auth_header_name FROM mcp_servers WHERE id = {}",
        id
    ))
    .await;
    assert_eq!(name, "X-Tapd-Access-Token", "the name survived the rename");
    let sealed = scalar(&format!(
        "SELECT auth_header_value FROM mcp_servers WHERE id = {}",
        id
    ))
    .await;
    assert!(
        sealed.starts_with("v2:gcm:"),
        "the value must still be the ciphertext: {sealed}"
    );
    let plain = aigw_core::mcp::credential::unseal_token(&sealed, &world.master_key)
        .expect("the stored value must still decrypt with the server's master key");
    assert_eq!(plain, expected, "a partial update wiped the credential");
}

#[then(expr = "real 库中该 server 的额外头仍含 {string}")]
async fn then_extras_survived(world: &mut TestWorld, expected: String) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    let sealed = scalar(&format!(
        "SELECT extra_headers FROM mcp_servers WHERE id = {}",
        id
    ))
    .await;
    assert!(sealed.starts_with("v2:gcm:"), "extras: {sealed}");
    let headers = aigw_core::mcp::headers::decrypt_extra_headers(Some(&sealed), &world.master_key);
    assert!(
        headers.iter().any(|h| h.name == expected),
        "a partial update dropped the extra header {expected}: {headers:?}"
    );
}

#[then(expr = "real 库中该 server 的认证头与额外头均为 null")]
async fn then_both_null(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let id = latest_server_id(world);
    for column in ["auth_header_name", "auth_header_value", "extra_headers"] {
        let value = scalar(&format!(
            "SELECT {} FROM mcp_servers WHERE id = {}",
            column, id
        ))
        .await;
        assert!(
            value.is_empty(),
            "{column} should be NULL after clearing, got {value:?}"
        );
    }
}

#[then(expr = "real MCP 响应里认证头值为 {string} 且不含明文")]
async fn then_response_redacted(world: &mut TestWorld, expected: String) {
    if !real_api_enabled() {
        return;
    }
    let response = body(world);
    assert_eq!(
        response["auth_header_value"],
        json!(expected),
        "value must come back redacted: {response}"
    );
    assert_eq!(
        response["extra_headers"][0]["value"],
        json!(expected),
        "extra values must come back redacted: {response}"
    );
    let text = response.to_string();
    assert!(!text.contains(SECRET), "plaintext leaked: {text}");
    assert!(!text.contains("v2:gcm:"), "ciphertext leaked: {text}");
}
