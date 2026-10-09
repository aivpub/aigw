//! Step bindings for websearch_admin.feature — Stage 138 /admin/websearch/* CRUD
//!
//! Mirrors `proxy_steps.rs`: an in-memory SQLite test DB behind a router built
//! for these endpoints only, with `aigw_master_key` set so instance api_key
//! encryption/redaction round-trips through the HTTP handlers.
//!
//! Step text is deliberately distinct from `proxy_steps.rs` where the behaviour
//! differs (different actor, different table); shared steps such as
//! `管理员已认证` / `数据库已初始化且已配置 master key` / `响应状态码为 {int}` /
//! `响应错误 type 为 {string}` are reused, never redefined.

use axum::http::Method;
use axum::Router;
use cucumber::{given, then, when};
use tower::ServiceExt;

use crate::TestWorld;

fn build_websearch_router(state: aigw_server::routes::keys::SharedState) -> Router {
    use aigw_server::routes::websearch as ws;
    Router::new()
        .route(
            "/admin/websearch/providers",
            axum::routing::get(ws::list_providers).post(ws::create_provider),
        )
        .route(
            "/admin/websearch/providers/batch-delete",
            axum::routing::post(ws::batch_delete_providers),
        )
        .route(
            "/admin/websearch/providers/{id}",
            axum::routing::get(ws::get_provider)
                .put(ws::update_provider)
                .delete(ws::delete_provider),
        )
        .route(
            "/admin/websearch/providers/{id}/toggle",
            axum::routing::post(ws::toggle_provider),
        )
        .route(
            "/admin/websearch/providers/{id}/instances",
            axum::routing::get(ws::list_instances).post(ws::create_instance),
        )
        .route(
            "/admin/websearch/instances/{id}",
            axum::routing::get(ws::get_instance)
                .put(ws::update_instance)
                .delete(ws::delete_instance),
        )
        .route(
            "/admin/websearch/instances/{id}/toggle",
            axum::routing::post(ws::toggle_instance),
        )
        .route(
            "/admin/websearch/instances/{id}/test",
            axum::routing::post(ws::test_instance),
        )
        .route("/admin/websearch/reload", axum::routing::post(ws::reload))
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
    let app = build_websearch_router(state);
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

/// Ids live under reserved `ws-provider:…` / `ws-instance:…` keys in
/// `created_keys` (the map is per-scenario, so entries do not leak).
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

fn latest_provider_id(world: &TestWorld) -> i64 {
    world
        .created_keys
        .get("ws-provider:latest")
        .expect("no provider created yet")
        .parse()
        .expect("provider id")
}

fn store_instance_id(world: &mut TestWorld, id: i64) {
    world
        .created_keys
        .insert("ws-instance:latest".to_string(), id.to_string());
}

fn latest_instance_id(world: &TestWorld) -> i64 {
    world
        .created_keys
        .get("ws-instance:latest")
        .expect("no instance created yet")
        .parse()
        .expect("instance id")
}

// `数据库已初始化且已配置 master key` lives in `proxy_steps.rs` and swaps in a
// state carrying `aigw_master_key` — exactly what the api_key seal/redact path
// here needs, so it is reused rather than redefined (a second binding panics).

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "已通过 API 创建搜索 provider {string}")]
async fn given_provider_via_api(world: &mut TestWorld, name: String) {
    let body = serde_json::json!({"name": name, "kind": "searxng"}).to_string();
    send_request(
        world,
        Method::POST,
        "/admin/websearch/providers",
        Some(&world.master_key.clone()),
        Some(&body),
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

#[given(expr = "已为该 provider 创建实例 {string}")]
async fn given_instance_via_api(world: &mut TestWorld, name: String) {
    let pid = latest_provider_id(world);
    let body = serde_json::json!({"name": name, "base_url": "http://127.0.0.1:9099"}).to_string();
    send_request(
        world,
        Method::POST,
        &format!("/admin/websearch/providers/{}/instances", pid),
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "create instance failed: {:?}",
        world.last_body
    );
    let id = world
        .last_body
        .as_ref()
        .and_then(|b| b.get("id"))
        .and_then(|v| v.as_i64())
        .expect("instance id");
    store_instance_id(world, id);
}

#[given(expr = "已为该 provider 创建带 api_key 的实例")]
async fn given_instance_with_key(world: &mut TestWorld) {
    let pid = latest_provider_id(world);
    let body = serde_json::json!({
        "name": "inst-key",
        "base_url": "http://127.0.0.1:9099",
        "api_key": "sk-secret-abcdef",
    })
    .to_string();
    send_request(
        world,
        Method::POST,
        &format!("/admin/websearch/providers/{}/instances", pid),
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "create instance failed: {:?}",
        world.last_body
    );
    let id = world
        .last_body
        .as_ref()
        .and_then(|b| b.get("id"))
        .and_then(|v| v.as_i64())
        .expect("instance id");
    store_instance_id(world, id);
}

#[given(expr = "数据库直插一个 kind 为 {string} 的搜索 provider 行")]
async fn given_bad_kind_row(world: &mut TestWorld, kind: String) {
    let state = world.ensure_state().await;
    let now = chrono::Utc::now().to_rfc3339();
    let name = format!("ws-bad-kind-{}", uuid::Uuid::new_v4());
    state
        .db
        .create_web_search_provider(&aigw_core::models::WebSearchProviderRow {
            id: 0,
            name,
            kind,
            cost_per_query: 0.01,
            enabled: 1,
            failover_order: None,
            timeout_ms: None,
            created_at: now.clone(),
            updated_at: now,
        })
        .await
        .expect("insert bad-kind provider row");
}

#[given(expr = "已生成普通 key {string}（搜索场景）")]
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
        models: serde_json::json!([]),
        aliases: serde_json::json!({}),
        config: serde_json::json!({}),
        router_settings: None,
        user_id: None,
        team_id: None,
        agent_id: None,
        project_id: None,
        permissions: serde_json::json!({}),
        max_parallel_requests: None,
        metadata: serde_json::json!({}),
        blocked: None,
        tpm_limit: None,
        rpm_limit: None,
        max_budget: None,
        soft_budget: None,
        budget_duration: None,
        budget_reset_at: None,
        allowed_cache_controls: serde_json::json!([]),
        allowed_routes: serde_json::json!([]),
        policies: serde_json::json!([]),
        access_group_ids: serde_json::json!([]),
        model_spend: serde_json::json!({}),
        model_max_budget: serde_json::json!({}),
        budget_id: None,
        organization_id: None,
        object_permission_id: None,
        created_at: Some(chrono::Utc::now()),
        created_by: Some("test".to_string()),
        updated_at: Some(chrono::Utc::now()),
        updated_by: Some("test".to_string()),
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

#[when(expr = "发送 POST \\/admin\\/websearch\\/providers 请求创建 provider")]
async fn when_create_provider(world: &mut TestWorld, step: &cucumber::gherkin::Step) {
    let body = step.docstring.as_ref().expect("docstring body").to_string();
    send_request(
        world,
        Method::POST,
        "/admin/websearch/providers",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    if let Some(id) = world
        .last_body
        .as_ref()
        .and_then(|b| b.get("id"))
        .and_then(|v| v.as_i64())
    {
        store_provider_id(world, &format!("id-{}", id), id);
    }
}

#[when(expr = "发送 GET \\/admin\\/websearch\\/providers 请求")]
async fn when_list_providers(world: &mut TestWorld) {
    send_request(
        world,
        Method::GET,
        "/admin/websearch/providers",
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "使用普通 key {string} 发送 GET \\/admin\\/websearch\\/providers 请求")]
async fn when_list_providers_with_key(world: &mut TestWorld, alias: String) {
    let token = world.created_keys.get(&alias).expect("key").clone();
    send_request(
        world,
        Method::GET,
        "/admin/websearch/providers",
        Some(&token),
        None,
    )
    .await;
}

#[when(expr = "发送 POST \\/admin\\/websearch\\/providers\\/\\{id\\}\\/instances 请求添加实例")]
async fn when_create_instance(world: &mut TestWorld, step: &cucumber::gherkin::Step) {
    let pid = latest_provider_id(world);
    let body = step.docstring.as_ref().expect("docstring body").to_string();
    send_request(
        world,
        Method::POST,
        &format!("/admin/websearch/providers/{}/instances", pid),
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    if let Some(id) = world
        .last_body
        .as_ref()
        .and_then(|b| b.get("id"))
        .and_then(|v| v.as_i64())
    {
        store_instance_id(world, id);
    }
}

#[when(expr = "发送 PUT \\/admin\\/websearch\\/instances\\/\\{id\\} 请求更新实例名称为 {string}")]
async fn when_update_instance(world: &mut TestWorld, name: String) {
    let id = latest_instance_id(world);
    let body = serde_json::json!({
        "name": name,
        "base_url": "http://127.0.0.1:9099",
    })
    .to_string();
    send_request(
        world,
        Method::PUT,
        &format!("/admin/websearch/instances/{}", id),
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
}

#[when(expr = "发送 DELETE \\/admin\\/websearch\\/providers\\/\\{id\\} 请求")]
async fn when_delete_provider(world: &mut TestWorld) {
    let id = latest_provider_id(world);
    send_request(
        world,
        Method::DELETE,
        &format!("/admin/websearch/providers/{}", id),
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "发送 POST \\/admin\\/websearch\\/providers\\/batch-delete 请求删除两个 provider")]
async fn when_batch_delete(world: &mut TestWorld) {
    let a = provider_id(world, "searx-first");
    let b = provider_id(world, "searx-second");
    let body = serde_json::json!({"ids": [a, b]}).to_string();
    send_request(
        world,
        Method::POST,
        "/admin/websearch/providers/batch-delete",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
}

#[when(expr = "发送 POST \\/admin\\/websearch\\/providers\\/\\{id\\}\\/toggle 请求")]
async fn when_toggle_provider(world: &mut TestWorld) {
    let id = latest_provider_id(world);
    send_request(
        world,
        Method::POST,
        &format!("/admin/websearch/providers/{}/toggle", id),
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "发送 POST \\/admin\\/websearch\\/instances\\/\\{id\\}\\/test 请求")]
async fn when_test_instance(world: &mut TestWorld) {
    let id = latest_instance_id(world);
    send_request(
        world,
        Method::POST,
        &format!("/admin/websearch/instances/{}/test", id),
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "发送 PUT \\/admin\\/websearch\\/providers\\/\\{id\\} 请求把单价改为 {float}")]
async fn when_update_price(world: &mut TestWorld, cost: f64) {
    let id = latest_provider_id(world);
    let body = serde_json::json!({"cost_per_query": cost}).to_string();
    send_request(
        world,
        Method::PUT,
        &format!("/admin/websearch/providers/{}", id),
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
}

#[when(expr = "发送 POST \\/admin\\/websearch\\/reload 请求")]
async fn when_reload(world: &mut TestWorld) {
    send_request(
        world,
        Method::POST,
        "/admin/websearch/reload",
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "搜索 provider 单价为 {float}")]
async fn then_provider_cost(world: &mut TestWorld, expected: f64) {
    let body = world.last_body.as_ref().expect("no response body");
    let got = body
        .get("cost_per_query")
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| panic!("no cost_per_query: {}", body));
    assert!(
        (got - expected).abs() < 1e-12,
        "cost_per_query {got} != {expected}"
    );
}

#[then(expr = "搜索 provider 为默认")]
async fn then_provider_is_default(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    assert_eq!(
        body.get("is_default").and_then(|v| v.as_bool()),
        Some(true),
        "only provider must be the default: {}",
        body
    );
}

#[then(expr = "搜索 provider enabled 为 {word}")]
async fn then_provider_enabled(world: &mut TestWorld, expected: bool) {
    let body = world.last_body.as_ref().expect("no response body");
    assert_eq!(
        body.get("enabled").and_then(|v| v.as_bool()),
        Some(expected),
        "unexpected enabled: {}",
        body
    );
}

#[then(expr = "列表首个 provider 的实例数为 {int}")]
async fn then_first_provider_instance_count(world: &mut TestWorld, expected: usize) {
    let body = world.last_body.as_ref().expect("no response body");
    let n = body
        .get("data")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|p| p.get("instances"))
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or_else(|| panic!("no data[0].instances: {}", body));
    assert_eq!(n, expected);
}

#[then(expr = "搜索实例 has_api_key 为 {word}")]
async fn then_instance_has_api_key(world: &mut TestWorld, expected: bool) {
    let body = world.last_body.as_ref().expect("no response body");
    assert_eq!(
        body.get("has_api_key").and_then(|v| v.as_bool()),
        Some(expected),
        "unexpected has_api_key: {}",
        body
    );
}

#[then(expr = "搜索实例 redacted_api_key 以 {string} 开头")]
async fn then_instance_redacted_prefix(world: &mut TestWorld, prefix: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let redacted = body
        .get("redacted_api_key")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no redacted_api_key: {}", body));
    assert!(
        redacted.starts_with(&prefix),
        "redacted_api_key {redacted} does not start with {prefix}"
    );
}

#[then(expr = "响应 JSON 不含明文 {string}")]
async fn then_body_has_no_plaintext(world: &mut TestWorld, needle: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let text = serde_json::to_string(body).expect("serialize body");
    assert!(
        !text.contains(&needle),
        "plaintext secret {needle} leaked in response: {text}"
    );
}

#[then(expr = "搜索实例落库 api_key 带 v2:gcm: 前缀")]
async fn then_stored_key_is_encrypted(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let id = latest_instance_id(world);
    let row = state
        .db
        .get_web_search_instance(id)
        .await
        .expect("get instance")
        .expect("instance exists");
    let stored = row.api_key.expect("api_key stored");
    assert!(
        stored.starts_with("v2:gcm:"),
        "stored key is not encrypted: {stored}"
    );
    assert!(
        !stored.contains("sk-secret-abcdef"),
        "stored key contains plaintext: {stored}"
    );
}

#[then(expr = "搜索实例落库 api_key 与创建后一致")]
async fn then_stored_key_unchanged(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let id = latest_instance_id(world);
    let row = state
        .db
        .get_web_search_instance(id)
        .await
        .expect("get instance")
        .expect("instance exists");
    let stored = row.api_key.expect("api_key still stored");
    assert!(
        stored.starts_with("v2:gcm:") && !stored.contains("sk-secret-abcdef"),
        "stored key changed shape after update: {stored}"
    );
}

#[then(expr = "响应错误 message 含 {string}")]
async fn then_error_message_contains(world: &mut TestWorld, needle: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let msg = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no error.message: {}", body));
    assert!(
        msg.contains(&needle),
        "error message {msg} does not contain {needle}"
    );
}

#[then(expr = "批量删除结果中跳过 {int} 个 provider")]
async fn then_batch_skipped_count(world: &mut TestWorld, expected: usize) {
    let body = world.last_body.as_ref().expect("no response body");
    let n = body
        .get("skipped")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or_else(|| panic!("no skipped array: {}", body));
    assert_eq!(n, expected);
}

#[then(expr = "该 provider 的实例数在数据库中为 {int}")]
async fn then_provider_instance_count_in_db(world: &mut TestWorld, expected: usize) {
    let state = world.ensure_state().await;
    let pid = latest_provider_id(world);
    let rows = state
        .db
        .list_web_search_instances(pid)
        .await
        .expect("list instances");
    assert_eq!(rows.len(), expected);
}

#[then(expr = "探测结果 ok 为 {word}")]
async fn then_probe_ok(world: &mut TestWorld, expected: bool) {
    let body = world.last_body.as_ref().expect("no response body");
    assert_eq!(
        body.get("ok").and_then(|v| v.as_bool()),
        Some(expected),
        "unexpected probe ok: {}",
        body
    );
}

#[then(expr = "该实例的 probe_result 快照非空")]
async fn then_probe_snapshot_written(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let id = latest_instance_id(world);
    let row = state
        .db
        .get_web_search_instance(id)
        .await
        .expect("get instance")
        .expect("instance exists");
    assert!(
        row.probe_result.as_object().is_some_and(|o| !o.is_empty()),
        "probe_result snapshot was not written: {}",
        row.probe_result
    );
}

#[then(expr = "响应 reload 成功")]
async fn then_reload_ok(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    assert_eq!(
        body.get("reload")
            .and_then(|r| r.get("ok"))
            .and_then(|v| v.as_bool()),
        Some(true),
        "reload did not report success: {}",
        body
    );
}
