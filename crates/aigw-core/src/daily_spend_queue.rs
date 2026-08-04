//! In-memory queue for daily_spend pre-aggregation.
//!
//! Provides a non-blocking queue that collects `DailySpendLog` records
//! on every request completion. A background task drains the queue every
//! 10 seconds, aggregates by composite key, and batch-upserts into the
//! corresponding `daily_*_spend` tables via `ON CONFLICT DO UPDATE`.
//!
//! Multi-instance safety: `col = col + EXCLUDED.col` SQL is atomic at
//! the database level — two instances updating the same row concurrently
//! both get their increments applied correctly.

use crate::db::Database;
use crate::db::DbError;
use crate::models::{DailySpendKind, DailySpendLog};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

/// A pending daily_spend record waiting to be flushed to the database.
#[derive(Debug)]
pub struct PendingDailySpend {
    pub log: DailySpendLog,
}

/// The daily_spend queue that collects records and flushes them periodically.
#[derive(Debug)]
pub struct DailySpendQueue {
    tx: mpsc::UnboundedSender<PendingDailySpend>,
}

impl DailySpendQueue {
    /// Create a new queue and start the background drain task.
    ///
    /// The drain task runs every 10 seconds, aggregating all queued
    /// records and upserting them in a single batch per table.
    pub fn new(db: Arc<Database>) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<PendingDailySpend>();

        tokio::spawn(async move {
            loop {
                // Drain all pending records
                let mut pending: Vec<PendingDailySpend> = Vec::new();
                // Collect available records (non-blocking)
                while let Ok(item) = rx.try_recv() {
                    pending.push(item);
                }
                // If queue was empty, wait for at least one item
                if pending.is_empty() {
                    match rx.recv().await {
                        Some(item) => pending.push(item),
                        None => break, // channel closed
                    }
                }

                // After first item, drain any additional queued items
                while let Ok(item) = rx.try_recv() {
                    pending.push(item);
                }

                // Aggregate by (kind, composite key)
                let grouped = aggregate_daily_spend(pending);

                // Batch upsert each group
                for (table_name, entries) in &grouped {
                    if let Err(e) = batch_upsert_daily_spend(&db, table_name, entries).await {
                        tracing::warn!(
                            table = %table_name,
                            count = entries.len(),
                            error = %e,
                            "daily_spend batch upsert failed"
                        );
                    }
                }

                // Wait 10 seconds before next drain
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        });

        Self { tx }
    }

    /// Queue a daily_spend record for the next flush. Non-blocking.
    pub fn queue(&self, log: DailySpendLog) {
        let _ = self.tx.send(PendingDailySpend { log });
    }
}

/// Aggregate pending records by table name and composite key.
///
/// Returns a map of table_name → vec of aggregated entries.
/// Each entry is (key_fields..., accumulated_metrics).
type AggEntry = (
    String, // entity_id
    String, // date
    String, // api_key
    String, // model
    String, // custom_llm_provider
    String, // mcp_namespaced_tool_name
    String, // endpoint
    String, // model_group
    i64,    // prompt_tokens
    i64,    // completion_tokens
    i64,    // cache_read_input_tokens
    i64,    // cache_creation_input_tokens
    f64,    // spend
    i64,    // api_requests
    i64,    // successful_requests
    i64,    // failed_requests
);

fn aggregate_daily_spend(pending: Vec<PendingDailySpend>) -> HashMap<String, Vec<AggEntry>> {
    let mut tables: HashMap<String, HashMap<String, AggEntry>> = HashMap::new();

    for item in pending {
        let log = &item.log;
        let table_name = match &log.kind {
            DailySpendKind::User => "daily_user_spend".to_string(),
            DailySpendKind::Team => "daily_team_spend".to_string(),
            DailySpendKind::Organization => "daily_organization_spend".to_string(),
            DailySpendKind::EndUser => "daily_end_user_spend".to_string(),
            DailySpendKind::Agent => "daily_agent_spend".to_string(),
            DailySpendKind::Tag { .. } => "daily_tag_spend".to_string(),
        };

        let key = format!(
            "{}|{}|{}|{}|{}|{}|{}",
            log.entity_id,
            log.date,
            log.api_key,
            log.model,
            log.custom_llm_provider,
            log.mcp_namespaced_tool_name,
            log.endpoint,
        );

        let table_map = tables.entry(table_name).or_default();
        let entry = table_map.entry(key).or_insert_with(|| {
            (
                log.entity_id.clone(),
                log.date.clone(),
                log.api_key.clone(),
                log.model.clone(),
                log.custom_llm_provider.clone(),
                log.mcp_namespaced_tool_name.clone(),
                log.endpoint.clone(),
                log.model_group.clone(),
                0,   // prompt_tokens
                0,   // completion_tokens
                0,   // cache_read_input_tokens
                0,   // cache_creation_input_tokens
                0.0, // spend
                0,   // api_requests
                0,   // successful_requests
                0,   // failed_requests
            )
        });

        entry.7 = log.model_group.clone(); // model_group — take last value
        entry.8 += log.prompt_tokens;
        entry.9 += log.completion_tokens;
        entry.10 += log.cache_read_input_tokens;
        entry.11 += log.cache_creation_input_tokens;
        entry.12 += log.spend;
        entry.13 += log.api_requests;
        entry.14 += log.successful_requests;
        entry.15 += log.failed_requests;
    }

    tables
        .into_iter()
        .map(|(table_name, map)| (table_name, map.into_values().collect()))
        .collect()
}

/// Batch upsert aggregated daily_spend entries into the target table.
///
/// Uses `ON CONFLICT DO UPDATE SET col = col + EXCLUDED.col` for
/// atomic increment semantics across multiple instances.
async fn batch_upsert_daily_spend(
    db: &Database,
    table_name: &str,
    entries: &[AggEntry],
) -> Result<(), DbError> {
    if entries.is_empty() {
        return Ok(());
    }

    // Build the SQL dynamically based on the table name and conflict columns.
    // All 6 tables share the same structure but differ in entity_id column name
    // and conflict key column names.
    let (entity_col, conflict_cols) = match table_name {
        "daily_user_spend" => ("user_id", "user_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint"),
        "daily_team_spend" => ("team_id", "team_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint"),
        "daily_organization_spend" => ("organization_id", "organization_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint"),
        "daily_end_user_spend" => ("end_user_id", "end_user_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint"),
        "daily_agent_spend" => ("agent_id", "agent_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint"),
        "daily_tag_spend" => ("tag", "call_id, tag, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint"),
        _ => return Err(crate::db::DbError::Other(format!("unknown daily_spend table: {}", table_name))),
    };

    // SQL is generated per database backend because PostgreSQL uses $N placeholders
    // while Sqlite and MySQL use ? placeholders. sqlx does NOT auto-convert between them.
    for entry in entries {
        let id = uuid::Uuid::new_v4().to_string();
        match db {
            Database::Sqlite(pool) => {
                let sql = format!(
                    "INSERT INTO {} (id, {}, date, api_key, model, model_group, custom_llm_provider, \
                     mcp_namespaced_tool_name, endpoint, prompt_tokens, completion_tokens, \
                     cache_read_input_tokens, cache_creation_input_tokens, spend, \
                     api_requests, successful_requests, failed_requests) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                     ON CONFLICT ({}) DO UPDATE SET \
                     prompt_tokens = {}.prompt_tokens + EXCLUDED.prompt_tokens, \
                     completion_tokens = {}.completion_tokens + EXCLUDED.completion_tokens, \
                     cache_read_input_tokens = {}.cache_read_input_tokens + EXCLUDED.cache_read_input_tokens, \
                    cache_creation_input_tokens = {}.cache_creation_input_tokens + EXCLUDED.cache_creation_input_tokens, \
                     spend = {}.spend + EXCLUDED.spend, \
                     api_requests = {}.api_requests + EXCLUDED.api_requests, \
                     successful_requests = {}.successful_requests + EXCLUDED.successful_requests, \
                     failed_requests = {}.failed_requests + EXCLUDED.failed_requests, \
                     updated_at = CURRENT_TIMESTAMP",
                    table_name, entity_col,
                    conflict_cols,
                    table_name, table_name, table_name, table_name, table_name, table_name,
                    table_name, table_name,
                );
                sqlx::query(&sql)
                    .bind(&id)
                    .bind(&entry.0)
                    .bind(&entry.1)
                    .bind(&entry.2)
                    .bind(&entry.3)
                    .bind(&entry.7)
                    .bind(&entry.4)
                    .bind(&entry.5)
                    .bind(&entry.6)
                    .bind(entry.8)
                    .bind(entry.9)
                    .bind(entry.10)
                    .bind(entry.11)
                    .bind(entry.12)
                    .bind(entry.13)
                    .bind(entry.14)
                    .bind(entry.15)
                    .execute(pool)
                    .await?;
            }
            Database::Mysql(pool) => {
                let sql = format!(
                    "INSERT INTO {} (id, {}, date, api_key, model, model_group, custom_llm_provider, \
                     mcp_namespaced_tool_name, endpoint, prompt_tokens, completion_tokens, \
                     cache_read_input_tokens, cache_creation_input_tokens, spend, \
                     api_requests, successful_requests, failed_requests) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                     ON CONFLICT ({}) DO UPDATE SET \
                     prompt_tokens = {}.prompt_tokens + EXCLUDED.prompt_tokens, \
                     completion_tokens = {}.completion_tokens + EXCLUDED.completion_tokens, \
                     cache_read_input_tokens = {}.cache_read_input_tokens + EXCLUDED.cache_read_input_tokens, \
                    cache_creation_input_tokens = {}.cache_creation_input_tokens + EXCLUDED.cache_creation_input_tokens, \
                     spend = {}.spend + EXCLUDED.spend, \
                     api_requests = {}.api_requests + EXCLUDED.api_requests, \
                     successful_requests = {}.successful_requests + EXCLUDED.successful_requests, \
                     failed_requests = {}.failed_requests + EXCLUDED.failed_requests, \
                     updated_at = CURRENT_TIMESTAMP",
                    table_name, entity_col,
                    conflict_cols,
                    table_name, table_name, table_name, table_name, table_name, table_name,
                    table_name, table_name,
                );
                sqlx::query(&sql)
                    .bind(&id)
                    .bind(&entry.0)
                    .bind(&entry.1)
                    .bind(&entry.2)
                    .bind(&entry.3)
                    .bind(&entry.7)
                    .bind(&entry.4)
                    .bind(&entry.5)
                    .bind(&entry.6)
                    .bind(entry.8)
                    .bind(entry.9)
                    .bind(entry.10)
                    .bind(entry.11)
                    .bind(entry.12)
                    .bind(entry.13)
                    .bind(entry.14)
                    .bind(entry.15)
                    .execute(pool)
                    .await?;
            }
            Database::Postgres(pool) => {
                let sql = format!(
                    "INSERT INTO {} (id, {}, date, api_key, model, model_group, custom_llm_provider, \
                     mcp_namespaced_tool_name, endpoint, prompt_tokens, completion_tokens, \
                     cache_read_input_tokens, cache_creation_input_tokens, spend, \
                     api_requests, successful_requests, failed_requests) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) \
                     ON CONFLICT ({}) DO UPDATE SET \
                     prompt_tokens = {}.prompt_tokens + EXCLUDED.prompt_tokens, \
                     completion_tokens = {}.completion_tokens + EXCLUDED.completion_tokens, \
                     cache_read_input_tokens = {}.cache_read_input_tokens + EXCLUDED.cache_read_input_tokens, \
                    cache_creation_input_tokens = {}.cache_creation_input_tokens + EXCLUDED.cache_creation_input_tokens, \
                     spend = {}.spend + EXCLUDED.spend, \
                     api_requests = {}.api_requests + EXCLUDED.api_requests, \
                     successful_requests = {}.successful_requests + EXCLUDED.successful_requests, \
                     failed_requests = {}.failed_requests + EXCLUDED.failed_requests, \
                     updated_at = CURRENT_TIMESTAMP",
                    table_name, entity_col,
                    conflict_cols,
                    table_name, table_name, table_name, table_name, table_name, table_name,
                    table_name, table_name,
                );
                sqlx::query(&sql)
                    .bind(&id)
                    .bind(&entry.0)
                    .bind(&entry.1)
                    .bind(&entry.2)
                    .bind(&entry.3)
                    .bind(&entry.7)
                    .bind(&entry.4)
                    .bind(&entry.5)
                    .bind(&entry.6)
                    .bind(entry.8)
                    .bind(entry.9)
                    .bind(entry.10)
                    .bind(entry.11)
                    .bind(entry.12)
                    .bind(entry.13)
                    .bind(entry.14)
                    .bind(entry.15)
                    .execute(pool)
                    .await?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_log(
        kind: DailySpendKind,
        entity_id: &str,
        date: &str,
        api_key: &str,
        model: &str,
        spend: f64,
        prompt_tokens: i64,
        completion_tokens: i64,
        requests: i64,
    ) -> PendingDailySpend {
        PendingDailySpend {
            log: DailySpendLog {
                kind,
                entity_id: entity_id.to_string(),
                date: date.to_string(),
                api_key: api_key.to_string(),
                model: model.to_string(),
                custom_llm_provider: "openai".to_string(),
                mcp_namespaced_tool_name: String::new(),
                endpoint: "/chat/completions".to_string(),
                model_group: model.to_string(),
                prompt_tokens,
                completion_tokens,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
                spend,
                api_requests: requests,
                successful_requests: requests,
                failed_requests: 0,
            },
        }
    }

    #[test]
    fn test_aggregate_single_kind_user() {
        let items = vec![mk_log(
            DailySpendKind::User,
            "user-a",
            "2026-01-01",
            "sk-test",
            "gpt-4",
            0.05,
            100,
            50,
            1,
        )];
        let result = aggregate_daily_spend(items);
        assert_eq!(result.len(), 1);
        let entries = result.get("daily_user_spend").expect("user table");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "user-a"); // entity_id
        assert_eq!(entries[0].1, "2026-01-01"); // date
        assert_eq!(entries[0].12, 0.05); // spend
    }

    #[test]
    fn test_aggregate_multiple_kinds() {
        let items = vec![
            mk_log(DailySpendKind::User, "u1", "2026-01-01", "sk", "m", 1.0, 10, 20, 1),
            mk_log(DailySpendKind::Team, "t1", "2026-01-01", "sk", "m", 2.0, 30, 40, 1),
            mk_log(
                DailySpendKind::Organization,
                "o1",
                "2026-01-01",
                "sk",
                "m",
                3.0,
                50,
                60,
                1,
            ),
        ];
        let result = aggregate_daily_spend(items);
        assert_eq!(result.len(), 3);
        assert!(result.contains_key("daily_user_spend"));
        assert!(result.contains_key("daily_team_spend"));
        assert!(result.contains_key("daily_organization_spend"));
    }

    #[test]
    fn test_aggregate_empty_returns_empty() {
        let result = aggregate_daily_spend(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_aggregate_sum_correct() {
        // Same key → aggregated into one entry with summed tokens/spend
        let items = vec![
            mk_log(DailySpendKind::User, "u1", "2026-01-01", "sk", "gpt-4", 0.05, 100, 50, 1),
            mk_log(DailySpendKind::User, "u1", "2026-01-01", "sk", "gpt-4", 0.03, 200, 100, 1),
            mk_log(DailySpendKind::User, "u1", "2026-01-01", "sk", "gpt-4", 0.02, 300, 150, 1),
        ];
        let result = aggregate_daily_spend(items);
        let entries = result.get("daily_user_spend").expect("user table");
        assert_eq!(entries.len(), 1, "same key should aggregate to 1 entry");
        assert_eq!(entries[0].8, 600, "prompt_tokens: 100+200+300");
        assert_eq!(entries[0].9, 300, "completion_tokens: 50+100+150");
        assert!(
            (entries[0].12 - 0.10).abs() < 0.001,
            "spend: 0.05+0.03+0.02 = {}",
            entries[0].12
        );
        assert_eq!(entries[0].13, 3, "api_requests: 1+1+1");
        assert_eq!(entries[0].14, 3, "successful_requests: 1+1+1");
    }

    #[test]
    fn test_aggregate_different_entities_not_merged() {
        let items = vec![
            mk_log(DailySpendKind::User, "u1", "2026-01-01", "sk", "m", 1.0, 10, 20, 1),
            mk_log(DailySpendKind::User, "u2", "2026-01-01", "sk", "m", 2.0, 30, 40, 1),
        ];
        let result = aggregate_daily_spend(items);
        let entries = result.get("daily_user_spend").expect("user table");
        assert_eq!(entries.len(), 2, "different entity_id → 2 entries");
    }

    #[test]
    fn test_aggregate_different_dates_not_merged() {
        let items = vec![
            mk_log(DailySpendKind::User, "u1", "2026-01-01", "sk", "m", 1.0, 10, 20, 1),
            mk_log(DailySpendKind::User, "u1", "2026-01-02", "sk", "m", 2.0, 30, 40, 1),
        ];
        let result = aggregate_daily_spend(items);
        let entries = result.get("daily_user_spend").expect("user table");
        assert_eq!(entries.len(), 2, "different date → 2 entries");
    }

    #[test]
    fn test_aggregate_different_models_not_merged() {
        let items = vec![
            mk_log(DailySpendKind::User, "u1", "2026-01-01", "sk", "gpt-4", 1.0, 10, 20, 1),
            mk_log(DailySpendKind::User, "u1", "2026-01-01", "sk", "claude-3", 2.0, 30, 40, 1),
        ];
        let result = aggregate_daily_spend(items);
        let entries = result.get("daily_user_spend").expect("user table");
        assert_eq!(entries.len(), 2, "different model → 2 entries");
    }
}
