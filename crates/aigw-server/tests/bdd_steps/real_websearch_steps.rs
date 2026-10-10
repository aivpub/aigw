//! Step bindings for real/websearch_crud.feature — Stage 138 real BDD
//!
//! Drives the real aigw server over HTTP (like real_proxy_steps.rs) for the
//! `/admin/websearch/*` CRUD, plus a read-only DB probe that confirms the
//! instance `api_key` was encrypted at rest (`v2:gcm:`, no plaintext). The
//! probe is what actually proves the security property — the HTTP response
//! only proves the redaction.

use crate::TestWorld;
use cucumber::{given, then, when};
use serde_json::Value;

use super::real_api_steps::{base_url, client, real_api_enabled};

const GCM_PREFIX: &str = "v2:gcm:";

fn test_db_url() -> String {
    std::env::var("AIGW_TEST_DB_URL").expect("AIGW_TEST_DB_URL must be set by the BDD test harness")
}

fn store_provider_id(world: &mut TestWorld, name: &str, id: i64) {
    world
        .created_keys
        .insert(format!("ws-provider:{}", name), id.to_string());
    world
        .created_keys
        .insert("ws-provider:latest".to_string(), id.to_string());
}

fn provider_id(world: &TestWorld, name: &str) -> i64 {
    world
        .created_keys
        .get(&format!("ws-provider:{}", name))
        .unwrap_or_else(|| panic!("provider {} not created", name))
        .parse()
        .expect("provider id")
}

fn store_instance_id(world: &mut TestWorld, name: &str, id: i64) {
    world
        .created_keys
        .insert(format!("ws-instance:{}", name), id.to_string());
    world
        .created_keys
        .insert("ws-instance:latest".to_string(), id.to_string());
}

fn instance_id(world: &TestWorld, name: &str) -> i64 {
    world
        .created_keys
        .get(&format!("ws-instance:{}", name))
        .unwrap_or_else(|| panic!("instance {} not created", name))
        .parse()
        .expect("instance id")
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

/// Read one scalar column straight from the test DB.
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
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Number of rows a query returns.
///
/// Deliberately counts rows rather than reading `COUNT(*)`: on MySQL the shared
/// value coercion (`aigw_migrate::native`) tries `bool` before `i64`, and a
/// MySQL `BIGINT` is compatible with both — so `COUNT(*)` comes back as
/// `Value::Bool(false)` for zero. Selecting the ids sidesteps that entirely.
async fn row_count(sql: &str) -> usize {
    let pool = aigw_migrate::native::SourcePool::connect(&test_db_url())
        .await
        .expect("connect to test db");
    pool.read_rows_sql(sql).await.expect("read rows").len()
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "通过 API 创建搜索 provider {string}")]
async fn given_create_provider(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    post_json(
        world,
        "/admin/websearch/providers",
        serde_json::json!({"name": name, "kind": "searxng"}),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "create provider failed: {:?}",
        world.last_body
    );
    let id = world
        .last_body
        .as_ref()
        .and_then(|b| b.get("id"))
        .and_then(|v| v.as_i64())
        .expect("provider id");
    store_provider_id(world, &name, id);
}

/// Add an instance to the most recently created provider.
#[given(expr = "通过 API 为该 provider 添加实例 {string}")]
async fn given_add_instance(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    let pid: i64 = world
        .created_keys
        .get("ws-provider:latest")
        .expect("provider created first")
        .parse()
        .expect("provider id");
    post_json(
        world,
        &format!("/admin/websearch/providers/{}/instances", pid),
        serde_json::json!({"name": name, "base_url": "http://127.0.0.1:9099"}),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "add instance failed: {:?}",
        world.last_body
    );
    let id = world
        .last_body
        .as_ref()
        .and_then(|b| b.get("id"))
        .and_then(|v| v.as_i64())
        .expect("instance id");
    store_instance_id(world, &name, id);
}

#[when(expr = "通过 API 为该 provider 添加带 api_key {string} 的实例 {string}")]
async fn when_add_instance_with_key(world: &mut TestWorld, api_key: String, name: String) {
    if !real_api_enabled() {
        return;
    }
    let pid: i64 = world
        .created_keys
        .get("ws-provider:latest")
        .expect("provider created first")
        .parse()
        .expect("provider id");
    post_json(
        world,
        &format!("/admin/websearch/providers/{}/instances", pid),
        serde_json::json!({
            "name": name,
            "base_url": "http://127.0.0.1:9099",
            "api_key": api_key,
        }),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "add instance failed: {:?}",
        world.last_body
    );
    let id = world
        .last_body
        .as_ref()
        .and_then(|b| b.get("id"))
        .and_then(|v| v.as_i64())
        .expect("instance id");
    store_instance_id(world, &name, id);
}

/// Create a provider that is guaranteed to be the default (`failover_order`
/// far below every other row) — the shared real DB already holds providers
/// from earlier scenarios, so "the only row" is not an option here.
#[given(expr = "通过 API 创建搜索 provider {string} 且 failover_order 为 {int}")]
async fn given_create_default_provider(world: &mut TestWorld, name: String, order: i64) {
    if !real_api_enabled() {
        return;
    }
    post_json(
        world,
        "/admin/websearch/providers",
        serde_json::json!({"name": name, "kind": "searxng", "failover_order": order}),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "create provider failed: {:?}",
        world.last_body
    );
    let id = world
        .last_body
        .as_ref()
        .and_then(|b| b.get("id"))
        .and_then(|v| v.as_i64())
        .expect("provider id");
    store_provider_id(world, &name, id);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "通过 API 查询搜索 provider 列表")]
async fn when_list_providers(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    auth_get(world, "/admin/websearch/providers").await;
}

#[when(expr = "通过 API 更新搜索实例 {string} 名称为 {string}")]
async fn when_update_instance(world: &mut TestWorld, name: String, new_name: String) {
    if !real_api_enabled() {
        return;
    }
    let id = instance_id(world, &name);
    put_json(
        world,
        &format!("/admin/websearch/instances/{}", id),
        serde_json::json!({"name": new_name, "base_url": "http://127.0.0.1:9099"}),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "update instance failed: {:?}",
        world.last_body
    );
    world
        .created_keys
        .insert(format!("ws-instance:{}", new_name), id.to_string());
}

#[when(expr = "通过 API 删除搜索 provider {string}")]
async fn when_delete_provider(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    let id = provider_id(world, &name);
    delete_req(world, &format!("/admin/websearch/providers/{}", id)).await;
}

#[when(expr = "通过 API toggle 搜索 provider {string}")]
async fn when_toggle_provider(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    let id = provider_id(world, &name);
    post_json(
        world,
        &format!("/admin/websearch/providers/{}/toggle", id),
        serde_json::json!({}),
    )
    .await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "搜索 provider 列表包含 {string}")]
async fn then_provider_in_list(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    auth_get(world, "/admin/websearch/providers").await;
    let body = world.last_body.as_ref().expect("list body");
    let data = body["data"].as_array().expect("data array");
    assert!(
        data.iter()
            .any(|p| p["name"] == Value::String(name.clone())),
        "provider '{}' not in list: {}",
        name,
        serde_json::to_string_pretty(body).unwrap_or_default()
    );
}

#[then(expr = "搜索 provider 列表包含实例 {string}")]
async fn then_instance_in_list(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    auth_get(world, "/admin/websearch/providers").await;
    let body = world.last_body.as_ref().expect("list body");
    let flat = serde_json::to_string(body).unwrap_or_default();
    assert!(
        flat.contains(&name),
        "instance '{}' not present in provider list: {}",
        name,
        flat
    );
}

#[then(expr = "该 provider 列表项含实例 {string}")]
async fn then_provider_item_has_instance(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("list body");
    let data = body["data"].as_array().expect("data array");
    let found = data.iter().any(|p| {
        p["instances"]
            .as_array()
            .is_some_and(|a| a.iter().any(|i| i["name"] == Value::String(name.clone())))
    });
    assert!(
        found,
        "no provider carries instance '{}': {}",
        name,
        serde_json::to_string_pretty(body).unwrap_or_default()
    );
}

#[then(expr = "搜索实例响应 has_api_key 为 {word}")]
async fn then_has_api_key(world: &mut TestWorld, expected: bool) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no body");
    assert_eq!(
        body.get("has_api_key").and_then(|v| v.as_bool()),
        Some(expected),
        "unexpected has_api_key: {}",
        body
    );
}

#[then(expr = "搜索实例响应 redacted_api_key 以 {string} 开头")]
async fn then_redacted_prefix(world: &mut TestWorld, prefix: String) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no body");
    let redacted = body
        .get("redacted_api_key")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no redacted_api_key: {}", body));
    assert!(redacted.starts_with(&prefix), "got {redacted}");
}

#[then(expr = "实例 api_key 落库带 v2:gcm: 前缀且无明文")]
async fn then_key_encrypted_at_rest(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let id = world
        .created_keys
        .get("ws-instance:latest")
        .expect("instance created")
        .clone();
    let stored = scalar(&format!(
        "SELECT api_key FROM web_search_instances WHERE id = {}",
        id
    ))
    .await;
    assert!(
        stored.contains(GCM_PREFIX),
        "api_key is not encrypted at rest: {stored}"
    );
    assert!(
        !stored.contains("sk-real-secret-9f2c"),
        "plaintext api_key leaked into the DB: {stored}"
    );
}

#[then(expr = "real 搜索实例详情响应无明文 {string}")]
async fn then_detail_has_no_plaintext(world: &mut TestWorld, needle: String) {
    if !real_api_enabled() {
        return;
    }
    let id = world
        .created_keys
        .get("ws-instance:latest")
        .expect("instance created")
        .clone();
    auth_get(world, &format!("/admin/websearch/instances/{}", id)).await;
    let body = world.last_body.as_ref().expect("detail body");
    let flat = serde_json::to_string(body).unwrap_or_default();
    assert!(!flat.contains(&needle), "plaintext leaked: {flat}");
}

#[then(expr = "real 搜索 provider 删除返回 {int}")]
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

#[then(expr = "该 provider 的实例在库中已不存在")]
async fn then_instance_gone_from_db(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let _ = &world;
    let n = row_count("SELECT id FROM web_search_instances WHERE name = 'real-inst-cascade'").await;
    assert_eq!(n, 0, "instance survived its provider's deletion");
}

#[then(expr = "该 provider 在库中已不存在")]
async fn then_provider_gone_from_db(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let _ = &world;
    let n =
        row_count("SELECT id FROM web_search_providers WHERE name = 'real-searx-cascade'").await;
    assert_eq!(n, 0, "provider row survived deletion");
}

#[then(expr = "real 搜索 provider toggle 返回 {int}")]
async fn then_toggle_status(world: &mut TestWorld, expected: u16) {
    if !real_api_enabled() {
        return;
    }
    assert_eq!(
        world.last_status,
        Some(expected),
        "unexpected toggle status, body {:?}",
        world.last_body
    );
}
