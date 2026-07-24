//! Step bindings for spend_end_user.feature

use cucumber::{given, then, when};
use cucumber::gherkin::Step;
use aigw_core::models::SpendLog;

use crate::TestWorld;

/// Build a router with spend routes
fn build_router(
    state: aigw_server::routes::keys::SharedState,
) -> axum::Router {
    axum::Router::new()
        .route("/global/spend/logs", axum::routing::get(aigw_server::routes::spend::global_spend_logs))
        .with_state(state)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given: pre-insert SpendLog with specified end_user fields
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "已插入一条带 end_user 和 session_id 和 requester_ip 的 SpendLog")]
async fn given_spend_log_with_end_user(world: &mut TestWorld, step: &Step) {
    let state = world.ensure_state().await;
    let body = step.docstring.as_ref().expect("docstring body").to_string();
    let params: serde_json::Value = serde_json::from_str(&body).expect("parse params");

    let end_user_raw = params["end_user"].as_str().map(|s| s.to_string());
    let session_id = params["session_id"].as_str().map(|s| s.to_string());
    let requester_ip = params["requester_ip"].as_str().map(|s| s.to_string());

    let key_token = world.created_keys
        .get("e2u-test-key")
        .map(|t| aigw_core::crypto::hash_token(t))
        .unwrap_or_else(|| aigw_core::crypto::hash_token("sk-e2u-test"));

    let now = chrono::Utc::now();
    let log = SpendLog {
        request_id: uuid::Uuid::new_v4().to_string(),
        call_type: "completion".to_string(),
        api_key: key_token,
        spend: 0.015,
        total_tokens: 150,
        prompt_tokens: 100,
        completion_tokens: 50,
        start_time: now,
        end_time: now,
        request_duration_ms: Some(1200),
        completion_start_time: Some(now),
        model: "claude-sonnet-5".to_string(),
        model_id: None, model_group: None, custom_llm_provider: None,
        api_base: Some("https://api.anthropic.com".to_string()),
        user: Some("test-user".to_string()),
        metadata: None, cache_hit: None, cache_key: None, request_tags: None,
        team_id: None, organization_id: None,
        end_user: end_user_raw,
        requester_ip_address: requester_ip,
        messages: Some(serde_json::json!([{"role":"user","content":"hi"}])),
        response: Some(serde_json::json!({"type":"message","content":[{"type":"text","text":"hello"}]})),
        session_id,
        status: Some("success".to_string()),
        mcp_namespaced_tool_name: None, agent_id: None, proxy_server_request: None,
    body_archived: false,
    parquet_path: None,
    };
    state.db.insert_spend_log(&log).await.expect("insert spend log");
}

#[given(expr = "已插入一条 SpendLog 不含 end_user")]
async fn given_spend_log_without_end_user(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let key_token = world.created_keys
        .get("e2u-no-meta-key")
        .map(|t| aigw_core::crypto::hash_token(t))
        .unwrap_or_else(|| aigw_core::crypto::hash_token("sk-e2u-no-meta"));

    let now = chrono::Utc::now();
    let log = SpendLog {
        request_id: uuid::Uuid::new_v4().to_string(),
        call_type: "completion".to_string(),
        api_key: key_token,
        spend: 0.01, total_tokens: 50, prompt_tokens: 30, completion_tokens: 20,
        start_time: now, end_time: now,
        request_duration_ms: Some(800), completion_start_time: Some(now),
        model: "gpt-4".to_string(),
        model_id: None, model_group: None, custom_llm_provider: None,
        api_base: Some("https://api.openai.com".to_string()),
        user: Some("test-user".to_string()),
        metadata: None, cache_hit: None, cache_key: None, request_tags: None,
        team_id: None, organization_id: None,
        end_user: None, requester_ip_address: None,
        messages: Some(serde_json::json!([{"role":"user","content":"hello"}])),
        response: Some(serde_json::json!({"choices":[{"message":{"content":"hi"}}]})),
        session_id: None,
        status: Some("success".to_string()),
        mcp_namespaced_tool_name: None, agent_id: None, proxy_server_request: None,
    body_archived: false,
    parquet_path: None,
    };
    state.db.insert_spend_log(&log).await.expect("insert spend log");
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "master-key 查询 global spend logs 获取 end_user 相关 SpendLog")]
async fn when_master_get_global_spend_logs(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    use axum::http::Method;
    use tower::util::ServiceExt;

    let app = build_router(state);
    let mk = world.master_key.clone();

    let req = axum::http::Request::builder()
        .method(Method::GET)
        .uri("/global/spend/logs?page=1&page_size=10")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", mk))
        .body(axum::body::Body::empty())
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let json_body: Option<serde_json::Value> = serde_json::from_slice(&body_bytes).ok();
    world.last_status = Some(status);
    world.last_body = json_body;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "响应 data 第一条记录 end_user 字段存在")]
async fn then_first_record_has_end_user(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body.get("data").and_then(|d| d.as_array()).expect("data is not array");
    assert!(!data.is_empty(), "data array is empty");

    // end_user should be present and not null
    let end_user_val = data[0].get("end_user").expect("no end_user field in response");
    assert!(!end_user_val.is_null(), "end_user is null");
    let s = end_user_val.as_str().unwrap_or("");
    assert!(!s.is_empty(), "end_user is empty string");
}

#[then(expr = "响应 data 第一条记录 end_user 为空或不存在")]
async fn then_first_record_end_user_empty(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body.get("data").and_then(|d| d.as_array()).expect("data is not array");
    assert!(!data.is_empty(), "data array is empty");

    let raw = data[0].get("end_user");
    match raw {
        None | Some(serde_json::Value::Null) => {} // acceptable
        Some(v) => {
            let s = v.as_str().unwrap_or("");
            assert!(s.is_empty(), "Expected end_user to be null or empty, got '{}'", s);
        }
    }
}

#[then(regex = r#"^第一条日志的 requester_ip_address 为 "(.+)"$"#)]
async fn then_first_log_requester_ip_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body
        .get("data")
        .and_then(|d| d.as_array())
        .expect("data is not array");
    assert!(!data.is_empty(), "data array is empty");

    let ip = data[0]
        .get("requester_ip_address")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    assert_eq!(
        ip, expected,
        "Expected requester_ip_address = '{}', got '{}'",
        expected, ip
    );
}
