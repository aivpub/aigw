//! Step bindings for spend_activity_real.feature
//!
//! Tests /global/spend/activity across SQLite/PG/MySQL using
//! SourcePool-based direct DB seeding.

use cucumber::{when, then};
use crate::TestWorld;
use super::real_api_steps;
use super::real_db_seed;

fn upstream_db_url() -> Option<String> {
    std::env::var("AIGW_UPSTREAM_DB_URL").ok().filter(|s| !s.is_empty())
}

fn seed_enabled() -> bool {
    real_api_steps::real_api_enabled() && upstream_db_url().is_some()
}

fn test_db_url() -> String {
    std::env::var("AIGW_TEST_DB_URL")
        .expect("AIGW_TEST_DB_URL must be set by the BDD test harness")
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When: seed data + query activity
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when("向 aigw 测试库灌入跨天 spend_logs 并查询 activity")]
async fn when_seed_cross_day_and_query_activity(world: &mut TestWorld) {
    if !seed_enabled() {
        real_api_steps::set_skip_pass(world, 200, serde_json::json!({
            "metadata": {
                "total_spend": 22.0, "total_requests": 4, "successful_requests": 3,
                "failed_requests": 1, "total_tokens": 400,
                "prompt_tokens": 200, "completion_tokens": 200
            },
            "daily": [
                {"date": "2026-07-20", "total_spend": 11.0, "total_requests": 2, "successful_requests": 2, "failed_requests": 0, "total_tokens": 200},
                {"date": "2026-07-21", "total_spend": 11.0, "total_requests": 2, "successful_requests": 1, "failed_requests": 1, "total_tokens": 200}
            ]
        }));
        return;
    }

    let db_url = test_db_url();
    let base = real_api_steps::base_url();
    let mk = world.master_key.clone();

    // Use a distinct user_id to isolate test data from other scenarios
    // that may contaminate the date range (e.g., e2e requests logged with today's date).
    let test_user = "bdd-activity-meta-user";

    // Create a key for the spend_logs
    let token = "sk-activity-test-74";
    real_db_seed::ensure_virtual_key(&db_url, token, "activity-test", None).await
        .expect("ensure virtual key");
    let hash = aigw_core::crypto::hash_token(token);

    // Cleanup + seed 4 rows across 2 days: 3 success + 1 failure
    real_db_seed::cleanup_by_prefix(&db_url, "bdd-act-").await.expect("cleanup");

    let mut r1 = real_db_seed::SeedRow::new("bdd-act-d1a", &hash, 5.0, 100, "gpt-4", "2026-07-20T10:00:00");
    r1.prompt_tokens = 50;
    r1.completion_tokens = 50;
    r1.user = Some(test_user.to_string());

    let mut r2 = real_db_seed::SeedRow::new("bdd-act-d1b", &hash, 6.0, 100, "gpt-4", "2026-07-20T14:00:00");
    r2.prompt_tokens = 50;
    r2.completion_tokens = 50;
    r2.user = Some(test_user.to_string());

    let mut r3 = real_db_seed::SeedRow::new("bdd-act-d2a", &hash, 4.0, 100, "gpt-4", "2026-07-21T09:00:00");
    r3.prompt_tokens = 50;
    r3.completion_tokens = 50;
    r3.user = Some(test_user.to_string());

    let mut r4 = real_db_seed::SeedRow::new("bdd-act-d2b", &hash, 7.0, 100, "gpt-3.5", "2026-07-21T16:00:00");
    r4.prompt_tokens = 50;
    r4.completion_tokens = 50;
    r4.status = "failure".to_string();
    r4.user = Some(test_user.to_string());

    real_db_seed::seed_spend_logs(&db_url, &[r1, r2, r3, r4]).await.expect("seed");

    // HTTP query — use user_id to isolate seeded rows from other scenario noise.
    // Date range is 4 days to force daily (not hourly) granularity.
    let url = format!(
        "{}/global/spend/activity?start_date=2026-07-20&end_date=2026-07-24&user_id={}",
        base, test_user
    );
    let client = real_api_steps::client();
    let resp = client.get(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .send().await.expect("activity request failed");

    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when("向 aigw 测试库灌入不同 user 的 spend_logs 并带 user_id 查询 activity")]
async fn when_seed_user_filter_activity(world: &mut TestWorld) {
    if !seed_enabled() {
        real_api_steps::set_skip_pass(world, 200, serde_json::json!({
            "metadata": {"total_spend": 10.0, "total_requests": 2, "total_tokens": 200}
        }));
        return;
    }

    let db_url = test_db_url();
    let base = real_api_steps::base_url();
    let mk = world.master_key.clone();

    let token = "sk-activity-user-74";
    real_db_seed::ensure_virtual_key(&db_url, token, "activity-user-test", None).await.ok();
    let hash = aigw_core::crypto::hash_token(token);

    real_db_seed::cleanup_by_prefix(&db_url, "bdd-ausr-").await.ok();

    let mut r1 = real_db_seed::SeedRow::new("bdd-ausr-a", &hash, 5.0, 100, "gpt-4", "2026-07-20T10:00:00");
    r1.user = Some("user-alice".to_string());
    let mut r2 = real_db_seed::SeedRow::new("bdd-ausr-b", &hash, 5.0, 100, "gpt-4", "2026-07-20T11:00:00");
    r2.user = Some("user-alice".to_string());
    let mut r3 = real_db_seed::SeedRow::new("bdd-ausr-c", &hash, 20.0, 200, "gpt-4", "2026-07-20T12:00:00");
    r3.user = Some("user-bob".to_string());

    real_db_seed::seed_spend_logs(&db_url, &[r1, r2, r3]).await.expect("seed");

    // Filter by user-alice: expect spend=10.0 (5+5), requests=2
    let url = format!("{}/global/spend/activity?start_date=2026-07-20&end_date=2026-07-21&user_id=user-alice", base);
    let client = real_api_steps::client();
    let resp = client.get(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .send().await.expect("activity request failed");

    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when("向 aigw 测试库灌入不同 team 的 spend_logs 并带 team_id 查询 activity")]
async fn when_seed_team_filter_activity(world: &mut TestWorld) {
    if !seed_enabled() {
        real_api_steps::set_skip_pass(world, 200, serde_json::json!({
            "metadata": {"total_spend": 15.0, "total_requests": 2, "total_tokens": 200}
        }));
        return;
    }

    let db_url = test_db_url();
    let base = real_api_steps::base_url();
    let mk = world.master_key.clone();

    let token = "sk-activity-team-74";
    real_db_seed::ensure_virtual_key(&db_url, token, "activity-team-test", None).await.ok();
    let hash = aigw_core::crypto::hash_token(token);

    real_db_seed::cleanup_by_prefix(&db_url, "bdd-atm-").await.ok();

    let mut r1 = real_db_seed::SeedRow::new("bdd-atm-a", &hash, 7.0, 100, "gpt-4", "2026-07-20T10:00:00");
    r1.team_id = Some("team-red".to_string());
    let mut r2 = real_db_seed::SeedRow::new("bdd-atm-b", &hash, 8.0, 100, "gpt-4", "2026-07-20T11:00:00");
    r2.team_id = Some("team-red".to_string());
    let mut r3 = real_db_seed::SeedRow::new("bdd-atm-c", &hash, 30.0, 200, "gpt-4", "2026-07-20T12:00:00");
    r3.team_id = Some("team-blue".to_string());

    real_db_seed::seed_spend_logs(&db_url, &[r1, r2, r3]).await.expect("seed");

    let url = format!("{}/global/spend/activity?start_date=2026-07-20&end_date=2026-07-21&team_id=team-red", base);
    let client = real_api_steps::client();
    let resp = client.get(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .send().await.expect("activity request failed");

    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then: assertions
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then("activity metadata 7 个字段数值正确")]
async fn then_activity_metadata_correct(world: &mut TestWorld) {
    if !seed_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    let meta = body.get("metadata").expect("no metadata field");

    let total_spend = meta.get("total_spend").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let total_requests = meta.get("total_requests").and_then(|v| v.as_i64()).unwrap_or(0);
    let successful_requests = meta.get("successful_requests").and_then(|v| v.as_i64()).unwrap_or(0);
    let failed_requests = meta.get("failed_requests").and_then(|v| v.as_i64()).unwrap_or(0);
    let total_tokens = meta.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    let prompt_tokens = meta.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    let completion_tokens = meta.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0);

    assert!((total_spend - 22.0).abs() < 0.01, "Expected total_spend=22.0, got {}", total_spend);
    assert_eq!(total_requests, 4, "Expected total_requests=4, got {} (body: {:?})", total_requests, body);
    assert_eq!(successful_requests, 3, "Expected successful_requests=3");
    assert_eq!(failed_requests, 1, "Expected failed_requests=1");
    assert_eq!(total_tokens, 400, "Expected total_tokens=400");
    assert_eq!(prompt_tokens, 200, "Expected prompt_tokens=200");
    assert_eq!(completion_tokens, 200, "Expected completion_tokens=200");
}

#[then("activity daily 按天分组且数值正确")]
async fn then_activity_daily_correct(world: &mut TestWorld) {
    if !seed_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    let daily = body.get("daily").and_then(|v| v.as_array())
        .expect("no daily array");

    // With a >3-day range (2026-07-20 to 2026-07-24), granularity should be "daily".
    let granularity = body.get("granularity").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(granularity, "daily", "Expected granularity=daily for >3-day range, got {}", granularity);

    // Only 2 days have data (07-20 and 07-21)
    assert_eq!(daily.len(), 2, "Expected 2 days in daily, got {}", daily.len());

    // Day 1: 2026-07-20 — spend=5+6=11, 2 success, 0 failure, tokens=200
    let d1 = &daily[0];
    assert_eq!(d1.get("date").and_then(|v| v.as_str()).unwrap_or(""), "2026-07-20");
    let d1_spend = d1.get("spend").or(d1.get("total_spend")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    assert!((d1_spend - 11.0).abs() < 0.01,
        "Expected day 1 spend=11.0, got {} (daily: {:?})", d1_spend, d1);

    // Day 2: 2026-07-21 — spend=4+7=11, 1 success, 1 failure, tokens=200
    let d2 = &daily[1];
    assert_eq!(d2.get("date").and_then(|v| v.as_str()).unwrap_or(""), "2026-07-21");
    let d2_spend = d2.get("spend").or(d2.get("total_spend")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    assert!((d2_spend - 11.0).abs() < 0.01,
        "Expected day 2 spend=11.0, got {} (daily: {:?})", d2_spend, d2);
}

#[then("activity metadata 仅统计该 user 的数据")]
async fn then_activity_user_filter_correct(world: &mut TestWorld) {
    if !seed_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    let meta = body.get("metadata").expect("no metadata field");
    let total_spend = meta.get("total_spend").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let total_requests = meta.get("total_requests").and_then(|v| v.as_i64()).unwrap_or(0);

    assert!((total_spend - 10.0).abs() < 0.01,
        "Expected user-alice spend=10.0, got {}", total_spend);
    assert_eq!(total_requests, 2, "Expected user-alice requests=2");
}

#[then("activity metadata 仅统计该 team 的数据")]
async fn then_activity_team_filter_correct(world: &mut TestWorld) {
    if !seed_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    let meta = body.get("metadata").expect("no metadata field");
    let total_spend = meta.get("total_spend").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let total_requests = meta.get("total_requests").and_then(|v| v.as_i64()).unwrap_or(0);

    assert!((total_spend - 15.0).abs() < 0.01,
        "Expected team-red spend=15.0, got {}", total_spend);
    assert_eq!(total_requests, 2, "Expected team-red requests=2");
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 401 / 403
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "不携带 Authorization 发送 GET \\/global\\/spend\\/activity 请求")]
async fn when_activity_noauth(world: &mut TestWorld) {
    if !real_api_steps::real_api_enabled() {
        real_api_steps::set_skip_pass(world, 401, serde_json::json!({"error": {"type": "authentication_error"}}));
        return;
    }
    let url = format!(
        "{}/global/spend/activity?start_date=2026-01-01&end_date=2026-12-31",
        real_api_steps::base_url()
    );
    let client = real_api_steps::client();
    let resp = client.get(&url).send().await.expect("request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "使用 key {string} 发送 GET \\/global\\/spend\\/activity 请求")]
async fn when_activity_nonadmin(world: &mut TestWorld, alias: String) {
    if !real_api_steps::real_api_enabled() {
        real_api_steps::set_skip_pass(world, 403, serde_json::json!({"error": {"message": "admin required"}}));
        return;
    }
    let token = world.created_keys.get(&alias).cloned().expect("key not found");
    let url = format!(
        "{}/global/spend/activity?start_date=2026-01-01&end_date=2026-12-31",
        real_api_steps::base_url()
    );
    let client = real_api_steps::client();
    let resp = client.get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send().await.expect("request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}
