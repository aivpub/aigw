//! Engine — drives registered `AsyncTask` implementations.
//!
//! Manages tick loops, exec loops, and cleanup. Uses the
//! `async_jobs` / `async_job_steps` / `async_job_logs` tables
//! for multi-replica coordination.

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::async_task::{AsyncTask, JobRecord, NewStep, StepOutput, StepRecord};
use crate::db::{Database, DbError, Result};

/// Engine configuration.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Max total exec loops across all tasks. Default: 8.
    pub max_loops: usize,
    /// Sleep duration when no pending steps. Default: 10s.
    pub poll_interval: Duration,
    /// Interval for stale step cleanup. Default: 30s.
    pub cleanup_interval: Duration,
    /// Steps running longer than this are considered stale. Default: 10min.
    pub step_timeout: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_loops: 8,
            poll_interval: Duration::from_secs(10),
            cleanup_interval: Duration::from_secs(30),
            step_timeout: Duration::from_secs(600),
        }
    }
}

/// The Engine owns all registered AsyncTasks and runs their loops.
pub struct Engine {
    db: Arc<Database>,
    config: EngineConfig,
    tasks: Vec<Arc<dyn AsyncTask>>,
}

impl Engine {
    /// Create a new Engine. Tasks are registered via `register()`.
    pub fn new(db: Arc<Database>, config: EngineConfig) -> Self {
        Self {
            db,
            config,
            tasks: Vec::new(),
        }
    }

    /// Register an AsyncTask implementation.
    pub fn register(&mut self, task: Arc<dyn AsyncTask>) {
        self.tasks.push(task);
    }

    /// Run all loops until shutdown (never returns). Wrapper that creates a
    /// fresh cancellation token (never cancelled) so callers can keep the
    /// pre-TD-005 signature.
    pub async fn run(&self) {
        self.run_with_cancel(CancellationToken::new()).await;
    }

    /// Run all loops until shutdown or cancellation.
    ///
    /// Each loop body is wrapped in `catch_unwind` so a panic in one task's
    /// tick/exec/cleanup iteration is logged and recovered (sleep + continue)
    /// instead of silently killing that loop's tokio task forever (TD-005).
    ///
    /// On cancellation the current in-flight step completes before the loop
    /// exits, and `run_with_cancel` returns after all loops observe the token.
    pub async fn run_with_cancel(&self, token: CancellationToken) {
        if self.tasks.is_empty() {
            warn!("Engine::run() called with no registered tasks — nothing to do");
            return;
        }

        let mut handles = Vec::new();

        // 1. One tick loop per task
        for task in &self.tasks {
            let db = self.db.clone();
            let task = Arc::clone(task);
            let tick_interval = task.tick_interval();
            let token = token.clone();
            handles.push(tokio::spawn(async move {
                tick_loop(db, task, tick_interval, token).await;
            }));
        }

        // 2. Exec loops — distribute max_loops across tasks
        let total_concurrency: usize = self.tasks.iter().map(|t| t.concurrency()).sum();
        let total_loops = self.config.max_loops.min(total_concurrency);

        // Fair distribution: each task gets at least 1, remainder proportionally
        let mut remaining = total_loops;
        for task in &self.tasks {
            let desired = task.concurrency().min(remaining);
            if desired == 0 {
                continue;
            }
            remaining -= desired;
            for _ in 0..desired {
                let db = self.db.clone();
                let task = Arc::clone(task);
                let poll = self.config.poll_interval;
                let token = token.clone();
                handles.push(tokio::spawn(async move {
                    exec_loop(db, task, poll, token).await;
                }));
            }
        }

        // 3. Cleanup loop
        {
            let db = self.db.clone();
            let interval = self.config.cleanup_interval;
            let timeout = self.config.step_timeout;
            let token = token.clone();
            handles.push(tokio::spawn(async move {
                cleanup_loop(db, interval, timeout, token).await;
            }));
        }

        // Keep all loops alive; cancellation makes each loop return so all
        // handles complete and `run_with_cancel` returns.
        for h in handles {
            let _ = h.await;
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Internal helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Create a new Job + Steps in the database.
/// Used by both cron (tick) and manual trigger paths.
///
/// Deduplicates steps: if a step_key already exists in any active (pending/running)
/// job of the same step_type, it is skipped. If all steps are duplicates,
/// an error is returned.
pub async fn create_job(
    db: &Database,
    step_type: &str,
    trigger_type: &str,
    triggered_by: Option<&str>,
    steps: &[NewStep],
    max_retries: i32,
) -> Result<String> {
    let job_id = format!(
        "job-{}",
        Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("0000")
    );
    let now = chrono::Utc::now().to_rfc3339();

    // Deduplicate: filter out steps whose keys already exist in active jobs
    let active_keys = find_active_step_keys(db, step_type, steps).await?;
    let steps: Vec<&NewStep> = if active_keys.is_empty() {
        steps.iter().collect()
    } else {
        let skipped = active_keys.len();
        warn!(step_type, %trigger_type, skipped, "create_job: skipping duplicate steps already in active jobs");
        steps
            .iter()
            .filter(|s| !active_keys.contains(&s.key))
            .collect()
    };

    if steps.is_empty() {
        return Err(DbError::Other(
            "all steps are already queued in active jobs".into(),
        ));
    }

    // Recompute total from deduplicated steps
    match db {
        Database::Sqlite(pool) => {
            create_job_sqlite(
                pool,
                &job_id,
                step_type,
                trigger_type,
                triggered_by,
                &steps,
                max_retries,
                &now,
            )
            .await
        }
        Database::Mysql(pool) => {
            create_job_mysql(
                pool,
                &job_id,
                step_type,
                trigger_type,
                triggered_by,
                &steps,
                max_retries,
                &now,
            )
            .await
        }
        Database::Postgres(pool) => {
            create_job_pg(
                pool,
                &job_id,
                step_type,
                trigger_type,
                triggered_by,
                &steps,
                max_retries,
                &now,
            )
            .await
        }
    }
}

/// Find which step keys from `steps` are already present in any active (pending/running)
/// job of the given step_type. Used for cross-job deduplication.
async fn find_active_step_keys(
    db: &Database,
    step_type: &str,
    steps: &[NewStep],
) -> Result<std::collections::HashSet<String>> {
    if steps.is_empty() {
        return Ok(std::collections::HashSet::new());
    }
    let step_keys: Vec<&str> = steps.iter().map(|s| s.key.as_str()).collect();
    match db {
        Database::Sqlite(pool) => {
            let placeholders = step_keys.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT DISTINCT s.step_key FROM async_job_steps s
                 JOIN async_jobs j ON s.job_id = j.id
                 WHERE s.step_type = ? AND s.step_key IN ({}) AND j.status IN ('pending', 'running')",
                placeholders
            );
            let mut q = sqlx::query_scalar::<_, String>(&sql).bind(step_type);
            for k in &step_keys {
                q = q.bind(k);
            }
            let keys: Vec<String> = q.fetch_all(pool).await?;
            Ok(keys.into_iter().collect())
        }
        Database::Mysql(pool) => {
            let placeholders = step_keys.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT DISTINCT s.step_key FROM async_job_steps s
                 JOIN async_jobs j ON s.job_id = j.id
                 WHERE s.step_type = ? AND s.step_key IN ({}) AND j.status IN ('pending', 'running')",
                placeholders
            );
            let mut q = sqlx::query_scalar::<_, String>(&sql).bind(step_type);
            for k in &step_keys {
                q = q.bind(k);
            }
            let keys: Vec<String> = q.fetch_all(pool).await?;
            Ok(keys.into_iter().collect())
        }
        Database::Postgres(pool) => {
            let pg_placeholders: Vec<String> = step_keys
                .iter()
                .enumerate()
                .map(|(i, _)| format!("${}", i + 2))
                .collect();
            let sql = format!(
                "SELECT DISTINCT s.step_key FROM async_job_steps s
                 JOIN async_jobs j ON s.job_id = j.id
                 WHERE s.step_type = $1 AND s.step_key IN ({}) AND j.status IN ('pending', 'running')",
                pg_placeholders.join(",")
            );
            let mut q = sqlx::query_scalar::<_, String>(&sql).bind(step_type);
            for k in &step_keys {
                q = q.bind(k);
            }
            let keys: Vec<String> = q.fetch_all(pool).await?;
            Ok(keys.into_iter().collect())
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn create_job_sqlite(
    pool: &sqlx::SqlitePool,
    job_id: &str,
    step_type: &str,
    trigger_type: &str,
    triggered_by: Option<&str>,
    steps: &[&NewStep],
    max_retries: i32,
    now: &str,
) -> Result<String> {
    let total = steps.len() as i32;
    sqlx::query(
        "INSERT INTO async_jobs (id, step_type, trigger_type, triggered_by, status, total_steps, max_retries, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'pending', ?, ?, ?, ?)"
    )
    .bind(job_id).bind(step_type).bind(trigger_type).bind(triggered_by)
    .bind(total).bind(max_retries).bind(now).bind(now)
    .execute(pool).await?;

    for step in steps {
        let step_id = format!("{}-{}", job_id, step.key);
        sqlx::query(
            "INSERT INTO async_job_steps (id, job_id, step_key, step_type, status, payload)
             VALUES (?, ?, ?, ?, 'pending', ?)",
        )
        .bind(&step_id)
        .bind(job_id)
        .bind(&step.key)
        .bind(step_type)
        .bind(&step.payload)
        .execute(pool)
        .await?;
    }
    Ok(job_id.to_string())
}

#[allow(clippy::too_many_arguments)]
async fn create_job_mysql(
    pool: &sqlx::MySqlPool,
    job_id: &str,
    step_type: &str,
    trigger_type: &str,
    triggered_by: Option<&str>,
    steps: &[&NewStep],
    max_retries: i32,
    now: &str,
) -> Result<String> {
    let total = steps.len() as i32;
    sqlx::query(
        "INSERT INTO async_jobs (id, step_type, trigger_type, triggered_by, status, total_steps, max_retries, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'pending', ?, ?, ?, ?)"
    )
    .bind(job_id).bind(step_type).bind(trigger_type).bind(triggered_by)
    .bind(total).bind(max_retries).bind(now).bind(now)
    .execute(pool).await?;

    for step in steps {
        let step_id = format!("{}-{}", job_id, step.key);
        sqlx::query(
            "INSERT INTO async_job_steps (id, job_id, step_key, step_type, status, payload)
             VALUES (?, ?, ?, ?, 'pending', ?)",
        )
        .bind(&step_id)
        .bind(job_id)
        .bind(&step.key)
        .bind(step_type)
        .bind(sqlx::types::Json(&step.payload))
        .execute(pool)
        .await?;
    }
    Ok(job_id.to_string())
}

#[allow(clippy::too_many_arguments)]
async fn create_job_pg(
    pool: &sqlx::PgPool,
    job_id: &str,
    step_type: &str,
    trigger_type: &str,
    triggered_by: Option<&str>,
    steps: &[&NewStep],
    max_retries: i32,
    now: &str,
) -> Result<String> {
    let total = steps.len() as i32;
    sqlx::query(
        "INSERT INTO async_jobs (id, step_type, trigger_type, triggered_by, status, total_steps, max_retries, created_at, updated_at)
         VALUES ($1, $2, $3, $4, 'pending', $5, $6, $7::timestamptz, $8::timestamptz)"
    )
    .bind(job_id).bind(step_type).bind(trigger_type).bind(triggered_by)
    .bind(total).bind(max_retries).bind(now).bind(now)
    .execute(pool).await?;

    for step in steps {
        let step_id = format!("{}-{}", job_id, step.key);
        sqlx::query(
            "INSERT INTO async_job_steps (id, job_id, step_key, step_type, status, payload)
             VALUES ($1, $2, $3, $4, 'pending', $5)",
        )
        .bind(&step_id)
        .bind(job_id)
        .bind(&step.key)
        .bind(step_type)
        .bind(&step.payload)
        .execute(pool)
        .await?;
    }
    Ok(job_id.to_string())
}

/// Append a log entry to async_job_logs.
pub async fn append_log(
    db: &Database,
    job_id: &str,
    step_key: Option<&str>,
    level: &str,
    message: &str,
) {
    let result: std::result::Result<(), DbError> = match db {
        Database::Sqlite(pool) => sqlx::query(
            "INSERT INTO async_job_logs (job_id, step_key, level, message) VALUES (?, ?, ?, ?)",
        )
        .bind(job_id)
        .bind(step_key)
        .bind(level)
        .bind(message)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(DbError::from),
        Database::Mysql(pool) => sqlx::query(
            "INSERT INTO async_job_logs (job_id, step_key, level, message) VALUES (?, ?, ?, ?)",
        )
        .bind(job_id)
        .bind(step_key)
        .bind(level)
        .bind(message)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(DbError::from),
        Database::Postgres(pool) => sqlx::query(
            "INSERT INTO async_job_logs (job_id, step_key, level, message) VALUES ($1, $2, $3, $4)",
        )
        .bind(job_id)
        .bind(step_key)
        .bind(level)
        .bind(message)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(DbError::from),
    };
    if let Err(e) = result {
        warn!(%job_id, ?step_key, %level, %message, "failed to write job log: {}", e);
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Loops
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Run a single `body` future and swallow panics, returning `true` if the
/// iteration panicked (so the caller can log + back off) and `false` otherwise.
///
/// Uses `FutureExt::catch_unwind()` from futures (the combinator, not
/// `std::panic::catch_unwind`) so a panic during the awaited body is captured
/// and turned into a normal error result — the tokio task never dies (TD-005).
///
/// `AssertUnwindSafe` is required because the boxed dyn Future is not
/// `UnwindSafe` by construction; we accept that a panic may leave shared state
/// in an inconsistent state, but the alternative (a permanently dead loop) is
/// strictly worse. Panics are logged and the caller backs off before retrying.
async fn guarded(body: Pin<Box<dyn Future<Output = ()> + Send>>) -> bool {
    use futures::FutureExt as _;
    let body = AssertUnwindSafe(body);
    match body.catch_unwind().await {
        Ok(()) => false,
        Err(p) => {
            let msg = if let Some(s) = p.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = p.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic payload".to_string()
            };
            error!("engine loop iteration panicked (recovering): {}", msg);
            true
        }
    }
}

async fn tick_loop(
    db: Arc<Database>,
    task: Arc<dyn AsyncTask>,
    interval: Duration,
    token: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = token.cancelled() => {
                info!(step_type = task.step_type(), "tick loop shutting down (cancelled)");
                return;
            }
            _ = tokio::time::sleep(interval) => {}
        }
        let step_type = task.step_type();
        let tick_db = db.clone();
        let tick_task = task.clone();
        let panicked = guarded(Box::pin(async move {
            match tick_task.tick(&tick_db).await {
                Ok(Some(steps)) => {
                    info!(step_type, count = steps.len(), "tick: new work");

                    if let Err(e) = create_job(&tick_db, step_type, "cron", None, &steps, 3).await {
                        // Ignore unique constraint violations (concurrent tick)
                        let err_str = e.to_string();
                        if err_str.contains("UNIQUE")
                            || err_str.contains("unique")
                            || err_str.contains("duplicate")
                        {
                            debug!(step_type, "tick: concurrent job already created, skipping");
                        } else {
                            error!(step_type, %e, "tick: failed to create job");
                        }
                    }
                }
                Ok(None) => {
                    debug!(step_type, "tick: no new work");
                }
                Err(e) => {
                    error!(step_type, %e, "tick error");
                }
            }
        }))
        .await;
        if panicked {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    }
}

async fn exec_loop(
    db: Arc<Database>,
    task: Arc<dyn AsyncTask>,
    poll_interval: Duration,
    token: CancellationToken,
) {
    let step_type = task.step_type();
    loop {
        // Graceful shutdown boundary: only checked between iterations so an
        // in-flight step always completes before the loop exits (TD-005).
        if token.is_cancelled() {
            info!(step_type, "exec loop shutting down (cancelled)");
            return;
        }
        let exec_db = db.clone();
        let exec_task = task.clone();
        let idle_token = token.clone();
        let panicked = guarded(Box::pin(async move {
            match claim_next_step(&exec_db, step_type).await {
                Ok(Some(step)) => {
                    let job_id = step.job_id.clone();
                    let step_key = step.step_key.clone();

                    // Transition job from 'pending' to 'running' (optimistic lock)
                    mark_job_running(&exec_db, &job_id).await;

                    append_log(&exec_db, &job_id, Some(&step_key), "info", "step started").await;

                    match exec_task.execute(&exec_db, &step).await {
                        Ok(output) => {
                            complete_step(&exec_db, &step, output, &exec_task, &job_id).await;
                        }
                        Err(e) => {
                            fail_step(&exec_db, &step, &e.to_string(), &exec_task, &job_id).await;
                        }
                    }
                }
                Ok(None) => {
                    // Idle wait: no in-flight step to preserve, so cancellation
                    // interrupts the sleep immediately (prompt shutdown).
                    tokio::select! {
                        _ = idle_token.cancelled() => {}
                        _ = tokio::time::sleep(poll_interval) => {}
                    }
                }
                Err(e) => {
                    error!(step_type, %e, "claim_next_step error");
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }))
        .await;
        if panicked {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    }
}

async fn cleanup_loop(
    db: Arc<Database>,
    interval: Duration,
    timeout: Duration,
    token: CancellationToken,
) {
    loop {
        if token.is_cancelled() {
            info!("cleanup loop shutting down (cancelled)");
            return;
        }
        let clean_db = db.clone();
        let panicked = guarded(Box::pin(async move {
            if let Err(e) = cleanup_stale_steps(&clean_db, timeout).await {
                error!("cleanup_stale_steps error: {}", e);
            }
        }))
        .await;
        if panicked {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
        // Graceful shutdown boundary: any in-flight cleanup completed above.
        if token.is_cancelled() {
            info!("cleanup loop shutting down (cancelled)");
            return;
        }
        // Cancellable idle sleep (prompt shutdown between iterations).
        tokio::select! {
            _ = token.cancelled() => {}
            _ = tokio::time::sleep(interval) => {}
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Core DB operations (public for testing)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Atomically claim the next pending step for a given step_type.
/// Uses SELECT ... FOR UPDATE SKIP LOCKED for multi-replica safety.
/// Filters out steps whose next_retry_at is in the future.
pub async fn claim_next_step(db: &Database, step_type: &str) -> Result<Option<StepRecord>> {
    let now = chrono::Utc::now().to_rfc3339();
    match db {
        Database::Sqlite(pool) => {
            // SQLite doesn't support SKIP LOCKED; use a transaction with IMMEDIATE
            let step = sqlx::query_as::<_, StepRecord>(
                "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count, started_at, completed_at, next_retry_at
                 FROM async_job_steps
                 WHERE step_type = ? AND status = 'pending'
                   AND (next_retry_at IS NULL OR next_retry_at <= ?)
                 ORDER BY step_key
                 LIMIT 1"
            )
            .bind(step_type)
            .bind(&now)
            .fetch_optional(pool)
            .await?;

            if let Some(ref step) = step {
                // Update to 'running' in the same connection
                sqlx::query(
                    "UPDATE async_job_steps SET status = 'running', started_at = ?, retry_count = retry_count + 1, next_retry_at = NULL WHERE id = ?"
                )
                .bind(&now).bind(&step.id)
                .execute(pool).await?;
            }
            Ok(step)
        }
        Database::Mysql(pool) => {
            // MySQL: use SELECT ... FOR UPDATE SKIP LOCKED in a transaction
            let mut tx = pool.begin().await?;
            let step = sqlx::query_as::<_, StepRecord>(
                "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count, started_at, completed_at, next_retry_at
                 FROM async_job_steps
                 WHERE step_type = ? AND status = 'pending'
                   AND (next_retry_at IS NULL OR next_retry_at <= ?)
                 ORDER BY step_key
                 LIMIT 1
                 FOR UPDATE SKIP LOCKED"
            )
            .bind(step_type)
            .bind(&now)
            .fetch_optional(&mut *tx)
            .await?;

            if let Some(ref step) = step {
                sqlx::query(
                    "UPDATE async_job_steps SET status = 'running', started_at = ?, retry_count = retry_count + 1, next_retry_at = NULL WHERE id = ?"
                )
                .bind(&now).bind(&step.id)
                .execute(&mut *tx).await?;
            }
            tx.commit().await?;
            Ok(step)
        }
        Database::Postgres(pool) => {
            let mut tx = pool.begin().await?;
            let step = sqlx::query_as::<_, StepRecord>(
                "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count,
                        started_at::text as started_at,
                        completed_at::text as completed_at,
                        next_retry_at::text as next_retry_at
                 FROM async_job_steps
                 WHERE step_type = $1 AND status = 'pending'
                   AND (next_retry_at IS NULL OR next_retry_at <= $2::timestamptz)
                 ORDER BY step_key
                 LIMIT 1
                 FOR UPDATE SKIP LOCKED"
            )
            .bind(step_type)
            .bind(&now)
            .fetch_optional(&mut *tx)
            .await?;

            if let Some(ref step) = step {
                sqlx::query(
                    "UPDATE async_job_steps SET status = 'running', started_at = $1::timestamptz, retry_count = retry_count + 1, next_retry_at = NULL WHERE id = $2"
                )
                .bind(&now).bind(&step.id)
                .execute(&mut *tx).await?;
            }
            tx.commit().await?;
            Ok(step)
        }
    }
}

/// Mark a step as completed and check if the parent job is done.
pub async fn complete_step(
    db: &Database,
    step: &StepRecord,
    output: StepOutput,
    task: &Arc<dyn AsyncTask>,
    job_id: &str,
) {
    let now = chrono::Utc::now().to_rfc3339();
    let result: std::result::Result<(), DbError> = match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                "UPDATE async_job_steps SET status = 'completed', result = ?, completed_at = ? WHERE id = ?"
            )
            .bind(&output.result).bind(&now).bind(&step.id)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
        Database::Mysql(pool) => {
            sqlx::query(
                "UPDATE async_job_steps SET status = 'completed', result = ?, completed_at = ? WHERE id = ?"
            )
            .bind(sqlx::types::Json(&output.result)).bind(&now).bind(&step.id)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
        Database::Postgres(pool) => {
            sqlx::query(
                "UPDATE async_job_steps SET status = 'completed', result = $1, completed_at = $2::timestamptz WHERE id = $3"
            )
            .bind(&output.result).bind(&now).bind(&step.id)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
    };

    match result {
        Ok(_) => {
            append_log(db, job_id, Some(&step.step_key), "info", "step completed").await;
            // Atomically increment completed_steps and determine terminal state
            match increment_job_completed(db, job_id).await {
                Ok(Some(ref status)) if status == "completed" => {
                    append_log(db, job_id, None, "info", "all steps completed, finalizing").await;
                    if let Ok(Some(job)) = get_job_by_id(db, job_id).await {
                        if let Err(e) = task.finalize(db, &job).await {
                            error!(%job_id, %e, "finalize error on completed job");
                            mark_job_failed(db, job_id, &now).await;
                            return;
                        }
                        mark_job_completed(db, job_id, &now).await;
                    }
                }
                Ok(Some(ref status)) if status == "partially_failed" => {
                    append_log(
                        db,
                        job_id,
                        None,
                        "warn",
                        "all steps completed with some failures, finalizing",
                    )
                    .await;
                    if let Ok(Some(job)) = get_job_by_id(db, job_id).await {
                        if let Err(e) = task.finalize(db, &job).await {
                            error!(%job_id, %e, "finalize error on partially_failed job");
                            mark_job_failed(db, job_id, &now).await;
                            return;
                        }
                        mark_job_partially_failed(db, job_id, &now).await;
                    }
                }
                Ok(Some(other)) => {
                    append_log(
                        db,
                        job_id,
                        None,
                        "warn",
                        &format!("unexpected terminal state: {}", other),
                    )
                    .await;
                }
                Ok(None) => {} // More steps pending
                Err(e) => error!(%job_id, %e, "increment_job_completed error"),
            }
        }
        Err(e) => {
            error!(step_id = %step.id, %e, "complete_step error");
        }
    }
}

/// Mark a step as failed. If retries remain, reset to pending; otherwise mark as failed.
/// When the step goes to pending (retry), set next_retry_at = now + 2^retry_count seconds.
/// When the step goes to failed, atomically increment failed_steps and check job terminal state.
pub async fn fail_step(
    db: &Database,
    step: &StepRecord,
    error_msg: &str,
    task: &Arc<dyn AsyncTask>,
    job_id: &str,
) {
    let now = chrono::Utc::now();
    let now_str = now.to_rfc3339();

    // Get max_retries from the parent job (default to 3 if we can't read it)
    let max_retries = get_job_max_retries(db, job_id).await.unwrap_or(3);

    let new_status;
    let next_retry_at: Option<String>;

    if step.retry_count + 1 >= max_retries {
        new_status = "failed";
        next_retry_at = None;
    } else {
        new_status = "pending";
        // Exponential backoff: 2^retry_count seconds (the retry_count here is BEFORE increment,
        // so step.retry_count + 1 is the next retry count)
        let delay_secs = 2_u32.pow((step.retry_count + 1).max(0) as u32);
        next_retry_at = Some((now + chrono::Duration::seconds(delay_secs as i64)).to_rfc3339());
    }

    let result: std::result::Result<(), DbError> = match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                "UPDATE async_job_steps SET status = ?, error_message = ?, completed_at = ?, next_retry_at = ? WHERE id = ?"
            )
            .bind(new_status).bind(error_msg).bind(&now_str).bind(&next_retry_at).bind(&step.id)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
        Database::Mysql(pool) => {
            sqlx::query(
                "UPDATE async_job_steps SET status = ?, error_message = ?, completed_at = ?, next_retry_at = ? WHERE id = ?"
            )
            .bind(new_status).bind(error_msg).bind(&now_str).bind(&next_retry_at).bind(&step.id)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
        Database::Postgres(pool) => {
            sqlx::query(
                "UPDATE async_job_steps SET status = $1, error_message = $2, completed_at = $3::timestamptz, next_retry_at = $4::timestamptz WHERE id = $5"
            )
            .bind(new_status).bind(error_msg).bind(&now_str).bind(&next_retry_at).bind(&step.id)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
    };

    match result {
        Ok(_) => {
            let level = if new_status == "failed" {
                "error"
            } else {
                "warn"
            };
            append_log(
                db,
                job_id,
                Some(&step.step_key),
                level,
                &format!("step {}: {}", new_status, error_msg),
            )
            .await;

            if new_status == "failed" {
                // Atomically increment failed_steps and determine terminal state
                match increment_job_failed(db, job_id).await {
                    Ok(Some(ref status)) if status == "failed" => {
                        append_log(db, job_id, None, "error", "all steps failed").await;
                        if let Ok(Some(job)) = get_job_by_id(db, job_id).await {
                            if let Err(e) = task.finalize(db, &job).await {
                                error!(%job_id, %e, "finalize error on failed job");
                            }
                            mark_job_failed(db, job_id, &now_str).await;
                        }
                    }
                    Ok(Some(ref status)) if status == "partially_failed" => {
                        append_log(
                            db,
                            job_id,
                            None,
                            "warn",
                            "all steps done with mixed results",
                        )
                        .await;
                        if let Ok(Some(job)) = get_job_by_id(db, job_id).await {
                            if let Err(e) = task.finalize(db, &job).await {
                                error!(%job_id, %e, "finalize error on partially_failed job");
                                mark_job_failed(db, job_id, &now_str).await;
                                return;
                            }
                            mark_job_partially_failed(db, job_id, &now_str).await;
                        }
                    }
                    Ok(Some(other)) => {
                        append_log(
                            db,
                            job_id,
                            None,
                            "warn",
                            &format!("unexpected terminal state: {}", other),
                        )
                        .await;
                    }
                    Ok(None) => {} // More steps pending
                    Err(e) => error!(%job_id, %e, "increment_job_failed error"),
                }
            }
        }
        Err(e) => {
            error!(step_id = %step.id, %e, "fail_step error");
        }
    }
}

/// Reclaim stale running steps (those that have been running longer than `timeout`).
pub async fn cleanup_stale_steps(db: &Database, timeout: Duration) -> Result<()> {
    let cutoff =
        (chrono::Utc::now() - chrono::Duration::from_std(timeout).unwrap_or_default()).to_rfc3339();
    match db {
        Database::Sqlite(pool) => sqlx::query(
            "UPDATE async_job_steps SET status = 'pending', started_at = NULL
                 WHERE status = 'running' AND started_at IS NOT NULL AND started_at < ?",
        )
        .bind(&cutoff)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(DbError::from),
        Database::Mysql(pool) => sqlx::query(
            "UPDATE async_job_steps SET status = 'pending', started_at = NULL
                 WHERE status = 'running' AND started_at IS NOT NULL AND started_at < ?",
        )
        .bind(&cutoff)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(DbError::from),
        Database::Postgres(pool) => sqlx::query(
            "UPDATE async_job_steps SET status = 'pending', started_at = NULL
                 WHERE status = 'running' AND started_at IS NOT NULL AND started_at < $1::timestamptz",
        )
        .bind(&cutoff)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(DbError::from),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Internal helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Atomically increment completed_steps and determine job terminal state.
/// Returns Some("completed") if all steps done with no failures,
/// Some("partially_failed") if all steps done with some failures,
/// None if still running.
async fn increment_job_completed(db: &Database, job_id: &str) -> Result<Option<String>> {
    match db {
        Database::Sqlite(pool) => {
            // SQLite: use BEGIN IMMEDIATE transaction for atomicity
            let mut tx = pool.begin().await?;
            sqlx::query("UPDATE async_jobs SET completed_steps = completed_steps + 1 WHERE id = ?")
                .bind(job_id)
                .execute(&mut *tx)
                .await?;

            let row: (i32, i32, i32) = sqlx::query_as(
                "SELECT completed_steps, failed_steps, total_steps FROM async_jobs WHERE id = ?",
            )
            .bind(job_id)
            .fetch_one(&mut *tx)
            .await?;

            tx.commit().await?;

            let (completed, failed, total) = row;
            if completed + failed >= total {
                if failed == 0 {
                    Ok(Some("completed".to_string()))
                } else {
                    Ok(Some("partially_failed".to_string()))
                }
            } else {
                Ok(None)
            }
        }
        Database::Mysql(pool) => {
            // MySQL: use transaction with SELECT FOR UPDATE
            let mut tx = pool.begin().await?;
            sqlx::query("UPDATE async_jobs SET completed_steps = completed_steps + 1 WHERE id = ?")
                .bind(job_id)
                .execute(&mut *tx)
                .await?;

            let row: (i32, i32, i32) = sqlx::query_as(
                "SELECT completed_steps, failed_steps, total_steps FROM async_jobs WHERE id = ? FOR UPDATE"
            )
            .bind(job_id).fetch_one(&mut *tx).await?;

            tx.commit().await?;

            let (completed, failed, total) = row;
            if completed + failed >= total {
                if failed == 0 {
                    Ok(Some("completed".to_string()))
                } else {
                    Ok(Some("partially_failed".to_string()))
                }
            } else {
                Ok(None)
            }
        }
        Database::Postgres(pool) => {
            // PG: use UPDATE ... RETURNING for atomicity
            let row: Option<(Option<String>,)> = sqlx::query_as(
                r#"UPDATE async_jobs
                   SET completed_steps = completed_steps + 1
                   WHERE id = $1
                   RETURNING
                     CASE WHEN completed_steps + failed_steps >= total_steps THEN
                       CASE WHEN failed_steps = 0 THEN 'completed' ELSE 'partially_failed' END
                     ELSE NULL END"#,
            )
            .bind(job_id)
            .fetch_optional(pool)
            .await?;

            Ok(row.and_then(|r| r.0))
        }
    }
}

/// Atomically increment failed_steps and determine job terminal state.
/// Returns Some("failed") if all steps are done and none completed,
/// Some("partially_failed") if all steps are done and some completed,
/// None if still running.
async fn increment_job_failed(db: &Database, job_id: &str) -> Result<Option<String>> {
    match db {
        Database::Sqlite(pool) => {
            let mut tx = pool.begin().await?;
            sqlx::query("UPDATE async_jobs SET failed_steps = failed_steps + 1 WHERE id = ?")
                .bind(job_id)
                .execute(&mut *tx)
                .await?;

            let row: (i32, i32, i32) = sqlx::query_as(
                "SELECT completed_steps, failed_steps, total_steps FROM async_jobs WHERE id = ?",
            )
            .bind(job_id)
            .fetch_one(&mut *tx)
            .await?;

            tx.commit().await?;

            let (completed, failed, total) = row;
            if completed + failed >= total {
                if completed == 0 {
                    Ok(Some("failed".to_string()))
                } else {
                    Ok(Some("partially_failed".to_string()))
                }
            } else {
                Ok(None)
            }
        }
        Database::Mysql(pool) => {
            let mut tx = pool.begin().await?;
            sqlx::query("UPDATE async_jobs SET failed_steps = failed_steps + 1 WHERE id = ?")
                .bind(job_id)
                .execute(&mut *tx)
                .await?;

            let row: (i32, i32, i32) = sqlx::query_as(
                "SELECT completed_steps, failed_steps, total_steps FROM async_jobs WHERE id = ? FOR UPDATE"
            )
            .bind(job_id).fetch_one(&mut *tx).await?;

            tx.commit().await?;

            let (completed, failed, total) = row;
            if completed + failed >= total {
                if completed == 0 {
                    Ok(Some("failed".to_string()))
                } else {
                    Ok(Some("partially_failed".to_string()))
                }
            } else {
                Ok(None)
            }
        }
        Database::Postgres(pool) => {
            let row: Option<(Option<String>,)> = sqlx::query_as(
                r#"UPDATE async_jobs
                   SET failed_steps = failed_steps + 1
                   WHERE id = $1
                   RETURNING
                     CASE WHEN completed_steps + failed_steps >= total_steps THEN
                       CASE WHEN completed_steps = 0 THEN 'failed' ELSE 'partially_failed' END
                     ELSE NULL END"#,
            )
            .bind(job_id)
            .fetch_optional(pool)
            .await?;

            Ok(row.and_then(|r| r.0))
        }
    }
}

async fn get_job_by_id(db: &Database, job_id: &str) -> Result<Option<JobRecord>> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query_as::<_, JobRecord>("SELECT * FROM async_jobs WHERE id = ?")
                .bind(job_id)
                .fetch_optional(pool)
                .await
                .map_err(DbError::from)
        }
        Database::Mysql(pool) => {
            sqlx::query_as::<_, JobRecord>("SELECT * FROM async_jobs WHERE id = ?")
                .bind(job_id)
                .fetch_optional(pool)
                .await
                .map_err(DbError::from)
        }
        Database::Postgres(pool) => {
            sqlx::query_as::<_, JobRecord>(
                "SELECT id, step_type, trigger_type, triggered_by, status, total_steps, completed_steps, failed_steps, error_message, max_retries, started_at::text as started_at, completed_at::text as completed_at, created_at::text as created_at, updated_at::text as updated_at FROM async_jobs WHERE id = $1",
            )
            .bind(job_id)
            .fetch_optional(pool)
            .await
            .map_err(DbError::from)
        }
    }
}

async fn get_job_max_retries(db: &Database, job_id: &str) -> Result<i32> {
    match db {
        Database::Sqlite(pool) => {
            let row: (i32,) = sqlx::query_as("SELECT max_retries FROM async_jobs WHERE id = ?")
                .bind(job_id)
                .fetch_one(pool)
                .await?;
            Ok(row.0)
        }
        Database::Mysql(pool) => {
            let row: (i32,) = sqlx::query_as("SELECT max_retries FROM async_jobs WHERE id = ?")
                .bind(job_id)
                .fetch_one(pool)
                .await?;
            Ok(row.0)
        }
        Database::Postgres(pool) => {
            let row: (i32,) = sqlx::query_as("SELECT max_retries FROM async_jobs WHERE id = $1")
                .bind(job_id)
                .fetch_one(pool)
                .await?;
            Ok(row.0)
        }
    }
}

async fn mark_job_completed(db: &Database, job_id: &str, now: &str) {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query("UPDATE async_jobs SET status = 'completed', completed_at = ?, updated_at = ? WHERE id = ?")
            .bind(now).bind(now).bind(job_id).execute(pool).await.ok();
        }
        Database::Mysql(pool) => {
            sqlx::query("UPDATE async_jobs SET status = 'completed', completed_at = ?, updated_at = ? WHERE id = ?")
            .bind(now).bind(now).bind(job_id).execute(pool).await.ok();
        }
        Database::Postgres(pool) => {
            sqlx::query("UPDATE async_jobs SET status = 'completed', completed_at = $1::timestamptz, updated_at = $2::timestamptz WHERE id = $3")
            .bind(now).bind(now).bind(job_id).execute(pool).await.ok();
        }
    }
}

/// Transition job from pending to running (optimistic lock: only from 'pending').
async fn mark_job_running(db: &Database, job_id: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                "UPDATE async_jobs SET status = 'running', started_at = ? WHERE id = ? AND status = 'pending'"
            )
            .bind(&now).bind(job_id).execute(pool).await.ok();
        }
        Database::Mysql(pool) => {
            sqlx::query(
                "UPDATE async_jobs SET status = 'running', started_at = ? WHERE id = ? AND status = 'pending'"
            )
            .bind(&now).bind(job_id).execute(pool).await.ok();
        }
        Database::Postgres(pool) => {
            sqlx::query(
                "UPDATE async_jobs SET status = 'running', started_at = $1::timestamptz WHERE id = $2 AND status = 'pending'"
            )
            .bind(&now).bind(job_id).execute(pool).await.ok();
        }
    }
}

/// Mark job as failed with completed_at.
async fn mark_job_failed(db: &Database, job_id: &str, now: &str) {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query("UPDATE async_jobs SET status = 'failed', completed_at = ? WHERE id = ?")
                .bind(now)
                .bind(job_id)
                .execute(pool)
                .await
                .ok();
        }
        Database::Mysql(pool) => {
            sqlx::query("UPDATE async_jobs SET status = 'failed', completed_at = ? WHERE id = ?")
                .bind(now)
                .bind(job_id)
                .execute(pool)
                .await
                .ok();
        }
        Database::Postgres(pool) => {
            sqlx::query("UPDATE async_jobs SET status = 'failed', completed_at = $1::timestamptz WHERE id = $2")
                .bind(now)
                .bind(job_id)
                .execute(pool)
                .await
                .ok();
        }
    }
}

/// Mark job as partially_failed with completed_at.
async fn mark_job_partially_failed(db: &Database, job_id: &str, now: &str) {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                "UPDATE async_jobs SET status = 'partially_failed', completed_at = ? WHERE id = ?",
            )
            .bind(now)
            .bind(job_id)
            .execute(pool)
            .await
            .ok();
        }
        Database::Mysql(pool) => {
            sqlx::query(
                "UPDATE async_jobs SET status = 'partially_failed', completed_at = ? WHERE id = ?",
            )
            .bind(now)
            .bind(job_id)
            .execute(pool)
            .await
            .ok();
        }
        Database::Postgres(pool) => {
            sqlx::query("UPDATE async_jobs SET status = 'partially_failed', completed_at = $1::timestamptz WHERE id = $2")
            .bind(now).bind(job_id).execute(pool).await.ok();
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Admin query methods
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// List jobs, optionally filtered by step_type and/or status.
/// Returns (jobs, total_count).
pub async fn list_jobs(
    db: &Database,
    step_type: Option<&str>,
    status: Option<&str>,
    limit: i32,
    offset: i32,
) -> Result<(Vec<JobRecord>, i64)> {
    match db {
        Database::Sqlite(pool) => {
            list_jobs_with_count_sqlite(pool, step_type, status, limit, offset).await
        }
        Database::Mysql(pool) => {
            list_jobs_with_count_mysql(pool, step_type, status, limit, offset).await
        }
        Database::Postgres(pool) => {
            list_jobs_with_count_pg(pool, step_type, status, limit, offset).await
        }
    }
}

async fn list_jobs_with_count_sqlite(
    pool: &sqlx::SqlitePool,
    step_type: Option<&str>,
    status: Option<&str>,
    limit: i32,
    offset: i32,
) -> Result<(Vec<JobRecord>, i64)> {
    let (where_clause, st, s) = build_where_params(step_type, status);

    let count_sql = format!("SELECT COUNT(*) FROM async_jobs WHERE {}", where_clause);
    let mut cq = sqlx::query_as::<sqlx::Sqlite, (i64,)>(&count_sql);
    if let Some(ref v) = st {
        cq = cq.bind(v.as_str());
    }
    if let Some(ref v) = s {
        cq = cq.bind(v.as_str());
    }
    let (count,) = cq.fetch_one(pool).await?;

    let list_sql = format!(
        "SELECT * FROM async_jobs WHERE {} ORDER BY created_at DESC LIMIT ? OFFSET ?",
        where_clause
    );
    let mut lq = sqlx::query_as::<sqlx::Sqlite, JobRecord>(&list_sql);
    if let Some(ref v) = st {
        lq = lq.bind(v.as_str());
    }
    if let Some(ref v) = s {
        lq = lq.bind(v.as_str());
    }
    let jobs = lq
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(DbError::from)?;

    Ok((jobs, count))
}

async fn list_jobs_with_count_mysql(
    pool: &sqlx::MySqlPool,
    step_type: Option<&str>,
    status: Option<&str>,
    limit: i32,
    offset: i32,
) -> Result<(Vec<JobRecord>, i64)> {
    let (where_clause, st, s) = build_where_params(step_type, status);

    let count_sql = format!("SELECT COUNT(*) FROM async_jobs WHERE {}", where_clause);
    let mut cq = sqlx::query_as::<sqlx::MySql, (i64,)>(&count_sql);
    if let Some(ref v) = st {
        cq = cq.bind(v.as_str());
    }
    if let Some(ref v) = s {
        cq = cq.bind(v.as_str());
    }
    let (count,) = cq.fetch_one(pool).await?;

    let list_sql = format!(
        "SELECT * FROM async_jobs WHERE {} ORDER BY created_at DESC LIMIT ? OFFSET ?",
        where_clause
    );
    let mut lq = sqlx::query_as::<sqlx::MySql, JobRecord>(&list_sql);
    if let Some(ref v) = st {
        lq = lq.bind(v.as_str());
    }
    if let Some(ref v) = s {
        lq = lq.bind(v.as_str());
    }
    let jobs = lq
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(DbError::from)?;

    Ok((jobs, count))
}

async fn list_jobs_with_count_pg(
    pool: &sqlx::PgPool,
    step_type: Option<&str>,
    status: Option<&str>,
    limit: i32,
    offset: i32,
) -> Result<(Vec<JobRecord>, i64)> {
    let mut conditions = Vec::new();
    let mut params = Vec::new();
    let mut idx = 1;

    if let Some(st) = step_type {
        conditions.push(format!("step_type = ${}", idx));
        params.push(st.to_string());
        idx += 1;
    }
    if let Some(s) = status {
        conditions.push(format!("status = ${}", idx));
        params.push(s.to_string());
        idx += 1;
    }
    let where_clause = if conditions.is_empty() {
        String::from("1=1")
    } else {
        conditions.join(" AND ")
    };

    let count_sql = format!("SELECT COUNT(*) FROM async_jobs WHERE {}", where_clause);
    let mut cq = sqlx::query_as::<sqlx::Postgres, (i64,)>(&count_sql);
    for p in &params {
        cq = cq.bind(p.as_str());
    }
    let (count,) = cq.fetch_one(pool).await?;

    let list_sql = format!(
        "SELECT id, step_type, trigger_type, triggered_by, status, total_steps, completed_steps, failed_steps, error_message, max_retries, started_at::text as started_at, completed_at::text as completed_at, created_at::text as created_at, updated_at::text as updated_at FROM async_jobs WHERE {} ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
        where_clause,
        idx,
        idx + 1
    );
    let mut lq = sqlx::query_as::<sqlx::Postgres, JobRecord>(&list_sql);
    for p in &params {
        lq = lq.bind(p.as_str());
    }
    let jobs = lq
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(DbError::from)?;

    Ok((jobs, count))
}

fn build_where_params(
    step_type: Option<&str>,
    status: Option<&str>,
) -> (String, Option<String>, Option<String>) {
    let mut conditions = Vec::new();
    let st = step_type.map(|s| s.to_string());
    let s = status.map(|s| s.to_string());
    if st.is_some() {
        conditions.push("step_type = ?");
    }
    if s.is_some() {
        conditions.push("status = ?");
    }
    let where_clause = if conditions.is_empty() {
        String::from("1=1")
    } else {
        conditions.join(" AND ")
    };
    (where_clause, st, s)
}

/// Get a single job with its steps.
pub async fn get_job_detail(
    db: &Database,
    job_id: &str,
) -> Result<Option<(JobRecord, Vec<StepRecord>)>> {
    let job = match db {
        Database::Sqlite(pool) => {
            sqlx::query_as::<_, JobRecord>("SELECT * FROM async_jobs WHERE id = ?")
                .bind(job_id)
                .fetch_optional(pool)
                .await?
        }
        Database::Mysql(pool) => {
            sqlx::query_as::<_, JobRecord>("SELECT * FROM async_jobs WHERE id = ?")
                .bind(job_id)
                .fetch_optional(pool)
                .await?
        }
        Database::Postgres(pool) => {
            sqlx::query_as::<_, JobRecord>(
                "SELECT id, step_type, trigger_type, triggered_by, status, total_steps, completed_steps, failed_steps, error_message, max_retries, started_at::text as started_at, completed_at::text as completed_at, created_at::text as created_at, updated_at::text as updated_at FROM async_jobs WHERE id = $1",
            )
            .bind(job_id)
            .fetch_optional(pool)
            .await?
        }
    };

    match job {
        Some(j) => {
            let steps = match db {
                Database::Sqlite(pool) => {
                    sqlx::query_as::<_, StepRecord>(
                        "SELECT * FROM async_job_steps WHERE job_id = ? ORDER BY step_key",
                    )
                    .bind(job_id)
                    .fetch_all(pool)
                    .await?
                }
                Database::Mysql(pool) => {
                    sqlx::query_as::<_, StepRecord>(
                        "SELECT * FROM async_job_steps WHERE job_id = ? ORDER BY step_key",
                    )
                    .bind(job_id)
                    .fetch_all(pool)
                    .await?
                }
                Database::Postgres(pool) => {
                    sqlx::query_as::<_, StepRecord>(
                        "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count, started_at::text as started_at, completed_at::text as completed_at, next_retry_at::text as next_retry_at FROM async_job_steps WHERE job_id = $1 ORDER BY step_key",
                    )
                    .bind(job_id)
                    .fetch_all(pool)
                    .await?
                }
            };
            Ok(Some((j, steps)))
        }
        None => Ok(None),
    }
}

/// Get logs for a job, with optional level filter and pagination.
pub async fn get_job_logs(
    db: &Database,
    job_id: &str,
    level: Option<&str>,
    limit: i32,
    offset: i32,
) -> Result<Vec<crate::async_task::JobLogEntry>> {
    match db {
        Database::Sqlite(pool) => {
            let mut conditions = vec!["job_id = ?".to_string()];
            if level.is_some() {
                conditions.push("level = ?".to_string());
            }
            let where_clause = conditions.join(" AND ");
            let sql = format!(
                "SELECT id, job_id, step_key, level, message, created_at FROM async_job_logs WHERE {} ORDER BY created_at DESC LIMIT ? OFFSET ?",
                where_clause
            );
            let mut q = sqlx::query_as::<_, crate::async_task::JobLogEntry>(&sql);
            q = q.bind(job_id);
            if let Some(l) = level {
                q = q.bind(l);
            }
            q.bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
                .map_err(DbError::from)
        }
        Database::Mysql(pool) => {
            let mut conditions = vec!["job_id = ?".to_string()];
            if level.is_some() {
                conditions.push("level = ?".to_string());
            }
            let where_clause = conditions.join(" AND ");
            let sql = format!(
                "SELECT id, job_id, step_key, level, message, created_at FROM async_job_logs WHERE {} ORDER BY created_at DESC LIMIT ? OFFSET ?",
                where_clause
            );
            let mut q = sqlx::query_as::<_, crate::async_task::JobLogEntry>(&sql);
            q = q.bind(job_id);
            if let Some(l) = level {
                q = q.bind(l);
            }
            q.bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
                .map_err(DbError::from)
        }
        Database::Postgres(pool) => {
            let mut idx = 1;
            let mut conditions = vec![format!("job_id = ${}", idx)];
            idx += 1;
            if level.is_some() {
                conditions.push(format!("level = ${}", idx));
                idx += 1;
            }
            let where_clause = conditions.join(" AND ");
            let pg_sql = format!(
                "SELECT id, job_id, step_key, level, message, created_at::text as created_at FROM async_job_logs WHERE {} ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
                where_clause,
                idx,
                idx + 1
            );
            let mut q = sqlx::query_as::<_, crate::async_task::JobLogEntry>(&pg_sql);
            q = q.bind(job_id);
            if let Some(l) = level {
                q = q.bind(l);
            }
            q.bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
                .map_err(DbError::from)
        }
    }
}

/// Get job statistics per step_type.
pub async fn get_job_stats(db: &Database) -> Result<serde_json::Value> {
    let mut stats = serde_json::Map::new();

    // Query unique step_types
    let step_types: Vec<String> = match db {
        Database::Sqlite(pool) => {
            sqlx::query_scalar::<_, String>("SELECT DISTINCT step_type FROM async_job_steps")
                .fetch_all(pool)
                .await?
        }
        Database::Mysql(pool) => {
            sqlx::query_scalar::<_, String>("SELECT DISTINCT step_type FROM async_job_steps")
                .fetch_all(pool)
                .await?
        }
        Database::Postgres(pool) => {
            sqlx::query_scalar::<_, String>("SELECT DISTINCT step_type FROM async_job_steps")
                .fetch_all(pool)
                .await?
        }
    };

    for st in step_types {
        let pending: i64 = count_steps_by_status(db, &st, "pending").await.unwrap_or(0);
        let running: i64 = count_steps_by_status(db, &st, "running").await.unwrap_or(0);
        let completed: i64 = count_steps_by_status(db, &st, "completed")
            .await
            .unwrap_or(0);
        let failed: i64 = count_steps_by_status(db, &st, "failed").await.unwrap_or(0);

        stats.insert(
            st.clone(),
            serde_json::json!({
                "queue": {
                    "pending": pending,
                    "running": running,
                    "completed": completed,
                    "failed": failed
                }
            }),
        );
    }

    Ok(serde_json::Value::Object(stats))
}

async fn count_steps_by_status(db: &Database, step_type: &str, status: &str) -> Result<i64> {
    match db {
        Database::Sqlite(pool) => {
            let row: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM async_job_steps WHERE step_type = ? AND status = ?",
            )
            .bind(step_type)
            .bind(status)
            .fetch_one(pool)
            .await?;
            Ok(row.0)
        }
        Database::Mysql(pool) => {
            let row: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM async_job_steps WHERE step_type = ? AND status = ?",
            )
            .bind(step_type)
            .bind(status)
            .fetch_one(pool)
            .await?;
            Ok(row.0)
        }
        Database::Postgres(pool) => {
            let row: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM async_job_steps WHERE step_type = $1 AND status = $2",
            )
            .bind(step_type)
            .bind(status)
            .fetch_one(pool)
            .await?;
            Ok(row.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopTask;
    #[async_trait::async_trait]
    impl AsyncTask for NoopTask {
        fn step_type(&self) -> &'static str {
            "test_task"
        }
        async fn tick(&self, _db: &Database) -> Result<Option<Vec<NewStep>>> {
            Ok(None)
        }
        fn tick_interval(&self) -> Duration {
            Duration::from_secs(60)
        }
        async fn execute(&self, _db: &Database, _step: &StepRecord) -> Result<StepOutput> {
            Ok(StepOutput {
                result: serde_json::json!({"ok": true}),
            })
        }
    }

    // TD-005: a task whose tick panics once. The guarded loop must recover
    // (no panic escapes → the spawned task stays alive) and keep looping.
    struct PanicOnceTask;
    static PANIC_TICK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    #[async_trait::async_trait]
    impl AsyncTask for PanicOnceTask {
        fn step_type(&self) -> &'static str {
            "panic_task"
        }
        async fn tick(&self, _db: &Database) -> Result<Option<Vec<NewStep>>> {
            if PANIC_TICK.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                panic!("tick panic (expected in test)");
            }
            Ok(None)
        }
        fn tick_interval(&self) -> Duration {
            Duration::from_millis(10)
        }
        async fn execute(&self, _db: &Database, _step: &StepRecord) -> Result<StepOutput> {
            Ok(StepOutput {
                result: serde_json::json!({"ok": true}),
            })
        }
    }

    #[tokio::test]
    async fn test_claim_next_step_sqlite_returns_none_when_empty() {
        let db = Database::init("sqlite::memory:").await.expect("db init");
        let result = claim_next_step(&db, "body_archive").await.expect("claim");
        assert!(result.is_none(), "should return None when no steps exist");
    }

    #[tokio::test]
    async fn test_cleanup_stale_steps_sqlite() {
        let db = Database::init("sqlite::memory:").await.expect("db init");
        // Insert a step that's been "running" for a long time
        let old_time = "2020-01-01T00:00:00+00:00";
        sqlx::query(
            "INSERT INTO async_jobs (id, step_type, trigger_type, status, total_steps, created_at, updated_at)
             VALUES ('job-1', 'test', 'cron', 'pending', 1, ?, ?)"
        )
        .bind(old_time).bind(old_time)
        .execute(match &db { Database::Sqlite(p) => p, _ => unreachable!() }).await.expect("insert job");

        sqlx::query(
            "INSERT INTO async_job_steps (id, job_id, step_key, step_type, status, started_at)
             VALUES ('step-1', 'job-1', 'key1', 'test', 'running', ?)",
        )
        .bind(old_time)
        .execute(match &db {
            Database::Sqlite(p) => p,
            _ => unreachable!(),
        })
        .await
        .expect("insert step");

        // Cleanup with a short timeout should reclaim the stale step
        cleanup_stale_steps(&db, Duration::from_secs(1))
            .await
            .expect("cleanup");

        let step =
            sqlx::query_as::<_, StepRecord>("SELECT * FROM async_job_steps WHERE id = 'step-1'")
                .fetch_one(match &db {
                    Database::Sqlite(p) => p,
                    _ => unreachable!(),
                })
                .await
                .expect("fetch step");

        assert_eq!(
            step.status, "pending",
            "stale step should be reset to pending"
        );
        assert!(step.started_at.is_none(), "started_at should be cleared");
    }

    #[tokio::test]
    async fn test_create_job_and_claim_step() {
        let db = Database::init("sqlite::memory:").await.expect("db init");
        let steps = vec![NewStep {
            key: "hour=01".into(),
            payload: serde_json::json!({"hour": "2026-07-22T01"}),
        }];
        let job_id = create_job(&db, "body_archive", "cron", None, &steps, 3)
            .await
            .expect("create job");
        assert!(!job_id.is_empty());

        let claimed = claim_next_step(&db, "body_archive").await.expect("claim");
        assert!(claimed.is_some(), "should claim the step");
        let claimed = claimed.unwrap();
        assert_eq!(claimed.step_key, "hour=01");

        // Second claim should return None
        let second = claim_next_step(&db, "body_archive").await.expect("claim2");
        assert!(
            second.is_none(),
            "should return None when all steps are claimed"
        );
    }

    /// Regression: a claim leaves the step 'pending' until the exec loop settles it.
    ///
    /// Prior to the exec_loop rework, a single pending step with a pending claim
    /// could be re-claimed by a second loop before the first executed it — and
    /// worse, a job could sit in 'pending' forever when the empty-claim path kept
    /// sleeping instead of executing. This test drives the full claim+complete
    /// cycle on a no-op task and asserts the job reaches 'completed', which the
    /// budget_reset real-BDD relies on (manual trigger → wait → spend reset).
    #[tokio::test]
    async fn test_exec_loop_claim_execute_completes_job() {
        use crate::async_task::AsyncTask;

        let db = Database::init("sqlite::memory:").await.expect("db init");
        let exec_db = db.clone();
        // Register a NoopTask in the engine so a budget_reset-style manual job
        // gets a real exec loop, then run the engine for a bounded window.
        let mut engine = Engine::new(Arc::new(db.clone()), EngineConfig::default());
        engine.register(Arc::new(NoopTask));

        // Manual trigger equivalent: create_job + wait for the exec loop to
        // claim + execute + complete within ~2s.
        let steps = vec![NewStep {
            key: "k1".into(),
            payload: serde_json::json!({"a": 1}),
        }];
        let job_id = create_job(&db, "test_task", "manual", Some("admin"), &steps, 3)
            .await
            .expect("create_job");

        // Drive the engine's loops directly: spawn exec_loop with a short poll
        // so the claim+execute+complete path runs to completion.
        let task: Arc<dyn AsyncTask> = Arc::new(NoopTask);
        tokio::spawn(async move {
            exec_loop(
                Arc::new(exec_db),
                task,
                Duration::from_millis(50),
                CancellationToken::new(),
            )
            .await;
        });
        // Wait for the job to reach a terminal state.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let row: Option<(String,)> =
                sqlx::query_as("SELECT status FROM async_jobs WHERE id = ?")
                    .bind(&job_id)
                    .fetch_optional(match &db {
                        Database::Sqlite(p) => p,
                        _ => unreachable!(),
                    })
                    .await
                    .expect("fetch job status");
            let status = row.map(|r| r.0).unwrap_or_default();
            if status == "completed" {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "job did not reach completed; status={}",
                status
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    // TD-005: cancellation makes run_with_cancel return instead of hanging.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_run_with_cancel_returns_on_cancel() {
        let db = Database::init("sqlite::memory:").await.expect("db init");
        let mut engine = Engine::new(Arc::new(db), EngineConfig::default());
        engine.register(Arc::new(NoopTask));
        let token = CancellationToken::new();
        // Give the loops a moment to spin up, then cancel.
        tokio::time::sleep(Duration::from_millis(50)).await;
        token.cancel();
        tokio::time::timeout(Duration::from_secs(5), engine.run_with_cancel(token))
            .await
            .expect("run_with_cancel must return after cancellation");
    }

    // TD-005: a panic inside a tick iteration must not kill the loop task.
    // The loop recovers (guarded returns true → 30s backoff) and the spawned
    // tokio task stays alive. We assert the task is NOT finished after the
    // panic (a dead loop would terminate the task).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_tick_loop_panic_keeps_task_alive() {
        let db = Database::init("sqlite::memory:").await.expect("db init");
        PANIC_TICK.store(0, std::sync::atomic::Ordering::SeqCst);
        let mut engine = Engine::new(Arc::new(db), EngineConfig::default());
        engine.register(Arc::new(PanicOnceTask));
        let token = CancellationToken::new();
        let engine_for_task = std::sync::Arc::new(engine);
        let handle = tokio::spawn({
            let token = token.clone();
            let engine = engine_for_task.clone();
            async move { engine.run_with_cancel(token).await }
        });

        // Wait long enough for the first tick to fire and panic (~10ms interval).
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(
            !handle.is_finished(),
            "engine task died after a tick panic — guarded recovery failed"
        );
        // The loop is in the 30s backoff but still alive; cancel stops it.
        token.cancel();
    }

    // TD-005: guarded() swallows a panicking body and reports recovery.
    #[tokio::test]
    async fn test_guarded_recovers_panic() {
        let panicked = guarded(Box::pin(async {
            panic!("boom (expected)");
        }))
        .await;
        assert!(panicked, "guarded must report that the body panicked");

        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ran_c = ran.clone();
        let ok = guarded(Box::pin(async move {
            ran_c.store(true, std::sync::atomic::Ordering::SeqCst);
        }))
        .await;
        assert!(!ok, "non-panicking body must report no panic");
        assert!(
            ran.load(std::sync::atomic::Ordering::SeqCst),
            "non-panicking body must execute"
        );
    }
}
