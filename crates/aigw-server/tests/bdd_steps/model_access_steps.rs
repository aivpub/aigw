//! Step bindings for model_access.feature — sentinel value tests
//!
//! Uses global state to persist DB across steps within a scenario.

use cucumber::{given, then, when};
use axum::http::Method;
use axum::Router;
use std::sync::{Arc, OnceLock};

use super::common::make_request;
use crate::TestWorld;

/// Global shared state for model access BDD tests
static GLOBAL_STATE: OnceLock<aigw_server::routes::keys::SharedState> = OnceLock::new();

async fn ensure_global_state() -> aigw_server::routes::keys::SharedState {
    if let Some(s) = GLOBAL_STATE.get() {
        return s.clone();
    }
    let db = aigw_core::db::Database::init("sqlite::memory:").await.expect("db init");
    let state: aigw_server::routes::keys::SharedState = Arc::new(
        aigw_server::routes::keys::AppState {
            resolver: aigw_core::resolver::ModelResolver::new(db.clone(), None, "onprem"),
            router: aigw_core::router::Router::default(),
            db,
            master_key: Some("sk-master-test".to_string()),
            aigw_master_key: None,
            provider_registry: aigw_core::provider::ProviderRegistry::new(),
            router_state: aigw_core::router::RouterState::default(),
            rate_limiter: Arc::new(aigw_core::rate_limiter::RateLimiter::new()),
            deployment_mode: "test".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            metrics: None,
        },
    );
    // Set it — if another thread beat us, use theirs
    let _ = GLOBAL_STATE.set(state.clone());
    state
}

fn build_chat_router(state: aigw_server::routes::keys::SharedState) -> Router {
    Router::new()
        .route(
            "/v1/chat/completions",
            axum::routing::post(aigw_server::routes::chat::chat_completions),
        )
        .with_state(state)
}

fn build_vk(alias: &str, models: &serde_json::Value, team_id: Option<&str>) -> (String, aigw_core::models::VirtualKey) {
    let raw_key = format!("sk-bdd-{}", alias);
    let token_hash = aigw_core::crypto::hash_token(&raw_key);
    let key = aigw_core::models::VirtualKey {
        token: token_hash,
        key_name: Some(alias.to_string()),
        key_alias: Some(alias.to_string()),
        soft_budget_cooldown: "false".to_string(),
        spend: 0.0,
        expires: None,
        models: models.clone(),
        aliases: serde_json::json!({}),
        config: serde_json::json!({}),
        router_settings: None,
        user_id: None,
        team_id: team_id.map(String::from),
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
        created_at: Some(chrono::Utc::now()),
        created_by: None,
        updated_at: Some(chrono::Utc::now()),
        updated_by: None,
        last_active: None,
        rotation_count: None,
        auto_rotate: None,
        rotation_interval: None,
        last_rotation_at: None,
        key_rotation_at: None,
        budget_limits: None,
    };
    (raw_key, key)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(regex = r#"^已存在团队 "([^"]+)" 允许模型 (.+)$"#)]
async fn given_team_with_models(_world: &mut TestWorld, team_id: String, models_json: String) {
    let state = ensure_global_state().await;
    let models: serde_json::Value = serde_json::from_str(&models_json)
        .unwrap_or_else(|_| serde_json::json!([]));

    let team = aigw_core::models::Team {
        team_id: team_id.clone(),
        team_alias: Some(team_id),
        organization_id: None,
        object_permission_id: None,
        admins: serde_json::json!([]),
        members: serde_json::json!([]),
        members_with_roles: serde_json::json!([]),
        metadata: serde_json::json!({}),
        max_budget: None,
        soft_budget: None,
        spend: 0.0,
        models,
        max_parallel_requests: None,
        tpm_limit: None,
        rpm_limit: None,
        budget_duration: None,
        budget_reset_at: None,
        blocked: false,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        model_spend: serde_json::json!({}),
        model_max_budget: serde_json::json!({}),
        router_settings: None,
        team_member_permissions: serde_json::json!([]),
        access_group_ids: serde_json::json!([]),
        policies: serde_json::json!([]),
        default_team_member_models: serde_json::json!([]),
        budget_limits: None,
        model_id: None,
        allow_team_guardrail_config: false,
    };
    state.db.insert_team(&team).await.expect("insert team");
}

#[given(regex = r#"^已存在独立密钥 "([^"]+)" 模型 (.+)$"#)]
async fn given_standalone_key(world: &mut TestWorld, alias: String, models_json: String) {
    let state = ensure_global_state().await;
    let models: serde_json::Value = serde_json::from_str(&models_json)
        .unwrap_or_else(|_| serde_json::json!([]));
    let (raw_key, key) = build_vk(&alias, &models, None);
    state.db.insert_key(&key).await.expect("insert key");
    world.created_keys.insert(alias, raw_key);
}

#[given(regex = r#"^已存在密钥关联团队 "([^"]+)" 模型 (.+) 团队 "([^"]+)"$"#)]
async fn given_key_with_team(
    world: &mut TestWorld,
    alias: String,
    models_json: String,
    team_id: String,
) {
    let state = ensure_global_state().await;
    let models: serde_json::Value = serde_json::from_str(&models_json)
        .unwrap_or_else(|_| serde_json::json!([]));
    let (raw_key, key) = build_vk(&alias, &models, Some(&team_id));
    state.db.insert_key(&key).await.expect("insert key");
    world.created_keys.insert(alias, raw_key);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(regex = r#"^使用密钥 "([^"]+)" 请求 "([^"]+)" 模型 "([^"]+)"$"#)]
async fn when_chat_with_key(world: &mut TestWorld, alias: String, _path: String, model: String) {
    let state = ensure_global_state().await;
    let raw_key = world.created_keys.get(&alias).cloned().expect("key not found");

    let router = build_chat_router(state);
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Hello"}]
    })
    .to_string();

    let (status, body) = make_request(
        &router,
        Method::POST,
        "/v1/chat/completions",
        Some(&raw_key),
        Some(&body),
    )
    .await;
    world.last_status = Some(status);
    world.last_body = body;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "模型检查通过")]
async fn then_model_check_passed(world: &mut TestWorld) {
    let status = world.last_status.expect("no status");
    assert_ne!(status, 403, "Model check failed, got 403: {:?}", world.last_body);
}
