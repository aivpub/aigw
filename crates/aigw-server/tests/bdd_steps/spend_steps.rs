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
// Stage 34: pagination, request_id, page_size
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 key {string} 发送 GET \\/spend\\/logs 请求带 page=1&page_size=10")]
async fn when_key_get_spend_logs_paginated(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let token = world.created_keys.get(&alias).expect("key not found");
    let (s, b) = make_request(
        &app,
        Method::GET,
        "/spend/logs?page=1&page_size=10",
        Some(token),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "使用 key {string} 发送 GET \\/spend\\/logs 请求带 request_id 过滤")]
async fn when_key_get_spend_logs_request_id(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let token = world.created_keys.get(&alias).expect("key not found");
    let (s, b) = make_request(
        &app,
        Method::GET,
        "/spend/logs?request_id=test-req-123",
        Some(token),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "使用 master-key 发送 GET \\/global\\/spend\\/logs 请求带 page=1&page_size=5&request_id=nonexistent")]
async fn when_master_get_global_spend_logs_paginated(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let mk = world.master_key.clone();
    let (s, b) = make_request(
        &app,
        Method::GET,
        "/global/spend/logs?page=1&page_size=5&request_id=nonexistent",
        Some(&mk),
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

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 77: detail endpoint & body-less list
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "发送 GET \\/global\\/spend\\/logs\\/{word} 请求（无认证）")]
async fn get_spend_log_detail_noauth(world: &mut TestWorld, request_id: String) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let uri = format!("/global/spend/logs/{}", request_id);
    let (s, b) = make_request(&app, Method::GET, &uri, None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[given(expr = "一个支出记录 {string} 已入库")]
async fn given_spend_log_basic(world: &mut TestWorld, request_id: String) {
    let state = world.ensure_state().await;
    let log = aigw_core::models::SpendLog {
        call_id: request_id,
        request_id: None,
        call_type: "completion".to_string(),
        api_key: "master_key".to_string(),
        spend: 0.01,
        total_tokens: 10,
        prompt_tokens: 5,
        completion_tokens: 5,
        start_time: chrono::Utc::now(),
        end_time: chrono::Utc::now(),
        request_duration_ms: Some(100),
        completion_start_time: None,
        model: "gpt-4".to_string(),
        model_id: None,
        model_group: Some("gpt-4".to_string()),
        custom_llm_provider: Some("openai".to_string()),
        api_base: None,
        user: None,
        metadata: None,
        cache_hit: None,
        cache_key: None,
        request_tags: None,
        team_id: None,
        organization_id: None,
        end_user: None,
        requester_ip_address: None,
        messages: None,
        response: None,
        session_id: None,
        status: Some("success".to_string()),
        mcp_namespaced_tool_name: None,
        agent_id: None,
        proxy_server_request: None,
    body_archived: false,
    parquet_path: None,
    };
    state.db.insert_spend_log(&log).await.expect("insert spend log");
}

#[given(expr = "一个支出记录 {string} 含 body 已入库")]
async fn given_spend_log_with_body(world: &mut TestWorld, request_id: String) {
    let state = world.ensure_state().await;
    let log = aigw_core::models::SpendLog {
        call_id: request_id,
        request_id: None,
        call_type: "completion".to_string(),
        api_key: "master_key".to_string(),
        spend: 0.05,
        total_tokens: 100,
        prompt_tokens: 60,
        completion_tokens: 40,
        start_time: chrono::Utc::now(),
        end_time: chrono::Utc::now(),
        request_duration_ms: Some(500),
        completion_start_time: None,
        model: "gpt-4".to_string(),
        model_id: None,
        model_group: Some("gpt-4".to_string()),
        custom_llm_provider: Some("openai".to_string()),
        api_base: None,
        user: None,
        metadata: None,
        cache_hit: None,
        cache_key: None,
        request_tags: None,
        team_id: None,
        organization_id: None,
        end_user: None,
        requester_ip_address: None,
        messages: Some(serde_json::json!([{"role": "user", "content": "hello"}])),
        response: Some(serde_json::json!({"choices": [{"message": {"content": "hi"}}]})),
        session_id: None,
        status: Some("success".to_string()),
        mcp_namespaced_tool_name: None,
        agent_id: None,
        proxy_server_request: None,
    body_archived: false,
    parquet_path: None,
    };
    state.db.insert_spend_log(&log).await.expect("insert spend log");
}

#[when(expr = "使用 master-key 发送 GET \\/global\\/spend\\/logs\\/{word} 请求")]
async fn when_master_get_spend_log_detail(world: &mut TestWorld, request_id: String) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let mk = world.master_key.clone();
    let uri = format!("/global/spend/logs/{}", request_id);
    let (s, b) = make_request(&app, Method::GET, &uri, Some(&mk), None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "使用 master-key 发送 GET \\/global\\/spend\\/logs 请求带 page_size=10")]
async fn when_master_get_global_spend_logs_bodyless(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let mk = world.master_key.clone();
    let (s, b) = make_request(
        &app,
        Method::GET,
        "/global/spend/logs?page_size=10",
        Some(&mk),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

use cucumber::then;

#[then(expr = "响应 body 包含 {string} 和 {string} 字段")]
async fn then_body_has_keys(world: &mut TestWorld, key1: String, key2: String) {
    let body = world.last_body.as_ref().expect("no response body");
    assert!(body.get(&key1).is_some(), "body missing key: {}", key1);
    assert!(body.get(&key2).is_some(), "body missing key: {}", key2);
}

#[then(expr = "响应 data 不含 {string} 和 {string} 字段")]
async fn then_data_has_no_keys(world: &mut TestWorld, key1: String, key2: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body.get("data").and_then(|v| v.as_array()).expect("data array");
    assert!(!data.is_empty(), "data array is empty");
    let first = &data[0];
    assert!(first.get(&key1).is_none(), "data should not have key: {}", key1);
    assert!(first.get(&key2).is_none(), "data should not have key: {}", key2);
}

// ━━━━ Stage 85: call_id + upstream request_id 双列验收 ━━━━

/// Seed a SpendLog whose call_id (gateway id) and request_id (upstream id)
/// are BOTH populated — the core-expectation shape that lets the row be
/// reconciled against the provider.
#[given(expr = "一条含上游 request_id 的支出记录 {string} 已入库")]
async fn given_spend_log_with_upstream_id(world: &mut TestWorld, call_id: String) {
    let state = world.ensure_state().await;
    let log = aigw_core::models::SpendLog {
        call_id: call_id.clone(),
        // Stage 85: upstream id captured at INSERT (non-streaming success path).
        // Use a deterministic upstream id so a later search-by-upstream-id step
        // can hit it.
        request_id: Some("msg_upstream_001".to_string()),
        call_type: "completion".to_string(),
        api_key: "master_key".to_string(),
        spend: 0.05,
        total_tokens: 100,
        prompt_tokens: 50,
        completion_tokens: 50,
        start_time: chrono::Utc::now(),
        end_time: chrono::Utc::now(),
        request_duration_ms: Some(100),
        completion_start_time: None,
        model: "gpt-4".to_string(),
        model_id: None,
        model_group: Some("gpt-4".to_string()),
        custom_llm_provider: Some("openai".to_string()),
        api_base: None,
        user: None,
        metadata: None,
        cache_hit: None,
        cache_key: None,
        request_tags: None,
        team_id: None,
        organization_id: None,
        end_user: None,
        requester_ip_address: None,
        messages: None,
        response: None,
        session_id: None,
        status: Some("success".to_string()),
        mcp_namespaced_tool_name: None,
        agent_id: None,
        proxy_server_request: None,
        body_archived: false,
        parquet_path: None,
    };
    state.db.insert_spend_log(&log).await.expect("insert spend log");
}

#[then(expr = "响应 body 的 call_id 为 {string}")]
async fn then_body_call_id_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let got = body.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(got, expected, "call_id mismatch");
}

#[then(expr = "响应 body 的 request_id 为 {string}")]
async fn then_body_request_id_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let got = body.get("request_id").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(got, expected, "upstream request_id mismatch — core expectation (reconciliation) broken");
}

#[when(expr = "使用 master-key 发送 GET \\/global\\/spend\\/logs 请求搜索 request_id 为 {string}")]
async fn when_master_search_by_request_id(world: &mut TestWorld, search: String) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let mk = world.master_key.clone();
    let uri = format!("/global/spend/logs?request_id={}", search);
    let (s, b) = make_request(&app, Method::GET, &uri, Some(&mk), None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[then(expr = "响应 data 包含 call_id 为 {string} 的记录")]
async fn then_data_contains_call_id(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body.get("data").and_then(|v| v.as_array()).expect("data array");
    // Diagnostic: list present call_ids on miss so the failure is actionable
    // (the global list endpoint paginates page_size=30; test DBs are ephemeral
    // so the seeded rows land on page 1).
    let found = data.iter().any(|r| r.get("call_id").and_then(|v| v.as_str()) == Some(expected.as_str()));
    if !found {
        let present: Vec<&str> = data.iter().filter_map(|r| r.get("call_id").and_then(|v| v.as_str())).collect();
        panic!("no row in data has call_id = {} (present call_ids: {:?})", expected, present);
    }
}
