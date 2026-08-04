//! Step bindings for router_settings BDD scenarios (auth.feature + router_settings.feature)

use axum::http::Method;
use cucumber::{given, then, when};
use tower::ServiceExt;

use crate::TestWorld;

/// Build an axum Router for router_settings endpoints
fn build_router_settings_router(state: aigw_server::routes::keys::SharedState) -> axum::Router {
    axum::Router::new()
        .route(
            "/router/settings",
            axum::routing::get(aigw_server::routes::router_settings::get_global)
                .put(aigw_server::routes::router_settings::put_global),
        )
        .route(
            "/key/{token}/router/settings",
            axum::routing::patch(aigw_server::routes::router_settings::patch_key),
        )
        .route(
            "/team/{id}/router/settings",
            axum::routing::patch(aigw_server::routes::router_settings::patch_team),
        )
        .with_state(state)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When: router/settings no-auth requests (auth.feature)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "不携带 Authorization 发送 GET \\/router\\/settings 请求")]
async fn when_get_router_settings_noauth(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_router_settings_router(state);
    let req = axum::http::Request::builder()
        .method(Method::GET)
        .uri("/router/settings")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When: use key to call router/settings endpoints (auth.feature)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 key {string} 发送 PUT \\/router\\/settings 请求")]
async fn when_key_put_router_settings(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let app = build_router_settings_router(state);
    let token = world.created_keys.get(&alias).expect("key not found");
    let req = axum::http::Request::builder()
        .method(Method::PUT)
        .uri("/router/settings")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .body(axum::body::Body::from(
            r#"{"routing_strategy":"least_latency"}"#,
        ))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

#[when(expr = "使用 master-key 发送 GET \\/router\\/settings 请求")]
async fn when_master_get_router_settings(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_router_settings_router(state);
    let mk = world.master_key.clone();
    let req = axum::http::Request::builder()
        .method(Method::GET)
        .uri("/router/settings")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", mk))
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

#[when(expr = "使用 master-key 带有效 body 发送 PUT \\/router\\/settings 请求")]
async fn when_master_put_router_settings(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_router_settings_router(state);
    let mk = world.master_key.clone();
    let req = axum::http::Request::builder()
        .method(Method::PUT)
        .uri("/router/settings")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", mk))
        .body(axum::body::Body::from(
            r#"{"routing_strategy":"least_latency","num_retries":2}"#,
        ))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 98: patch_key / patch_team BDD steps (router_settings.feature)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn send_patch_req(
    world: &mut TestWorld,
    uri: String,
    auth: Option<&str>,
    body: serde_json::Value,
) {
    let state = world.ensure_state().await;
    let app = build_router_settings_router(state);
    let req = axum::http::Request::builder()
        .method(Method::PATCH)
        .uri(&uri)
        .header("Content-Type", "application/json");
    let req = if let Some(token) = auth {
        req.header("Authorization", format!("Bearer {}", token))
    } else {
        req
    };
    let req = req
        .body(axum::body::Body::from(body.to_string()))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

#[when(regex = r"^使用 master-key 发送 PATCH key (.+) 的 router_settings 设置 cooldown_time=(\d+)$")]
async fn when_master_patch_key_cooldown(world: &mut TestWorld, token: String, cooldown_time: u32) {
    let uri = format!("/key/{}/router/settings", token);
    send_patch_req(
        world,
        uri,
        Some(&world.master_key.clone()),
        serde_json::json!({"cooldown_time": cooldown_time}),
    )
    .await;
}

#[when(regex = r"^不认证发送 PATCH key (.+) 的 router_settings 设置 cooldown_time=(\d+)$")]
async fn when_patch_key_noauth(world: &mut TestWorld, _token: String, _cooldown_time: u32) {
    let uri = "/key/some-key/router/settings";
    send_patch_req(
        world,
        uri.to_string(),
        None,
        serde_json::json!({"cooldown_time": 30}),
    )
    .await;
}

#[when(regex = r"^使用 master-key 发送 PATCH team (.+) 的 router_settings 设置 num_retries=(\d+)$")]
async fn when_master_patch_team_retry(world: &mut TestWorld, team_id: String, num_retries: u32) {
    let uri = format!("/team/{}/router/settings", team_id);
    send_patch_req(
        world,
        uri,
        Some(&world.master_key.clone()),
        serde_json::json!({"num_retries": num_retries}),
    )
    .await;
}

#[when(regex = r"^不认证发送 PATCH team (.+) 的 router_settings 设置 num_retries=(\d+)$")]
async fn when_patch_team_noauth(world: &mut TestWorld, _team_id: String, _num_retries: u32) {
    let uri = "/team/some-team/router/settings";
    send_patch_req(
        world,
        uri.to_string(),
        None,
        serde_json::json!({"retry_count": 2}),
    )
    .await;
}

#[given(expr = "已存在 team {string}")]
async fn given_team_exists(world: &mut TestWorld, team_alias: String) {
    let state = world.ensure_state().await;
    let team = aigw_core::models::Team {
        team_id: team_alias.clone(),
        team_alias: Some(team_alias.clone()),
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
    };
    state.db.insert_team(&team).await.expect("insert team");
    world
        .created_keys
        .insert(format!("team:{}", team_alias), team_alias.clone());
}

#[then(regex = r"^key (.+) 的 router_settings cooldown_time 为 (\d+)$")]
async fn then_key_cooldown(world: &mut TestWorld, _token: String, expected: u32) {
    let body = world.last_body.as_ref().expect("no response body");
    let ct = body
        .get("cooldown_time")
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| panic!("Expected cooldown_time={expected} in response, got: {body}"));
    assert_eq!(ct as u32, expected, "cooldown_time mismatch");
}

#[then(regex = r"^team (.+) 的 router_settings num_retries 为 (\d+)$")]
async fn then_team_retry(world: &mut TestWorld, _team_id: String, expected: u32) {
    let body = world.last_body.as_ref().expect("no response body");
    let rc = body
        .get("num_retries")
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| panic!("Expected num_retries={expected} in response, got: {body}"));
    assert_eq!(rc as u32, expected, "num_retries mismatch");
}
