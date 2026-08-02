//! Step bindings for spend_sum_cluster_real.feature
//!
//! Tests /global/spend, /spend/users, /spend/tags, /global/spend/keys
//! across SQLite/PG/MySQL using SourcePool-based direct DB seeding.

use super::real_api_steps;
use super::real_db_seed;
use crate::TestWorld;
use cucumber::{then, when};

fn upstream_db_url() -> Option<String> {
    std::env::var("AIGW_UPSTREAM_DB_URL")
        .ok()
        .filter(|s| !s.is_empty())
}

fn seed_enabled() -> bool {
    real_api_steps::real_api_enabled() && upstream_db_url().is_some()
}

fn test_db_url() -> String {
    std::env::var("AIGW_TEST_DB_URL").expect("AIGW_TEST_DB_URL must be set by the BDD test harness")
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When: /global/spend (total SUM) — returns {"spend": ...}
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when("向 aigw 测试库灌入若干 spend_logs 并查询 /global/spend")]
async fn when_seed_and_query_global_spend(world: &mut TestWorld) {
    if !seed_enabled() {
        real_api_steps::set_skip_pass(world, 200, serde_json::json!({"spend": 12.0}));
        return;
    }

    let db_url = test_db_url();
    let base = real_api_steps::base_url();
    let mk = world.master_key.clone();

    let token = "sk-sum-global-76";
    real_db_seed::ensure_virtual_key(&db_url, token, "sum-global-test", None)
        .await
        .ok();
    let hash = aigw_core::crypto::hash_token(token);

    real_db_seed::cleanup_by_prefix(&db_url, "bdd-sgl-")
        .await
        .ok();

    real_db_seed::cleanup_by_prefix(&db_url, "bdd-").await.ok();
    real_db_seed::cleanup_keys_by_alias(&db_url, "sum-%")
        .await
        .ok();

    let rows = vec![
        real_db_seed::SeedRow::new("bdd-sgl-a", &hash, 5.0, 100, "gpt-4", "2026-07-20T10:00:00"),
        real_db_seed::SeedRow::new(
            "bdd-sgl-b",
            &hash,
            7.0,
            200,
            "gpt-3.5",
            "2026-07-20T11:00:00",
        ),
    ];
    real_db_seed::seed_spend_logs(&db_url, &rows)
        .await
        .expect("seed");

    let url = format!("{}/global/spend", base);
    let client = real_api_steps::client();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("global/spend request failed");

    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When: /spend/users — needs user_id on the key's auth
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when("向 aigw 测试库灌入该 user 的 spend_logs 并使用该 key 查询 /spend/users")]
async fn when_seed_user_and_query_spend_users(world: &mut TestWorld) {
    if !seed_enabled() {
        real_api_steps::set_skip_pass(world, 200, serde_json::json!({"spend": 15.0}));
        return;
    }

    let db_url = test_db_url();
    let base = real_api_steps::base_url();

    let alias = "sum-user-key";
    let raw_key = world
        .created_keys
        .get(alias)
        .cloned()
        .expect("key not found");

    let hash = aigw_core::crypto::hash_token(&raw_key);
    real_db_seed::cleanup_by_prefix(&db_url, "bdd-usr-")
        .await
        .ok();

    let mut r1 =
        real_db_seed::SeedRow::new("bdd-usr-a", &hash, 7.0, 100, "gpt-4", "2026-07-20T10:00:00");
    r1.user = Some("test-user-76".to_string());
    let mut r2 =
        real_db_seed::SeedRow::new("bdd-usr-b", &hash, 8.0, 100, "gpt-4", "2026-07-20T11:00:00");
    r2.user = Some("test-user-76".to_string());

    real_db_seed::seed_spend_logs(&db_url, &[r1, r2])
        .await
        .expect("seed");

    let spend_url = format!("{}/spend/users", base);
    let client = real_api_steps::client();
    let resp = client
        .get(&spend_url)
        .header("Authorization", format!("Bearer {}", raw_key))
        .send()
        .await
        .expect("spend/users request failed");

    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When: /spend/tags (LIKE matching) — returns {"spend": ...}
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when("向 aigw 测试库灌入带 request_tags 的 spend_logs 并查询 /spend/tags?tag=important")]
async fn when_seed_tags_and_query(world: &mut TestWorld) {
    if !seed_enabled() {
        real_api_steps::set_skip_pass(world, 200, serde_json::json!({"spend": 20.0}));
        return;
    }

    let db_url = test_db_url();
    let base = real_api_steps::base_url();
    let mk = world.master_key.clone();

    let token = "sk-sum-tags-76";
    real_db_seed::ensure_virtual_key(&db_url, token, "sum-tags-test", None)
        .await
        .ok();
    let hash = aigw_core::crypto::hash_token(token);

    real_db_seed::cleanup_by_prefix(&db_url, "bdd-tag-")
        .await
        .ok();

    // request_tags is a TEXT/BLOB column. The LIKE operator does '%important%'.
    // Store the tag value as plain text to make LIKE matching work.
    let mut r1 = real_db_seed::SeedRow::new(
        "bdd-tag-a",
        &hash,
        10.0,
        100,
        "gpt-4",
        "2026-07-20T10:00:00",
    );
    r1.request_tags = Some("important".to_string());
    let mut r2 = real_db_seed::SeedRow::new(
        "bdd-tag-b",
        &hash,
        10.0,
        100,
        "gpt-4",
        "2026-07-20T11:00:00",
    );
    r2.request_tags = Some("important".to_string());
    let r3 = real_db_seed::SeedRow::new(
        "bdd-tag-c",
        &hash,
        50.0,
        200,
        "gpt-4",
        "2026-07-20T12:00:00",
    );
    // r3: no tags → should not match

    real_db_seed::seed_spend_logs(&db_url, &[r1, r2, r3])
        .await
        .expect("seed");

    let url = format!("{}/spend/tags?tag=important", base);
    let client = real_api_steps::client();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("spend/tags request failed");

    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When: /global/spend/keys (app-layer aggregation) — returns {"data": [...]}
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when("向 aigw 测试库灌入多 key 的 spend_logs 并查询 /global/spend/keys")]
async fn when_seed_multi_key_and_query_global_keys(world: &mut TestWorld) {
    if !seed_enabled() {
        real_api_steps::set_skip_pass(
            world,
            200,
            serde_json::json!({"data": [
                {"api_key": "key-a", "total_spend": 3.0, "total_requests": 1},
                {"api_key": "key-b", "total_spend": 8.0, "total_requests": 2}
            ]}),
        );
        return;
    }

    let db_url = test_db_url();
    let base = real_api_steps::base_url();
    let mk = world.master_key.clone();

    let token_a = "sk-sum-keys-a-76";
    let token_b = "sk-sum-keys-b-76";
    real_db_seed::ensure_virtual_key(&db_url, token_a, "sum-keys-a", None)
        .await
        .ok();
    real_db_seed::ensure_virtual_key(&db_url, token_b, "sum-keys-b", None)
        .await
        .ok();
    let hash_a = aigw_core::crypto::hash_token(token_a);
    let hash_b = aigw_core::crypto::hash_token(token_b);

    real_db_seed::cleanup_by_prefix(&db_url, "bdd-gk-")
        .await
        .ok();

    let rows = vec![
        real_db_seed::SeedRow::new(
            "bdd-gk-a1",
            &hash_a,
            3.0,
            50,
            "gpt-4",
            "2026-07-20T10:00:00",
        ),
        real_db_seed::SeedRow::new(
            "bdd-gk-b1",
            &hash_b,
            3.0,
            50,
            "gpt-3.5",
            "2026-07-20T10:30:00",
        ),
        real_db_seed::SeedRow::new(
            "bdd-gk-b2",
            &hash_b,
            5.0,
            100,
            "gpt-3.5",
            "2026-07-20T11:00:00",
        ),
    ];
    real_db_seed::seed_spend_logs(&db_url, &rows)
        .await
        .expect("seed");

    let url = format!("{}/global/spend/keys", base);
    let client = real_api_steps::client();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("global/spend/keys request failed");

    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then: assertions — match actual response field names
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then("global spend 等于灌入总额")]
async fn then_global_spend_correct(world: &mut TestWorld) {
    if !seed_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    // /global/spend returns {"spend": <f64>}
    let spend = body.get("spend").and_then(|v| v.as_f64()).unwrap_or(0.0);
    assert!(
        (spend - 12.0).abs() < 0.01,
        "Expected spend=12.0, got {} (body: {})",
        spend,
        body
    );
}

#[then("spend/users 返回该 user 的累计 spend")]
async fn then_spend_users_correct(world: &mut TestWorld) {
    if !seed_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    // /spend/users returns {"user_id": ..., "spend": <f64>}
    let spend = body.get("spend").and_then(|v| v.as_f64()).unwrap_or(0.0);
    assert!(
        (spend - 15.0).abs() < 0.01,
        "Expected user spend=15.0, got {} (body: {})",
        spend,
        body
    );
}

#[then("spend/tags 返回匹配 tag 的累计 spend")]
async fn then_spend_tags_correct(world: &mut TestWorld) {
    if !seed_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    // /spend/tags returns {"tag": ..., "spend": <f64>}
    let spend = body.get("spend").and_then(|v| v.as_f64()).unwrap_or(0.0);
    assert!(
        (spend - 20.0).abs() < 0.01,
        "Expected tag 'important' spend=20.0, got {} (body: {})",
        spend,
        body
    );
}

#[then("global/spend/keys 应用层聚合结果正确")]
async fn then_global_keys_correct(world: &mut TestWorld) {
    if !seed_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");

    // /global/spend/keys returns data with per-key spend from spend_logs.
    // The response body is {"data": [{"api_key": ..., "spend": ...}, ...]}.
    // Our test keys may only appear among upstream data, so check they are present
    // with the expected spend values.
    let arr = body
        .get("data")
        .and_then(|v| v.as_array())
        .expect("expected data array in /global/spend/keys response");

    assert!(!arr.is_empty(), "keys data array is empty");

    // Find our test keys by known token hashes
    let hash_a = aigw_core::crypto::hash_token("sk-sum-keys-a-76");
    let hash_b = aigw_core::crypto::hash_token("sk-sum-keys-b-76");

    let key_a_entry = arr
        .iter()
        .find(|e| e.get("api_key").and_then(|v| v.as_str()) == Some(&hash_a));
    let key_b_entry = arr
        .iter()
        .find(|e| e.get("api_key").and_then(|v| v.as_str()) == Some(&hash_b));

    let spend_a = key_a_entry
        .and_then(|e| e.get("spend").and_then(|v| v.as_f64()))
        .unwrap_or(0.0);
    let spend_b = key_b_entry
        .and_then(|e| e.get("spend").and_then(|v| v.as_f64()))
        .unwrap_or(0.0);

    assert!(
        (spend_a - 3.0).abs() < 0.01,
        "Expected key_a spend=3.0, got {} (body: {})",
        spend_a,
        body
    );
    assert!(
        (spend_b - 8.0).abs() < 0.01,
        "Expected key_b spend=8.0, got {} (body: {})",
        spend_b,
        body
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 401 / 400 — mock-oriented steps (handled by build_spend_router in mock mode)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 key {string} 发送 GET \\/spend\\/tags 请求（无 tag 参数）")]
async fn when_tags_no_param(world: &mut TestWorld, alias: String) {
    if !real_api_steps::real_api_enabled() {
        real_api_steps::set_skip_pass(
            world,
            400,
            serde_json::json!({"error": {"message": "missing tag parameter"}}),
        );
        return;
    }
    let token = world
        .created_keys
        .get(&alias)
        .cloned()
        .expect("key not found");
    let url = format!("{}/spend/tags", real_api_steps::base_url());
    let client = real_api_steps::client();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .expect("request failed");

    eprintln!(
        "[DEBUG] /spend/tags no-param status={} body={:?}",
        resp.status().as_u16(),
        resp.text().await.unwrap_or_default()
    );
}
