//! Async task framework — trait and types for background job execution.
//!
//! Each task type (body_archive, budget_reset, etc.) implements the
//! `AsyncTask` trait. The `Engine` in `engine.rs` drives all registered
//! tasks via tick/execute/finalize loops.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::db::Database;

/// A new Step to be created via `tick()` or `steps_from_payload()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewStep {
    /// Unique key for this step within a job, e.g. "hour=2026-07-24T14"
    pub key: String,
    /// Arbitrary JSON payload passed to `execute()`.
    pub payload: serde_json::Value,
}

/// Output of executing a Step — stored in the step's result column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepOutput {
    pub result: serde_json::Value,
}

/// A record from the `async_jobs` table.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct JobRecord {
    pub id: String,
    pub step_type: String,
    pub trigger_type: String,
    pub triggered_by: Option<String>,
    pub status: String,
    pub total_steps: i32,
    pub completed_steps: i32,
    pub failed_steps: i32,
    pub error_message: Option<String>,
    pub max_retries: i32,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A log entry from the `async_job_logs` table.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct JobLogEntry {
    /// Auto-increment primary key (INTEGER/BIGSERIAL/BIGINT across backends).
    pub id: i64,
    pub job_id: String,
    pub step_key: Option<String>,
    pub level: String,
    pub message: String,
    pub created_at: String,
}

/// A record from the `async_job_steps` table.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct StepRecord {
    pub id: String,
    pub job_id: String,
    pub step_key: String,
    pub step_type: String,
    pub status: String,
    pub payload: serde_json::Value,
    pub result: serde_json::Value,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub next_retry_at: Option<String>,
}

/// The AsyncTask trait — implement this for each background job type.
#[async_trait]
pub trait AsyncTask: Send + Sync + 'static {
    /// Task type identifier. Corresponds to `async_job_steps.step_type`.
    fn step_type(&self) -> &'static str;

    // ── cron path ──

    /// Periodic check. Returns Some(steps) when new work is found, None otherwise.
    async fn tick(&self, db: &Database) -> crate::db::Result<Option<Vec<NewStep>>>;
    /// How often to call `tick()`.
    fn tick_interval(&self) -> Duration;

    // ── cron + manual shared ──

    /// Execute a single Step.
    async fn execute(&self, db: &Database, step: &StepRecord) -> crate::db::Result<StepOutput>;
    /// Called after all Steps in a Job complete. Default: no-op.
    async fn finalize(&self, _db: &Database, _job: &JobRecord) -> crate::db::Result<()> {
        Ok(())
    }

    // ── concurrency ──

    /// Number of concurrent exec loops for this task. Default: 1.
    fn concurrency(&self) -> usize {
        1
    }

    // ── manual trigger ──

    /// Convert a JSON payload (from POST /admin/jobs/trigger) into Steps.
    /// Default: returns an error (manual trigger not supported).
    async fn steps_from_payload(
        &self,
        _payload: &serde_json::Value,
    ) -> crate::db::Result<Vec<NewStep>> {
        Err(crate::db::DbError::Other(
            "manual trigger not supported for this task".into(),
        ))
    }
}
