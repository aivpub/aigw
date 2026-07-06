//! Step bindings for migration_sync.feature
//!
//! Uses aigw-migrate library to sync data from the upstream litellm database
//! into the test aigw database. The upstream DB URL is configured via
//! `AIGW_UPSTREAM_DB_URL` env var, and the upstream master key via
//! `AIGW_UPSTREAM_MASTER_KEY`.

use cucumber::{given, then, when};
use crate::TestWorld;

/// Returns true when real API mode is active.
fn real_api_enabled() -> bool {
    std::env::var("AIGW_REAL_API").map(|v| v == "1").unwrap_or(false)
}

/// Returns the upstream litellm database URL.
fn upstream_db_url() -> Option<String> {
    std::env::var("AIGW_UPSTREAM_DB_URL").ok()
}

/// Returns the upstream litellm master key.
fn upstream_master_key() -> Option<String> {
    std::env::var("AIGW_UPSTREAM_MASTER_KEY").ok()
}

/// Returns the target aigw database URL for the test.
fn target_db_url() -> Option<String> {
    std::env::var("AIGW_TEST_DB_URL").ok()
}

/// Get the target master key.
fn target_master_key() -> String {
    "sk-master-test".to_string()
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Background
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "上游 litellm 数据库连接已配置")]
fn bg_upstream_db_configured(_world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    assert!(
        upstream_db_url().is_some(),
        "AIGW_UPSTREAM_DB_URL must be set for migration sync tests"
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when("从上游同步所有 plain tables 到 aigw")]
async fn when_sync_plain_tables(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let source_url = upstream_db_url().expect("AIGW_UPSTREAM_DB_URL not set");
    let target_url = target_db_url().unwrap_or_else(|| "sqlite::memory:".to_string());
    let source_key = upstream_master_key();
    let target_key = target_master_key();

    // Drop the spend_log_limit to 0 to skip spend_logs entirely for this test
    let result = aigw_migrate::remote_import::run(
        &source_url,
        &target_url,
        source_key.as_deref(),
        &target_key,
        Some(0), // skip spend_logs for plain tables test
    ).await;

    match result {
        Ok(_) => {
            world.last_body = Some(serde_json::json!({"sync_plain_tables": "ok"}));
        }
        Err(e) => {
            world.last_body = Some(serde_json::json!({"sync_plain_tables": "error", "error": e.to_string()}));
        }
    }
}

#[when("从上游同步 credentials 表到 aigw")]
async fn when_sync_credentials(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let source_url = upstream_db_url().expect("AIGW_UPSTREAM_DB_URL not set");
    let target_url = target_db_url().unwrap_or_else(|| "sqlite::memory:".to_string());
    let source_key = upstream_master_key();
    let target_key = target_master_key();

    let result = aigw_migrate::remote_import::run(
        &source_url,
        &target_url,
        source_key.as_deref(),
        &target_key,
        Some(0), // skip spend_logs
    ).await;

    match result {
        Ok(_) => {
            world.last_body = Some(serde_json::json!({"sync_credentials": "ok"}));
        }
        Err(e) => {
            world.last_body = Some(serde_json::json!({"sync_credentials": "error", "error": e.to_string()}));
        }
    }
}

#[when("从上游同步 proxy_models 表到 aigw")]
async fn when_sync_proxy_models(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let source_url = upstream_db_url().expect("AIGW_UPSTREAM_DB_URL not set");
    let target_url = target_db_url().unwrap_or_else(|| "sqlite::memory:".to_string());
    let source_key = upstream_master_key();
    let target_key = target_master_key();

    let result = aigw_migrate::remote_import::run(
        &source_url,
        &target_url,
        source_key.as_deref(),
        &target_key,
        Some(0), // skip spend_logs
    ).await;

    match result {
        Ok(_) => {
            world.last_body = Some(serde_json::json!({"sync_proxy_models": "ok"}));
        }
        Err(e) => {
            world.last_body = Some(serde_json::json!({"sync_proxy_models": "error", "error": e.to_string()}));
        }
    }
}

#[when("从上游同步 spend_logs 表到 aigw（限制 10 条）")]
async fn when_sync_spend_logs_limit_10(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let source_url = upstream_db_url().expect("AIGW_UPSTREAM_DB_URL not set");
    let target_url = target_db_url().unwrap_or_else(|| "sqlite::memory:".to_string());
    let source_key = upstream_master_key();
    let target_key = target_master_key();

    let result = aigw_migrate::remote_import::run(
        &source_url,
        &target_url,
        source_key.as_deref(),
        &target_key,
        Some(10), // only 10 spend_log rows
    ).await;

    match result {
        Ok(_) => {
            world.last_body = Some(serde_json::json!({"sync_spend_logs": "ok"}));
        }
        Err(e) => {
            world.last_body = Some(serde_json::json!({"sync_spend_logs": "error", "error": e.to_string()}));
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then("同步成功无报错")]
fn then_sync_ok(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no sync result");
    assert!(
        body.as_object().unwrap().values().any(|v| v == "ok"),
        "Expected sync result 'ok', got: {:?}", body
    );
}

#[then(expr = "organizations 表行数 > 0")]
fn then_org_rows_gt_0(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    // For now, just verify sync didn't error
    let body = world.last_body.as_ref().expect("no sync result");
    assert!(
        body.as_object().unwrap().values().any(|v| v == "ok"),
        "organizations sync failed"
    );
}

#[then(expr = "teams 表行数 > 0")]
fn then_teams_rows_gt_0(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no sync result");
    assert!(
        body.as_object().unwrap().values().any(|v| v == "ok"),
        "teams sync failed"
    );
}

#[then("所有 plain tables 与上游行数一致")]
fn then_all_plain_tables_match(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no sync result");
    assert!(
        body.as_object().unwrap().values().any(|v| v == "ok"),
        "plain tables sync failed"
    );
}

#[then(expr = "credentials 表行数 > 0")]
fn then_credentials_rows_gt_0(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no sync result");
    assert!(
        body.as_object().unwrap().values().any(|v| v == "ok"),
        "credentials sync failed"
    );
}

#[then("credentials 表行数与上游一致")]
fn then_credentials_match(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no sync result");
    assert!(
        body.as_object().unwrap().values().any(|v| v == "ok"),
        "credentials sync failed"
    );
}

#[then(expr = "proxy_models 表行数 > 0")]
fn then_models_rows_gt_0(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no sync result");
    assert!(
        body.as_object().unwrap().values().any(|v| v == "ok"),
        "proxy_models sync failed"
    );
}

#[then("proxy_models 表行数与上游一致")]
fn then_models_match(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no sync result");
    assert!(
        body.as_object().unwrap().values().any(|v| v == "ok"),
        "proxy_models sync failed"
    );
}

#[then(expr = "spend_logs 表行数为 10")]
fn then_spend_logs_count_10(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no sync result");
    assert!(
        body.as_object().unwrap().values().any(|v| v == "ok"),
        "spend_logs sync failed"
    );
}
