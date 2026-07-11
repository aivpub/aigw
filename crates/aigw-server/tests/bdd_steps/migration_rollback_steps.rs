//! Step bindings for migration_rollback.feature
//!
//! Reverse migration: aigw → litellm with encryption key rotation.
//! Uses aigw-migrate remote_export to sync data from aigw back to the
//! upstream litellm database.

use cucumber::{then, when};
use sqlx::any::AnyPoolOptions;
use sqlx::Row as _;
use crate::TestWorld;

fn real_api_enabled() -> bool {
    std::env::var("AIGW_REAL_API").map(|v| v == "1").unwrap_or(false)
}

fn upstream_db_url() -> Option<String> {
    std::env::var("AIGW_UPSTREAM_DB_URL").ok()
}

fn target_db_url() -> String {
    std::env::var("AIGW_TEST_DB_URL")
        .expect("AIGW_TEST_DB_URL must be set by the BDD test harness")
}

fn source_master_key() -> String {
    "sk-master-test".to_string()
}

fn is_mysql(url: &str) -> bool {
    url.starts_with("mysql://") || url.starts_with("mariadb://")
}

fn quote_table(url: &str, table: &str) -> String {
    if is_mysql(url) {
        format!("`{}`", table)
    } else {
        format!("\"{}\"", table)
    }
}

async fn get_row_count(url: &str, table: &str) -> anyhow::Result<i64> {
    sqlx::any::install_default_drivers();
    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await?;
    let quoted = quote_table(url, table);
    let count: i64 = sqlx::query(&format!("SELECT COUNT(*) FROM {}", quoted))
        .fetch_one(&pool)
        .await
        .map(|row| row.get(0))?;
    pool.close().await;
    Ok(count)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// NOTE: Background step "上游 litellm 数据库连接已配置" is shared
// with migration_sync_steps.rs — no duplicate definition here.
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Rollback all plain tables: aigw → litellm (reverse direction).
/// First does a forward sync to ensure aigw has data, then exports back.
#[when("从 aigw 回滚所有 plain tables 到上游 litellm")]
async fn when_rollback_plain_tables(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let upstream_url = upstream_db_url().expect("AIGW_UPSTREAM_DB_URL not set");
    let aigw_url = target_db_url();
    let upstream_key = std::env::var("AIGW_UPSTREAM_MASTER_KEY").ok();
    let aigw_key = source_master_key();

    // Step 1: Forward sync first to ensure aigw has data
    let _ = aigw_migrate::remote_import_run_filtered(
        &upstream_url,
        &aigw_url,
        upstream_key.as_deref(),
        &aigw_key,
        Some(0),
        None,
        false,
        &std::collections::HashSet::new(),
    )
    .await;

    // Step 2: Reverse export aigw → litellm
    let result = aigw_migrate::remote_export_run(
        &aigw_url,
        &upstream_url,
        &aigw_key,
        upstream_key.as_deref(),
    )
    .await;

    match result {
        Ok(_all_match) => {
            // NOTE: virtual_keys and config are excluded — they are shared
            // across scenarios and mutated by server-side operations.
            let plain_tables: &[(&str, &str)] = &[
                ("organizations", "LiteLLM_OrganizationTable"),
                ("teams", "LiteLLM_TeamTable"),
                ("users", "LiteLLM_UserTable"),
                ("projects", "LiteLLM_ProjectTable"),
                ("budgets", "LiteLLM_BudgetTable"),
                ("organization_memberships", "LiteLLM_OrganizationMembership"),
                ("team_memberships", "LiteLLM_TeamMembership"),
            ];
            let mut counts = serde_json::json!({"success": true});
            for (src, tgt) in plain_tables {
                let src_count = get_row_count(&aigw_url, src).await.unwrap_or(-1);
                let tgt_count = get_row_count(&upstream_url, tgt).await.unwrap_or(-1);
                counts[format!("{src}_count")] = serde_json::json!(src_count);
                counts[format!("{src}_tgt_count")] = serde_json::json!(tgt_count);
            }
            world.last_body = Some(counts);
        }
        Err(e) => {
            world.last_body = Some(serde_json::json!({
                "success": false,
                "error": e.to_string()
            }));
        }
    }
}

/// Rollback credentials: aigw → litellm with key rotation.
#[when("从 aigw 回滚 credentials 表到上游 litellm")]
async fn when_rollback_credentials(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let upstream_url = upstream_db_url().expect("AIGW_UPSTREAM_DB_URL not set");
    let aigw_url = target_db_url();
    let upstream_key = std::env::var("AIGW_UPSTREAM_MASTER_KEY").ok();
    let aigw_key = source_master_key();

    // Forward sync first to ensure aigw has data
    let _ = aigw_migrate::remote_import_run_filtered(
        &upstream_url,
        &aigw_url,
        upstream_key.as_deref(),
        &aigw_key,
        Some(0),
        None,
        false,
        &std::collections::HashSet::new(),
    )
    .await;

    // Reverse export aigw → litellm
    let result = aigw_migrate::remote_export_run(
        &aigw_url,
        &upstream_url,
        &aigw_key,
        upstream_key.as_deref(),
    )
    .await;

    match result {
        Ok(_all_match) => {
            let src_count = get_row_count(&aigw_url, "credentials").await.unwrap_or(-1);
            let tgt_count = get_row_count(&upstream_url, "LiteLLM_CredentialsTable").await.unwrap_or(-1);
            world.last_body = Some(serde_json::json!({
                "success": true,
                "credentials_count": src_count,
                "credentials_tgt_count": tgt_count,
            }));
        }
        Err(e) => {
            world.last_body = Some(serde_json::json!({
                "success": false,
                "error": e.to_string()
            }));
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn assert_success(body: &serde_json::Value) {
    let success = body.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    assert!(success, "Expected success=true, got: {:?}", body);
}

fn assert_counts_match(body: &serde_json::Value, name: &str) {
    let src_key = format!("{name}_count");
    let tgt_key = format!("{name}_tgt_count");
    let src = body.get(&src_key).and_then(|v| v.as_i64()).unwrap_or(-1);
    let tgt = body.get(&tgt_key).and_then(|v| v.as_i64()).unwrap_or(-2);
    assert_eq!(
        tgt, src,
        "Expected {name} count match: aigw={src} litellm={tgt}",
    );
}

#[then("回滚同步成功无报错")]
fn then_rollback_ok(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no rollback result");
    assert_success(body);
}

#[then("回滚后 plain tables 与源 aigw 行数一致")]
fn then_rollback_plain_match(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no rollback result");
    assert_success(body);
    // Exclude shared tables (virtual_keys, config) that are mutated by
    // server-side operations during real API tests.
    for tbl in &[
        "organizations", "teams", "users", "projects", "budgets",
        "organization_memberships", "team_memberships",
    ] {
        assert_counts_match(body, tbl);
    }
}

#[then("回滚后 credentials 表与源 aigw 行数一致")]
fn then_rollback_credentials_match(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no rollback result");
    assert_success(body);
    assert_counts_match(body, "credentials");
}
