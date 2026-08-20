//! Step bindings for models.feature

use aigw_server::routes::keys::DEFAULT_KEY_TOKEN_LEN;
use axum::http::Method;
use axum::Router;
use cucumber::gherkin::Step;
use cucumber::{given, then, when};
use std::sync::Arc;
use tower::util::ServiceExt;

use super::common::make_request;
use super::e2e_steps;
use crate::TestWorld;

/// Build a router with only model routes
fn build_model_router(state: aigw_server::routes::keys::SharedState) -> Router {
    Router::new()
        .route(
            "/model/new",
            axum::routing::post(aigw_server::routes::models::model_new),
        )
        .route(
            "/model/info",
            axum::routing::get(aigw_server::routes::models::model_info),
        )
        .route(
            "/model/list",
            axum::routing::get(aigw_server::routes::models::model_list),
        )
        .route(
            "/model/update",
            axum::routing::put(aigw_server::routes::models::model_update),
        )
        .route(
            "/model/delete",
            axum::routing::delete(aigw_server::routes::models::model_delete),
        )
        .with_state(state)
}

/// Helper: create a model and return (status, body)
async fn create_model(router: &Router, mk: &str, name: &str) -> (u16, Option<serde_json::Value>) {
    let body = serde_json::json!({
        "model_name": name,
        "litellm_params": {"model": format!("openai/{}", name)},
    })
    .to_string();
    make_request(router, Method::POST, "/model/new", Some(mk), Some(&body)).await
}

/// Stage 121 — create a model pointing at the mock upstream (so an enabled
/// model returns 200 and a disabled one is blocked before reaching upstream).
async fn create_mock_model(world: &mut TestWorld, name: &str) {
    let state = world.ensure_state().await;
    let mu = e2e_steps::mock_upstream().lock().await;
    let mock_base = mu
        .as_ref()
        .expect("mock upstream not started; add Given mock 上游已启动")
        .url()
        .to_string();
    drop(mu);

    let model = aigw_core::models::ProxyModel {
        model_id: uuid::Uuid::new_v4().to_string(),
        model_name: name.to_string(),
        litellm_params: serde_json::json!({
            "model": format!("openai/{}", name),
            "api_base": format!("{mock_base}/v1"),
        }),
        model_info: serde_json::json!({}),
        created_at: chrono::Utc::now().to_rfc3339(),
        created_by: Some("test".to_string()),
        updated_at: chrono::Utc::now().to_rfc3339(),
        updated_by: Some("test".to_string()),
        enabled: true,
    };
    state.db.insert_model(&model).await.expect("insert model");
    world
        .created_keys
        .insert(format!("model:{}", name), model.model_id);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "已存在模型 {string}")]
async fn existing_model(world: &mut TestWorld, name: String) {
    let state = world.ensure_state().await;
    let router = build_model_router(state);
    let (_, body) = create_model(&router, &world.master_key.clone(), &name).await;
    // Store model_id for later lookup
    if let Some(ref b) = body {
        if let Some(id) = b.get("model_id").and_then(|v| v.as_str()) {
            world
                .created_keys
                .insert(format!("model:{}", name), id.to_string());
        }
    }
}

/// Stage 121 — a model pointing at the mock upstream, stored for enable/disable
/// toggling and forward checks.
#[given(expr = "已存在指向 mock 上游的模型 {string}")]
async fn existing_mock_model(world: &mut TestWorld, name: String) {
    create_mock_model(world, &name).await;
}

#[given(expr = "已存在 {int} 个模型")]
async fn existing_n_models(world: &mut TestWorld, count: usize) {
    let state = world.ensure_state().await;
    let router = build_model_router(state);
    for i in 0..count {
        let name = format!("multi-model-{}", i);
        let (_, body) = create_model(&router, &world.master_key.clone(), &name).await;
        if let Some(ref b) = body {
            if let Some(id) = b.get("model_id").and_then(|v| v.as_str()) {
                world
                    .created_keys
                    .insert(format!("model:{}", name), id.to_string());
            }
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 103: /v1/models model_info.mode exposure
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Insert a proxy_models row directly with a model_info carrying `mode: "image"`
/// (multimodal). Used by the /v1/models scenarios.
#[given(expr = "已存在多模态模型 {string} 其 model_info.mode 为 {string}")]
async fn given_multimodal_model(world: &mut TestWorld, name: String, mode: String) {
    let state = world.ensure_state().await;
    let model = aigw_core::models::ProxyModel {
        model_id: uuid::Uuid::new_v4().to_string(),
        model_name: name.clone(),
        litellm_params: serde_json::json!({"model": format!("qwen/{}", name)}),
        model_info: serde_json::json!({"id": name, "mode": mode}),
        created_at: chrono::Utc::now().to_rfc3339(),
        created_by: Some("test".to_string()),
        updated_at: chrono::Utc::now().to_rfc3339(),
        updated_by: Some("test".to_string()),
        enabled: true,
    };
    state.db.insert_model(&model).await.expect("insert model");
}

/// Create a non-master virtual key bound to a specific model allow-list.
/// The key's `models` array carries model-name strings only — no model_info.
#[given(expr = "一个普通 key {string} 已生成且绑定模型 {string}")]
async fn given_regular_key_with_model(world: &mut TestWorld, alias: String, model: String) {
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
        models: serde_json::json!([model]),
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
        user_email: None,
        user_alias: None,
    };
    state.db.insert_key(&key).await.expect("insert key");
    world.created_keys.insert(alias, raw_token);
}

#[when(expr = "发送 GET \\/v1\\/models 请求")]
async fn when_get_v1_models(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = Router::new()
        .route(
            "/v1/models",
            axum::routing::get(aigw_server::routes::chat::models_list),
        )
        .with_state(state);
    let (s, b) = make_request(
        &router,
        Method::GET,
        "/v1/models",
        Some(&world.master_key.clone()),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

/// Send GET /v1/models using a non-master (regular) virtual key — the key's
/// model list carries name strings only, so model_info must be absent.
#[when(expr = "使用普通 key {string} 发送 GET \\/v1\\/models 请求")]
async fn when_get_v1_models_with_key(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let router = Router::new()
        .route(
            "/v1/models",
            axum::routing::get(aigw_server::routes::chat::models_list),
        )
        .with_state(state);
    let token = world
        .created_keys
        .get(&alias)
        .expect("key not found")
        .clone();
    let (s, b) = make_request(&router, Method::GET, "/v1/models", Some(&token), None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[then(regex = r#"^\/v1\/models 中模型 "(.+)" 的 model_info.mode 为 "(.+)"$"#)]
async fn then_v1_models_mode(world: &mut TestWorld, model: String, expected_mode: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body
        .get("data")
        .and_then(|v| v.as_array())
        .expect("no data array");
    let entry = data
        .iter()
        .find(|m| m.get("id").and_then(|v| v.as_str()) == Some(model.as_str()))
        .unwrap_or_else(|| panic!("model {model} not in /v1/models: {body}"));
    let mode = entry
        .get("model_info")
        .and_then(|m| m.get("mode"))
        .and_then(|v| v.as_str())
        .expect("no model_info.mode");
    assert_eq!(
        mode, expected_mode,
        "expected /v1/models {model}.model_info.mode={expected_mode}, got {mode}"
    );
}

#[then(expr = "\\/v1\\/models 不返回 model_info 字段")]
async fn then_v1_models_no_model_info(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body
        .get("data")
        .and_then(|v| v.as_array())
        .expect("no data array");
    for entry in data {
        assert!(
            entry.get("model_info").is_none(),
            "expected no model_info in /v1/models entry: {}",
            entry
        );
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "发送 POST \\/model\\/new 请求")]
async fn when_post_model_new(world: &mut TestWorld, step: &Step) {
    let state = world.ensure_state().await;
    let router = build_model_router(state);
    let body = step
        .docstring
        .as_ref()
        .expect("docstring body not found")
        .to_string();
    let (s, b) = make_request(
        &router,
        Method::POST,
        "/model/new",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 POST \\/model\\/new 请求（无认证）")]
async fn when_post_model_new_noauth(world: &mut TestWorld, step: &Step) {
    let state = world.ensure_state().await;
    let router = build_model_router(state);
    let body = step
        .docstring
        .as_ref()
        .expect("docstring body not found")
        .to_string();
    let (s, b) = make_request(&router, Method::POST, "/model/new", None, Some(&body)).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 GET \\/model\\/info 请求查询该模型")]
async fn when_get_model_info_by_stored(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = build_model_router(state);
    // Find the stored model_id from the most recently created model
    let model_id = world
        .created_keys
        .iter()
        .find(|(k, _)| k.starts_with("model:query-model"))
        .map(|(_, v)| v.clone())
        .expect("model not stored");
    let uri = format!("/model/info?model_id={}", model_id);
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

#[when(regex = r"^发送 GET /model/info\?model_id=(.+)$")]
async fn when_get_model_info_by_id(world: &mut TestWorld, model_id: String) {
    let state = world.ensure_state().await;
    let router = build_model_router(state);
    let uri = format!("/model/info?model_id={}", model_id);
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

#[when(expr = "发送 GET \\/model\\/list")]
async fn when_get_model_list(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = build_model_router(state);
    let (s, b) = make_request(
        &router,
        Method::GET,
        "/model/list",
        Some(&world.master_key.clone()),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(regex = r"^发送 PUT /model/update 请求更新模型名称为 (.+)$")]
async fn when_put_model_update(world: &mut TestWorld, new_name: String) {
    let state = world.ensure_state().await;
    let router = build_model_router(state);
    let model_id = world
        .created_keys
        .iter()
        .find(|(k, _)| k.starts_with("model:update-model"))
        .map(|(_, v)| v.clone())
        .expect("model update-model not stored");
    let body = serde_json::json!({
        "model_id": model_id,
        "model_name": new_name,
    })
    .to_string();
    let (s, b) = make_request(
        &router,
        Method::PUT,
        "/model/update",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

/// Stage 121 — disable (enabled=false) or enable (enabled=true) a model via
/// PUT /model/update. The model must have been created through a prior
/// `已存在模型 "name"` step so its model_id is stored under `model:{name}`.
async fn when_set_model_enabled(world: &mut TestWorld, model_name: &str, enabled: bool) {
    let state = world.ensure_state().await;
    let router = build_model_router(state);
    let model_id = world
        .created_keys
        .get(&format!("model:{}", model_name))
        .expect("model not stored; create it with 已存在模型 first")
        .clone();
    let body = serde_json::json!({
        "model_id": model_id,
        "enabled": enabled,
    })
    .to_string();
    let (s, b) = make_request(
        &router,
        Method::PUT,
        "/model/update",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 PUT \\/model\\/update 请求停用模型 {string}")]
async fn when_disable_model(world: &mut TestWorld, model_name: String) {
    when_set_model_enabled(world, &model_name, false).await;
}

#[when(expr = "发送 PUT \\/model\\/update 请求启用模型 {string}")]
async fn when_enable_model(world: &mut TestWorld, model_name: String) {
    when_set_model_enabled(world, &model_name, true).await;
}

/// Stage 121 — POST /chat/completions to a model. The test-world runs with
/// deployment_mode="test", so the env-var fallback is disabled and a disabled
/// model yields 400 model_not_found (no upstream forward). Distinct text from
/// e2e_steps's identical chat step to avoid a cucumber ambiguity (the feature
/// files share the whole step registry).
#[when(
    expr = "使用 key {string} 发送 POST \\/chat\\/completions 请求用 model {string} 且模型已停用"
)]
async fn when_chat_completions_disabled_model(world: &mut TestWorld, alias: String, model: String) {
    let state = world.ensure_state().await;
    let app = Router::new()
        .route(
            "/chat/completions",
            axum::routing::post(aigw_server::routes::chat::chat_completions),
        )
        .layer(tower_http::request_id::SetRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-request-id"),
            aigw_core::request_id::UuidV7RequestId,
        ))
        .with_state(state);

    let token = world.created_keys.get(&alias).expect("key not found");
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "hi"}]
    })
    .to_string();

    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/chat/completions")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .body(axum::body::Body::from(body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    world.last_body = serde_json::from_slice(&body_bytes).ok();
}

#[when(expr = "发送 DELETE \\/model\\/delete 请求删除该模型")]
async fn when_delete_model(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = build_model_router(state);
    let model_id = world
        .created_keys
        .iter()
        .find(|(k, _)| k.starts_with("model:delete-model"))
        .map(|(_, v)| v.clone())
        .expect("model delete-model not stored");
    let uri = format!("/model/delete?model_id={}", model_id);
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
    // Store the model_id so we can verify deletion
    world
        .created_keys
        .insert("deleted_model_id".to_string(), model_id);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "响应包含 model_id 字段")]
async fn then_has_model_id(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    assert!(
        body.get("model_id").is_some(),
        "Body missing 'model_id': {:?}",
        body
    );
}

#[then(regex = "^响应包含 model_name 字段值为 (.+)$")]
async fn then_model_name_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let name = body
        .get("model_name")
        .and_then(|v| v.as_str())
        .expect("no model_name");
    assert_eq!(
        name, expected,
        "Expected model_name '{}', got '{}'",
        expected, name
    );
}

#[then(regex = "^响应中的 data 包含 (\\d+) 个模型$")]
async fn then_data_has_n_models(world: &mut TestWorld, expected: usize) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body
        .get("data")
        .and_then(|v| v.as_array())
        .expect("no data array in response");
    assert_eq!(
        data.len(),
        expected,
        "Expected {} models in data, got {}",
        expected,
        data.len()
    );
}

/// Stage 121 — the /model/update response (a ModelResponse) echoes the
/// current `enabled` state back to the caller.
#[then(regex = "^响应中的 enabled 字段为 (true|false)$")]
async fn then_response_enabled_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let actual = body
        .get("enabled")
        .and_then(|v| v.as_bool())
        .expect("no enabled field in response");
    let expected_bool = expected == "true";
    assert_eq!(
        actual, expected_bool,
        "Expected response.enabled={}, got {}",
        expected, actual
    );
}

/// Stage 121 — find a model in the /model/list `data` array and assert its
/// `enabled` field equals the expected boolean.
#[then(regex = r#"^/model/list 中模型 "(.+)" 的 enabled 字段为 (true|false)$"#)]
async fn then_list_model_enabled_is(world: &mut TestWorld, model_name: String, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body
        .get("data")
        .and_then(|v| v.as_array())
        .expect("no data array in /model/list response");
    let entry = data
        .iter()
        .find(|m| m.get("model_name").and_then(|v| v.as_str()) == Some(model_name.as_str()))
        .unwrap_or_else(|| panic!("model '{}' not in /model/list: {}", model_name, body));
    let actual = entry
        .get("enabled")
        .and_then(|v| v.as_bool())
        .expect("no enabled field in model entry");
    let expected_bool = expected == "true";
    assert_eq!(
        actual, expected_bool,
        "Expected /model/list {} enabled={}, got {}",
        model_name, expected, actual
    );
}

/// Stage 121 — a 400 error from the resolve step carries `error.code =
/// "model_not_found"` (the "disabled ⇒ not forwarded" assertion).
#[then(expr = "响应错误 code 为 {string}")]
async fn then_error_code_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let actual = body
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            panic!(
                "Expected error.code '{}', response has no error.code: {}",
                expected,
                serde_json::to_string_pretty(body).unwrap_or_default()
            )
        });
    assert_eq!(
        actual, expected,
        "Expected error.code '{}', got '{}'",
        expected, actual
    );
}

#[then(expr = "该模型不再存在")]
async fn then_model_gone(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = build_model_router(state);
    let model_id = world
        .created_keys
        .get("deleted_model_id")
        .expect("deleted_model_id not stored");
    let uri = format!("/model/info?model_id={}", model_id);
    let (s, _) = make_request(
        &router,
        Method::GET,
        &uri,
        Some(&world.master_key.clone()),
        None,
    )
    .await;
    assert_eq!(s, 404, "Deleted model still accessible, got status {}", s);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Decryption BDD — uses dedicated global state with AIGW_MASTER_KEY set
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use std::sync::OnceLock;
static DECRYPT_STATE: OnceLock<aigw_server::routes::keys::SharedState> = OnceLock::new();

const DECRYPT_MASTER_KEY: &str = "bdd-decrypt-master-key";

async fn ensure_decrypt_state() -> aigw_server::routes::keys::SharedState {
    if let Some(s) = DECRYPT_STATE.get() {
        return s.clone();
    }
    let db = aigw_core::db::Database::init("sqlite::memory:")
        .await
        .expect("db init");
    let state: aigw_server::routes::keys::SharedState =
        Arc::new(aigw_server::routes::keys::AppState {
            resolver: aigw_core::resolver::ModelResolver::new(db.clone(), None, "onprem"),
            router: aigw_core::router::Router::default(),
            db,
            master_key: Some("sk-decrypt-test".to_string()),
            aigw_master_key: Some(DECRYPT_MASTER_KEY.to_string()),
            key_generate_length: DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: false,
            provider_registry: aigw_core::provider::ProviderRegistry::new(),
            router_state: aigw_core::router::RouterState::default(),
            rate_limiter: Arc::new(aigw_core::rate_limiter::RateLimiter::new()),
            deployment_mode: "test".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            token_provider: std::sync::Arc::new(aigw_core::claude_token::TokenProvider::new()),
            metrics: None,
        });
    let _ = DECRYPT_STATE.set(state.clone());
    state
}

#[given(expr = "已存在一个模型其 litellm_params 包含加密的 api_base 和 api_key")]
async fn existing_model_with_encrypted_fields(_world: &mut TestWorld) {
    let state = ensure_decrypt_state().await;
    let encrypted_api_base = aigw_core::crypto::encrypt_litellm_value(
        "https://decrypted-api.example.com",
        DECRYPT_MASTER_KEY,
    )
    .expect("encrypt api_base");
    let encrypted_api_key =
        aigw_core::crypto::encrypt_litellm_value("sk-decrypted-secret", DECRYPT_MASTER_KEY)
            .expect("encrypt api_key");
    let litellm_params = serde_json::json!({
        "model": "openai/decrypt-test-model",
        "api_base": encrypted_api_base,
        "api_key": encrypted_api_key,
        "custom_llm_provider": "openai",
        "rpm": 100,
    });
    let now = chrono::Utc::now().to_rfc3339();
    let model = aigw_core::models::ProxyModel {
        model_id: uuid::Uuid::new_v4().to_string(),
        model_name: "decrypt-test-model".to_string(),
        litellm_params,
        model_info: serde_json::json!({}),
        created_at: now.clone(),
        created_by: None,
        updated_at: now,
        updated_by: None,
        enabled: true,
    };
    state
        .db
        .insert_model(&model)
        .await
        .expect("insert model with encrypted fields");
}

#[then(regex = r#"^响应中首个模型的 api_base 已解密为 "([^"]+)"$"#)]
async fn then_first_model_api_base_decrypted(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body
        .get("data")
        .and_then(|v| v.as_array())
        .expect("no data array");
    let first = &data[0];
    let api_base = first
        .get("litellm_params")
        .and_then(|v| v.get("api_base"))
        .and_then(|v| v.as_str())
        .expect("no api_base in first model");
    assert_eq!(
        api_base, expected,
        "expected decrypted api_base '{}', got '{}'",
        expected, api_base
    );
}

#[then(regex = r#"^响应中首个模型的 api_key 已解密为 "([^"]+)"$"#)]
async fn then_first_model_api_key_decrypted(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body
        .get("data")
        .and_then(|v| v.as_array())
        .expect("no data array");
    let first = &data[0];
    let api_key = first
        .get("litellm_params")
        .and_then(|v| v.get("api_key"))
        .and_then(|v| v.as_str())
        .expect("no api_key in first model");
    assert_eq!(
        api_key, expected,
        "expected decrypted api_key '{}', got '{}'",
        expected, api_key
    );
}

#[when(expr = "通过解密路由发送 GET \\/model\\/list")]
async fn when_get_model_list_with_decrypt(world: &mut TestWorld) {
    let state = ensure_decrypt_state().await;
    let router = build_model_router(state);
    let mk = "sk-decrypt-test".to_string();
    let (s, b) = make_request(&router, Method::GET, "/model/list", Some(&mk), None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 116: config.yaml model_list seed（静态配置模型接入）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Seed a model via `aigw_core::config_loader::seed_models_from_config` — the
/// exact boot path main.rs uses for `config.yaml model_list`. Reuses the shared
/// test DB so `GET /v1/models` / `/model/list` see the seeded rows.
#[given(expr = "通过 config_loader 从 model_list seed 模型 {string}")]
async fn given_seed_model_from_config(world: &mut TestWorld, name: String) {
    let state = world.ensure_state().await;
    let entry = aigw_core::config::ModelEntry {
        model_name: name.clone(),
        litellm_params: aigw_core::config::ModelParams {
            model: format!("openai/{}", name),
            api_base: Some("https://api.openai.com/v1".to_string()),
            api_key: None,
            rpm: None,
            tpm: None,
            max_parallel_requests: None,
            input_cost_per_token: None,
            output_cost_per_token: None,
            tpm_limit: None,
            rpm_limit: None,
        },
    };
    let stats = aigw_core::config_loader::seed_models_from_config(&state.db, &[entry])
        .await
        .expect("seed models from config");
    // Second call in the idempotency scenario re-seeds the same name; the
    // assertion happens via the model-count step.
    let _ = stats;
}
