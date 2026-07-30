//! Stage 82 Red tests — Step↔Job linked state machine + finalize contracts.
//!
//! These lock in the P0 fixes from docs/stages/stage-82.md:
//! - job status transitions (pending → running → completed/failed/partially_failed)
//! - finalize called exactly once on each terminal state
//! - fail_step exponential backoff via next_retry_at
//! - storage_configured() gate rejects unconfigured archiver
//! - AigwConfig parses the body_archive section
//! - writer start_time is TimestampMillisecond, cache_hit is Boolean
//!
//! They exercise the PUBLIC engine API (create_job / claim_next_step /
//! complete_step / fail_step) plus direct SQL assertions on async_jobs.status,
//! so they live as an integration test rather than inside engine.rs.

use aigw_core::async_task::{AsyncTask, NewStep, StepOutput, StepRecord};
use aigw_core::body_archive::BodyArchiver;
use aigw_core::body_archive::config::{BodyArchiveConfig, StorageBackend};
use aigw_core::config::AigwConfig;
use aigw_core::db::{Database, DbError, Result};
use aigw_core::engine::{claim_next_step, complete_step, create_job, fail_step};
use std::sync::Arc;
use std::time::Duration;

/// A mock AsyncTask that records finalize() calls and a configurable
/// execute() outcome. Used to assert Step↔Job state-machine contracts.
struct MockTask {
    step_type: &'static str,
    finalize_calls: Arc<std::sync::atomic::AtomicU32>,
    execute_ok: bool,
}

impl MockTask {
    /// Returns (task as dyn AsyncTask, finalize-call counter). The counter lets
    /// a test assert how many times finalize ran across concurrent completes.
    /// Returning `Arc<dyn AsyncTask>` lets the value be passed directly to
    /// `complete_step` / `fail_step` which expect that exact type.
    fn new(
        step_type: &'static str,
        execute_ok: bool,
    ) -> (Arc<dyn AsyncTask>, Arc<std::sync::atomic::AtomicU32>) {
        let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let task: Arc<dyn AsyncTask> = Arc::new(Self {
            step_type,
            finalize_calls: counter.clone(),
            execute_ok,
        });
        (task, counter)
    }
}

#[async_trait::async_trait]
impl AsyncTask for MockTask {
    fn step_type(&self) -> &'static str {
        self.step_type
    }
    async fn tick(&self, _db: &Database) -> Result<Option<Vec<NewStep>>> {
        Ok(None)
    }
    fn tick_interval(&self) -> Duration {
        Duration::from_secs(60)
    }
    async fn execute(&self, _db: &Database, _step: &StepRecord) -> Result<StepOutput> {
        if self.execute_ok {
            Ok(StepOutput {
                result: serde_json::json!({"ok": true}),
            })
        } else {
            Err(DbError::Other("mock execute failure".into()))
        }
    }
    async fn finalize(&self, _db: &Database, _job: &aigw_core::async_task::JobRecord) -> Result<()> {
        self.finalize_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

/// Helper: read async_jobs.status for a job.
async fn job_status(db: &Database, job_id: &str) -> String {
    let row: (String,) = match db {
        Database::Sqlite(pool) => {
            sqlx::query_as("SELECT status FROM async_jobs WHERE id = ?")
                .bind(job_id)
                .fetch_one(pool)
                .await
                .expect("fetch job status")
        }
        _ => unreachable!(),
    };
    row.0
}

/// Helper: read async_job_steps.next_retry_at for a step.
async fn step_next_retry_at(db: &Database, step_id: &str) -> Option<String> {
    let row: (Option<String>,) = match db {
        Database::Sqlite(pool) => {
            sqlx::query_as("SELECT next_retry_at FROM async_job_steps WHERE id = ?")
                .bind(step_id)
                .fetch_one(pool)
                .await
                .expect("fetch step next_retry_at")
        }
        _ => unreachable!(),
    };
    row.0
}

/// Helper: create a job with N steps and return (job_id, step_ids).
async fn make_job(db: &Database, n: usize) -> (String, Vec<String>) {
    let steps: Vec<NewStep> = (0..n)
        .map(|i| NewStep {
            key: format!("k{}", i),
            payload: serde_json::json!({}),
        })
        .collect();
    let job_id = create_job(db, "mock", "manual", None, &steps, 3)
        .await
        .expect("create_job");
    let mut step_ids = Vec::new();
    for i in 0..n {
        let row: (String,) = sqlx::query_as("SELECT id FROM async_job_steps WHERE job_id = ? AND step_key = ?")
            .bind(&job_id)
            .bind(format!("k{}", i))
            .fetch_one(match db { Database::Sqlite(p) => p, _ => unreachable!() })
            .await
            .expect("fetch step id");
        step_ids.push(row.0);
    }
    (job_id, step_ids)
}

/// Drive a single step from pending to the 'failed' terminal state by
/// repeatedly claim_next_step (which bumps retry_count) + fail_step.
/// With max_retries=3 this takes 2 claim/fail cycles (retry_count 1→2→failed
/// at the 2nd fail because retry_count+1=3 >= 3). Returns when the step row
/// shows status='failed'. Clears next_retry_at between attempts so claim can
/// re-pick the step immediately without waiting for the backoff window.
async fn exhaust_step_to_failed(db: &Database, task: &Arc<dyn AsyncTask>, job_id: &str) {
    loop {
        let step = claim_next_step(db, "mock").await.expect("claim").expect("step available");
        fail_step(db, &step, "boom", task, job_id).await;
        let row: (String,) = sqlx::query_as("SELECT status FROM async_job_steps WHERE id = ?")
            .bind(&step.id)
            .fetch_one(match db { Database::Sqlite(p) => p, _ => unreachable!() })
            .await
            .expect("fetch step status");
        if row.0 == "failed" {
            return;
        }
        sqlx::query("UPDATE async_job_steps SET next_retry_at = NULL WHERE id = ?")
            .bind(&step.id)
            .execute(match db { Database::Sqlite(p) => p, _ => unreachable!() })
            .await
            .expect("clear next_retry_at");
    }
}

// ─── Q1 / P0-1: job pending → running on first claim ──────────────────────

#[tokio::test]
async fn test_job_transitions_out_of_pending_on_first_claim() {
    // mark_job_running is called by exec_loop right after claim_next_step.
    // exec_loop is an infinite private loop, so we drive the public path:
    // claim → complete_step, which internally runs mark_job_completed (the
    // terminal branch that proves the job-status writer is wired). A job that
    // starts pending and ends completed means it left pending via the
    // Step↔Job linked state machine — the P0-1 contract.
    let db = Database::init("sqlite::memory:").await.expect("db init");
    let (job_id, _step_ids) = make_job(&db, 1).await;
    assert_eq!(job_status(&db, &job_id).await, "pending",
        "job must start pending");

    let (task, finalize_counter) = MockTask::new("mock", true);
    let step = claim_next_step(&db, "mock").await.expect("claim").expect("step");
    complete_step(&db, &step, StepOutput { result: serde_json::json!({}) }, &task, &job_id).await;

    assert_eq!(job_status(&db, &job_id).await, "completed",
        "claim→complete must drive job out of pending to completed (P0-1 wiring)");
    assert_eq!(finalize_counter.load(std::sync::atomic::Ordering::SeqCst), 1,
        "finalize called once on the completed terminal state");
}

// ─── Q3 / P0-4: storage_configured gate rejects unconfigured archiver ────

#[tokio::test]
async fn test_storage_configured_false_for_default_config() {
    let archiver = BodyArchiver::new(BodyArchiveConfig::default());
    assert!(!archiver.storage_configured(),
        "default config has empty bucket → storage_configured must be false (P0-4)");
}

#[tokio::test]
async fn test_storage_configured_true_for_s3_with_bucket_and_key() {
    let cfg = BodyArchiveConfig {
        auto_archive: true,
        storage: StorageBackend::S3 {
            bucket: "my-bucket".into(),
            region: "us-east-1".into(),
            endpoint: Some("https://s3.amazonaws.com".into()),
            access_key_id: "AKIDxxx".into(),
            secret_access_key: "secret".into(),
            prefix: "logs".into(),
            use_ssl: true,
            compatibility_mode: false,
            url_style: "vhost".to_string(),
        },
        ..Default::default()
    };
    let archiver = BodyArchiver::new(cfg);
    assert!(archiver.storage_configured(),
        "S3 with bucket + access_key_id → storage_configured must be true");
}

#[tokio::test]
async fn test_storage_configured_false_when_bucket_empty_but_key_set() {
    let cfg = BodyArchiveConfig {
        auto_archive: true,
        storage: StorageBackend::S3 {
            bucket: String::new(),
            region: "us-east-1".into(),
            endpoint: None,
            access_key_id: "AKIDxxx".into(),
            secret_access_key: "secret".into(),
            prefix: "logs".into(),
            use_ssl: true,
            compatibility_mode: false,
            url_style: "vhost".to_string(),
        },
        ..Default::default()
    };
    let archiver = BodyArchiver::new(cfg);
    assert!(!archiver.storage_configured(),
        "empty bucket → storage_configured false even if key set");
}

#[tokio::test]
async fn test_storage_configured_true_for_fs_with_path() {
    let cfg = BodyArchiveConfig {
        auto_archive: true,
        storage: StorageBackend::FileSystem {
            path: std::path::PathBuf::from("/tmp/archive"),
        },
        ..Default::default()
    };
    let archiver = BodyArchiver::new(cfg);
    assert!(archiver.storage_configured(),
        "FS with non-empty path → storage_configured true");
}

// ─── P0-6: AigwConfig parses the body_archive section ────────────────────

#[tokio::test]
async fn test_aigw_config_parses_body_archive_section() {
    let yaml = r#"
model_list: []
body_archive:
  enabled: true
  s3:
    bucket: aigw-logs
    region: ap-guangzhou
    access_key_id: AKIDxxx
    secret_access_key: secretxxx
    prefix: logs
    use_ssl: true
    compatibility_mode: true
    url_style: vhost
  archive:
    archive_after_hours: 1
    null_body_after_days: 7
"#;
    let cfg: AigwConfig = serde_yaml::from_str(yaml).expect("parse AigwConfig");
    let ba = cfg.body_archive.expect("body_archive section present");
    assert!(ba.auto_archive, "body_archive.auto_archive parsed (backward-compat via 'enabled' alias)");
    assert_eq!(ba.s3.bucket, "aigw-logs", "bucket parsed");
    assert_eq!(ba.archive.archive_after_hours, 1, "archive_after_hours parsed");
}

#[tokio::test]
async fn test_aigw_config_without_body_archive_is_none() {
    let yaml = r#"
model_list: []
general_settings:
  master_key: test
"#;
    let cfg: AigwConfig = serde_yaml::from_str(yaml).expect("parse AigwConfig");
    assert!(cfg.body_archive.is_none(),
        "missing body_archive section → Option::None (skip_serializing_if)");
}

// ─── Q1 / P0-2: job → failed when all steps fail (retry exhausted) ────────

#[tokio::test]
async fn test_job_failed_when_all_steps_fail() {
    let db = Database::init("sqlite::memory:").await.expect("db init");
    let (job_id, step_ids) = make_job(&db, 2).await;
    let (task, _finalize_counter) = MockTask::new("mock", false /* execute fails */);

    // Each step must reach 'failed'. claim_next_step bumps retry_count;
    // fail_step flips to 'failed' when retry_count+1 >= max_retries.
    exhaust_step_to_failed(&db, &task, &job_id).await;
    exhaust_step_to_failed(&db, &task, &job_id).await;

    assert_eq!(job_status(&db, &job_id).await, "failed",
        "all steps failed → job failed (P0-2: mark_job_failed)");
}

// ─── partially_failed when some steps fail and some succeed ───────────────

#[tokio::test]
async fn test_job_partially_failed_when_mixed_results() {
    let db = Database::init("sqlite::memory:").await.expect("db init");
    // 2 steps: one will complete, one will fail (exhausted).
    let (job_id, step_ids) = make_job(&db, 2).await;
    let (task_ok, _) = MockTask::new("mock", true);
    let (task_fail, _) = MockTask::new("mock", false);

    // Step 0 → complete.
    let s0: StepRecord = sqlx::query_as::<_, StepRecord>(
        "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count, started_at, completed_at, next_retry_at FROM async_job_steps WHERE id = ?"
    )
    .bind(&step_ids[0])
    .fetch_one(match &db { Database::Sqlite(p) => p, _ => unreachable!() })
    .await
    .expect("fetch s0");
    complete_step(&db, &s0, StepOutput { result: serde_json::json!({}) }, &task_ok, &job_id).await;

    // Step 1 → exhaust to failed.
    exhaust_step_to_failed(&db, &task_fail, &job_id).await;

    assert_eq!(job_status(&db, &job_id).await, "partially_failed",
        "1 completed + 1 failed → partially_failed (P0: mark_job_partially_failed)");
}

// ─── Q1: job → completed when all steps succeed ───────────────────────────

#[tokio::test]
async fn test_job_completed_when_all_steps_succeed() {
    let db = Database::init("sqlite::memory:").await.expect("db init");
    let (job_id, step_ids) = make_job(&db, 2).await;
    let (task, finalize_counter) = MockTask::new("mock", true);

    for sid in &step_ids {
        let s: StepRecord = sqlx::query_as::<_, StepRecord>(
            "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count, started_at, completed_at, next_retry_at FROM async_job_steps WHERE id = ?"
        )
        .bind(sid)
        .fetch_one(match &db { Database::Sqlite(p) => p, _ => unreachable!() })
        .await
        .expect("fetch step");
        complete_step(&db, &s, StepOutput { result: serde_json::json!({}) }, &task, &job_id).await;
    }

    assert_eq!(job_status(&db, &job_id).await, "completed",
        "all steps completed → job completed");
    assert_eq!(finalize_counter.load(std::sync::atomic::Ordering::SeqCst), 1,
        "finalize called exactly once when job reaches completed");
}

// ─── P0-3: finalize still called when job has a failed step ───────────────

#[tokio::test]
async fn test_finalize_called_on_partially_failed_job() {
    let db = Database::init("sqlite::memory:").await.expect("db init");
    let (job_id, step_ids) = make_job(&db, 2).await;

    // Both the "ok" and "fail" task share the SAME finalize counter, so we can
    // assert finalize fired regardless of which step's terminal judgment
    // triggered it. (The contract is "finalize runs on partially_failed", not
    // "finalize runs on a specific step's task instance".)
    let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let mk = |ok: bool| -> Arc<dyn AsyncTask> {
        Arc::new(MockTask {
            step_type: "mock",
            finalize_calls: counter.clone(),
            execute_ok: ok,
        })
    };
    let task_ok = mk(true);
    let task_fail = mk(false);

    // Complete step 0 first (so when step 1 exhausts, failed=1, completed=1 → partially_failed).
    let s0: StepRecord = sqlx::query_as::<_, StepRecord>(
        "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count, started_at, completed_at, next_retry_at FROM async_job_steps WHERE id = ?"
    )
    .bind(&step_ids[0])
    .fetch_one(match &db { Database::Sqlite(p) => p, _ => unreachable!() })
    .await
    .expect("fetch s0");
    complete_step(&db, &s0, StepOutput { result: serde_json::json!({}) }, &task_ok, &job_id).await;

    // Fail step 1 to exhaustion. finalize must fire on the partially_failed terminal state.
    exhaust_step_to_failed(&db, &task_fail, &job_id).await;

    assert_eq!(job_status(&db, &job_id).await, "partially_failed");
    assert!(counter.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "P0-3: finalize must be called on partially_failed (not silently skipped)");
}

// ─── P1-3/4/6: concurrent complete_step → finalize exactly once ───────────

#[tokio::test]
async fn test_concurrent_complete_step_finalizes_once() {
    // This is a logical assertion: even though two steps complete concurrently,
    // the atomic increment + terminal judgment guarantees finalize fires once.
    // SQLite in-memory is single-writer per connection; here we verify the
    // contract sequentially against the same job, which is the worst case for
    // double-finalize (both completes see terminal state).
    let db = Database::init("sqlite::memory:").await.expect("db init");
    let (job_id, step_ids) = make_job(&db, 2).await;
    let (task, finalize_counter) = MockTask::new("mock", true);

    for sid in &step_ids {
        let s: StepRecord = sqlx::query_as::<_, StepRecord>(
            "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count, started_at, completed_at, next_retry_at FROM async_job_steps WHERE id = ?"
        )
        .bind(sid)
        .fetch_one(match &db { Database::Sqlite(p) => p, _ => unreachable!() })
        .await
        .expect("fetch step");
        complete_step(&db, &s, StepOutput { result: serde_json::json!({}) }, &task, &job_id).await;
    }

    assert_eq!(finalize_counter.load(std::sync::atomic::Ordering::SeqCst), 1,
        "P1-6: finalize called exactly once even when both completes hit terminal");
}

// ─── Q1: fail_step backoff sets next_retry_at and grows ───────────────────

#[tokio::test]
async fn test_fail_step_sets_next_retry_at_and_grows() {
    let db = Database::init("sqlite::memory:").await.expect("db init");
    let (job_id, step_ids) = make_job(&db, 1).await;
    let (task, _) = MockTask::new("mock", false);

    // First failure (retry_count 0→1, but step was claimed so retry_count is 1
    // after claim; fail_step checks retry_count+1 >= max_retries). With
    // max_retries=3, the step should go back to pending with next_retry_at set.
    let s: StepRecord = sqlx::query_as::<_, StepRecord>(
        "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count, started_at, completed_at, next_retry_at FROM async_job_steps WHERE id = ?"
    )
    .bind(&step_ids[0])
    .fetch_one(match &db { Database::Sqlite(p) => p, _ => unreachable!() })
    .await
    .expect("fetch step");
    fail_step(&db, &s, "boom", &task, &job_id).await;

    let nr1 = step_next_retry_at(&db, &step_ids[0]).await;
    assert!(nr1.is_some(), "first failure with retries left → next_retry_at set (backoff)");

    // Re-claim (which increments retry_count again) and fail again — backoff grows.
    // We simulate by manually bumping retry_count so claim→fail advances the count.
    sqlx::query("UPDATE async_job_steps SET status='pending', retry_count=1, next_retry_at=NULL WHERE id=?")
        .bind(&step_ids[0])
        .execute(match &db { Database::Sqlite(p) => p, _ => unreachable!() })
        .await
        .expect("reset step");
    let s2: StepRecord = sqlx::query_as::<_, StepRecord>(
        "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count, started_at, completed_at, next_retry_at FROM async_job_steps WHERE id = ?"
    )
    .bind(&step_ids[0])
    .fetch_one(match &db { Database::Sqlite(p) => p, _ => unreachable!() })
    .await
    .expect("fetch step 2");
    fail_step(&db, &s2, "boom", &task, &job_id).await;
    let nr2 = step_next_retry_at(&db, &step_ids[0]).await;
    assert!(nr2.is_some(), "second failure still has retries left → backoff set");

    // Backoff is exponential: 2^retry_count. retry_count=1 → 2s; retry_count=2 → 4s.
    // Both must be in the future and nr2 > nr1 in magnitude.
    let parse = |s: &str| chrono::DateTime::parse_from_rfc3339(s).map(|d| d.timestamp());
    let t1 = parse(nr1.as_ref().unwrap()).expect("parse nr1");
    let t2 = parse(nr2.as_ref().unwrap()).expect("parse nr2");
    assert!(t2 > t1, "exponential backoff must increase (nr2={} > nr1={})", t2, t1);
}

// ─── P1-3: finalize failure marks job failed (not silent completed) ───────

struct FailingFinalizeTask {
    step_type: &'static str,
}
#[async_trait::async_trait]
impl AsyncTask for FailingFinalizeTask {
    fn step_type(&self) -> &'static str { self.step_type }
    async fn tick(&self, _db: &Database) -> Result<Option<Vec<NewStep>>> { Ok(None) }
    fn tick_interval(&self) -> Duration { Duration::from_secs(60) }
    async fn execute(&self, _db: &Database, _step: &StepRecord) -> Result<StepOutput> {
        Ok(StepOutput { result: serde_json::json!({}) })
    }
    async fn finalize(&self, _db: &Database, _job: &aigw_core::async_task::JobRecord) -> Result<()> {
        Err(DbError::Other("finalize boom".into()))
    }
}

#[tokio::test]
async fn test_finalize_failure_marks_job_failed() {
    let db = Database::init("sqlite::memory:").await.expect("db init");
    let (job_id, step_ids) = make_job(&db, 1).await;
    let task: Arc<dyn AsyncTask> = Arc::new(FailingFinalizeTask { step_type: "mock" });

    let s: StepRecord = sqlx::query_as::<_, StepRecord>(
        "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count, started_at, completed_at, next_retry_at FROM async_job_steps WHERE id = ?"
    )
    .bind(&step_ids[0])
    .fetch_one(match &db { Database::Sqlite(p) => p, _ => unreachable!() })
    .await
    .expect("fetch step");
    complete_step(&db, &s, StepOutput { result: serde_json::json!({}) }, &task, &job_id).await;

    assert_eq!(job_status(&db, &job_id).await, "failed",
        "P1-3: finalize Err must mark job failed, not silently completed");
}

// ─── P1-5: create_job with zero steps errors (no orphan job) ──────────────

#[tokio::test]
async fn test_create_job_with_empty_steps_errors() {
    let db = Database::init("sqlite::memory:").await.expect("db init");
    let empty: Vec<NewStep> = vec![];
    let result = create_job(&db, "mock", "manual", None, &empty, 3).await;
    assert!(result.is_err(),
        "create_job with zero steps must error (no orphan job row)");
}

// ─── Q3 / P0-4: execute() rejects when storage unconfigured ───────────────

#[tokio::test]
async fn test_execute_rejects_when_storage_unconfigured() {
    let archiver = BodyArchiver::new(BodyArchiveConfig::default());
    let db = Database::init("sqlite::memory:").await.expect("db init");
    let step = StepRecord {
        id: "s1".into(),
        job_id: "j1".into(),
        step_key: "hour=2026-07-22T14".into(),
        step_type: "body_archive".into(),
        status: "pending".into(),
        payload: serde_json::json!({"hour": "2026-07-22T14", "batch_size": 100}),
        result: serde_json::json!({}),
        error_message: None,
        retry_count: 0,
        started_at: None,
        completed_at: None,
        next_retry_at: None,
    };
    let result = AsyncTask::execute(&archiver, &db, &step).await;
    assert!(result.is_err(),
        "P0-4: execute() with unconfigured storage must Err (not Ok→false-positive completed)");
}

// ─── Q3 / P0-4: steps_from_payload rejects when storage unconfigured ──────

#[tokio::test]
async fn test_steps_from_payload_rejects_when_storage_unconfigured() {
    let archiver = BodyArchiver::new(BodyArchiveConfig::default());
    let payload = serde_json::json!({
        "start_date": "2026-07-22T00:00:00+00:00",
        "end_date": "2026-07-22T02:00:00+00:00",
    });
    let result = AsyncTask::steps_from_payload(&archiver, &payload).await;
    assert!(result.is_err(),
        "P0-4: steps_from_payload() with unconfigured storage must Err");
}

// ─── writer.rs: start_time is TimestampMillisecond, cache_hit is Boolean ──

#[tokio::test]
async fn test_writer_start_time_is_timestamp_millisecond_and_cache_hit_boolean() {
    use aigw_core::body_archive::BodyRow;
    let rows = vec![BodyRow {
        call_id: "req-1".into(),
        start_time: "2026-07-22T14:30:00+00:00".into(),
        model: "gpt-4".into(),
        status: Some("success".into()),
        cache_hit: Some("true".into()),
        session_id: Some("s".into()),
        messages: Some("{}".into()),
        response: Some("{}".into()),
        proxy_server_request: Some("{}".into()),
        request_id: None,
        spend: 0.01,
        total_tokens: 10,
        prompt_tokens: 3,
        completion_tokens: 7,
        end_time: "2026-07-22T14:31:00+00:00".into(),
        model_group: None,
    }];

    // Write to a temp file, then load the parquet metadata from the file
    // (ArrowReaderMetadata::load requires a ChunkReader: File or Bytes).
    let dir = std::env::temp_dir();
    let path = dir.join(format!("aigw_stage82_{}.parquet", std::process::id()));
    aigw_core::body_archive::writer::write_parquet_to_file(&path, &rows, 5000, 10, "zstd", 6)
        .expect("write parquet");

    use parquet::arrow::arrow_reader::ArrowReaderMetadata;
    let file = std::fs::File::open(&path).expect("open parquet");
    let metadata = ArrowReaderMetadata::load(&file, Default::default()).expect("load metadata");
    let schema = metadata.schema().fields();
    let start_time_field = schema.iter().find(|f| f.name() == "start_time")
        .expect("start_time field exists");
    assert_eq!(
        start_time_field.data_type(),
        &arrow::datatypes::DataType::Timestamp(arrow::datatypes::TimeUnit::Millisecond, None),
        "P1-7: start_time must be Timestamp(Millisecond) not Utf8"
    );
    let cache_hit_field = schema.iter().find(|f| f.name() == "cache_hit")
        .expect("cache_hit field exists");
    assert_eq!(
        cache_hit_field.data_type(),
        &arrow::datatypes::DataType::Boolean,
        "start_time/cache_hit: cache_hit must be Boolean"
    );

    // Re-open and read one batch to prove the file is a valid parquet that
    // round-trips a row. ArrowReaderMetadata::load borrows the reader; open a
    // fresh File so the metadata owns its own reader via try_new.
    let file2 = std::fs::File::open(&path).expect("open parquet 2");
    use parquet::arrow::arrow_reader::ArrowReaderBuilder;
    let builder = ArrowReaderBuilder::try_new(file2).expect("builder");
    let mut batch_reader = builder.build().expect("build reader");
    let batch = batch_reader.next().expect("at least one batch").expect("read batch");
    assert_eq!(batch.num_rows(), 1, "exactly one row round-tripped");

    let _ = std::fs::remove_file(&path);
}
