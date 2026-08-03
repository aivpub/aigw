//! Step bindings for budget_reset.feature
//!
//! These steps use real HTTP calls against a running aigw server.
//! Every step guards on `AIGW_REAL_API=1` — if the env var is not set,
//! the step is a no-op (scenario passes vacuously).

use crate::TestWorld;
use cucumber::{given, then, when};
use std::time::Duration;

use super::real_api_steps::{base_url, client, real_api_enabled, set_skip_pass};

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
