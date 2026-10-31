//! MCP log retention — deletes call-log and rejection rows past their window.
//!
//! Registered on the shared `AsyncTask` engine so it uses the same clock,
//! cancellation and multi-replica coordination as `BodyArchiver` and
//! `BudgetResetter`; a second scheduler would mean two answers to "is it
//! running?".
//!
//! Deletion is irreversible, so the defaults are conservative (90 / 30 days)
//! and a configured `0` means "never delete" rather than "delete everything".

use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration as StdDuration;

use crate::async_task::{AsyncTask, NewStep, StepOutput, StepRecord};
use crate::db::{Database, DbError, Result};

/// `mcp_log_retention` in config.yaml. Every field has a default, so an
/// operator can set only the one they care about.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpLogRetentionConfig {
    /// Days of `mcp_call_log` to keep. `0` disables deleting them.
    #[serde(default = "default_call_log_days")]
    pub call_log_days: u32,
    /// Days of `mcp_auth_rejection` to keep. `0` disables deleting them.
    #[serde(default = "default_rejection_days")]
    pub rejection_days: u32,
    /// Rows removed per `SELECT`/`DELETE` round. Bounds the lock a deletion
    /// holds, not the total per run.
    #[serde(default = "default_batch_size")]
    pub batch_size: i64,
    /// How often `tick()` looks for expired rows.
    #[serde(default = "default_check_interval_secs")]
    pub check_interval_secs: u64,
    /// Store each call's request and response (Stage 152). Off means no body
    /// rows at all and a 404 from the detail endpoint — an operator who turned
    /// it off must not be able to read what was never written.
    #[serde(default = "default_record_bodies")]
    pub record_bodies: bool,
    /// Per-body ceiling in KiB; longer bodies are stored as a prefix with
    /// `truncated` set. KB-sized, unlike a chat message: MCP arguments are tool
    /// inputs, not conversations.
    #[serde(default = "default_max_body_kib")]
    pub max_body_kib: u64,
}

impl Default for McpLogRetentionConfig {
    fn default() -> Self {
        Self {
            call_log_days: default_call_log_days(),
            rejection_days: default_rejection_days(),
            batch_size: default_batch_size(),
            check_interval_secs: default_check_interval_secs(),
            record_bodies: default_record_bodies(),
            max_body_kib: default_max_body_kib(),
        }
    }
}

fn default_call_log_days() -> u32 {
    90
}
fn default_rejection_days() -> u32 {
    30
}
fn default_batch_size() -> i64 {
    5000
}
fn default_check_interval_secs() -> u64 {
    3600
}
fn default_record_bodies() -> bool {
    true
}
fn default_max_body_kib() -> u64 {
    64
}

/// One run's ceiling, as a multiple of `batch_size`. A table that somehow has
/// millions of expired rows is drained over several runs rather than in one
/// unbounded DELETE that would hold a write lock for minutes.
const MAX_BATCHES_PER_RUN: usize = 20;

pub struct McpLogRetention {
    config: McpLogRetentionConfig,
}

impl McpLogRetention {
    pub fn new(config: McpLogRetentionConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &McpLogRetentionConfig {
        &self.config
    }

    /// Delete one table's expired rows in batches. Returns the number deleted.
    async fn drain(&self, db: &Database, table: RetentionTable, cutoff: &str) -> Result<u64> {
        let batch = self.config.batch_size.max(1);
        let mut deleted = 0;
        for _ in 0..MAX_BATCHES_PER_RUN {
            let ids = match table {
                RetentionTable::CallLog => db.stale_mcp_call_ids(cutoff, batch).await?,
                RetentionTable::Rejection => db.stale_mcp_rejection_ids(cutoff, batch).await?,
                RetentionTable::CallBody => db.stale_mcp_call_body_ids(cutoff, batch).await?,
            };
            if ids.is_empty() {
                break;
            }
            let removed = match table {
                RetentionTable::CallLog => db.delete_mcp_call_ids(&ids).await?,
                RetentionTable::Rejection => db.delete_mcp_rejection_ids(&ids).await?,
                RetentionTable::CallBody => db.delete_mcp_call_body_ids(&ids).await?,
            };
            deleted += removed;
            if (ids.len() as i64) < batch {
                break;
            }
        }
        Ok(deleted)
    }
}

#[derive(Debug, Clone, Copy)]
enum RetentionTable {
    CallLog,
    Rejection,
    CallBody,
}

fn cutoff_for(days: u32) -> String {
    (Utc::now() - Duration::days(days as i64)).to_rfc3339()
}

#[async_trait]
impl AsyncTask for McpLogRetention {
    fn step_type(&self) -> &'static str {
        "mcp_log_retention"
    }

    fn tick_interval(&self) -> StdDuration {
        StdDuration::from_secs(self.config.check_interval_secs.max(1))
    }

    /// One step per tick — the work is a single bounded sweep, not a unit per
    /// entity like the budget resetter. The cutoff is captured here rather than
    /// in `execute()` so a retried step deletes exactly what the tick found.
    async fn tick(&self, db: &Database) -> Result<Option<Vec<NewStep>>> {
        if self.config.call_log_days == 0 && self.config.rejection_days == 0 {
            return Ok(None);
        }

        let call_cutoff =
            (self.config.call_log_days > 0).then(|| cutoff_for(self.config.call_log_days));
        let rejection_cutoff =
            (self.config.rejection_days > 0).then(|| cutoff_for(self.config.rejection_days));

        let has_work = match (&call_cutoff, &rejection_cutoff) {
            (Some(cutoff), _) if !db.stale_mcp_call_ids(cutoff, 1).await?.is_empty() => true,
            (Some(cutoff), _) if !db.stale_mcp_call_body_ids(cutoff, 1).await?.is_empty() => true,
            (_, Some(cutoff)) if !db.stale_mcp_rejection_ids(cutoff, 1).await?.is_empty() => true,
            _ => false,
        };
        if !has_work {
            return Ok(None);
        }

        Ok(Some(vec![NewStep {
            key: format!("retention={}", Utc::now().format("%Y%m%dT%H%M%S")),
            payload: serde_json::json!({
                "call_cutoff": call_cutoff,
                "rejection_cutoff": rejection_cutoff,
            }),
        }]))
    }

    async fn execute(&self, db: &Database, step: &StepRecord) -> Result<StepOutput> {
        let call_cutoff = step.payload["call_cutoff"].as_str();
        let rejection_cutoff = step.payload["rejection_cutoff"].as_str();

        let calls_deleted = match call_cutoff {
            Some(cutoff) => self.drain(db, RetentionTable::CallLog, cutoff).await?,
            None => 0,
        };
        let bodies_deleted = match call_cutoff {
            Some(cutoff) => self.drain(db, RetentionTable::CallBody, cutoff).await?,
            None => 0,
        };
        let rejections_deleted = match rejection_cutoff {
            Some(cutoff) => self.drain(db, RetentionTable::Rejection, cutoff).await?,
            None => 0,
        };

        if calls_deleted + bodies_deleted + rejections_deleted == 0 {
            return Err(DbError::Other(
                "retention step found nothing to delete".into(),
            ));
        }

        Ok(StepOutput {
            result: serde_json::json!({
                "calls_deleted": calls_deleted,
                "bodies_deleted": bodies_deleted,
                "rejections_deleted": rejections_deleted,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{McpAuthRejectionRow, McpCallBodyRow, McpCallLogRow};

    fn call_row(id: &str, created_at: &str) -> McpCallLogRow {
        McpCallLogRow {
            call_id: id.to_string(),
            server_alias: "alpha".to_string(),
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
        }
    }

    fn rejection_row(id: &str, created_at: &str) -> McpAuthRejectionRow {
        McpAuthRejectionRow {
            id: id.to_string(),
            created_at: created_at.to_string(),
            path: "/mcp/alpha".to_string(),
            http_method: "POST".to_string(),
            reason: "missing_header".to_string(),
            token_hash: None,
            client_ip: None,
            user_agent: None,
        }
    }

    fn body_row(id: &str, created_at: &str) -> McpCallBodyRow {
        McpCallBodyRow {
            call_id: id.to_string(),
            request_body: Some("{}".to_string()),
            response_body: Some("{}".to_string()),
            truncated: false,
            created_at: created_at.to_string(),
        }
    }

    async fn seed(db: &Database, expired: usize, fresh: usize) {
        let old = "2020-01-01T00:00:00+00:00";
        let now = Utc::now().to_rfc3339();
        for i in 0..expired {
            db.insert_mcp_call_log(&call_row(&format!("old-{i}"), old))
                .await
                .expect("call");
            db.insert_mcp_call_body(&body_row(&format!("old-{i}"), old))
                .await
                .expect("body");
            db.insert_mcp_rejection(&rejection_row(&format!("old-{i}"), old))
                .await
                .expect("rejection");
        }
        for i in 0..fresh {
            db.insert_mcp_call_log(&call_row(&format!("new-{i}"), &now))
                .await
                .expect("call");
            db.insert_mcp_call_body(&body_row(&format!("new-{i}"), &now))
                .await
                .expect("body");
            db.insert_mcp_rejection(&rejection_row(&format!("new-{i}"), &now))
                .await
                .expect("rejection");
        }
    }

    #[tokio::test]
    async fn deletes_only_rows_past_the_window() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        seed(&db, 2, 1).await;
        let worker = McpLogRetention::new(McpLogRetentionConfig::default());

        let cutoff = cutoff_for(90);
        let calls = worker
            .drain(&db, RetentionTable::CallLog, &cutoff)
            .await
            .expect("drain calls");
        let bodies = worker
            .drain(&db, RetentionTable::CallBody, &cutoff)
            .await
            .expect("drain bodies");
        let rejections = worker
            .drain(&db, RetentionTable::Rejection, &cutoff)
            .await
            .expect("drain rejections");

        assert_eq!((calls, bodies, rejections), (2, 2, 2));
        let remaining = db
            .list_mcp_call_log(Some("alpha"), 50, 0)
            .await
            .expect("list");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].call_id, "new-0");
        assert!(
            db.get_mcp_call_body("old-0").await.expect("get").is_none(),
            "an expired body must not outlive its log row"
        );
        assert!(db.get_mcp_call_body("new-0").await.expect("get").is_some());
    }

    /// Same sweep, same cutoff: a body row must never survive the log row it
    /// belongs to, or the detail endpoint answers for a call the list no longer
    /// shows.
    #[tokio::test]
    async fn the_tick_sweeps_bodies_with_the_log() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        seed(&db, 2, 1).await;
        let worker = McpLogRetention::new(McpLogRetentionConfig::default());

        let steps = worker.tick(&db).await.expect("tick").expect("work");
        let step = StepRecord {
            id: "s".to_string(),
            job_id: "j".to_string(),
            step_key: steps[0].key.clone(),
            step_type: "mcp_log_retention".to_string(),
            status: "pending".to_string(),
            payload: steps[0].payload.clone(),
            result: None,
            error_message: None,
            retry_count: 0,
            started_at: None,
            completed_at: None,
            next_retry_at: None,
        };
        let out = worker.execute(&db, &step).await.expect("execute");
        assert_eq!(out.result["bodies_deleted"], 2);
        assert!(db.get_mcp_call_body("old-0").await.expect("get").is_none());
        assert!(db.get_mcp_call_body("new-0").await.expect("get").is_some());
    }

    /// The point of the loop: a batch smaller than the backlog must not leave
    /// rows behind. One DELETE could pass this by accident; only a loop does.
    #[tokio::test]
    async fn a_small_batch_still_drains_the_whole_backlog() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        seed(&db, 5, 0).await;
        let worker = McpLogRetention::new(McpLogRetentionConfig {
            batch_size: 1,
            ..Default::default()
        });

        let deleted = worker
            .drain(&db, RetentionTable::CallLog, &cutoff_for(1))
            .await
            .expect("drain");
        assert_eq!(deleted, 5);
        assert!(db
            .list_mcp_call_log(None, 50, 0)
            .await
            .expect("list")
            .is_empty());
    }

    #[tokio::test]
    async fn zero_days_means_never_delete() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        seed(&db, 3, 0).await;
        let worker = McpLogRetention::new(McpLogRetentionConfig {
            call_log_days: 0,
            rejection_days: 0,
            ..Default::default()
        });

        assert!(
            worker.tick(&db).await.expect("tick").is_none(),
            "0 must disable the sweep, not make everything expired"
        );
    }

    #[tokio::test]
    async fn a_configured_zero_leaves_that_table_alone() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        seed(&db, 3, 0).await;
        let worker = McpLogRetention::new(McpLogRetentionConfig {
            call_log_days: 0,
            rejection_days: 7,
            ..Default::default()
        });

        let steps = worker.tick(&db).await.expect("tick").expect("work");
        let step = StepRecord {
            id: "s".to_string(),
            job_id: "j".to_string(),
            step_key: steps[0].key.clone(),
            step_type: "mcp_log_retention".to_string(),
            status: "pending".to_string(),
            payload: steps[0].payload.clone(),
            result: None,
            error_message: None,
            retry_count: 0,
            started_at: None,
            completed_at: None,
            next_retry_at: None,
        };
        let out = worker.execute(&db, &step).await.expect("execute");
        assert_eq!(out.result["calls_deleted"], 0);
        assert_eq!(out.result["rejections_deleted"], 3);
        assert_eq!(
            db.list_mcp_call_log(None, 50, 0).await.expect("list").len(),
            3,
            "the disabled table must be untouched"
        );
    }
}
