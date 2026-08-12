//! Stage 86 — aigw-migrate sync integration tests (TDD red-green).
//!
//! These tests drive the full `run_sync` path between two SQLite file DBs
//! whose schema is set up via `aigw_core::db::run_migrations_sqlite` (so the
//! 023 rename `call_id` PK + nullable `request_id` is in place).  SQLite is
//! enough to cover the core expected behaviour — PG/MySQL cross-dialect
//! coverage reuses the `bdd-real-*` testcontainers harness, not these UTs.

use aigw_migrate::native::SourcePool;
use aigw_migrate::sync::{resolve_cursor, resolve_tables, run_sync, DEFAULT_TABLES};
use aigw_migrate::CursorRange;

/// Build a fresh aigw SQLite DB in `dir` with all migrations applied.
async fn fresh_aigw_db(dir: &tempfile::TempDir, name: &str) -> String {
    let path = dir.path().join(name);
    // Database::init normalises the sqlite: URL, sets create_if_missing, and
    // runs the full aigw migration suite (001..023) — including the 023
    // rename of spend_logs PK to `call_id` + nullable `request_id`.
    // sqlite:///abs/path is the canonical absolute form (3 slashes).
    let url = format!("sqlite://{}", path.display());
    let db = aigw_core::db::Database::init(&url)
        .await
        .expect("Database::init + migrations");
    drop(db);
    url
}

/// Connect via SourcePool and seed spend_logs rows with explicit start_time.
async fn seed_spend_log(url: &str, call_id: &str, start_time: &str, model: &str, body: &str) {
    let pool = SourcePool::connect(url).await.unwrap();
    pool.execute_raw(&format!(
        // SQLite binds via execute_raw can't take params, so build literal.
        "INSERT INTO spend_logs \
         (call_id, call_type, api_key, spend, total_tokens, prompt_tokens, \
          completion_tokens, start_time, end_time, model, messages, response) \
         VALUES ('{}', 'chat', 'sk-seed', 0.0, 10, 5, 5, '{}', '{}', '{}', '{}', '{}')",
        call_id, start_time, start_time, model, body, body
    ))
    .await
    .expect("seed spend_log");
}

/// Count rows in a table via SourcePool.
async fn count_rows(url: &str, table: &str) -> i64 {
    let pool = SourcePool::connect(url).await.unwrap();
    pool.count_rows(table).await.unwrap()
}

/// Read a single spend_logs row's body columns (messages, response).
async fn read_spend_log_bodies(url: &str, call_id: &str) -> (Option<String>, Option<String>) {
    let pool = SourcePool::connect(url).await.unwrap();
    let rows = pool
        .read_rows_with_limit(
            "spend_logs",
            // limit 1 filtered by call_id — read_rows has no WHERE, so use
            // raw query via execute_raw + a tiny SELECT.
            Some(1),
        )
        .await
        .unwrap();
    // Fall back: query_scalar_string returns one column only; run twice.
    let messages = pool
        .query_scalar_string(&format!(
            "SELECT messages FROM spend_logs WHERE call_id = '{}'",
            call_id
        ))
        .await
        .unwrap();
    let response = pool
        .query_scalar_string(&format!(
            "SELECT response FROM spend_logs WHERE call_id = '{}'",
            call_id
        ))
        .await
        .unwrap();
    let _ = rows; // silence unused
    (messages, response)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test 1: SQLite→SQLite full-table sync (default 11 tables)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[tokio::test]
async fn test_full_sync_sqlite_to_sqlite() {
    let src_dir = tempfile::tempdir().unwrap();
    let tgt_dir = tempfile::tempdir().unwrap();
    let source_url = fresh_aigw_db(&src_dir, "source.db").await;
    let target_url = fresh_aigw_db(&tgt_dir, "target.db").await;

    // Seed a few plain tables + a spend_log.  Use the real aigw column names
    // (organizations.organization_alias, not organization_name).
    let src = SourcePool::connect(&source_url).await.unwrap();
    src.execute_raw(
        "INSERT INTO organizations (organization_id, organization_alias, budget_id, \
         metadata, models, model_spend, created_at, created_by, updated_at, updated_by) \
         VALUES ('org-1', 'Acme', '', '{}', '{}', '{}', '2026-01-01', 'sys', '2026-01-01', 'sys')",
    )
    .await
    .unwrap();
    src.execute_raw(
        "INSERT INTO teams (team_id, team_alias, organization_id, admins, members, \
         members_with_roles, metadata, models, model_spend, model_max_budget, created_at, \
         updated_at, team_member_permissions, access_group_ids, policies, \
         default_team_member_models) \
         VALUES ('team-1', 'alpha', 'org-1', '{}', '{}', '{}', '{}', '{}', '{}', '{}', \
         '2026-01-01', '2026-01-01', '{}', '{}', '{}', '{}')",
    )
    .await
    .unwrap();
    drop(src);
    seed_spend_log(
        &source_url,
        "call-1",
        "2026-07-20T10:00:00Z",
        "gpt-4",
        "body-1",
    )
    .await;

    let tables = resolve_tables(None).unwrap();
    assert_eq!(tables.len(), 11);
    let stats = run_sync(
        &source_url,
        &target_url,
        &tables,
        &CursorRange::default(),
        false,
        10,
        false,
    )
    .await
    .expect("run_sync");

    // organizations + teams + spend_logs each gained 1 row.
    assert_eq!(count_rows(&target_url, "organizations").await, 1);
    assert_eq!(count_rows(&target_url, "teams").await, 1);
    assert_eq!(count_rows(&target_url, "spend_logs").await, 1);
    // config NOT synced (default excluded).
    assert_eq!(count_rows(&target_url, "config").await, 0);
    // total inserted >= 3 (orgs+teams+spend_logs; other empty tables = 0).
    assert!(stats.total_inserted() >= 3, "stats: {:?}", stats);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test 2: --tables selects a subset
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[tokio::test]
async fn test_tables_subset_only_syncs_named() {
    let src_dir = tempfile::tempdir().unwrap();
    let tgt_dir = tempfile::tempdir().unwrap();
    let source_url = fresh_aigw_db(&src_dir, "source.db").await;
    let target_url = fresh_aigw_db(&tgt_dir, "target.db").await;

    let src = SourcePool::connect(&source_url).await.unwrap();
    src.execute_raw(
        "INSERT INTO organizations (organization_id, organization_alias, budget_id, \
         metadata, models, model_spend, created_at, created_by, updated_at, updated_by) \
         VALUES ('org-2', 'Beta', '', '{}', '{}', '{}', '2026-01-01', 'sys', '2026-01-01', 'sys')",
    )
    .await
    .unwrap();
    src.execute_raw(
        "INSERT INTO teams (team_id, team_alias, organization_id, admins, members, \
         members_with_roles, metadata, models, model_spend, model_max_budget, created_at, \
         updated_at, team_member_permissions, access_group_ids, policies, \
         default_team_member_models) \
         VALUES ('team-2', 'beta', 'org-2', '{}', '{}', '{}', '{}', '{}', '{}', '{}', \
         '2026-01-01', '2026-01-01', '{}', '{}', '{}', '{}')",
    )
    .await
    .unwrap();
    drop(src);
    seed_spend_log(
        &source_url,
        "call-2",
        "2026-07-21T10:00:00Z",
        "gpt-4",
        "body-2",
    )
    .await;

    let tables = resolve_tables(Some("spend_logs,teams")).unwrap();
    assert_eq!(tables, vec!["spend_logs", "teams"]);
    run_sync(
        &source_url,
        &target_url,
        &tables,
        &CursorRange::default(),
        false,
        10,
        false,
    )
    .await
    .unwrap();

    assert_eq!(count_rows(&target_url, "spend_logs").await, 1);
    assert_eq!(count_rows(&target_url, "teams").await, 1);
    // organizations NOT synced (not in subset).
    assert_eq!(count_rows(&target_url, "organizations").await, 0);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test 3: --days 7 filters spend_logs by start_time (other tables full)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[tokio::test]
async fn test_days_filter_spend_logs_only() {
    let src_dir = tempfile::tempdir().unwrap();
    let tgt_dir = tempfile::tempdir().unwrap();
    let source_url = fresh_aigw_db(&src_dir, "source.db").await;
    let target_url = fresh_aigw_db(&tgt_dir, "target.db").await;

    // 3 rows: 1 within last 7 days, 2 older (30 days and 60 days ago).
    let now = chrono::Utc::now();
    let recent = (now - chrono::Duration::days(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let old_30 = (now - chrono::Duration::days(30))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let old_60 = (now - chrono::Duration::days(60))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    seed_spend_log(&source_url, "call-recent", &recent, "gpt-4", "r").await;
    seed_spend_log(&source_url, "call-30", &old_30, "gpt-4", "o30").await;
    seed_spend_log(&source_url, "call-60", &old_60, "gpt-4", "o60").await;

    // A plain table row to confirm --days does NOT gate plain tables.
    let src = SourcePool::connect(&source_url).await.unwrap();
    src.execute_raw(
        "INSERT INTO organizations (organization_id, organization_alias, budget_id, \
         metadata, models, model_spend, created_at, created_by, updated_at, updated_by) \
         VALUES ('org-x', 'Gamma', '', '{}', '{}', '{}', '2026-01-01', 'sys', '2026-01-01', 'sys')",
    )
    .await
    .unwrap();
    drop(src);

    let cursor = resolve_cursor(Some(7), None, None, None).unwrap();
    run_sync(
        &source_url,
        &target_url,
        &["spend_logs".to_string(), "organizations".to_string()],
        &cursor,
        false,
        10,
        false,
    )
    .await
    .unwrap();

    // Only the 1 recent spend_log should land.
    assert_eq!(count_rows(&target_url, "spend_logs").await, 1);
    // organizations is a plain table — full copy regardless of --days.
    assert_eq!(count_rows(&target_url, "organizations").await, 1);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test 4: idempotent rerun — second sync ignores all rows
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[tokio::test]
async fn test_idempotent_rerun() {
    let src_dir = tempfile::tempdir().unwrap();
    let tgt_dir = tempfile::tempdir().unwrap();
    let source_url = fresh_aigw_db(&src_dir, "source.db").await;
    let target_url = fresh_aigw_db(&tgt_dir, "target.db").await;

    seed_spend_log(
        &source_url,
        "call-idem",
        "2026-07-22T10:00:00Z",
        "gpt-4",
        "body",
    )
    .await;

    let tables = vec!["spend_logs".to_string()];
    let first = run_sync(
        &source_url,
        &target_url,
        &tables,
        &CursorRange::default(),
        false,
        10,
        false,
    )
    .await
    .unwrap();
    assert_eq!(count_rows(&target_url, "spend_logs").await, 1);

    let second = run_sync(
        &source_url,
        &target_url,
        &tables,
        &CursorRange::default(),
        false,
        10,
        false,
    )
    .await
    .unwrap();
    // Target count unchanged; second run's inserts == 0, ignored >= 1.
    assert_eq!(count_rows(&target_url, "spend_logs").await, 1);
    assert_eq!(second.total_inserted(), 0);
    assert!(second.total_ignored() >= 1);
    // First run should have inserted 1.
    assert_eq!(first.total_inserted(), 1);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test 5: --skip-body nulls spend_logs body columns on target
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[tokio::test]
async fn test_skip_body_nulls_body_columns() {
    let src_dir = tempfile::tempdir().unwrap();
    let tgt_dir = tempfile::tempdir().unwrap();
    let source_url = fresh_aigw_db(&src_dir, "source.db").await;
    let target_url = fresh_aigw_db(&tgt_dir, "target.db").await;

    seed_spend_log(
        &source_url,
        "call-body",
        "2026-07-23T10:00:00Z",
        "gpt-4",
        "BODY-CONTENT",
    )
    .await;

    let tables = vec!["spend_logs".to_string()];
    run_sync(
        &source_url,
        &target_url,
        &tables,
        &CursorRange::default(),
        true, // skip_body
        10,
        false,
    )
    .await
    .unwrap();

    assert_eq!(count_rows(&target_url, "spend_logs").await, 1);
    let (messages, response) = read_spend_log_bodies(&target_url, "call-body").await;
    // Body columns must be NULL on target (never selected from source).
    assert!(
        messages.is_none() || messages.as_deref() == Some(""),
        "messages should be null/empty, got {:?}",
        messages
    );
    assert!(
        response.is_none() || response.as_deref() == Some(""),
        "response should be null/empty, got {:?}",
        response
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test 6: illegal table name errors out
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[tokio::test]
async fn test_illegal_table_name_errors() {
    let result = resolve_tables(Some("spend_logs,foo,teams"));
    assert!(result.is_err(), "should reject unknown table 'foo'");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("foo"),
        "error should name the bad table: {}",
        err
    );
    assert!(
        err.contains("known"),
        "error should list known tables: {}",
        err
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test 7: config excluded by default; explicit --tables config syncs it
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[tokio::test]
async fn test_config_excluded_by_default_explicit_syncs() {
    let src_dir = tempfile::tempdir().unwrap();
    let tgt_dir = tempfile::tempdir().unwrap();
    let source_url = fresh_aigw_db(&src_dir, "source.db").await;
    let target_url = fresh_aigw_db(&tgt_dir, "target.db").await;

    // Seed config on both ends — source has 'master_key', target has a
    // different 'master_key' that must NOT be overwritten (INSERT OR IGNORE).
    // After migration 018, config's PK is `param_name` (no `id` column).
    let src = SourcePool::connect(&source_url).await.unwrap();
    src.execute_raw(
        "INSERT INTO config (param_name, param_value) \
         VALUES ('litellm_master_key', 'sk-SOURCE')",
    )
    .await
    .unwrap();
    drop(src);
    let tgt = SourcePool::connect(&target_url).await.unwrap();
    tgt.execute_raw(
        "INSERT INTO config (param_name, param_value) \
         VALUES ('litellm_master_key', 'sk-TARGET')",
    )
    .await
    .unwrap();
    drop(tgt);

    // Default: config NOT in table list.
    let default_tables = resolve_tables(None).unwrap();
    assert!(!default_tables.contains(&"config".to_string()));

    // Run default sync — config row count on target must stay at 1 (untouched).
    run_sync(
        &source_url,
        &target_url,
        &default_tables,
        &CursorRange::default(),
        false,
        10,
        false,
    )
    .await
    .unwrap();
    assert_eq!(count_rows(&target_url, "config").await, 1);

    // Verify the target's master_key was NOT overwritten by the default sync
    // (config wasn't synced at all, so the value is the target's own).
    let tgt_val = SourcePool::connect(&target_url)
        .await
        .unwrap()
        .query_scalar_string(
            "SELECT param_value FROM config WHERE param_name = 'litellm_master_key'",
        )
        .await
        .unwrap();
    assert_eq!(tgt_val.as_deref(), Some("sk-TARGET"));

    // Explicit --tables config: INSERT OR IGNORE does NOT overwrite the
    // existing row (same PK 'cfg-1' already present).
    let stats = run_sync(
        &source_url,
        &target_url,
        &["config".to_string()],
        &CursorRange::default(),
        false,
        10,
        false,
    )
    .await
    .unwrap();
    assert_eq!(count_rows(&target_url, "config").await, 1);
    assert_eq!(
        stats.total_inserted(),
        0,
        "existing config row should be ignored, not overwritten"
    );
    let tgt_val2 = SourcePool::connect(&target_url)
        .await
        .unwrap()
        .query_scalar_string(
            "SELECT param_value FROM config WHERE param_name = 'litellm_master_key'",
        )
        .await
        .unwrap();
    assert_eq!(
        tgt_val2.as_deref(),
        Some("sk-TARGET"),
        "master_key must not be overwritten"
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Sanity: DEFAULT_TABLES is the 11 business tables, config excluded.
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_default_tables_excludes_config_and_has_11() {
    assert_eq!(DEFAULT_TABLES.len(), 11);
    assert!(!DEFAULT_TABLES.contains(&"config"));
    for t in [
        "virtual_keys",
        "spend_logs",
        "organizations",
        "teams",
        "users",
        "projects",
        "budgets",
        "organization_memberships",
        "team_memberships",
        "credentials",
        "proxy_models",
    ] {
        assert!(DEFAULT_TABLES.contains(&t), "missing default table {}", t);
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Regression: source schema OLDER than target (pre-025 source lacks the
// `image_tokens` column added by migration 025). sync_spend_logs must
// intersect source columns instead of projecting the full target column list,
// or the SELECT fails with "column image_tokens does not exist".
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Build an aigw SQLite DB with migration 025 NOT applied — i.e. a source
/// whose spend_logs lacks `image_tokens` (older release). SQLite has no DROP
/// COLUMN, so we derive the post-024 column list from the freshly-initialized
/// DB, rebuild `spend_logs` without `image_tokens`, and copy the rows over.
async fn fresh_aigw_db_without_025(dir: &tempfile::TempDir, name: &str) -> String {
    let path = dir.path().join(name);
    let url = format!("sqlite://{}", path.display());
    let db = aigw_core::db::Database::init(&url)
        .await
        .expect("Database::init + migrations (incl 025)");
    drop(db);

    let pool = SourcePool::connect(&url).await.unwrap();
    // Current (post-025) spend_logs columns; drop image_tokens to simulate a
    // pre-025 source.
    let cols: Vec<(String, String, bool)> = pool
        .column_types("spend_logs")
        .await
        .unwrap()
        .into_iter()
        .filter(|(n, _, _)| n != "image_tokens")
        .collect();
    let col_names = cols
        .iter()
        .map(|(n, _, _)| format!("\"{}\"", n))
        .collect::<Vec<_>>()
        .join(", ");

    pool.execute_raw("ALTER TABLE spend_logs RENAME TO spend_logs_tmp")
        .await
        .expect("rename spend_logs");
    // Recreate WITHOUT image_tokens. Keep it simple: TEXT/INTEGER/REAL by
    // nullable flag is enough for the migration-copy to work; the row VALUES
    // are carried verbatim and sqlite is dynamically typed.
    let create = format!(
        "CREATE TABLE spend_logs ({})",
        cols.iter()
            .map(|(n, _ty, nullable)| {
                let ty = if *nullable { "TEXT" } else { "INTEGER" };
                format!("\"{}\" {}", n, ty)
            })
            .collect::<Vec<_>>()
            .join(", ")
    );
    pool.execute_raw(&create)
        .await
        .expect("create pre-025 spend_logs");
    let insert = format!(
        "INSERT INTO spend_logs ({}) SELECT {} FROM spend_logs_tmp",
        col_names, col_names
    );
    pool.execute_raw(&insert)
        .await
        .expect("copy rows into pre-025 spend_logs");
    pool.execute_raw("DROP TABLE spend_logs_tmp")
        .await
        .expect("drop tmp");
    url
}

#[tokio::test]
async fn test_sync_older_source_schema_without_image_tokens() {
    let src_dir = tempfile::tempdir().unwrap();
    let tgt_dir = tempfile::tempdir().unwrap();
    let source_url = fresh_aigw_db_without_025(&src_dir, "source.db").await;
    let target_url = fresh_aigw_db(&tgt_dir, "target.db").await; // post-025 (has image_tokens)

    // Seed a spend_log row on the OLD source.
    let now = chrono::Utc::now();
    let ts = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    seed_spend_log(&source_url, "src-old-001", &ts, "gpt-4", "{}").await;

    // Sync spend_logs only. Must NOT fail on image_tokens; the row lands with
    // image_tokens NULL (pre-025 source has no such data).
    let cursor = CursorRange {
        resume_after: None,
        end_before: None,
    };
    run_sync(
        &source_url,
        &target_url,
        &["spend_logs".to_string()],
        &cursor,
        false,
        10,
        false,
    )
    .await
    .expect("sync must succeed with a pre-025 source (no image_tokens)");

    assert_eq!(count_rows(&target_url, "spend_logs").await, 1);
    // The synced row carries NULL image_tokens (column exists on target only).
    let pool = SourcePool::connect(&target_url).await.unwrap();
    let rows = pool.read_rows("spend_logs").await.unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    // UnifiedRow is Vec<(name, Value)> — find image_tokens (present, NULL).
    let img = row.iter().find(|(n, _)| n == "image_tokens");
    assert!(
        img.map(|(_, v)| v.is_null()).unwrap_or(true),
        "image_tokens should be NULL on a row synced from a pre-025 source"
    );
}
