//! Step bindings for deleted_list.feature — Stage 98

use axum::http::Method;
use axum::Router;
use cucumber::{given, then, when};
use tower::ServiceExt;

use crate::TestWorld;

/// Build an axum Router with deleted list endpoints for testing.
fn build_deleted_list_router(state: aigw_server::routes::keys::SharedState) -> Router {
    Router::new()
        .route(
            "/team/deleted",
            axum::routing::get(aigw_server::routes::team::team_deleted_list),
        )
        .route(
            "/model/deleted",
            axum::routing::get(aigw_server::routes::models::model_deleted_list),
        )
        .route(
            "/user/deleted",
            axum::routing::get(aigw_server::routes::user::user_deleted_list),
        )
        .route(
            "/org/deleted",
            axum::routing::get(aigw_server::routes::org::org_deleted_list),
        )
        .with_state(state)
}

/// Helper: send a simple GET request and store status/body on world.
async fn send_get(world: &mut TestWorld, uri: &str, auth: Option<&str>) {
    let state = world.ensure_state().await;
    let app = build_deleted_list_router(state);
    let req = axum::http::Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header("Content-Type", "application/json");
    let req = if let Some(token) = auth {
        req.header("Authorization", format!("Bearer {}", token))
    } else {
        req
    };
    let req = req.body(axum::body::Body::empty()).unwrap();
    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

// ━━━━ Given helpers ━━━━

fn make_team(alias: &str) -> aigw_core::models::Team {
    aigw_core::models::Team {
        team_id: alias.to_string(),
        team_alias: Some(alias.to_string()),
        organization_id: None,
        object_permission_id: None,
        admins: serde_json::json!([]),
        members: serde_json::json!([]),
        members_with_roles: serde_json::json!([]),
        metadata: serde_json::json!({}),
        max_budget: None,
        soft_budget: None,
        spend: 0.0,
        models: serde_json::json!([]),
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
    }
}

/// Assert an alias appears in the response. The response may be either
/// `{"data": [...]}` (paginated: team/model/org) or a plain JSON array (user).
fn assert_alias_in_data(body: &serde_json::Value, alias: &str) {
    let entries: Vec<String> = if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
        // Paginated: team/model/org responses
        data.iter()
            .map(|e| serde_json::to_string(e).unwrap_or_default())
            .collect()
    } else if let Some(arr) = body.as_array() {
        // Plain array: user response
        arr.iter()
            .map(|e| serde_json::to_string(e).unwrap_or_default())
            .collect()
    } else {
        panic!(
            "Expected response to have 'data' array or be a plain array, got: {}",
            serde_json::to_string_pretty(body).unwrap_or_default()
        );
    };
    let found = entries.iter().any(|s| s.contains(alias));
    assert!(
        found,
        "Expected '{}' in deleted list, got entries: {:?}",
        alias, entries
    );
}

/// Assert an alias does NOT appear in the response.
fn assert_alias_not_in_data(body: &serde_json::Value, alias: &str) {
    let entries: Vec<String> = if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
        data.iter()
            .map(|e| serde_json::to_string(e).unwrap_or_default())
            .collect()
    } else if let Some(arr) = body.as_array() {
        arr.iter()
            .map(|e| serde_json::to_string(e).unwrap_or_default())
            .collect()
    } else {
        panic!("Expected response to have 'data' array or be a plain array");
    };
    let found = entries.iter().any(|s| s.contains(alias));
    assert!(
        !found,
        "Did NOT expect '{}' in deleted list, but it was found",
        alias
    );
}

// ━━━━ Background & Given ━━━━

#[given(expr = "数据库中已有 {string} 和 {string} 两个正常 team")]
async fn given_background_teams(world: &mut TestWorld, team1: String, team2: String) {
    let state = world.ensure_state().await;
    for alias in [&team1, &team2] {
        state
            .db
            .insert_team(&make_team(alias))
            .await
            .expect("insert team");
    }
}

/// Soft-delete a team: insert it, then call delete_team() which
/// tombstone-then-delete (archive into deleted_teams + remove from teams).
#[given(expr = "team {string} 已被软删除")]
async fn given_team_soft_deleted(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    state
        .db
        .insert_team(&make_team(&alias))
        .await
        .expect("insert team");
    state
        .db
        .delete_team(&alias)
        .await
        .expect("delete team -> soft delete");
}

/// Soft-delete a model: insert then delete.
#[given(expr = "model {string} 已被软删除")]
async fn given_model_soft_deleted(world: &mut TestWorld, model_name: String) {
    let state = world.ensure_state().await;
    let model_id = uuid::Uuid::new_v4().to_string();
    let model = aigw_core::models::ProxyModel {
        model_id: model_id.clone(),
        model_name: model_name.clone(),
        litellm_params: serde_json::json!({"model": &model_name, "custom_llm_provider": "openai"}),
        model_info: serde_json::json!({}),
        created_at: chrono::Utc::now().to_rfc3339(),
        created_by: Some("test".to_string()),
        updated_at: chrono::Utc::now().to_rfc3339(),
        updated_by: Some("test".to_string()),
    };
    state.db.insert_model(&model).await.expect("insert model");
    state
        .db
        .delete_model(&model_id)
        .await
        .expect("delete model -> soft delete");
}

/// Soft-delete a user: insert then delete.
#[given(expr = "user {string} 已被软删除")]
async fn given_user_soft_deleted(world: &mut TestWorld, user_id: String) {
    let state = world.ensure_state().await;
    let user = aigw_core::models::User {
        user_id: user_id.clone(),
        user_alias: Some(user_id.clone()),
        team_id: None,
        sso_user_id: None,
        organization_id: None,
        object_permission_id: None,
        password: None,
        teams: serde_json::json!([]),
        user_role: Some("user".to_string()),
        max_budget: None,
        spend: 0.0,
        user_email: Some(format!("{}@test.com", user_id)),
        models: serde_json::json!([]),
        metadata: serde_json::json!({}),
        max_parallel_requests: None,
        tpm_limit: None,
        rpm_limit: None,
        budget_duration: None,
        budget_reset_at: None,
        allowed_cache_controls: serde_json::json!([]),
        policies: serde_json::json!([]),
        model_spend: serde_json::json!({}),
        model_max_budget: serde_json::json!({}),
        virtual_keys_count: None,
        created_at: Some(chrono::Utc::now()),
        updated_at: Some(chrono::Utc::now()),
    };
    state.db.insert_user(&user).await.expect("insert user");
    state
        .db
        .delete_user(&user_id)
        .await
        .expect("delete user -> soft delete");
}

/// Soft-delete an org: insert then delete.
#[given(expr = "org {string} 已被软删除")]
async fn given_org_soft_deleted(world: &mut TestWorld, org_id: String) {
    let state = world.ensure_state().await;
    let org = aigw_core::models::Organization {
        organization_id: org_id.clone(),
        organization_alias: org_id.clone(),
        budget_id: "default".to_string(),
        metadata: serde_json::json!({}),
        models: serde_json::json!([]),
        spend: 0.0,
        model_spend: serde_json::json!({}),
        object_permission_id: None,
        created_at: chrono::Utc::now(),
        created_by: "test".to_string(),
        updated_at: chrono::Utc::now(),
        updated_by: "test".to_string(),
    };
    state
        .db
        .insert_organization(&org)
        .await
        .expect("insert org");
    state
        .db
        .delete_organization(&org_id)
        .await
        .expect("delete org -> soft delete");
}

// ━━━━ When ━━━━

#[when(expr = "使用 admin 认证发送 GET \\/team\\/deleted 请求")]
async fn when_get_team_deleted(world: &mut TestWorld) {
    send_get(world, "/team/deleted", Some(&world.master_key.clone())).await;
}

#[when(expr = "使用 admin 认证发送 GET \\/model\\/deleted 请求")]
async fn when_get_model_deleted(world: &mut TestWorld) {
    send_get(world, "/model/deleted", Some(&world.master_key.clone())).await;
}

#[when(expr = "使用 admin 认证发送 GET \\/user\\/deleted 请求")]
async fn when_get_user_deleted(world: &mut TestWorld) {
    send_get(world, "/user/deleted", Some(&world.master_key.clone())).await;
}

#[when(expr = "使用 admin 认证发送 GET \\/org\\/deleted 请求")]
async fn when_get_org_deleted(world: &mut TestWorld) {
    send_get(world, "/org/deleted", Some(&world.master_key.clone())).await;
}

// ━━━━ Then ━━━━

#[then(expr = "{string} 在返回的 deleted 列表中")]
async fn then_alias_in_list(world: &mut TestWorld, alias: String) {
    let body = world.last_body.as_ref().expect("no response body");
    assert_alias_in_data(body, &alias);
}

#[then(expr = "{string} 不在返回结果中")]
async fn then_alias_not_in_list(world: &mut TestWorld, alias: String) {
    let body = world.last_body.as_ref().expect("no response body");
    assert_alias_not_in_data(body, &alias);
}
