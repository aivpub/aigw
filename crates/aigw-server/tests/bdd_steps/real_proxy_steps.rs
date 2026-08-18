//! Step bindings for real/proxy_crud.feature — Stage 125 real BDD
//!
//! These steps drive the real aigw server over HTTP (like real_api_steps.rs)
//! and exercise the /admin/proxies CRUD + toggle + in-use guard against the
//! real SQLite/PG/MySQL backend. Probe endpoints (test) are tolerated to return
//! 500 when the proxy is unreachable — the CRUD assertions are the focus.

use crate::TestWorld;
use cucumber::{given, then, when};
use serde_json::Value;

use super::real_api_steps::{base_url, client, real_api_enabled};

/// Track the most recently created proxy id per scenario (created_keys is the
/// shared per-scenario map).
fn store_proxy_id(world: &mut TestWorld, name: &str, id: i64) {
    world
        .created_keys
        .insert(format!("proxy:{}", name), id.to_string());
}

fn proxy_id(world: &mut TestWorld, name: &str) -> i64 {
    world
        .created_keys
        .get(&format!("proxy:{}", name))
        .expect("proxy not created yet")
        .parse()
        .expect("proxy id")
}

/// POST /admin/proxies — create a proxy, store its id.
async fn create_proxy_via_api(world: &mut TestWorld, name: &str, url: &str) -> i64 {
    let mk = world.master_key.clone();
    let body = serde_json::json!({ "name": name, "proxy_url": url });
    let resp = client()
        .post(format!("{}/admin/proxies", base_url()))
        .header("Authorization", format!("Bearer {}", mk))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("create proxy request");
    let status = resp.status().as_u16();
    let json: Value = resp.json().await.unwrap_or_default();
    assert_eq!(status, 200, "create proxy failed: {}", json);
    let id = json["id"].as_i64().expect("proxy id in response");
    store_proxy_id(world, name, id);
    id
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "通过 API 创建代理 {string} 使用 URL {string}")]
async fn given_create_proxy(world: &mut TestWorld, name: String, url: String) {
    if !real_api_enabled() {
        return;
    }
    create_proxy_via_api(world, &name, &url).await;
}

#[given(expr = "通过 API 创建凭证 {string} 引用该代理")]
async fn given_create_cred_referencing_proxy(world: &mut TestWorld, cred_name: String) {
    if !real_api_enabled() {
        return;
    }
    // Latest created proxy id — created_keys stores proxy:{name} → id string
    let id = world
        .created_keys
        .iter()
        .filter(|(k, _)| k.starts_with("proxy:"))
        .max_by_key(|(_, v)| v.parse::<i64>().unwrap_or(0))
        .map(|(_, v)| v.clone())
        .expect("proxy created first");
    let body = serde_json::json!({
        "credential_name": cred_name,
        "credential_values": { "proxy_id": id.parse::<i64>().unwrap() },
    });
    let mk = world.master_key.clone();
    let resp = client()
        .post(format!("{}/credential/new", base_url()))
        .header("Authorization", format!("Bearer {}", mk))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("create credential request");
    assert!(
        resp.status().is_success(),
        "create credential failed: {}",
        resp.text().await.unwrap_or_default()
    );
    // Store so cleanup can delete it
    world.created_keys.insert(cred_name, "cred".to_string());
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "通过 API 查询代理列表")]
async fn when_list_proxies(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let mk = world.master_key.clone();
    let resp = client()
        .get(format!("{}/admin/proxies", base_url()))
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("list proxies request");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "通过 API 更新代理 {string} 名称为 {string}")]
async fn when_update_proxy(world: &mut TestWorld, name: String, new_name: String) {
    if !real_api_enabled() {
        return;
    }
    let id = proxy_id(world, &name);
    let mk = world.master_key.clone();
    let resp = client()
        .put(format!("{}/admin/proxies/{}", base_url(), id))
        .header("Authorization", format!("Bearer {}", mk))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "name": new_name }))
        .send()
        .await
        .expect("update proxy request");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
    // Re-key so delete/other steps can reference the new name
    store_proxy_id(world, &new_name, id);
}

#[when(expr = "通过 API 删除代理 {string}")]
async fn when_delete_proxy(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    let id = proxy_id(world, &name);
    let mk = world.master_key.clone();
    let resp = client()
        .delete(format!("{}/admin/proxies/{}", base_url(), id))
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("delete proxy request");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "通过 API 触发出口检测 {string}")]
async fn when_test_proxy(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    let id = proxy_id(world, &name);
    let mk = world.master_key.clone();
    let resp = client()
        .post(format!("{}/admin/proxies/{}/test", base_url(), id))
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("test proxy request");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "通过 API toggle 代理 {string}")]
async fn when_toggle_proxy(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    let id = proxy_id(world, &name);
    let mk = world.master_key.clone();
    let resp = client()
        .post(format!("{}/admin/proxies/{}/toggle", base_url(), id))
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("toggle proxy request");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "代理列表包含 {string}")]
async fn then_proxy_in_list(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    // Fetch the list fresh — the preceding When step (create/update) left
    // last_body as a single proxy object, not a list.
    let mk = world.master_key.clone();
    let resp = client()
        .get(format!("{}/admin/proxies", base_url()))
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("list proxies request");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or_default();
    assert_eq!(
        status,
        200,
        "list proxies failed: {}",
        serde_json::to_string_pretty(&body).unwrap_or_default()
    );
    let data = body["data"].as_array().expect("data array");
    assert!(
        data.iter()
            .any(|p| p["name"] == Value::String(name.clone())),
        "proxy '{}' not in list: {}",
        name,
        serde_json::to_string_pretty(&body).unwrap_or_default()
    );
}

#[then(expr = "代理列表不包含 {string}")]
async fn then_proxy_not_in_list(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    let mk = world.master_key.clone();
    let resp = client()
        .get(format!("{}/admin/proxies", base_url()))
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("list proxies request");
    let body: Value = resp.json().await.unwrap_or_default();
    let data = body["data"].as_array().expect("data array");
    assert!(
        !data
            .iter()
            .any(|p| p["name"] == Value::String(name.clone())),
        "proxy '{}' should have been deleted but is still in list",
        name
    );
}

#[then(expr = "代理响应 proxy_url 已 redact 不包含明文密码")]
async fn then_proxy_url_redacted_real(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let mk = world.master_key.clone();
    let resp = client()
        .get(format!("{}/admin/proxies", base_url()))
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("list proxies request");
    let body: Value = resp.json().await.unwrap_or_default();
    let data = body["data"].as_array().expect("data array");
    let p = data
        .iter()
        .find(|p| p["name"].as_str() == Some("real-proxy-a"))
        .or_else(|| data.first())
        .expect("proxy in list");
    let url = p["proxy_url"].as_str().expect("proxy_url field");
    assert!(
        url.contains(":***@") || url == "[encrypted]",
        "proxy_url should be redacted, got: {}",
        url
    );
    assert!(
        !url.contains("secret"),
        "plaintext password leaked: {}",
        url
    );
}

#[then(expr = "real 代理删除返回 409 PROXY_IN_USE")]
async fn then_real_proxy_delete_409(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    assert_eq!(world.last_status, Some(409), "expected 409 PROXY_IN_USE");
    if let Some(body) = world.last_body.as_ref() {
        assert_eq!(body["error"]["type"], "PROXY_IN_USE");
    }
}

#[then(expr = "real 代理探测返回 200 或 500")]
async fn then_real_proxy_probe_200_or_500(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    match world.last_status {
        Some(200) | Some(500) => {}
        other => panic!("expected probe status 200 or 500, got {:?}", other),
    }
}

#[then(expr = "real 代理 toggle 返回 200")]
async fn then_real_proxy_toggle_200(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    assert_eq!(world.last_status, Some(200), "expected toggle 200");
}
