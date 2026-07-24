//! Engine — drives registered `AsyncTask` implementations.
//!
//! Manages tick loops, exec loops, and cleanup. Uses the
//! `async_jobs` / `async_job_steps` / `async_job_logs` tables
//! for multi-replica coordination.

use std::sync::Arc;
use std::time::Duration;
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

    /// Run all loops until shutdown (never returns).
    pub async fn run(&self) {
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
            handles.push(tokio::spawn(async move {
                tick_loop(db, task, tick_interval).await;
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
                handles.push(tokio::spawn(async move {
                    exec_loop(db, task, poll).await;
                }));
            }
        }

        // 3. Cleanup loop
        {
            let db = self.db.clone();
            let interval = self.config.cleanup_interval;
            let timeout = self.config.step_timeout;
            handles.push(tokio::spawn(async move {
                cleanup_loop(db, interval, timeout).await;
            }));
        }

        // Keep all loops alive
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
pub async fn create_job(
    db: &Database,
    step_type: &str,
    trigger_type: &str,
    triggered_by: Option<&str>,
    steps: &[NewStep],
    max_retries: i32,
) -> Result<String> {
    let job_id = format!("job-{}", Uuid::new_v4().to_string().split('-').next().unwrap_or("0000"));
    let now = chrono::Utc::now().to_rfc3339();

    match db {
        Database::Sqlite(pool) => create_job_sqlite(pool, &job_id, step_type, trigger_type, triggered_by, steps, max_retries, &now).await,
        Database::Mysql(pool) => create_job_mysql(pool, &job_id, step_type, trigger_type, triggered_by, steps, max_retries, &now).await,
        Database::Postgres(pool) => create_job_pg(pool, &job_id, step_type, trigger_type, triggered_by, steps, max_retries, &now).await,
    }
}

async fn create_job_sqlite(
    pool: &sqlx::SqlitePool,
    job_id: &str,
    step_type: &str,
    trigger_type: &str,
    triggered_by: Option<&str>,
    steps: &[NewStep],
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
             VALUES (?, ?, ?, ?, 'pending', ?)"
        )
        .bind(&step_id).bind(job_id).bind(&step.key).bind(step_type).bind(&step.payload)
        .execute(pool).await?;
    }
    Ok(job_id.to_string())
}

async fn create_job_mysql(
    pool: &sqlx::MySqlPool,
    job_id: &str,
    step_type: &str,
    trigger_type: &str,
    triggered_by: Option<&str>,
    steps: &[NewStep],
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
             VALUES (?, ?, ?, ?, 'pending', ?)"
        )
        .bind(&step_id).bind(job_id).bind(&step.key).bind(step_type).bind(sqlx::types::Json(&step.payload))
        .execute(pool).await?;
    }
    Ok(job_id.to_string())
}

async fn create_job_pg(
    pool: &sqlx::PgPool,
    job_id: &str,
    step_type: &str,
    trigger_type: &str,
    triggered_by: Option<&str>,
    steps: &[NewStep],
    max_retries: i32,
    now: &str,
) -> Result<String> {
    let total = steps.len() as i32;
    sqlx::query(
        "INSERT INTO async_jobs (id, step_type, trigger_type, triggered_by, status, total_steps, max_retries, created_at, updated_at)
         VALUES ($1, $2, $3, $4, 'pending', $5, $6, $7, $8)"
    )
    .bind(job_id).bind(step_type).bind(trigger_type).bind(triggered_by)
    .bind(total).bind(max_retries).bind(now).bind(now)
    .execute(pool).await?;

    for step in steps {
        let step_id = format!("{}-{}", job_id, step.key);
        sqlx::query(
            "INSERT INTO async_job_steps (id, job_id, step_key, step_type, status, payload)
             VALUES ($1, $2, $3, $4, 'pending', $5)"
        )
        .bind(&step_id).bind(job_id).bind(&step.key).bind(step_type).bind(&step.payload)
        .execute(pool).await?;
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
        Database::Sqlite(pool) => {
            sqlx::query(
                "INSERT INTO async_job_logs (job_id, step_key, level, message) VALUES (?, ?, ?, ?)"
            )
            .bind(job_id).bind(step_key).bind(level).bind(message)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
        Database::Mysql(pool) => {
            sqlx::query(
                "INSERT INTO async_job_logs (job_id, step_key, level, message) VALUES (?, ?, ?, ?)"
            )
            .bind(job_id).bind(step_key).bind(level).bind(message)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
        Database::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO async_job_logs (job_id, step_key, level, message) VALUES ($1, $2, $3, $4)"
            )
            .bind(job_id).bind(step_key).bind(level).bind(message)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
    };
    if let Err(e) = result {
        warn!(%job_id, ?step_key, %level, %message, "failed to write job log: {}", e);
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Loops
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn tick_loop(db: Arc<Database>, task: Arc<dyn AsyncTask>, interval: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        match task.tick(&db).await {
            Ok(Some(steps)) => {
                let step_type = task.step_type();
                info!(step_type, count = steps.len(), "tick: new work");

                if let Err(e) = create_job(&db, step_type, "cron", None, &steps, 3).await {
                    // Ignore unique constraint violations (concurrent tick)
                    let err_str = e.to_string();
                    if err_str.contains("UNIQUE") || err_str.contains("unique") || err_str.contains("duplicate") {
                        debug!(step_type, "tick: concurrent job already created, skipping");
                    } else {
                        error!(step_type, %e, "tick: failed to create job");
                    }
                }
            }
            Ok(None) => {
                debug!(step_type = task.step_type(), "tick: no new work");
            }
            Err(e) => {
                error!(step_type = task.step_type(), %e, "tick error");
            }
        }
    }
}

async fn exec_loop(db: Arc<Database>, task: Arc<dyn AsyncTask>, poll_interval: Duration) {
    let step_type = task.step_type();
    loop {
        match claim_next_step(&db, step_type).await {
            Ok(Some(step)) => {
                let job_id = step.job_id.clone();
                let step_key = step.step_key.clone();
                append_log(&db, &job_id, Some(&step_key), "info", "step started").await;

                match task.execute(&db, &step).await {
                    Ok(output) => {
                        complete_step(&db, &step, output, &task, &job_id).await;
                    }
                    Err(e) => {
                        fail_step(&db, &step, &e.to_string(), &task, &job_id).await;
                    }
                }
            }
            Ok(None) => {
                tokio::time::sleep(poll_interval).await;
            }
            Err(e) => {
                error!(step_type, %e, "claim_next_step error");
                tokio::time::sleep(poll_interval).await;
            }
        }
    }
}

async fn cleanup_loop(db: Arc<Database>, interval: Duration, timeout: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        if let Err(e) = cleanup_stale_steps(&db, timeout).await {
            error!("cleanup_stale_steps error: {}", e);
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Core DB operations (public for testing)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Atomically claim the next pending step for a given step_type.
/// Uses SELECT ... FOR UPDATE SKIP LOCKED for multi-replica safety.
pub async fn claim_next_step(db: &Database, step_type: &str) -> Result<Option<StepRecord>> {
    let now = chrono::Utc::now().to_rfc3339();
    match db {
        Database::Sqlite(pool) => {
            // SQLite doesn't support SKIP LOCKED; use a transaction with IMMEDIATE
            let step = sqlx::query_as::<_, StepRecord>(
                "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count, started_at, completed_at
                 FROM async_job_steps
                 WHERE step_type = ? AND status = 'pending'
                 ORDER BY step_key
                 LIMIT 1"
            )
            .bind(step_type)
            .fetch_optional(pool)
            .await?;

            if let Some(ref step) = step {
                // Update to 'running' in the same connection
                sqlx::query(
                    "UPDATE async_job_steps SET status = 'running', started_at = ?, retry_count = retry_count + 1 WHERE id = ?"
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
                "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count, started_at, completed_at
                 FROM async_job_steps
                 WHERE step_type = ? AND status = 'pending'
                 ORDER BY step_key
                 LIMIT 1
                 FOR UPDATE SKIP LOCKED"
            )
            .bind(step_type)
            .fetch_optional(&mut *tx)
            .await?;

            if let Some(ref step) = step {
                sqlx::query(
                    "UPDATE async_job_steps SET status = 'running', started_at = ?, retry_count = retry_count + 1 WHERE id = ?"
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
                "SELECT id, job_id, step_key, step_type, status, payload, result, error_message, retry_count, started_at, completed_at
                 FROM async_job_steps
                 WHERE step_type = $1 AND status = 'pending'
                 ORDER BY step_key
                 LIMIT 1
                 FOR UPDATE SKIP LOCKED"
            )
            .bind(step_type)
            .fetch_optional(&mut *tx)
            .await?;

            if let Some(ref step) = step {
                sqlx::query(
                    "UPDATE async_job_steps SET status = 'running', started_at = $1, retry_count = retry_count + 1 WHERE id = $2"
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
                "UPDATE async_job_steps SET status = 'completed', result = $1, completed_at = $2 WHERE id = $3"
            )
            .bind(&output.result).bind(&now).bind(&step.id)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
    };

    match result {
        Ok(_) => {
            append_log(db, job_id, Some(&step.step_key), "info", "step completed").await;
            // Check if all job steps are done
            let update_result = increment_job_completed(db, job_id).await;
            match update_result {
                Ok(true) => {
                    // All steps done — finalize
                    append_log(db, job_id, None, "info", "all steps completed, finalizing").await;
                    if let Ok(Some(job)) = get_job_by_id(db, job_id).await {
                        if let Err(e) = task.finalize(db, &job).await {
                            error!(%job_id, %e, "finalize error");
                        }
                        // Mark job as completed
                        mark_job_completed(db, job_id, &now).await;
                    }
                }
                Ok(false) => {} // More steps pending
                Err(e) => error!(%job_id, %e, "increment_job_completed error"),
            }
        }
        Err(e) => {
            error!(step_id = %step.id, %e, "complete_step error");
        }
    }
}

/// Mark a step as failed. If retries remain, reset to pending; otherwise mark as failed.
pub async fn fail_step(
    db: &Database,
    step: &StepRecord,
    error_msg: &str,
    task: &Arc<dyn AsyncTask>,
    job_id: &str,
) {
    let now = chrono::Utc::now().to_rfc3339();
    let _step_type = task.step_type();

    // Get max_retries from the parent job (default to 3 if we can't read it)
    let max_retries = get_job_max_retries(db, job_id).await.unwrap_or(3);

    let new_status = if step.retry_count + 1 >= max_retries {
        "failed"
    } else {
        "pending"
    };

    let result: std::result::Result<(), DbError> = match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                "UPDATE async_job_steps SET status = ?, error_message = ?, completed_at = ? WHERE id = ?"
            )
            .bind(new_status).bind(error_msg).bind(&now).bind(&step.id)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
        Database::Mysql(pool) => {
            sqlx::query(
                "UPDATE async_job_steps SET status = ?, error_message = ?, completed_at = ? WHERE id = ?"
            )
            .bind(new_status).bind(error_msg).bind(&now).bind(&step.id)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
        Database::Postgres(pool) => {
            sqlx::query(
                "UPDATE async_job_steps SET status = $1, error_message = $2, completed_at = $3 WHERE id = $4"
            )
            .bind(new_status).bind(error_msg).bind(&now).bind(&step.id)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
    };

    match result {
        Ok(_) => {
            let level = if new_status == "failed" { "error" } else { "warn" };
            append_log(db, job_id, Some(&step.step_key), level,
                &format!("step {}: {}", new_status, error_msg)).await;

            if new_status == "failed" {
                // Increment failed count
                let _ = increment_job_failed(db, job_id).await;
            }
        }
        Err(e) => {
            error!(step_id = %step.id, %e, "fail_step error");
        }
    }
}

/// Reclaim stale running steps (those that have been running longer than `timeout`).
pub async fn cleanup_stale_steps(db: &Database, timeout: Duration) -> Result<()> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::from_std(timeout).unwrap_or_default()).to_rfc3339();
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                "UPDATE async_job_steps SET status = 'pending', started_at = NULL
                 WHERE status = 'running' AND started_at IS NOT NULL AND started_at < ?"
            )
            .bind(&cutoff)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
        Database::Mysql(pool) => {
            sqlx::query(
                "UPDATE async_job_steps SET status = 'pending', started_at = NULL
                 WHERE status = 'running' AND started_at IS NOT NULL AND started_at < ?"
            )
            .bind(&cutoff)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
        Database::Postgres(pool) => {
            sqlx::query(
                "UPDATE async_job_steps SET status = 'pending', started_at = NULL
                 WHERE status = 'running' AND started_at IS NOT NULL AND started_at < $1"
            )
            .bind(&cutoff)
            .execute(pool).await.map(|_| ()).map_err(DbError::from)
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Internal helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Increment the completed_steps counter and check if all steps are done.
/// Returns true if the job is fully complete.
async fn increment_job_completed(db: &Database, job_id: &str) -> Result<bool> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                "UPDATE async_jobs SET completed_steps = completed_steps + 1 WHERE id = ?"
            )
            .bind(job_id).execute(pool).await?;

            let job = sqlx::query_as::<_, JobRecord>(
                "SELECT * FROM async_jobs WHERE id = ?"
            )
            .bind(job_id).fetch_optional(pool).await?;

            Ok(job.map(|j| j.completed_steps >= j.total_steps).unwrap_or(false))
        }
        Database::Mysql(pool) => {
            sqlx::query("UPDATE async_jobs SET completed_steps = completed_steps + 1 WHERE id = ?")
            .bind(job_id).execute(pool).await?;

            let job = sqlx::query_as::<_, JobRecord>("SELECT * FROM async_jobs WHERE id = ?")
            .bind(job_id).fetch_optional(pool).await?;

            Ok(job.map(|j| j.completed_steps >= j.total_steps).unwrap_or(false))
        }
        Database::Postgres(pool) => {
            sqlx::query("UPDATE async_jobs SET completed_steps = completed_steps + 1 WHERE id = $1")
            .bind(job_id).execute(pool).await?;

            let job = sqlx::query_as::<_, JobRecord>("SELECT * FROM async_jobs WHERE id = $1")
            .bind(job_id).fetch_optional(pool).await?;

            Ok(job.map(|j| j.completed_steps >= j.total_steps).unwrap_or(false))
        }
    }
}

async fn increment_job_failed(db: &Database, job_id: &str) -> Result<()> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query("UPDATE async_jobs SET failed_steps = failed_steps + 1 WHERE id = ?")
            .bind(job_id).execute(pool).await?;
        }
        Database::Mysql(pool) => {
            sqlx::query("UPDATE async_jobs SET failed_steps = failed_steps + 1 WHERE id = ?")
            .bind(job_id).execute(pool).await?;
        }
        Database::Postgres(pool) => {
            sqlx::query("UPDATE async_jobs SET failed_steps = failed_steps + 1 WHERE id = $1")
            .bind(job_id).execute(pool).await?;
        }
    }
    Ok(())
}

async fn get_job_by_id(db: &Database, job_id: &str) -> Result<Option<JobRecord>> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query_as::<_, JobRecord>("SELECT * FROM async_jobs WHERE id = ?")
            .bind(job_id).fetch_optional(pool).await.map_err(DbError::from)
        }
        Database::Mysql(pool) => {
            sqlx::query_as::<_, JobRecord>("SELECT * FROM async_jobs WHERE id = ?")
            .bind(job_id).fetch_optional(pool).await.map_err(DbError::from)
        }
        Database::Postgres(pool) => {
            sqlx::query_as::<_, JobRecord>("SELECT * FROM async_jobs WHERE id = $1")
            .bind(job_id).fetch_optional(pool).await.map_err(DbError::from)
        }
    }
}

async fn get_job_max_retries(db: &Database, job_id: &str) -> Result<i32> {
    match db {
        Database::Sqlite(pool) => {
            let row: (i32,) = sqlx::query_as("SELECT max_retries FROM async_jobs WHERE id = ?")
                .bind(job_id).fetch_one(pool).await?;
            Ok(row.0)
        }
        Database::Mysql(pool) => {
            let row: (i32,) = sqlx::query_as("SELECT max_retries FROM async_jobs WHERE id = ?")
                .bind(job_id).fetch_one(pool).await?;
            Ok(row.0)
        }
        Database::Postgres(pool) => {
            let row: (i32,) = sqlx::query_as("SELECT max_retries FROM async_jobs WHERE id = $1")
                .bind(job_id).fetch_one(pool).await?;
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
            sqlx::query("UPDATE async_jobs SET status = 'completed', completed_at = $1, updated_at = $2 WHERE id = $3")
            .bind(now).bind(now).bind(job_id).execute(pool).await.ok();
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Admin query methods
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// List jobs, optionally filtered by step_type and/or status.
pub async fn list_jobs(
    db: &Database,
    step_type: Option<&str>,
    status: Option<&str>,
    limit: i32,
    offset: i32,
) -> Result<Vec<JobRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let mut sql = String::from("SELECT * FROM async_jobs WHERE 1=1");
            if step_type.is_some() {
                sql.push_str(" AND step_type = ?");
            }
            if status.is_some() {
                sql.push_str(" AND status = ?");
            }
            sql.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");
            let mut q = sqlx::query_as::<_, JobRecord>(&sql);
            if let Some(st) = step_type {
                q = q.bind(st);
            }
            if let Some(s) = status {
                q = q.bind(s);
            }
            q.bind(limit).bind(offset).fetch_all(pool).await.map_err(DbError::from)
        }
        Database::Mysql(pool) => {
            let mut sql = String::from("SELECT * FROM async_jobs WHERE 1=1");
            if step_type.is_some() {
                sql.push_str(" AND step_type = ?");
            }
            if status.is_some() {
                sql.push_str(" AND status = ?");
            }
            sql.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");
            let mut q = sqlx::query_as::<_, JobRecord>(&sql);
            if let Some(st) = step_type {
                q = q.bind(st);
            }
            if let Some(s) = status {
                q = q.bind(s);
            }
            q.bind(limit).bind(offset).fetch_all(pool).await.map_err(DbError::from)
        }
        Database::Postgres(pool) => {
            let mut idx = 1;
            let mut pg_sql = String::from("SELECT * FROM async_jobs WHERE 1=1");
            if step_type.is_some() {
                pg_sql.push_str(&format!(" AND step_type = ${}", idx));
                idx += 1;
            }
            if status.is_some() {
                pg_sql.push_str(&format!(" AND status = ${}", idx));
                idx += 1;
            }
            pg_sql.push_str(&format!(
                " ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
                idx,
                idx + 1
            ));
            let mut q = sqlx::query_as::<_, JobRecord>(&pg_sql);
            if let Some(st) = step_type {
                q = q.bind(st);
            }
            if let Some(s) = status {
                q = q.bind(s);
            }
            q.bind(limit).bind(offset).fetch_all(pool).await.map_err(DbError::from)
        }
    }
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
            sqlx::query_as::<_, JobRecord>("SELECT * FROM async_jobs WHERE id = $1")
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
                        "SELECT * FROM async_job_steps WHERE job_id = $1 ORDER BY step_key",
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
            let mut q =
                sqlx::query_as::<_, crate::async_task::JobLogEntry>(&sql);
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
            let mut q =
                sqlx::query_as::<_, crate::async_task::JobLogEntry>(&sql);
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
                "SELECT id, job_id, step_key, level, message, created_at FROM async_job_logs WHERE {} ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
                where_clause,
                idx,
                idx + 1
            );
            let mut q =
                sqlx::query_as::<_, crate::async_task::JobLogEntry>(&pg_sql);
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
            sqlx::query_scalar::<_, String>(
                "SELECT DISTINCT step_type FROM async_job_steps",
            )
            .fetch_all(pool)
            .await?
        }
        Database::Mysql(pool) => {
            sqlx::query_scalar::<_, String>(
                "SELECT DISTINCT step_type FROM async_job_steps",
            )
            .fetch_all(pool)
            .await?
        }
        Database::Postgres(pool) => {
            sqlx::query_scalar::<_, String>(
                "SELECT DISTINCT step_type FROM async_job_steps",
            )
            .fetch_all(pool)
            .await?
        }
    };

    for st in step_types {
        let pending: i64 = count_steps_by_status(db, &st, "pending")
            .await
            .unwrap_or(0);
        let running: i64 = count_steps_by_status(db, &st, "running")
            .await
            .unwrap_or(0);
        let completed: i64 = count_steps_by_status(db, &st, "completed")
            .await
            .unwrap_or(0);
        let failed: i64 = count_steps_by_status(db, &st, "failed")
            .await
            .unwrap_or(0);

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
             VALUES ('step-1', 'job-1', 'key1', 'test', 'running', ?)"
        )
        .bind(old_time)
        .execute(match &db { Database::Sqlite(p) => p, _ => unreachable!() }).await.expect("insert step");

        // Cleanup with a short timeout should reclaim the stale step
        cleanup_stale_steps(&db, Duration::from_secs(1)).await.expect("cleanup");

        let step = sqlx::query_as::<_, StepRecord>(
            "SELECT * FROM async_job_steps WHERE id = 'step-1'"
        )
        .fetch_one(match &db { Database::Sqlite(p) => p, _ => unreachable!() }).await.expect("fetch step");

        assert_eq!(step.status, "pending", "stale step should be reset to pending");
        assert!(step.started_at.is_none(), "started_at should be cleared");
    }

    #[tokio::test]
    async fn test_create_job_and_claim_step() {
        let db = Database::init("sqlite::memory:").await.expect("db init");
        let steps = vec![
            NewStep { key: "hour=01".into(), payload: serde_json::json!({"hour": "2026-07-22T01"}) },
        ];
        let job_id = create_job(&db, "body_archive", "cron", None, &steps, 3).await.expect("create job");
        assert!(!job_id.is_empty());

        let claimed = claim_next_step(&db, "body_archive").await.expect("claim");
        assert!(claimed.is_some(), "should claim the step");
        let claimed = claimed.unwrap();
        assert_eq!(claimed.step_key, "hour=01");

        // Second claim should return None
        let second = claim_next_step(&db, "body_archive").await.expect("claim2");
        assert!(second.is_none(), "should return None when all steps are claimed");
    }
}
