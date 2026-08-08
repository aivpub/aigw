//! Step bindings for keys_permission.feature — admin vs non-admin key ownership tests

use aigw_core::auth::{encode_jwt, JwtClaims};
use aigw_core::crypto::hash_token;
use aigw_core::models::VirtualKey;
use aigw_core::password::hash_password;
use axum::http::Method;
use cucumber::{given, then, when};

use super::common::{build_key_router, make_request, make_request_with_cookie};
use crate::TestWorld;

/// Generate a random session token (sk-xxx) similar to login flow.
fn session_token() -> String {
    let mut buf = [0u8; 16];
    for b in &mut buf {
        *b = fastrand::u8(..);
    }
    // base64url encode, take first 22 chars
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in buf.chunks(3) {
        let n = chunk.len();
        let b0 = chunk[0] as u32;
        let b1 = if n > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if n > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(char::from(CHARS[((triple >> 18) & 0x3F) as usize]));
        out.push(char::from(CHARS[((triple >> 12) & 0x3F) as usize]));
        if n > 1 {
            out.push(char::from(CHARS[((triple >> 6) & 0x3F) as usize]));
        }
        if n > 2 {
            out.push(char::from(CHARS[(triple & 0x3F) as usize]));
        }
    }
    format!("sk-{}", &out[..22])
}

/// Create a DB user, insert a session key, and return a valid JWT cookie string.
async fn create_user_and_login(
    world: &mut TestWorld,
    user_id: &str,
    email: &str,
    password: &str,
    role: &str,
) -> String {
    let state = world.ensure_state().await;
    let router = build_key_router(state);

    // Hash the password
    let hashed = hash_password(password).expect("hash password");

    // Insert user via API
    let body = serde_json::json!({
        "user_id": user_id,
        "user_email": email,
        "password": hashed,
        "user_role": role,
    })
    .to_string();
    let (status, _) = make_request(
        &router,
        Method::POST,
        "/user/new",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    assert_eq!(
        status, 200,
        "Failed to create user {}: status={}",
        user_id, status
    );

    world.created_users.insert(
        user_id.to_string(),
        (email.to_string(), password.to_string()),
    );

    // Create a session key in virtual_keys (same as login flow)
    let raw_token = session_token();
    let token_hash = hash_token(&raw_token);
    let now = chrono::Utc::now();
    let session_key = VirtualKey {
        token: token_hash.clone(),
        key_name: Some(format!("ui-session-{}", user_id)),
        key_alias: Some(format!("ui-session-{}", user_id)),
        soft_budget_cooldown: "false".to_string(),
        spend: 0.0,
        expires: Some(now + chrono::Duration::hours(24)),
        models: serde_json::json!([]),
        aliases: serde_json::json!({}),
        config: serde_json::json!({}),
        router_settings: None,
        user_id: Some(user_id.to_string()),
        team_id: Some("litellm-dashboard".to_string()),
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
        created_by: None,
        updated_at: Some(now),
        updated_by: None,
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

    let _ = world.ensure_state().await; // ensure state is live
    let state2 = world.state.clone().unwrap();
    state2.db.insert_key(&session_key).await.unwrap();

    // Create JWT cookie with real session token
    let mk = world.master_key.clone();
    let claims = JwtClaims {
        user_id: user_id.to_string(),
        key: raw_token,
        user_email: Some(email.to_string()),
        user_role: role.to_string(),
        login_method: "username_password".to_string(),
    };
    let jwt = encode_jwt(&claims, &mk).expect("encode JWT");
    format!("token={}", jwt)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(regex = r#"^已存在用户 "(.+)" 邮箱 "(.+)" 角色 "(.+)"$"#)]
async fn given_user_exists(world: &mut TestWorld, user_id: String, email: String, role: String) {
    let cookie = create_user_and_login(world, &user_id, &email, "pass123", &role).await;
    world
        .created_keys
        .insert(format!("cookie-{}", user_id), cookie);
}

#[given(regex = r#"^管理员已创建 key "(.+)" 归属用户 "(.+)"$"#)]
async fn given_admin_created_key_for_user(world: &mut TestWorld, alias: String, user_id: String) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let body = serde_json::json!({"key_alias": &alias, "user_id": &user_id, "models": ["gpt-4"]})
        .to_string();
    let (status, resp_body) = make_request(
        &router,
        Method::POST,
        "/key/generate",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    assert_eq!(status, 200, "Admin create key failed: status={}", status);
    if let Some(ref b) = resp_body {
        if let Some(raw) = b.get("key").and_then(|v| v.as_str()) {
            world.created_keys.insert(alias, raw.to_string());
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(regex = r#"^用户 "(.+)" 创建 key 请求 body:$"#)]
async fn when_user_creates_key(
    world: &mut TestWorld,
    user_id: String,
    step: &cucumber::gherkin::Step,
) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let cookie = world
        .created_keys
        .get(&format!("cookie-{}", user_id))
        .cloned()
        .unwrap_or_else(|| panic!("No cookie for user {}", user_id));
    let body = step
        .docstring
        .as_ref()
        .expect("docstring body not found")
        .to_string();
    let (s, b) = make_request_with_cookie(
        &router,
        Method::POST,
        "/key/generate",
        Some(&cookie),
        Some(&body),
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(regex = r#"^管理员创建 key 请求 body:$"#)]
async fn when_admin_creates_key(world: &mut TestWorld, step: &cucumber::gherkin::Step) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let body = step
        .docstring
        .as_ref()
        .expect("docstring body not found")
        .to_string();
    let (s, b) = make_request(
        &router,
        Method::POST,
        "/key/generate",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(regex = r#"^用户 "(.+)" 查询 key 列表$"#)]
async fn when_user_lists_keys(world: &mut TestWorld, user_id: String) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let cookie = world
        .created_keys
        .get(&format!("cookie-{}", user_id))
        .cloned()
        .unwrap_or_else(|| panic!("No cookie for user {}", user_id));
    let (s, b) =
        make_request_with_cookie(&router, Method::GET, "/key/list", Some(&cookie), None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(regex = r#"^用户 "(.+)" 查询 key "(.+)"$"#)]
async fn when_user_gets_key_info(world: &mut TestWorld, user_id: String, key_ref: String) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let cookie = world
        .created_keys
        .get(&format!("cookie-{}", user_id))
        .cloned()
        .unwrap_or_else(|| panic!("No cookie for user {}", user_id));
    let raw_key = world.created_keys.get(&key_ref).cloned().unwrap_or(key_ref);
    let uri = format!("/key/info?key={}", raw_key);
    let (s, b) = make_request_with_cookie(&router, Method::GET, &uri, Some(&cookie), None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(regex = r#"^响应 JSON "(.+)" 字段值为 "(.+)"$"#)]
async fn then_json_field_eq(world: &mut TestWorld, field: String, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let actual = body
        .get(&field)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            panic!(
                "Expected JSON field '{}' to be string, got: {}",
                field,
                serde_json::to_string_pretty(body).unwrap_or_default()
            )
        });
    assert_eq!(
        actual, expected,
        "Expected field '{}' = '{}', got '{}'",
        field, expected, actual
    );
}

#[then(regex = r#"^响应 key 列表仅包含 "(.+)" 个 key$"#)]
async fn then_key_list_count_is(world: &mut TestWorld, expected: usize) {
    let body = world.last_body.as_ref().expect("no response body");
    let keys = body
        .get("keys")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| {
            panic!(
                "No 'keys' array in response: {}",
                serde_json::to_string_pretty(body).unwrap_or_default()
            )
        });
    assert_eq!(
        keys.len(),
        expected,
        "Expected {} keys in list, got {}: {}",
        expected,
        keys.len(),
        serde_json::to_string_pretty(body).unwrap_or_default()
    );
}

#[then(regex = r#"^响应 key 列表中的 user_id 均为 "(.+)"$"#)]
async fn then_all_keys_belong_to(world: &mut TestWorld, user_id: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let keys = body
        .get("keys")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("No 'keys' array in response"));
    for key_entry in keys {
        let kid = key_entry
            .get("user_id")
            .and_then(|v| v.as_str())
            .unwrap_or("NONE");
        assert_eq!(
            kid, &user_id,
            "Key has user_id '{}', expected '{}'",
            kid, user_id
        );
    }
}
