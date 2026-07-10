//! Step bindings for models.feature

use cucumber::{given, then, when};
use cucumber::gherkin::Step;
use axum::http::Method;
use axum::Router;
use std::sync::Arc;

use super::common::make_request;
use crate::TestWorld;

/// Build a router with only model routes
fn build_model_router(
    state: aigw_server::routes::keys::SharedState,
) -> Router {
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
async fn create_model(
    router: &Router,
    mk: &str,
    name: &str,
) -> (u16, Option<serde_json::Value>) {
    let body = serde_json::json!({
        "model_name": name,
        "litellm_params": {"model": format!("openai/{}", name)},
    })
    .to_string();
    make_request(router, Method::POST, "/model/new", Some(mk), Some(&body)).await
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
    assert_eq!(name, expected, "Expected model_name '{}', got '{}'", expected, name);
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
    let db = aigw_core::db::Database::init("sqlite::memory:").await.expect("db init");
    let state: aigw_server::routes::keys::SharedState = Arc::new(
        aigw_server::routes::keys::AppState {
            db,
            master_key: Some("sk-decrypt-test".to_string()),
            aigw_master_key: Some(DECRYPT_MASTER_KEY.to_string()),
            provider_registry: aigw_core::provider::ProviderRegistry::new(),
            router_state: aigw_core::router::RouterState::default(),
            rate_limiter: Arc::new(aigw_core::rate_limiter::RateLimiter::new()),
            deployment_mode: "test".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
        },
    );
    let _ = DECRYPT_STATE.set(state.clone());
    state
}

#[given(expr = "已存在一个模型其 litellm_params 包含加密的 api_base 和 api_key")]
async fn existing_model_with_encrypted_fields(_world: &mut TestWorld) {
    let state = ensure_decrypt_state().await;
    let encrypted_api_base =
        aigw_core::crypto::encrypt_litellm_value("https://decrypted-api.example.com", DECRYPT_MASTER_KEY)
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
    };
    state.db.insert_model(&model).await.expect("insert model with encrypted fields");
}

#[then(regex = r#"^响应中首个模型的 api_base 已解密为 "([^"]+)"$"#)]
async fn then_first_model_api_base_decrypted(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body.get("data").and_then(|v| v.as_array()).expect("no data array");
    let first = &data[0];
    let api_base = first
        .get("litellm_params")
        .and_then(|v| v.get("api_base"))
        .and_then(|v| v.as_str())
        .expect("no api_base in first model");
    assert_eq!(api_base, expected, "expected decrypted api_base '{}', got '{}'", expected, api_base);
}

#[then(regex = r#"^响应中首个模型的 api_key 已解密为 "([^"]+)"$"#)]
async fn then_first_model_api_key_decrypted(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body.get("data").and_then(|v| v.as_array()).expect("no data array");
    let first = &data[0];
    let api_key = first
        .get("litellm_params")
        .and_then(|v| v.get("api_key"))
        .and_then(|v| v.as_str())
        .expect("no api_key in first model");
    assert_eq!(api_key, expected, "expected decrypted api_key '{}', got '{}'", expected, api_key);
}

#[when(expr = "通过解密路由发送 GET \\/model\\/list")]
async fn when_get_model_list_with_decrypt(world: &mut TestWorld) {
    let state = ensure_decrypt_state().await;
    let router = build_model_router(state);
    let mk = "sk-decrypt-test".to_string();
    let (s, b) = make_request(
        &router,
        Method::GET,
        "/model/list",
        Some(&mk),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}
