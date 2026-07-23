//! Step bindings for spend_rankings_real.feature
//!
//! Tests /global/spend/keys/rankings across SQLite/PG/MySQL using
//! SourcePool-based direct DB seeding.

use cucumber::{when, then};
use crate::TestWorld;
use super::real_api_steps;
use super::real_db_seed;

/// Returns the upstream litellm database URL.
fn upstream_db_url() -> Option<String> {
    std::env::var("AIGW_UPSTREAM_DB_URL").ok().filter(|s| !s.is_empty())
}

fn seed_enabled() -> bool {
    real_api_steps::real_api_enabled() && upstream_db_url().is_some()
}

/// The test DB URL for real BDD scenarios (set by bdd.rs harness).
fn test_db_url() -> String {
    std::env::var("AIGW_TEST_DB_URL")
        .expect("AIGW_TEST_DB_URL must be set by the BDD test harness")
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When: seed data + query rankings
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when("向 aigw 测试库灌入两条已知 spend_logs 并查询 keys/rankings")]
async fn when_seed_and_query_rankings(world: &mut TestWorld) {
    if !seed_enabled() {
        real_api_steps::set_skip_pass(world, 200, serde_json::json!([
            {"api_key": "key-a", "key_alias": "ranking-a", "total_spend": 13.0, "total_requests": 3, "total_tokens": 300},
            {"api_key": "key-b", "key_alias": "ranking-b", "total_spend": 5.0, "total_requests": 1, "total_tokens": 100}
        ]));
        return;
    }

    let db_url = test_db_url();
    let base = real_api_steps::base_url();
    let mk = world.master_key.clone();

    // Create two virtual keys in the test DB.
    let token_a = "sk-ranking-test-a-73";
    let token_b = "sk-ranking-test-b-73";
    real_db_seed::ensure_virtual_key(&db_url, token_a, "ranking-a", None).await
        .expect("ensure virtual key a");
    real_db_seed::ensure_virtual_key(&db_url, token_b, "ranking-b", None).await
        .expect("ensure virtual key b");

    // Hash tokens to match spend_logs.api_key
    let hash_a = aigw_core::crypto::hash_token(token_a);
    let hash_b = aigw_core::crypto::hash_token(token_b);

    // Cleanup only our test rows
    real_db_seed::cleanup_by_prefix(&db_url, "bdd-rank-").await
        .expect("cleanup rankings");

    // Insert spend_logs: key_a has 3 rows (spend=3+4+6=13), key_b has 1 row (spend=5)
    let rows = vec![
        real_db_seed::SeedRow::new("bdd-rank-a1", &hash_a, 3.0, 100, "gpt-4", "2026-07-20T10:00:00"),
        real_db_seed::SeedRow::new("bdd-rank-a2", &hash_a, 4.0, 100, "gpt-4", "2026-07-20T11:00:00"),
        real_db_seed::SeedRow::new("bdd-rank-a3", &hash_a, 6.0, 100, "gpt-4", "2026-07-20T12:00:00"),
        real_db_seed::SeedRow::new("bdd-rank-b1", &hash_b, 5.0, 100, "gpt-3.5", "2026-07-20T10:30:00"),
    ];
    real_db_seed::seed_spend_logs(&db_url, &rows).await
        .expect("seed spend logs");

    // Query HTTP endpoint
    let url = format!(
        "{}/global/spend/keys/rankings?start_date=2026-07-20&end_date=2026-07-21&limit=10",
        base
    );
    let client = real_api_steps::client();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("keys/rankings request failed");

    let status = resp.status().as_u16();
    let body: Option<serde_json::Value> = resp.json().await.ok();
    world.last_status = Some(status);
    world.last_body = body;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then: assertions
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then("keys/rankings 首条 total_spend 最大且 key_alias 已回填")]
async fn then_rankings_first_max_with_alias(world: &mut TestWorld) {
    if !seed_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    let arr = body.as_array().expect("expected JSON array");

    assert!(!arr.is_empty(), "rankings array is empty");

    // Find our test keys by known token hashes.
    let hash_a = aigw_core::crypto::hash_token("sk-ranking-test-a-73");
    let hash_b = aigw_core::crypto::hash_token("sk-ranking-test-b-73");

    let entry_a = arr.iter().find(|e| e.get("api_key").and_then(|v| v.as_str()) == Some(&hash_a));
    let entry_b = arr.iter().find(|e| e.get("api_key").and_then(|v| v.as_str()) == Some(&hash_b));

    let spend_a = entry_a.and_then(|e| e.get("total_spend").and_then(|v| v.as_f64())).unwrap_or(0.0);
    let spend_b = entry_b.and_then(|e| e.get("total_spend").and_then(|v| v.as_f64())).unwrap_or(0.0);

    assert!((spend_a - 13.0).abs() < 0.01,
        "Expected ranking-a spend=13.0, got {} (body: {})", spend_a, body);
    assert!((spend_b - 5.0).abs() < 0.01,
        "Expected ranking-b spend=5.0, got {} (body: {})", spend_b, body);

    // Verify key_alias is backfilled from virtual_keys JOIN
    let ka = entry_a.and_then(|e| e.get("key_alias").and_then(|v| v.as_str())).unwrap_or("");
    assert!(!ka.is_empty(), "key_alias for ranking-a should not be empty");
    let kb = entry_b.and_then(|e| e.get("key_alias").and_then(|v| v.as_str())).unwrap_or("");
    assert!(!kb.is_empty(), "key_alias for ranking-b should not be empty");
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 401 / 403 scenarios
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "不携带 Authorization 发送 GET \\/global\\/spend\\/keys\\/rankings 请求")]
async fn when_rankings_noauth(world: &mut TestWorld) {
    if !real_api_steps::real_api_enabled() {
        real_api_steps::set_skip_pass(world, 401, serde_json::json!({"error": {"type": "authentication_error"}}));
        return;
    }
    let url = format!(
        "{}/global/spend/keys/rankings?start_date=2026-01-01&end_date=2026-12-31",
        real_api_steps::base_url()
    );
    let client = real_api_steps::client();
    let resp = client
        .get(&url)
        .send()
        .await
        .expect("request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "使用 key {string} 发送 GET \\/global\\/spend\\/keys\\/rankings 请求")]
async fn when_rankings_nonadmin(world: &mut TestWorld, alias: String) {
    if !real_api_steps::real_api_enabled() {
        real_api_steps::set_skip_pass(world, 403, serde_json::json!({"error": {"message": "admin required"}}));
        return;
    }
    let token = world.created_keys.get(&alias)
        .cloned()
        .expect("key not found");
    let url = format!(
        "{}/global/spend/keys/rankings?start_date=2026-01-01&end_date=2026-12-31",
        real_api_steps::base_url()
    );
    let client = real_api_steps::client();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .expect("request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}
