//! Step bindings for spend.feature, global.feature, and spend_aggregation.feature

use cucumber::{given, when};
use axum::http::Method;

use super::common::{build_spend_router, make_request};
use crate::TestWorld;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When: no-auth requests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "发送 GET \\/spend\\/logs 请求（无认证）")]
async fn get_spend_logs_noauth(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let (s, b) = make_request(&app, Method::GET, "/spend/logs", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 GET \\/spend\\/keys 请求（无认证）")]
async fn get_spend_keys_noauth(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let (s, b) = make_request(&app, Method::GET, "/spend/keys", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 GET \\/global\\/spend 请求（无认证）")]
async fn get_global_spend_noauth(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let (s, b) = make_request(&app, Method::GET, "/global/spend", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 GET \\/spend\\/models 请求（无认证）")]
async fn get_spend_models_noauth(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let (s, b) = make_request(&app, Method::GET, "/spend/models", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 GET \\/spend\\/providers 请求（无认证）")]
async fn get_spend_providers_noauth(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let (s, b) = make_request(&app, Method::GET, "/spend/providers", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given: generate keys
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "一个普通 key {string} 已生成")]
async fn given_regular_key(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let raw_token = format!("sk-{}", uuid::Uuid::new_v4());
    let now = chrono::Utc::now();
    let key = aigw_core::models::VirtualKey {
        token: aigw_core::crypto::hash_token(&raw_token),
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
        created_at: Some(now),
        created_by: Some("test".to_string()),
        updated_at: Some(now),
        updated_by: Some("test".to_string()),
        last_active: None,
        rotation_count: None,
        auto_rotate: None,
        rotation_interval: None,
        last_rotation_at: None,
        key_rotation_at: None,
        budget_limits: None,
    };
    state.db.insert_key(&key).await.expect("insert key");
    world.created_keys.insert(alias, raw_token);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When: use key / master-key to call endpoints
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 key {string} 发送 GET \\/global\\/spend\\/models 请求")]
async fn when_key_get_global_spend_models(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let token = world.created_keys.get(&alias).expect("key not found");
    let (s, b) = make_request(&app, Method::GET, "/global/spend/models", Some(token), None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "使用 key {string} 发送 GET \\/global\\/spend\\/providers 请求")]
async fn when_key_get_global_spend_providers(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let token = world.created_keys.get(&alias).expect("key not found");
    let (s, b) = make_request(&app, Method::GET, "/global/spend/providers", Some(token), None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "使用 key {string} 发送 GET \\/spend\\/models 请求")]
async fn when_key_get_spend_models(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let token = world.created_keys.get(&alias).expect("key not found");
    let (s, b) = make_request(&app, Method::GET, "/spend/models", Some(token), None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "使用 key {string} 发送 GET \\/spend\\/providers 请求")]
async fn when_key_get_spend_providers(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let token = world.created_keys.get(&alias).expect("key not found");
    let (s, b) = make_request(&app, Method::GET, "/spend/providers", Some(token), None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "使用 key {string} 发送 GET \\/spend\\/logs 请求带 model 过滤")]
async fn when_key_get_spend_logs_filtered_model(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let token = world.created_keys.get(&alias).expect("key not found");
    let (s, b) = make_request(&app, Method::GET, "/spend/logs?model=gpt-4", Some(token), None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "使用 key {string} 发送 GET \\/spend\\/logs 请求带时间过滤")]
async fn when_key_get_spend_logs_filtered_date(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let token = world.created_keys.get(&alias).expect("key not found");
    let (s, b) = make_request(
        &app,
        Method::GET,
        "/spend/logs?start_date=2024-01-01&end_date=2024-12-31",
        Some(token),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When: admin / master-key requests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 master-key 发送 GET \\/global\\/spend\\/models 请求")]
async fn when_master_get_global_spend_models(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let mk = world.master_key.clone();
    let (s, b) = make_request(&app, Method::GET, "/global/spend/models", Some(&mk), None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "使用 master-key 发送 GET \\/global\\/spend\\/providers 请求")]
async fn when_master_get_global_spend_providers(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let mk = world.master_key.clone();
    let (s, b) = make_request(&app, Method::GET, "/global/spend/providers", Some(&mk), None).await;
    world.last_status = Some(s);
    world.last_body = b;
}
