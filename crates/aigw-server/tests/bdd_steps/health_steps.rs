//! Step bindings for health.feature

use cucumber::{then, when};
use axum::http::Method;

use super::common::{build_health_router, make_request};
use crate::TestWorld;

#[when(expr = "发送 GET \\/health 请求")]
async fn get_health(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_health_router(state);
    let (s, b) = make_request(&app, Method::GET, "/health", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 GET \\/health\\/liveliness 请求")]
async fn get_liveliness(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_health_router(state);
    let (s, b) = make_request(&app, Method::GET, "/health/liveliness", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 GET \\/health\\/readiness 请求")]
async fn get_readiness(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_health_router(state);
    let (s, b) = make_request(&app, Method::GET, "/health/readiness", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[then(expr = "响应包含 status 字段")]
async fn response_has_status(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    assert!(
        body.get("status").is_some(),
        "Response missing status field: {:?}",
        body
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 91: model health-check probe + spend_logs 留存 BDD
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use cucumber::given;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;

use crate::bdd_support::mock_upstream::MockUpstream;

/// Global mock upstream shared across health-check scenarios (started on demand).
static HEALTH_MOCK_UPSTREAM: OnceLock<Arc<Mutex<Option<MockUpstream>>>> = OnceLock::new();

fn health_mock_upstream() -> &'static Arc<Mutex<Option<MockUpstream>>> {
    HEALTH_MOCK_UPSTREAM.get_or_init(|| Arc::new(Mutex::new(None)))
}

#[given(expr = "健康检查 mock 上游已启动")]
async fn given_health_mock_started(_world: &mut TestWorld) {
    let mu = health_mock_upstream();
    let mut guard = mu.lock().await;
    if guard.is_none() {
        let upstream = MockUpstream::start().await;
        *guard = Some(upstream);
    } else {
        // Reset mock responses to defaults for scenario isolation
        guard.as_mut().unwrap().reset_responses();
    }
}

/// Configure an OpenAI-compatible model whose api_base points at the mock.
#[given(expr = "已配置 OpenAI 模型 {string} 指向健康检查 mock 上游")]
async fn given_openai_model_points_to_health_mock(
    world: &mut TestWorld,
    name: String,
) {
    let state = world.ensure_state().await;
    let mu = health_mock_upstream().lock().await;
    let mock_base = mu
        .as_ref()
        .expect("health mock upstream not started")
        .url()
        .to_string();
    let model = aigw_core::models::ProxyModel {
        model_id: uuid::Uuid::new_v4().to_string(),
        model_name: name.clone(),
        litellm_params: serde_json::json!({
            "model": name,
            "api_base": format!("{mock_base}/v1"),
            "custom_llm_provider": "openai"
        }),
        model_info: serde_json::json!({
            "input_cost_per_token": 0.00003,
            "output_cost_per_token": 0.00006
        }),
        created_at: chrono::Utc::now().to_rfc3339(),
        created_by: Some("test".to_string()),
        updated_at: chrono::Utc::now().to_rfc3339(),
        updated_by: Some("test".to_string()),
    };
    state.db.insert_model(&model).await.expect("insert model");
    world.created_keys.insert(format!("model:{name}"), model.model_id.clone());
}

/// Configure an Anthropic-native model whose api_base points at the mock.
/// The provider_type is inferred from custom_llm_provider="anthropic".
#[given(expr = "已配置 Anthropic 模型 {string} 指向健康检查 mock 上游")]
async fn given_anthropic_model_points_to_health_mock(
    world: &mut TestWorld,
    name: String,
) {
    let state = world.ensure_state().await;
    let mu = health_mock_upstream().lock().await;
    let mock_base = mu
        .as_ref()
        .expect("health mock upstream not started")
        .url()
        .to_string();
    let model = aigw_core::models::ProxyModel {
        model_id: uuid::Uuid::new_v4().to_string(),
        model_name: name.clone(),
        litellm_params: serde_json::json!({
            "model": name,
            "api_base": format!("{mock_base}/v1"),
            "custom_llm_provider": "anthropic"
        }),
        model_info: serde_json::json!({
            "input_cost_per_token": 0.000003,
            "output_cost_per_token": 0.000015
        }),
        created_at: chrono::Utc::now().to_rfc3339(),
        created_by: Some("test".to_string()),
        updated_at: chrono::Utc::now().to_rfc3339(),
        updated_by: Some("test".to_string()),
    };
    state.db.insert_model(&model).await.expect("insert model");
    world.created_keys.insert(format!("model:{name}"), model.model_id.clone());
}

/// Make the mock upstream return an error status for a given path.
#[given(expr = "健康检查 mock 上游 {string} 返回状态码 {int}")]
async fn given_health_mock_returns_status(_world: &mut TestWorld, path: String, status: u16) {
    let mu = health_mock_upstream().lock().await;
    let upstream = mu.as_ref().expect("health mock upstream not started");
    upstream.set_response(
        &path,
        status,
        serde_json::json!({"error": {"message": "mock error", "type": "server_error"}}),
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When — trigger probes via the admin API
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Fire POST /model/health-check/all and wait for the background probes to
/// finish writing health_checks + spend_logs rows.
#[when(expr = "发送 POST \\/model\\/health-check\\/all 请求")]
async fn when_post_health_check_all(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_health_router(state.clone());
    let (s, b) = make_request(&app, Method::POST, "/model/health-check/all", Some(&world.master_key.clone()), None).await;
    world.last_status = Some(s);
    world.last_body = b;
    // The probe runs in a spawned task; poll the DB until every model has a
    // non-"checking" latest row (or timeout). max_concurrent_scenarios(1) +
    // sqlite::memory: makes this safe.
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(8);
    loop {
        if tokio::time::Instant::now() > deadline {
            break;
        }
        let models = state.db.list_models().await.unwrap_or_default();
        let latest = state.db.get_latest_health_checks().await.unwrap_or_default();
        let all_done = models.iter().all(|m| {
            latest
                .iter()
                .find(|c| c.model_name == m.model_name)
                .map(|c| c.status != "checking")
                .unwrap_or(false)
        });
        if all_done && !models.is_empty() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
    }
}

/// Fire POST /model/health-check?model_id=<id> for the named model.
#[when(expr = "发送 POST \\/model\\/health-check 请求查询模型 {string}")]
async fn when_post_health_check_one(world: &mut TestWorld, model_name: String) {
    let state = world.ensure_state().await;
    let app = build_health_router(state.clone());
    let model_id = world
        .created_keys
        .get(&format!("model:{model_name}"))
        .cloned()
        .unwrap_or_else(|| panic!("model {model_name} not configured"));
    let uri = format!("/model/health-check?model_id={}", model_id);
    let (s, b) = make_request(&app, Method::POST, &uri, Some(&world.master_key.clone()), None).await;
    world.last_status = Some(s);
    world.last_body = b;
    // Wait for the single probe to leave "checking" state.
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(8);
    loop {
        if tokio::time::Instant::now() > deadline {
            break;
        }
        let latest = state.db.get_latest_health_checks().await.unwrap_or_default();
        let done = latest
            .iter()
            .find(|c| c.model_name == model_name)
            .map(|c| c.status != "checking")
            .unwrap_or(false);
        if done {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then — assert health_checks + spend_logs outcomes
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "健康检查结果中 {string} 为 healthy")]
async fn then_health_result_healthy(world: &mut TestWorld, model_name: String) {
    let state = world.ensure_state().await;
    let app = build_health_router(state.clone());
    let (s, b) = make_request(&app, Method::GET, "/health/latest", Some(&world.master_key.clone()), None).await;
    assert_eq!(s, 200, "GET /health/latest returned {}", s);
    let body = b.expect("no body");
    let data = body.get("data").and_then(|d| d.as_array()).expect("no data array");
    let entry = data
        .iter()
        .find(|d| d.get("model_name").and_then(|v| v.as_str()) == Some(&model_name))
        .unwrap_or_else(|| panic!("model {model_name} not in /health/latest data: {body}"));
    let status = entry.get("status").and_then(|v| v.as_str()).expect("no status");
    assert_eq!(status, "healthy", "expected healthy, got {status} for {model_name}");
}

#[then(expr = "健康检查结果中 {string} 为 unhealthy")]
async fn then_health_result_unhealthy(world: &mut TestWorld, model_name: String) {
    let state = world.ensure_state().await;
    let app = build_health_router(state.clone());
    let (s, b) = make_request(&app, Method::GET, "/health/latest", Some(&world.master_key.clone()), None).await;
    assert_eq!(s, 200, "GET /health/latest returned {}", s);
    let body = b.expect("no body");
    let data = body.get("data").and_then(|d| d.as_array()).expect("no data array");
    let entry = data
        .iter()
        .find(|d| d.get("model_name").and_then(|v| v.as_str()) == Some(&model_name))
        .unwrap_or_else(|| panic!("model {model_name} not in /health/latest data: {body}"));
    let status = entry.get("status").and_then(|v| v.as_str()).expect("no status");
    assert_eq!(status, "unhealthy", "expected unhealthy, got {status} for {model_name}");
}

/// Assert that a health_check spend_log exists for the model with the expected
/// status kind (success / failure). Uses the DB directly (like e2e_steps'
/// then_spend_logs_model_is) to avoid spending-router auth complexity.
#[then(expr = "spend_logs 中存在 model={string} 且 call_type=health_check 且 status {string} 的记录")]
async fn then_spend_log_health_check_exists(
    world: &mut TestWorld,
    model_name: String,
    status_kind: String,
) {
    let state = world.ensure_state().await;
    // Fetch a generous window of recent logs and find the health_check probe.
    let logs = state
        .db
        .query_spend_logs(None, Some(200))
        .await
        .expect("query spend logs");
    let matching: Vec<_> = logs
        .iter()
        .filter(|l| l.model == model_name && l.call_type == "health_check")
        .collect();
    assert!(
        !matching.is_empty(),
        "Expected a health_check spend_log for model='{model_name}', found none. \
         call_types seen: {:?}",
        logs.iter().map(|l| &l.call_type).collect::<Vec<_>>()
    );
    let log = matching[0];
    assert_eq!(
        log.api_key, "health_check",
        "health_check spend_log api_key must be the sentinel, got {}",
        log.api_key
    );
    let st = log.status.clone().unwrap_or_default();
    match status_kind.as_str() {
        "success" => {
            assert_eq!(st, "success", "expected status=success, got {st}");
            // Probe hit a 200 mock upstream — usage returned (prompt=10/completion=5),
            // so spend must be > 0 (pricing is configured in the Given step).
            assert!(log.spend > 0.0, "expected spend>0 for healthy probe, got {}", log.spend);
            assert!(
                log.prompt_tokens > 0 || log.completion_tokens > 0,
                "expected non-zero tokens for healthy probe, got prompt={} completion={}",
                log.prompt_tokens,
                log.completion_tokens
            );
        }
        "failure" => {
            assert!(
                st.starts_with("failure"),
                "expected status starting with 'failure', got {st}"
            );
            assert_eq!(log.spend, 0.0, "failure probe spend should be 0, got {}", log.spend);
        }
        other => panic!("unknown status kind '{other}' in step"),
    }
}

