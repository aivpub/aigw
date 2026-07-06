//! Step bindings for migration.feature

use cucumber::{given, then, when};
use axum::http::Method;
use axum::Router;

use super::common::make_request;
use crate::TestWorld;

fn build_credential_router(
    state: aigw_server::routes::keys::SharedState,
) -> Router {
    Router::new()
        .route(
            "/credential/new",
            axum::routing::post(aigw_server::routes::credentials::credential_new),
        )
        .route(
            "/credential/info",
            axum::routing::get(aigw_server::routes::credentials::credential_info),
        )
        .route(
            "/credential/list",
            axum::routing::get(aigw_server::routes::credentials::credential_list),
        )
        .route(
            "/credential/update",
            axum::routing::put(aigw_server::routes::credentials::credential_update),
        )
        .route(
            "/credential/delete",
            axum::routing::delete(aigw_server::routes::credentials::credential_delete),
        )
        .with_state(state)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "一个 credential {string} 已创建")]
async fn given_credential_created(world: &mut TestWorld, name: String) {
    let state = world.ensure_state().await;
    let router = build_credential_router(state);
    let body = serde_json::json!({
        "credential_name": name,
        "credential_values": {"api_key": format!("sk-test-{}", name)},
    })
    .to_string();
    let (s, b) = make_request(
        &router,
        Method::POST,
        "/credential/new",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
    if s == 200 {
        world
            .created_keys
            .insert(format!("cred:{}", name), name.clone());
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 master key 查询 credential {string}")]
async fn when_get_credential_info(world: &mut TestWorld, name: String) {
    let state = world.ensure_state().await;
    let router = build_credential_router(state);
    let uri = format!("/credential/info?credential_name={}", name);
    let (s, b) = make_request(
        &router,
        Method::GET,
        &uri,
        Some(&world.master_key.clone()),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "使用 master key 发送 GET credential list 请求")]
async fn when_get_credential_list(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = build_credential_router(state);
    let (s, b) = make_request(
        &router,
        Method::GET,
        "/credential/list",
        Some(&world.master_key.clone()),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "使用 master key 更新 credential {string} 的 api_key 为 {string}")]
async fn when_update_credential(world: &mut TestWorld, name: String, api_key: String) {
    let state = world.ensure_state().await;
    let router = build_credential_router(state);
    let body = serde_json::json!({
        "credential_name": name,
        "credential_values": {"api_key": api_key},
    })
    .to_string();
    let (s, b) = make_request(
        &router,
        Method::PUT,
        "/credential/update",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "使用 master key 删除 credential {string}")]
async fn when_delete_credential(world: &mut TestWorld, name: String) {
    let state = world.ensure_state().await;
    let router = build_credential_router(state);
    let uri = format!("/credential/delete?credential_name={}", name);
    let (s, b) = make_request(
        &router,
        Method::DELETE,
        &uri,
        Some(&world.master_key.clone()),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "响应 JSON 字段 {string} 值为 {string}")]
async fn then_field_equals(world: &mut TestWorld, field: String, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let value = body
        .get(&field)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("field '{}' not found in body: {:?}", field, body));
    assert_eq!(
        value, expected,
        "Expected field '{}' to be '{}', got '{}'",
        field, expected, value
    );
}

#[then(expr = "响应 JSON 列表中应包含 credential 名称为 {string}")]
async fn then_list_contains_credential(world: &mut TestWorld, name: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body
        .get("data")
        .and_then(|v| v.as_array())
        .expect("no 'data' array in response");
    let found = data.iter().any(|item| {
        item.get("credential_name")
            .and_then(|v| v.as_str())
            .map(|n| n == name)
            .unwrap_or(false)
    });
    assert!(
        found,
        "Expected credential '{}' in list, got: {:?}",
        name,
        data.iter()
            .filter_map(|item| item.get("credential_name").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
    );
}

#[then(expr = "响应 JSON 字段 {string} 包含 {string}")]
async fn then_field_contains(world: &mut TestWorld, field: String, substring: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let value_str = if let Some(v) = body.get(&field) {
        serde_json::to_string(v).unwrap_or_default()
    } else {
        panic!("field '{}' not found in body: {:?}", field, body);
    };
    assert!(
        value_str.contains(&substring),
        "Expected field '{}' to contain '{}', got: {}",
        field,
        substring,
        value_str
    );
}
