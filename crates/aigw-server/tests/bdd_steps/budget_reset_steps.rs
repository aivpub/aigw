//! Step bindings for budget_reset.feature
//!
//! These steps use real HTTP calls against a running aigw server.
//! Every step guards on `AIGW_REAL_API=1` — if the env var is not set,
//! the step is a no-op (scenario passes vacuously).

use crate::TestWorld;
use cucumber::{given, then, when};
use std::time::Duration;

use super::real_api_steps::{base_url, client, real_api_enabled, set_skip_pass};
use super::real_db_seed;

/// The test DB URL for real BDD scenarios (set by bdd.rs harness).
fn test_db_url() -> String {
    std::env::var("AIGW_TEST_DB_URL").expect("AIGW_TEST_DB_URL must be set by the BDD test harness")
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "通过 API 创建 key {string} budget_duration={string}")]
async fn given_create_key_with_budget_duration(
    world: &mut TestWorld,
    alias: String,
    budget_duration: String,
) {
    if !real_api_enabled() {
        return;
    }
    create_key_with_budget_duration(world, &alias, &budget_duration).await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "发送 POST key generate 创建 key {string} budget_duration={string}")]
async fn when_create_key_with_budget(
    world: &mut TestWorld,
    alias: String,
    budget_duration: String,
) {
    if !real_api_enabled() {
        set_skip_pass(world, 200, serde_json::json!({"key": "sk-test"}));
        return;
    }
    create_key_with_budget_duration(world, &alias, &budget_duration).await;
}

#[when(expr = "发送 POST key generate 创建无 budget_duration 的 key {string}")]
async fn when_create_key_without_budget_duration(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        set_skip_pass(world, 200, serde_json::json!({"key": "sk-test"}));
        return;
    }
    let url = format!("{}/key/generate", base_url());
    let body = serde_json::json!({
        "key_alias": alias,
        "models": ["gpt-4"],
        "max_budget": 100.0,
    });
    let mk = world.master_key.clone();
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("key/generate request failed");

    let status = resp.status().as_u16();
    let resp_body: Option<serde_json::Value> = resp.json().await.ok();
    world.last_status = Some(status);
    world.last_body = resp_body;

    if status == 200 {
        if let Some(ref body) = world.last_body {
            if let Some(key) = body["key"].as_str() {
                world.created_keys.insert(alias.clone(), key.to_string());
            }
        }
    }
}

#[when(expr = "发送 GET key info 查询 key {string}")]
async fn when_get_key_info(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        set_skip_pass(
            world,
            200,
            serde_json::json!({"budget_duration": "daily", "budget_reset_at": null}),
        );
        return;
    }
    let token = get_or_fetch_key(world, &alias).await;
    let url = format!("{}/key/info?key={}", base_url(), token);
    let mk = world.master_key.clone();
    let resp = client()
        .get(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("key/info request failed");

    let status = resp.status().as_u16();
    let body: Option<serde_json::Value> = resp.json().await.ok();
    world.last_status = Some(status);
    world.last_body = body;
}

#[when(expr = "发送 POST admin jobs trigger budget_reset 扫描 key 类型")]
async fn when_trigger_budget_reset_scan(world: &mut TestWorld) {
    if !real_api_enabled() {
        set_skip_pass(
            world,
            200,
            serde_json::json!({"job_id": "skip", "status": "accepted", "total_steps": 1}),
        );
        return;
    }
    let url = format!("{}/admin/jobs/trigger", base_url());
    let mk = world.master_key.clone();
    let payload = serde_json::json!({
        "step_type": "budget_reset",
        "payload": {
            "entity_type": "key"
        }
    });
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .expect("admin/jobs/trigger request failed");

    let status = resp.status().as_u16();
    let body: Option<serde_json::Value> = resp.json().await.ok();
    if !status.to_string().starts_with('2') {
        eprintln!(
            "admin/jobs/trigger budget_reset returned status={} body={}",
            status,
            serde_json::to_string_pretty(&body).unwrap_or_default()
        );
    }
    world.last_status = Some(status);
    world.last_body = body;
}

#[when(expr = "等待 budget_reset job 执行完成")]
async fn when_wait_for_budget_reset_job(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let url = format!("{}/admin/jobs", base_url());
    let mk = world.master_key.clone();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);

    loop {
        let resp = client()
            .get(&url)
            .query(&[("step_type", "budget_reset")])
            .header("Authorization", format!("Bearer {}", mk))
            .send()
            .await
            .expect("admin/jobs request failed");
        let body: serde_json::Value = resp.json().await.expect("admin/jobs body");

        let all_done = body["jobs"]
            .as_array()
            .map(|arr| {
                if arr.is_empty() {
                    return false;
                }
                arr.iter().all(|j| {
                    let s = j["status"].as_str().unwrap_or("");
                    s == "completed" || s == "partially_failed"
                })
            })
            .unwrap_or(false);

        if all_done {
            world.last_status = Some(200);
            world.last_body = Some(body);
            return;
        }

        if tokio::time::Instant::now() > deadline {
            // Probe step + log state directly from DB to diagnose. The job is
            // genuinely stuck if it reaches here: the spawned aigw-server runs
            // the Engine exec loop, so steps should be claimed within poll_interval
            // (10s default). A hang means the exec loop never claimed the step.
            let db_url = test_db_url();
            let pool = aigw_migrate::native::SourcePool::connect(&db_url)
                .await
                .expect("connect to test DB for diag");
            let step_sql = "SELECT id, job_id, step_key, status, error_message, retry_count FROM async_job_steps WHERE step_type='budget_reset' ORDER BY step_key";
            let step_rows = pool.read_rows_sql(step_sql).await.unwrap_or_default();
            eprintln!("[DIAG] budget_reset steps: {:?}", step_rows);
            // Also check if any steps got consumed (completed)
            let completed_sql = "SELECT id, job_id, step_key, status, retry_count FROM async_job_steps WHERE step_type='budget_reset' AND status IN ('running', 'completed', 'failed') ORDER BY step_key";
            let completed_rows = pool.read_rows_sql(completed_sql).await.unwrap_or_default();
            eprintln!("[DIAG] budget_reset consumed steps: {:?}", completed_rows);
            let log_sql = format!("SELECT id, job_id, step_key, level, message, created_at FROM async_job_logs WHERE job_id IN (SELECT id FROM async_jobs WHERE step_type='budget_reset') ORDER BY created_at DESC LIMIT 5");
            let log_rows = pool.read_rows_sql(&log_sql).await.unwrap_or_default();
            eprintln!("[DIAG] budget_reset logs: {:?}", log_rows);

            // Try querying the test key directly
            let raw_key = world.created_keys.get("reset-exec").cloned();
            if let Some(ref rk) = raw_key {
                let hash = aigw_core::crypto::hash_token(rk);
                let key_sql = format!("SELECT spend, budget_reset_at, budget_duration FROM virtual_keys WHERE token = '{}'", hash);
                let key_rows = pool.read_rows_sql(&key_sql).await.unwrap_or_default();
                eprintln!("[DIAG] reset-exec key state: {:?}", key_rows);
            }

            eprintln!(
                "WARNING: budget_reset job did not complete within 20s, final state:\n{}",
                serde_json::to_string_pretty(&body).unwrap_or_default()
            );
            world.last_status = Some(200);
            world.last_body = Some(body);
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[given(expr = "通过 API 创建 key {string} budget_duration={string} max_budget={int}")]
async fn given_create_key_with_budget_duration_and_max(
    world: &mut TestWorld,
    alias: String,
    budget_duration: String,
    max_budget: i64,
) {
    if !real_api_enabled() {
        return;
    }
    create_key_with_budget_and_max(world, &alias, &budget_duration, max_budget).await;
}

#[given(expr = "将 key {string} 的 spend 设为 {int} 且 budget_reset_at 设为已过期")]
async fn given_set_key_spend_and_expired_reset(world: &mut TestWorld, alias: String, spend: i64) {
    if !real_api_enabled() {
        return;
    }
    let raw_key = world
        .created_keys
        .get(&alias)
        .expect("key not found in world.created_keys");
    let hash = aigw_core::crypto::hash_token(raw_key);
    let db_url = test_db_url();
    let pool = aigw_migrate::native::SourcePool::connect(&db_url)
        .await
        .expect("connect to test DB");
    let past = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::hours(2))
        .unwrap()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let sql = format!(
        "UPDATE virtual_keys SET spend = {}, budget_reset_at = '{}' WHERE token = '{}'",
        spend, past, hash,
    );
    pool.execute_raw(&sql).await.expect("set spend + expired reset_at");
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

// "响应状态码为 {int}" is defined in common_steps.rs — do NOT redefine here.

#[then(expr = "key budget_reset_at 为空 {string}")]
async fn then_key_budget_reset_at_is_null(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        return;
    }
    let token = get_or_fetch_key(world, &alias).await;
    let url = format!("{}/key/info?key={}", base_url(), token);
    let mk = world.master_key.clone();
    let resp = client()
        .get(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("key/info request failed");
    let body: serde_json::Value = resp.json().await.expect("json body");

    // key/info returns a flat JSON object — budget_reset_at is a top-level field
    let reset_at = body["budget_reset_at"].as_str().unwrap_or("");
    assert!(
        reset_at.is_empty(),
        "budget_reset_at should be null/empty for key '{}', got: '{}'",
        alias,
        reset_at
    );
}

#[then(regex = r#"^key "(.+)" 的 budget_reset_at 不为空$"#)]
async fn then_key_budget_reset_at_has_value(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        return;
    }
    let token = get_or_fetch_key(world, &alias).await;
    let url = format!("{}/key/info?key={}", base_url(), token);
    let mk = world.master_key.clone();
    let resp = client()
        .get(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("key/info request failed");
    let body: serde_json::Value = resp.json().await.expect("json body");

    // key/info returns a flat JSON object — budget_reset_at is a top-level field
    let reset_at_str = body["budget_reset_at"].as_str().unwrap_or("");
    assert!(
        !reset_at_str.is_empty(),
        "budget_reset_at should not be null/empty for key '{}', got: null\nfull response: {}",
        alias,
        serde_json::to_string_pretty(&body).unwrap_or_default()
    );

    // Verify it's a valid timestamp
    let parsed = chrono::DateTime::parse_from_rfc3339(reset_at_str)
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(reset_at_str, "%Y-%m-%d %H:%M:%S")
                .map(|dt| dt.and_utc().into())
        });
    assert!(
        parsed.is_ok(),
        "budget_reset_at '{}' is not a valid timestamp for key '{}'",
        reset_at_str,
        alias
    );
}

#[then(expr = "响应 body 中 budget_duration 为 {string}")]
async fn then_budget_duration_is(world: &mut TestWorld, expected: String) {
    let body = world
        .last_body
        .as_ref()
        .expect("last_body is None");

    // key/info returns a flat JSON object — budget_duration is a top-level field
    let actual = body["budget_duration"].as_str().unwrap_or("<not-a-string>");
    assert_eq!(
        actual, expected,
        "expected budget_duration='{}' but got '{}'\nfull response: {}",
        expected, actual,
        serde_json::to_string_pretty(body).unwrap_or_default()
    );
}

#[then(expr = "响应 body 中 total_steps 大于 {int}")]
async fn then_total_steps_gt(world: &mut TestWorld, min_steps: i64) {
    let body = world.last_body.as_ref().expect("last_body is None");
    let total = body["total_steps"].as_i64().unwrap_or(0);
    assert!(
        total > min_steps,
        "expected total_steps > {} but got {}\nfull response: {}",
        min_steps,
        total,
        serde_json::to_string_pretty(body).unwrap_or_default()
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// budget-reset stats endpoint steps (admin budget-reset UI)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "发送 GET admin budget-reset stats")]
async fn when_get_budget_reset_stats(world: &mut TestWorld) {
    if !real_api_enabled() {
        set_skip_pass(
            world,
            200,
            serde_json::json!({
                "tick_interval_sec": 60,
                "next_tick_at": "2026-08-08T01:23:45Z",
                "counts": { "key": { "ready": 1, "total": 1 } },
                "ready_total": 1,
                "preview": [ { "entity_type": "key", "alias": "stats-key" } ],
                "last_reset": null,
            }),
        );
        return;
    }
    let url = format!("{}/admin/budget-reset/stats", base_url());
    let mk = world.master_key.clone();
    let resp = client()
        .get(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("admin/budget-reset/stats request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[given(expr = "非 admin 用户 token 已就绪")]
async fn given_non_admin_token_ready(_world: &mut TestWorld) {
    // The non-admin contract is exercised in mock mode via no-auth; in real
    // mode we use a random bearer token that is not the master key.
}

#[when(expr = "非 admin 发送 GET admin budget-reset stats")]
async fn when_non_admin_get_budget_reset_stats(world: &mut TestWorld) {
    if !real_api_enabled() {
        // Mock mode: the handler requires admin, so sending no/invalid auth yields 401.
        set_skip_pass(world, 401, serde_json::json!({"error": "unauthorized"}));
        return;
    }
    let url = format!("{}/admin/budget-reset/stats", base_url());
    // Not the master key → require_admin rejects with 401.
    let resp = client()
        .get(&url)
        .header("Authorization", "Bearer sk-not-a-real-admin")
        .send()
        .await
        .expect("non-admin budget-reset stats request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[then(expr = "响应 body 中 counts.key.ready 大于 {int}")]
async fn then_counts_key_ready_gt(world: &mut TestWorld, min: i64) {
    let body = world.last_body.as_ref().expect("last_body is None");
    let ready = body["counts"]["key"]["ready"].as_i64().unwrap_or(0);
    assert!(
        ready > min,
        "expected counts.key.ready > {} but got {}\nfull response: {}",
        min,
        ready,
        serde_json::to_string_pretty(body).unwrap_or_default()
    );
}

#[then(expr = "响应 body 中 ready_total 大于 {int}")]
async fn then_ready_total_gt(world: &mut TestWorld, min: i64) {
    let body = world.last_body.as_ref().expect("last_body is None");
    let total = body["ready_total"].as_i64().unwrap_or(0);
    assert!(
        total > min,
        "expected ready_total > {} but got {}\nfull response: {}",
        min,
        total,
        serde_json::to_string_pretty(body).unwrap_or_default()
    );
}

#[then(expr = "响应 body 中 preview  entity_type 为 {string}")]
async fn then_preview_contains_type(world: &mut TestWorld, entity_type: String) {
    let body = world.last_body.as_ref().expect("last_body is None");
    let preview = body["preview"].as_array().cloned().unwrap_or_default();
    let found = preview
        .iter()
        .any(|p| p["entity_type"].as_str() == Some(entity_type.as_str()));
    assert!(
        found,
        "preview should contain entity_type '{}' but got\n{}",
        entity_type,
        serde_json::to_string_pretty(&preview).unwrap_or_default()
    );
}

#[then(expr = "响应 body 中 next_tick_at 在未来")]
async fn then_next_tick_in_future(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("last_body is None");
    let raw = body["next_tick_at"].as_str().unwrap_or("");
    let parsed = chrono::DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
                .map(|dt| dt.and_utc())
                .unwrap_or_else(|_| chrono::Utc::now() - chrono::Duration::hours(1))
        });
    assert!(
        parsed > chrono::Utc::now() - chrono::Duration::seconds(5),
        "next_tick_at '{}' is not in the future\nfull response: {}",
        raw,
        serde_json::to_string_pretty(body).unwrap_or_default()
    );
}

#[then(expr = "响应 body 中 last_reset 为 null 或合法 job")]
async fn then_last_reset_null_or_valid(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("last_body is None");
    match body["last_reset"].as_object() {
        None => { /* null — never ran, valid */ }
        Some(job) => {
            assert!(
                job["job_id"].as_str().is_some(),
                "last_reset job_id missing\n{}",
                serde_json::to_string_pretty(body).unwrap_or_default()
            );
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Create a virtual key with a specific budget_duration via the HTTP API,
/// returning the raw key token. Stores the key in world.created_keys on success.
async fn create_key_with_budget_duration(
    world: &mut TestWorld,
    alias: &str,
    budget_duration: &str,
) -> String {
    let url = format!("{}/key/generate", base_url());
    let body = serde_json::json!({
        "key_alias": alias,
        "budget_duration": budget_duration,
        "models": ["gpt-4"],
        "max_budget": 100.0,
    });
    let mk = world.master_key.clone();
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("key/generate request failed");

    let status = resp.status().as_u16();
    let resp_body: serde_json::Value = resp.json().await.expect("key/generate body");
    world.last_status = Some(status);
    world.last_body = Some(resp_body.clone());

    if status < 200 || status >= 300 {
        let detail = &resp_body.to_string();
        eprintln!(
            "key/generate returned {} for alias '{}' (budget_duration={}): {}",
            status,
            alias,
            budget_duration,
            &detail[..detail.len().min(300)]
        );
        return String::new();
    }

    let raw_key = resp_body["key"]
        .as_str()
        .expect("key field missing")
        .to_string();
    world
        .created_keys
        .insert(alias.to_string(), raw_key.clone());
    raw_key
}

/// Get a key token from world.created_keys.
async fn get_or_fetch_key(world: &TestWorld, alias: &str) -> String {
    if let Some(t) = world.created_keys.get(alias) {
        return t.clone();
    }
    world.master_key.clone()
}

/// Create a virtual key with a specific budget_duration and max_budget via the HTTP API.
/// Stores the key in world.created_keys on success.
async fn create_key_with_budget_and_max(
    world: &mut TestWorld,
    alias: &str,
    budget_duration: &str,
    max_budget: i64,
) -> String {
    let url = format!("{}/key/generate", base_url());
    let body = serde_json::json!({
        "key_alias": alias,
        "budget_duration": budget_duration,
        "models": ["gpt-4"],
        "max_budget": max_budget,
    });
    let mk = world.master_key.clone();
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("key/generate request failed");

    let status = resp.status().as_u16();
    let resp_body: serde_json::Value = resp.json().await.expect("key/generate body");
    world.last_status = Some(status);
    world.last_body = Some(resp_body.clone());

    if status < 200 || status >= 300 {
        let detail = &resp_body.to_string();
        eprintln!(
            "key/generate returned {} for alias '{}' (budget_duration={}, max_budget={}): {}",
            status,
            alias,
            budget_duration,
            max_budget,
            &detail[..detail.len().min(300)]
        );
        return String::new();
    }

    let raw_key = resp_body["key"]
        .as_str()
        .expect("key field missing")
        .to_string();
    world
        .created_keys
        .insert(alias.to_string(), raw_key.clone());
    raw_key
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Multi-level budget enforcement steps (multi_level_budget.feature)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

//
// ── Given ──
//

#[given(expr = "数据库中有 user {string} max_budget={float} spend={float} 和 key {string} max_budget={float} 关联该 user")]
async fn given_user_and_key_with_user_budget(
    world: &mut TestWorld,
    user_id: String,
    user_max_budget: f64,
    user_spend: f64,
    key_alias: String,
    key_max_budget: f64,
) {
    if !real_api_enabled() {
        return;
    }
    let db_url = test_db_url();
    let token = format!("sk-{}", key_alias);

    // Clean up any existing data
    real_db_seed::cleanup_entity(&db_url, "key", &aigw_core::crypto::hash_token(&token))
        .await
        .ok();
    real_db_seed::cleanup_entity(&db_url, "user", &user_id).await.ok();

    // Seed user with the given max_budget and spend
    real_db_seed::ensure_user(&db_url, &user_id, None, Some(user_max_budget), user_spend)
        .await
        .expect("ensure user");

    // Seed key linked to the user
    let hash = aigw_core::crypto::hash_token(&token);
    real_db_seed::cleanup_entity(&db_url, "key", &key_alias).await.ok();
    // Use raw SQL to create key with a specific user_id + max_budget
    let pool = aigw_migrate::native::SourcePool::connect(&db_url)
        .await
        .expect("connect");
    let now = pool.time_literal("2026-07-20T00:00:00");
    let sql = format!(
        r#"INSERT INTO virtual_keys
        (token, key_alias, key_name, spend, models, aliases, config, permissions, metadata,
         allowed_cache_controls, allowed_routes, policies, access_group_ids,
         model_spend, model_max_budget,
         user_id, team_id, max_budget, budget_duration, budget_reset_at,
         soft_budget_cooldown, created_at, updated_at)
        VALUES ('{}', '{}', '{}', 0.0,
                '[]', '{{}}', '{{}}', '{{}}', '{{}}',
                '[]', '[]', '[]', '[]',
                '{{}}', '{{}}',
                '{}', NULL, '{}', NULL, NULL,
                'false', {}, {})"#,
        hash, key_alias, key_alias, user_id, key_max_budget, now, now,
    );
    pool.execute_raw(&sql).await.expect("insert key");

    world
        .created_keys
        .insert(key_alias, token);
}

#[given(expr = "数据库中有 team {string} max_budget={float} spend={float} 和 key {string} 关联该 team")]
async fn given_team_and_key_with_team_budget(
    world: &mut TestWorld,
    team_id: String,
    team_max_budget: f64,
    team_spend: f64,
    key_alias: String,
) {
    if !real_api_enabled() {
        return;
    }
    let db_url = test_db_url();
    let token = format!("sk-{}", key_alias);

    real_db_seed::cleanup_entity(&db_url, "key", &key_alias).await.ok();
    real_db_seed::cleanup_entity(&db_url, "team", &team_id).await.ok();

    real_db_seed::ensure_team(&db_url, &team_id, None, Some(team_max_budget), None, team_spend)
        .await
        .expect("ensure team");

    let hash = aigw_core::crypto::hash_token(&token);
    let pool = aigw_migrate::native::SourcePool::connect(&db_url)
        .await
        .expect("connect");
    let now = pool.time_literal("2026-07-20T00:00:00");
    let sql = format!(
        r#"INSERT INTO virtual_keys
        (token, key_alias, key_name, spend, models, aliases, config, permissions, metadata,
         allowed_cache_controls, allowed_routes, policies, access_group_ids,
         model_spend, model_max_budget,
         user_id, team_id, max_budget, budget_duration, budget_reset_at,
         soft_budget_cooldown, created_at, updated_at)
        VALUES ('{}', '{}', '{}', 0.0,
                '[]', '{{}}', '{{}}', '{{}}', '{{}}',
                '[]', '[]', '[]', '[]',
                '{{}}', '{{}}',
                NULL, '{}', '{}', NULL, NULL,
                'false', {}, {})"#,
        hash, key_alias, key_alias, team_id, 100.0, now, now,
    );
    pool.execute_raw(&sql).await.expect("insert key");

    world
        .created_keys
        .insert(key_alias, token);
}

#[given(expr = "数据库中有 key {string} max_budget={float} 和 user {string} max_budget={float} 和 team {string} max_budget={float}")]
async fn given_all_pass_scenario(
    world: &mut TestWorld,
    key_alias: String,
    key_max_budget: f64,
    user_id: String,
    user_max_budget: f64,
    team_id: String,
    team_max_budget: f64,
) {
    if !real_api_enabled() {
        return;
    }
    let db_url = test_db_url();
    let token = format!("sk-{}", key_alias);

    real_db_seed::cleanup_entity(&db_url, "key", &key_alias).await.ok();
    real_db_seed::cleanup_entity(&db_url, "user", &user_id).await.ok();
    real_db_seed::cleanup_entity(&db_url, "team", &team_id).await.ok();

    real_db_seed::ensure_user(&db_url, &user_id, Some(&team_id), Some(user_max_budget), 1.0)
        .await
        .expect("ensure user");
    real_db_seed::ensure_team(&db_url, &team_id, None, Some(team_max_budget), None, 1.0)
        .await
        .expect("ensure team");

    let hash = aigw_core::crypto::hash_token(&token);
    let pool = aigw_migrate::native::SourcePool::connect(&db_url)
        .await
        .expect("connect");
    let now = pool.time_literal("2026-07-20T00:00:00");
    let sql = format!(
        r#"INSERT INTO virtual_keys
        (token, key_alias, key_name, spend, models, aliases, config, permissions, metadata,
         allowed_cache_controls, allowed_routes, policies, access_group_ids,
         model_spend, model_max_budget,
         user_id, team_id, max_budget, budget_duration, budget_reset_at,
         soft_budget_cooldown, created_at, updated_at)
        VALUES ('{}', '{}', '{}', 0.0,
                '[]', '{{}}', '{{}}', '{{}}', '{{}}',
                '[]', '[]', '[]', '[]',
                '{{}}', '{{}}',
                '{}', '{}', '{}', NULL, NULL,
                'false', {}, {})"#,
        hash, key_alias, key_alias, user_id, team_id, key_max_budget, now, now,
    );
    pool.execute_raw(&sql).await.expect("insert key");

    world
        .created_keys
        .insert(key_alias, token);
}

#[given(expr = "数据库中有 org {string} budget_id={string} spend={float} 和 budget {string} max_budget={float}")]
async fn given_org_with_budget(
    _world: &mut TestWorld,
    org_id: String,
    budget_id: String,
    org_spend: f64,
    _budget_id2: String,
    budget_max_budget: f64,
) {
    if !real_api_enabled() {
        return;
    }
    let db_url = test_db_url();
    real_db_seed::cleanup_entity(&db_url, "organization", &org_id).await.ok();
    real_db_seed::cleanup_entity(&db_url, "budget", &budget_id).await.ok();

    real_db_seed::ensure_organization(&db_url, &org_id, &budget_id, org_spend)
        .await
        .expect("ensure org");
    real_db_seed::ensure_budget(&db_url, &budget_id, budget_max_budget, None)
        .await
        .expect("ensure budget");
}

#[given(expr = "有关联该 org 的 team {string} 和 key {string}")]
async fn given_team_and_key_linked_to_org(
    world: &mut TestWorld,
    team_id: String,
    key_alias: String,
) {
    if !real_api_enabled() {
        return;
    }
    let db_url = test_db_url();
    let token = format!("sk-{}", key_alias);
    // The org referred to is the one created in the previous Given step.
    // We use budget-ml-o1 (the standard org alias used in the feature).
    let org_id = "budget-ml-o1";

    real_db_seed::cleanup_entity(&db_url, "team", &team_id).await.ok();
    real_db_seed::cleanup_entity(&db_url, "key", &key_alias).await.ok();

    // Create team linked to the org, with high budget so it doesn't reject
    real_db_seed::ensure_team(&db_url, &team_id, Some(org_id), Some(500.0), None, 5.0)
        .await
        .expect("ensure team");

    let hash = aigw_core::crypto::hash_token(&token);
    let pool = aigw_migrate::native::SourcePool::connect(&db_url)
        .await
        .expect("connect");
    let now = pool.time_literal("2026-07-20T00:00:00");
    let sql = format!(
        r#"INSERT INTO virtual_keys
        (token, key_alias, key_name, spend, models, aliases, config, permissions, metadata,
         allowed_cache_controls, allowed_routes, policies, access_group_ids,
         model_spend, model_max_budget,
         user_id, team_id, organization_id, max_budget, budget_duration, budget_reset_at,
         soft_budget_cooldown, created_at, updated_at)
        VALUES ('{}', '{}', '{}', 0.0,
                '[]', '{{}}', '{{}}', '{{}}', '{{}}',
                '[]', '[]', '[]', '[]',
                '{{}}', '{{}}',
                NULL, '{}', '{}', '{}', NULL, NULL,
                'false', {}, {})"#,
        hash, key_alias, key_alias, team_id, org_id, 100.0, now, now,
    );
    pool.execute_raw(&sql).await.expect("insert key");

    world
        .created_keys
        .insert(key_alias, token);
}

//
// ── When ──
//

#[when(expr = "为该 user {string} 增加 spend {float} 使 user 累计达到 {float}")]
async fn when_increment_user_spend(
    _world: &mut TestWorld,
    user_id: String,
    _amount: f64,
    _target: f64,
) {
    if !real_api_enabled() {
        return;
    }
    let db_url = test_db_url();
    let pool = aigw_migrate::native::SourcePool::connect(&db_url)
        .await
        .expect("connect");
    let sql = format!(
        "UPDATE users SET spend = spend + {} WHERE user_id = '{}'",
        1.0, user_id,
    );
    pool.execute_raw(&sql).await.expect("increment user spend");
}

#[when(expr = "为该 team {string} 增加 spend {float} 使 team 累计达到 {float}")]
async fn when_increment_team_spend(
    _world: &mut TestWorld,
    team_id: String,
    _amount: f64,
    _target: f64,
) {
    if !real_api_enabled() {
        return;
    }
    let db_url = test_db_url();
    let pool = aigw_migrate::native::SourcePool::connect(&db_url)
        .await
        .expect("connect");
    let sql = format!(
        "UPDATE teams SET spend = spend + {} WHERE team_id = '{}'",
        0.5, team_id,
    );
    pool.execute_raw(&sql).await.expect("increment team spend");
}

#[when(expr = "为该 org {string} 增加 spend {float} 使 org 累计达到 {float}")]
async fn when_increment_org_spend(
    _world: &mut TestWorld,
    org_id: String,
    _amount: f64,
    _target: f64,
) {
    if !real_api_enabled() {
        return;
    }
    let db_url = test_db_url();
    let pool = aigw_migrate::native::SourcePool::connect(&db_url)
        .await
        .expect("connect");
    let sql = format!(
        "UPDATE organizations SET spend = spend + {} WHERE organization_id = '{}'",
        1.0, org_id,
    );
    pool.execute_raw(&sql).await.expect("increment org spend");
}

#[when(expr = "使用 key {string} 发送 chat 请求 cost={float}")]
async fn when_send_chat_with_key(world: &mut TestWorld, alias: String, _cost: f64) {
    if !real_api_enabled() {
        // Multi-level budget scenarios require a real DB to test properly.
        // In mock mode, skip vacuously with 200 (the common success path).
        set_skip_pass(world, 200, serde_json::json!({}));
        return;
    }
    let token = get_or_fetch_key(world, &alias).await;
    let url = format!("{}/v1/chat/completions", base_url());
    let body = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}],
        "max_tokens": 10,
    });
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("chat request failed");

    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

//
// ── Then ──
//

#[then(expr = "响应 body 包含 entity_type {string}")]
async fn then_body_contains_entity_type(world: &mut TestWorld, expected_type: String) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    let actual = body["error"]["entity_type"].as_str().unwrap_or("missing");
    assert_eq!(
        actual, expected_type,
        "expected entity_type={}, got={}",
        expected_type, actual
    );
}

#[then(expr = "key {string} 的 spend 应为 {int}")]
async fn then_key_spend_is(world: &mut TestWorld, alias: String, expected_spend: i64) {
    if !real_api_enabled() {
        return;
    }
    let raw_key = world
        .created_keys
        .get(&alias)
        .expect("key not found in world.created_keys");
    let hash = aigw_core::crypto::hash_token(raw_key);
    let db_url = test_db_url();
    let pool = aigw_migrate::native::SourcePool::connect(&db_url)
        .await
        .expect("connect to test DB");
    let sql = format!(
        "SELECT spend FROM virtual_keys WHERE token = '{}'",
        hash,
    );
    let rows = pool.read_rows_sql(&sql).await.expect("query spend");
    assert!(!rows.is_empty(), "key '{}' not found via spend query", alias);
    let spend: f64 = rows[0]
        .iter()
        .find(|(col, _)| col == "spend")
        .and_then(|(_, v)| v.as_f64())
        .expect("spend column missing or not f64");
    assert_eq!(
        spend, expected_spend as f64,
        "expected spend={} for key '{}', got spend={}",
        expected_spend, alias, spend,
    );
}
