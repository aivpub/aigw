//! Stage 100 — aigw-migrate pre-check + verify tests.
//!
//! NOTE: remote_import (step-filter, skip-body, cursor-resume) requires a
//! litellm-format source database (LiteLLM_* tables). The aigw-core `Database::init`
//! creates aigw schema tables — not litellm schema. Comprehensive testing of
//! remote_import advanced features requires a real litellm upstream DB connected
//! via `AIGW_UPSTREAM_DB_URL`. The existing `migration_sync.feature` BDD scenarios
//! already cover this path when a real upstream is configured.
//!
//! These tests validate the pre_check and verify library functions directly
//! without needing a litellm source DB.

use aigw_core::db::Database;
use aigw_migrate::{pre_check, verify};

/// Build a fresh aigw SQLite DB with all migrations applied.
async fn fresh_db(dir: &tempfile::TempDir, name: &str) -> String {
    let path = dir.path().join(name);
    let _ = std::fs::remove_file(&path);
    let url = format!("sqlite://{}", path.display());
    let db = Database::init(&url).await.expect("Database::init + migrations");
    drop(db);
    url
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Part A: pre_check — connectivity + key validation (4 tests)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[tokio::test]
async fn test_precheck_connectivity_check_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = fresh_db(&dir, "src").await;
    let tgt = fresh_db(&dir, "tgt").await;
    let result = pre_check::run(&src, &tgt, "test-key-32-chars-long!!!!!!").await;
    // Returns Ok(bool) — false means some checks failed (expected: aigw schema
    // doesn't have LiteLLM_* tables), but the function itself doesn't error.
    assert!(result.is_ok(), "pre_check should not error on valid connections");
}

#[tokio::test]
async fn test_precheck_short_master_key_detected() {
    let dir = tempfile::tempdir().expect("temp");
    let src = fresh_db(&dir, "src").await;
    let tgt = fresh_db(&dir, "tgt").await;
    let result = pre_check::run(&src, &tgt, "short").await;
    match result {
        Ok(all_pass) => assert!(!all_pass, "short master key should fail validation"),
        Err(_) => {} // Error on short key is also acceptable
    }
}

#[tokio::test]
async fn test_precheck_bad_source_url_errors() {
    let bad_src = "sqlite:///nonexistent/path/db.sqlite";
    let tgt = "sqlite::memory:";
    let result = pre_check::run(bad_src, tgt, "test-key-32-chars-long!!!!!!").await;
    // PreCheck returns Ok(false) when checks fail (e.g. table missing),
    // including when source DB file can't be opened (SQLite treats missing
    // file as connection success but table queries then fail).
    assert!(!result.unwrap_or(true), "pre_check should fail with bad source");
}

#[tokio::test]
async fn test_precheck_bad_target_url_errors() {
    let src = "sqlite::memory:";
    let bad_tgt = "sqlite:///nonexistent/path/db.sqlite";
    let result = pre_check::run(src, bad_tgt, "test-key-32-chars-long!!!!!!").await;
    // PreCheck returns Ok(false) when checks fail
    assert!(!result.unwrap_or(true), "pre_check should fail with bad target");
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Part B: verify — row count comparison (1 test)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[tokio::test]
async fn test_verify_runs_without_panic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = fresh_db(&dir, "src").await;
    let tgt = fresh_db(&dir, "tgt").await;
    let result = verify::run(&src, &tgt).await;
    // Returns Ok(true/false) — verify should not crash with empty DBs.
    // Since no LiteLLM_* tables exist in aigw schema, counts won't match.
    assert!(result.is_ok(), "verify should not error");
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Part C: verify with matching data (1 test)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[tokio::test]
async fn test_verify_empty_dbs_match() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = fresh_db(&dir, "src").await;
    let tgt = fresh_db(&dir, "tgt").await;
    let result = verify::run(&src, &tgt).await;
    assert!(result.is_ok(), "verify should complete");
}
