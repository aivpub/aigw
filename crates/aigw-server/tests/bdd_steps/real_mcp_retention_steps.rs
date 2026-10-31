//! Step bindings for real/mcp_log_retention_real.feature — Stage 150 real BDD.
//!
//! The retention sweep is the one place where "它删对了行" cannot be shown on a
//! mock database: the batch loop is `SELECT id … LIMIT n` then `DELETE … WHERE
//! <key> IN (…)`, and the two tables disagree on what the key is (`mcp_call_log`
//! uses `call_id`, `mcp_auth_rejection` uses `id`). A wrong column name passes on
//! SQLite only if the delete silently affects zero rows — and it does, so an
//! assertion on the response code alone would be green while nothing was deleted.
//!
//! So these steps run the real [`McpLogRetention`] worker against the test
//! database (PostgreSQL / MySQL / SQLite depending on `AIGW_TEST_DB_DRIVER`),
//! seed rows through the same `Database` handle the server uses, and read the
//! survivors back from the catalog.

use cucumber::{given, then, when};
use serde_json::Value;

use aigw_core::async_task::{AsyncTask, StepRecord};
use aigw_core::db::Database;
use aigw_core::models::{McpAuthRejectionRow, McpCallBodyRow, McpCallLogRow};
use aigw_core::retention::{McpLogRetention, McpLogRetentionConfig};

use super::real_api_steps::real_api_enabled;
use crate::TestWorld;

fn test_db_url() -> String {
    std::env::var("AIGW_TEST_DB_URL").expect("AIGW_TEST_DB_URL must be set by the BDD harness")
}

/// A connection of its own: the spawned server holds its own pool, and the
/// rows these steps seed must be visible to a separate `Database` handle for the
/// "was it really deleted" question to mean anything.
async fn db() -> Database {
    Database::init(&test_db_url())
        .await
        .expect("open the test database")
}

/// `old` is far enough back that no configured window can include it, and
/// `fresh` is stamped now — the two marker sets are what the assertions count.
const OLD_STAMP: &str = "2020-01-01T00:00:00+00:00";

fn marker(prefix: &str, i: usize) -> String {
    format!("retention-real-{prefix}-{i}")
}

async fn seed_call(db: &Database, id: &str, created_at: &str) {
    db.insert_mcp_call_log(&McpCallLogRow {
        call_id: id.to_string(),
        server_alias: "retention-real".to_string(),
        tool_name: None,
        method: "tools/list".to_string(),
        key_id: Some("*master*".to_string()),
        user_id: None,
        token_owner: "server".to_string(),
        status: "success".to_string(),
        error_code: None,
        latency_ms: 0,
        request_bytes: 0,
        response_bytes: 0,
        created_at: created_at.to_string(),
    })
    .await
    .expect("insert mcp_call_log");
}

async fn seed_rejection(db: &Database, id: &str, created_at: &str) {
    db.insert_mcp_rejection(&McpAuthRejectionRow {
        id: id.to_string(),
        created_at: created_at.to_string(),
        path: "/mcp".to_string(),
        http_method: "POST".to_string(),
        reason: "missing_header".to_string(),
        token_hash: None,
        client_ip: None,
        user_agent: None,
    })
    .await
    .expect("insert mcp_auth_rejection");
}

async fn seed_body(db: &Database, id: &str, created_at: &str) {
    db.insert_mcp_call_body(&McpCallBodyRow {
        call_id: id.to_string(),
        request_body: Some(r#"{"jsonrpc":"2.0","id":1}"#.to_string()),
        response_body: Some(r#"{"result":{}}"#.to_string()),
        truncated: false,
        created_at: created_at.to_string(),
    })
    .await
    .expect("insert mcp_call_body");
}

fn new_stamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Rows left behind by an earlier scenario — or by an earlier run against the
/// same volume, which `task test-db-down` keeps — would make "everything expired
/// is gone" true for the wrong reason. Every marker this file writes starts
/// with this prefix, so the cleanup cannot reach real test data.
async fn clear_markers() {
    let pool = aigw_migrate::native::SourcePool::connect(&test_db_url())
        .await
        .expect("connect");
    pool.execute_raw("DELETE FROM mcp_call_log WHERE call_id LIKE 'retention-real-%'")
        .await
        .expect("clear mcp_call_log markers");
    pool.execute_raw("DELETE FROM mcp_call_body WHERE call_id LIKE 'retention-real-%'")
        .await
        .expect("clear mcp_call_body markers");
    pool.execute_raw("DELETE FROM mcp_auth_rejection WHERE id LIKE 'retention-real-%'")
        .await
        .expect("clear mcp_auth_rejection markers");
}

async fn row_count(pool: &aigw_migrate::native::SourcePool, sql: &str) -> i64 {
    let rows = pool.read_rows_sql(sql).await.expect("count rows");
    rows.first()
        .and_then(|r| r.first())
        .map(|(_, v)| match v {
            Value::Number(n) => n.as_i64().unwrap_or(-1),
            Value::String(s) => s.parse().unwrap_or(-1),
            _ => -1,
        })
        .unwrap_or(-1)
}

fn retention_step(payload: serde_json::Value) -> StepRecord {
    StepRecord {
        id: "retention-real-step".to_string(),
        job_id: "retention-real-job".to_string(),
        step_key: "retention=real".to_string(),
        step_type: "mcp_log_retention".to_string(),
        status: "running".to_string(),
        payload,
        result: None,
        error_message: None,
        retry_count: 0,
        started_at: None,
        completed_at: None,
        next_retry_at: None,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "真库保留期场景在三张表各写入 {int} 行过期与 {int} 行新鲜记录")]
async fn given_seed_retention_rows(_world: &mut TestWorld, old: usize, fresh: usize) {
    if !real_api_enabled() {
        return;
    }
    clear_markers().await;
    let db = db().await;
    let stamp = new_stamp();
    for i in 0..old {
        seed_call(&db, &marker("old", i), OLD_STAMP).await;
        seed_body(&db, &marker("old", i), OLD_STAMP).await;
        seed_rejection(&db, &marker("old", i), OLD_STAMP).await;
    }
    for i in 0..fresh {
        seed_call(&db, &marker("new", i), &stamp).await;
        seed_body(&db, &marker("new", i), &stamp).await;
        seed_rejection(&db, &marker("new", i), &stamp).await;
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "真库保留期 worker 以 batch_size={int} 执行一次清理")]
async fn when_run_retention(_world: &mut TestWorld, batch_size: i64) {
    if !real_api_enabled() {
        return;
    }
    let db = db().await;
    let worker = McpLogRetention::new(McpLogRetentionConfig {
        call_log_days: 90,
        rejection_days: 30,
        batch_size,
        check_interval_secs: 3600,
        ..Default::default()
    });

    let steps = worker
        .tick(&db)
        .await
        .expect("tick")
        .expect("the seeded expired rows must produce a step");
    worker
        .execute(&db, &retention_step(steps[0].payload.clone()))
        .await
        .expect("execute");
}

#[when(expr = "真库保留期 worker 配置 call_log_days=0 执行一次清理")]
async fn when_run_retention_with_calls_disabled(_world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let db = db().await;
    let worker = McpLogRetention::new(McpLogRetentionConfig {
        call_log_days: 0,
        rejection_days: 30,
        batch_size: 5000,
        check_interval_secs: 3600,
        ..Default::default()
    });
    let steps = worker
        .tick(&db)
        .await
        .expect("tick")
        .expect("the seeded expired rejections must produce a step");
    worker
        .execute(&db, &retention_step(steps[0].payload.clone()))
        .await
        .expect("execute");
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "真库中过期记录已全部删除、{int} 行新鲜记录仍在")]
async fn then_expired_gone(_world: &mut TestWorld, fresh: usize) {
    if !real_api_enabled() {
        return;
    }
    let pool = aigw_migrate::native::SourcePool::connect(&test_db_url())
        .await
        .expect("connect");
    let old_prefix = marker("old", 0).trim_end_matches('0').to_string();
    let fresh_prefix = marker("new", 0).trim_end_matches('0').to_string();
    let old_calls = row_count(
        &pool,
        &format!("SELECT COUNT(*) FROM mcp_call_log WHERE call_id LIKE '{old_prefix}%'"),
    )
    .await;
    assert_eq!(old_calls, 0, "expired mcp_call_log rows must be gone");
    let old_bodies = row_count(
        &pool,
        &format!("SELECT COUNT(*) FROM mcp_call_body WHERE call_id LIKE '{old_prefix}%'"),
    )
    .await;
    assert_eq!(
        old_bodies, 0,
        "expired mcp_call_body rows must be gone with their call"
    );
    let old_rejections = row_count(
        &pool,
        &format!("SELECT COUNT(*) FROM mcp_auth_rejection WHERE id LIKE '{old_prefix}%'"),
    )
    .await;
    assert_eq!(
        old_rejections, 0,
        "expired mcp_auth_rejection rows must be gone"
    );

    let fresh_calls = row_count(
        &pool,
        &format!("SELECT COUNT(*) FROM mcp_call_log WHERE call_id LIKE '{fresh_prefix}%'"),
    )
    .await;
    assert_eq!(
        fresh_calls, fresh as i64,
        "fresh mcp_call_log rows must be untouched"
    );
    let fresh_bodies = row_count(
        &pool,
        &format!("SELECT COUNT(*) FROM mcp_call_body WHERE call_id LIKE '{fresh_prefix}%'"),
    )
    .await;
    assert_eq!(
        fresh_bodies, fresh as i64,
        "fresh mcp_call_body rows must be untouched"
    );
    let fresh_rejections = row_count(
        &pool,
        &format!("SELECT COUNT(*) FROM mcp_auth_rejection WHERE id LIKE '{fresh_prefix}%'"),
    )
    .await;
    assert_eq!(
        fresh_rejections, fresh as i64,
        "fresh mcp_auth_rejection rows must be untouched"
    );
}

#[then(expr = "真库中过期调用日志与 body 仍在而过期拒绝记录已删除")]
async fn then_calls_kept(_world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let pool = aigw_migrate::native::SourcePool::connect(&test_db_url())
        .await
        .expect("connect");
    let old_prefix = marker("old", 0).trim_end_matches('0').to_string();
    let calls = row_count(
        &pool,
        &format!("SELECT COUNT(*) FROM mcp_call_log WHERE call_id LIKE '{old_prefix}%'"),
    )
    .await;
    assert!(
        calls > 0,
        "call_log_days=0 must keep every expired call-log row, found none"
    );
    let bodies = row_count(
        &pool,
        &format!("SELECT COUNT(*) FROM mcp_call_body WHERE call_id LIKE '{old_prefix}%'"),
    )
    .await;
    assert_eq!(
        bodies, calls,
        "bodies follow their call rows: turning call_log_days off must keep both"
    );
    let rejections = row_count(
        &pool,
        &format!("SELECT COUNT(*) FROM mcp_auth_rejection WHERE id LIKE '{old_prefix}%'"),
    )
    .await;
    assert_eq!(
        rejections, 0,
        "the table with a configured window must still be cleaned"
    );
}

#[then(expr = "真库 mcp_auth_rejection 表存在")]
async fn then_rejection_table_exists(_world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let pool = aigw_migrate::native::SourcePool::connect(&test_db_url())
        .await
        .expect("connect");
    let count = row_count(&pool, "SELECT COUNT(*) FROM mcp_auth_rejection").await;
    assert!(
        count >= 0,
        "035 must have created mcp_auth_rejection in this dialect"
    );
}

#[then(expr = "真库 mcp_call_body 表存在")]
async fn then_call_body_table_exists(_world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let pool = aigw_migrate::native::SourcePool::connect(&test_db_url())
        .await
        .expect("connect");
    let count = row_count(&pool, "SELECT COUNT(*) FROM mcp_call_body").await;
    assert!(
        count >= 0,
        "036 must have created mcp_call_body in this dialect"
    );
}
