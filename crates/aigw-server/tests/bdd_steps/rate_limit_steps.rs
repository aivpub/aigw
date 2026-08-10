//! Step bindings for rate_limit.feature — Stage 99

use cucumber::{given, then, when};

use crate::TestWorld;

fn make_virtual_key(
    alias: &str,
    rpm: Option<String>,
    tpm: Option<String>,
) -> aigw_core::models::VirtualKey {
    let now = chrono::Utc::now();
    aigw_core::models::VirtualKey {
        token: alias.to_string(),
        key_name: Some(alias.to_string()),
        key_alias: Some(alias.to_string()),
        soft_budget_cooldown: String::new(),
        spend: 0.0,
        expires: None,
        models: serde_json::json!(["gpt-4"]),
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
        blocked: Some(false),
        tpm_limit: tpm,
        rpm_limit: rpm,
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
    }
}

/// Call enforce_limits and store the result on the world.
async fn enforce_limits_for_key(world: &mut TestWorld, token: &str, token_estimate: u32) {
    let state = world.ensure_state().await;
    let key_identity = aigw_core::middleware::KeyIdentity {
        token_hash: token.to_string(),
        key_alias: Some(token.to_string()),
        user_id: None,
        team_id: None,
        organization_id: None,
        is_master_key: false,
        user_role: None,
    };
    let result = aigw_core::middleware::rate_limit::enforce_limits(
        &state.db,
        &state.rate_limiter,
        &key_identity,
        token_estimate,
    )
    .await;

    match result {
        Ok(()) => {
            world.last_status = Some(200);
            world.last_body = Some(serde_json::json!({"result": "ok"}));
        }
        Err(e) => {
            let (status_code, err_type) = match &e {
                aigw_core::middleware::rate_limit::LimitError::RateLimited { .. } => {
                    (429, "rate_limited")
                }
                aigw_core::middleware::rate_limit::LimitError::BudgetExceeded { .. } => {
                    (429, "budget_exceeded")
                }
                aigw_core::middleware::rate_limit::LimitError::Internal(_) => (500, "internal"),
            };
            world.last_status = Some(status_code);
            world.last_body = Some(serde_json::json!({"error": err_type}));
        }
    }
}

// ━━━━ Given ━━━━

#[given(regex = r"^key (.+) 的 rpm_limit=(\d+)$")]
async fn given_key_rpm_limit(world: &mut TestWorld, alias: String, rpm_limit: u32) {
    let state = world.ensure_state().await;
    let key = make_virtual_key(&alias, Some(rpm_limit.to_string()), None);
    state.db.insert_key(&key).await.expect("insert key");
    world.created_keys.insert(alias.clone(), alias);
}

#[given(regex = r"^key (.+) 的 tpm_limit=(\d+)$")]
async fn given_key_tpm_limit(world: &mut TestWorld, alias: String, tpm_limit: u32) {
    let state = world.ensure_state().await;
    let key = make_virtual_key(&alias, None, Some(tpm_limit.to_string()));
    state.db.insert_key(&key).await.expect("insert key");
    world.created_keys.insert(alias.clone(), alias);
}

/// Stage 117: update the RPM limit of an already-existing key (one that was
/// created via a mock/BDD step with a hashed token). Unlike `key ... 的
/// rpm_limit=` (which inserts a key whose token is the raw alias — only usable
/// by the function-level `enforce_limits` steps), this step updates the row in
/// place so the HTTP-level guard path can exercise RPM enforcement end-to-end.
/// The alias is matched WITHOUT surrounding quotes (regex captures the bare
/// token) so it aligns with the `一个普通 key ... 已生成` steps.
#[given(regex = r#"^更新 key "(.+)" 的 rpm_limit=(\d+)$"#)]
async fn given_update_key_rpm_limit(world: &mut TestWorld, alias: String, rpm_limit: u32) {
    let state = world.ensure_state().await;
    let raw = world
        .created_keys
        .get(&alias)
        .cloned()
        .unwrap_or_else(|| format!("sk-{}", alias));
    let token_hash = aigw_core::crypto::hash_token(&raw);
    let key = state
        .db
        .get_key_by_token(&token_hash)
        .await
        .expect("lookup");
    if let Some(mut k) = key {
        k.rpm_limit = Some(rpm_limit.to_string());
        state
            .db
            .update_key(&token_hash, &k)
            .await
            .expect("update key rpm");
    } else {
        // No existing row (shouldn't happen with a prior 已生成 step) — create
        // one with a hashed token so the HTTP guard path authenticates.
        let mut k = make_virtual_key(&alias, Some(rpm_limit.to_string()), None);
        k.token = token_hash;
        state.db.insert_key(&k).await.expect("insert key");
        world.created_keys.insert(alias.clone(), raw);
    }
}

/// Stage 117: set spend/soft_budget/max_budget on an already-hashed key so the
/// HTTP-level guard path exercises the soft/hard budget branches of
/// `check_budget_multi`. Alias matched WITHOUT quotes (aligns with the
/// `一个普通 key ... 已生成` steps).
#[given(regex = r#"^key "(.+)" 的 spend=(\d+) soft_budget=(\d+) max_budget=(\d+)$"#)]
async fn given_key_budget_limits(
    world: &mut TestWorld,
    alias: String,
    spend: u32,
    soft_budget: u32,
    max_budget: u32,
) {
    let state = world.ensure_state().await;
    let raw = world
        .created_keys
        .get(&alias)
        .cloned()
        .unwrap_or_else(|| format!("sk-{}", alias));
    let token_hash = aigw_core::crypto::hash_token(&raw);
    let key = state
        .db
        .get_key_by_token(&token_hash)
        .await
        .expect("lookup");
    if let Some(mut k) = key {
        k.spend = spend as f64;
        k.soft_budget = Some(soft_budget.to_string());
        k.max_budget = Some(max_budget.to_string());
        state
            .db
            .update_key(&token_hash, &k)
            .await
            .expect("update key budget");
    } else {
        let mut k = make_virtual_key(&alias, None, None);
        k.token = token_hash;
        k.spend = spend as f64;
        k.soft_budget = Some(soft_budget.to_string());
        k.max_budget = Some(max_budget.to_string());
        state.db.insert_key(&k).await.expect("insert key");
        world.created_keys.insert(alias.clone(), raw);
    }
}

#[given(regex = r"^过去 1 分钟内已使用 key (.+) 发送 (\d+) 个请求$")]
async fn given_key_already_used_rpm(world: &mut TestWorld, alias: String, count: u32) {
    let state = world.ensure_state().await;
    let limit = count as i64;
    for _ in 0..count {
        state
            .rate_limiter
            .check(&alias, Some(limit), None, 0)
            .await
            .ok();
    }
}

#[given(regex = r"^过去 1 分钟内已使用 key (.+) 消费 (\d+) tokens$")]
async fn given_key_already_used_tpm(world: &mut TestWorld, alias: String, tokens: u32) {
    let state = world.ensure_state().await;
    state
        .rate_limiter
        .check(&alias, None, Some(tokens as i64), tokens)
        .await
        .ok();
}

// ━━━━ When ━━━━

#[when(regex = r"^使用 key (.+) 的 enforce_limits（token_estimate=(\d+)）$")]
async fn when_enforce_limits(world: &mut TestWorld, alias: String, token_estimate: u32) {
    enforce_limits_for_key(world, &alias, token_estimate).await;
}

// ━━━━ Then ━━━━

#[then(expr = "enforce_limits 返回 LimitError::RateLimited")]
async fn then_rate_limited(world: &mut TestWorld) {
    assert_eq!(
        world.last_status,
        Some(429),
        "Expected status 429 (RateLimited), got {:?}",
        world.last_status
    );
}

#[then(regex = r"^错误类型是 (.+)$")]
async fn then_error_type_is(world: &mut TestWorld, error_type: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let actual = body["error"].as_str().unwrap_or("");
    assert_eq!(
        actual, error_type,
        "Expected error type '{}', got '{}'",
        error_type, actual
    );
}

#[then(expr = "enforce_limits 返回 OK 不触发限制")]
async fn then_ok_no_limit(world: &mut TestWorld) {
    assert_eq!(
        world.last_status,
        Some(200),
        "Expected status 200 (no limit), got {:?}",
        world.last_status
    );
}

#[then(regex = r"^响应头包含 x-ratelimit-limit$")]
async fn then_headers_contain_ratelimit_limit(world: &mut TestWorld) {
    let headers = world
        .last_headers
        .as_ref()
        .expect("no captured response headers");
    assert!(
        headers.contains_key("x-ratelimit-limit"),
        "response missing x-ratelimit-limit header"
    );
}
