//! Body Archiver — archives spend_logs body fields to Parquet cold storage.
//!
//! Implements AsyncTask for cron-based and manually triggered archive jobs.
//!
//! # Architecture
//! - tick(): discovers hours with unarchived data, returns NewStep[]
//! - execute(): reads unarchived rows for a given hour → writes Parquet → uploads → marks archived
//! - finalize(): nulls body columns for rows past retention period

pub mod config;
pub mod storage;
pub mod writer;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use tracing::info;

use crate::async_task::{AsyncTask, NewStep, StepOutput};
use crate::body_archive::config::BodyArchiveConfig;
use crate::body_archive::storage::build_object_store;
use crate::body_archive::writer::write_parquet_to_store;
use crate::db::{Database, DbError, Result};

/// BodyArchiver: archives spend_logs body fields to Parquet on object storage.
pub struct BodyArchiver {
    config: BodyArchiveConfig,
}

impl BodyArchiver {
    pub fn new(config: BodyArchiveConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &BodyArchiveConfig {
        &self.config
    }
}

#[async_trait]
impl AsyncTask for BodyArchiver {
    fn step_type(&self) -> &'static str {
        "body_archive"
    }

    /// Cron: discover hours with unarchived body data.
    async fn tick(&self, db: &Database) -> Result<Option<Vec<NewStep>>> {
        if !self.config.enabled {
            return Ok(None);
        }

        let cutoff = Utc::now() - Duration::hours(self.config.archive.archive_after_hours as i64);
        let cutoff_str = cutoff.format("%Y-%m-%dT%H:00:00+00:00").to_string();

        // Find hours where there are unarchived rows with body data
        let query = r#"
            SELECT DISTINCT strftime('%Y-%m-%dT%H', start_time) as hour
            FROM spend_logs
            WHERE body_archived = FALSE
              AND messages IS NOT NULL
              AND start_time < ?
            ORDER BY hour
            LIMIT 24
        "#;

        let hours: Vec<String> = match db {
            Database::Sqlite(pool) => {
                sqlx::query_scalar::<_, String>(query)
                    .bind(&cutoff_str)
                    .fetch_all(pool)
                    .await?
            }
            Database::Mysql(pool) => {
                let mysql_query = r#"
                    SELECT DISTINCT DATE_FORMAT(start_time, '%Y-%m-%dT%H') as hour
                    FROM spend_logs
                    WHERE body_archived = FALSE
                      AND messages IS NOT NULL
                      AND start_time < ?
                    ORDER BY hour
                    LIMIT 24
                "#;
                sqlx::query_scalar::<_, String>(mysql_query)
                    .bind(&cutoff_str)
                    .fetch_all(pool)
                    .await?
            }
            Database::Postgres(pool) => {
                let pg_query = r#"
                    SELECT DISTINCT to_char(start_time, 'YYYY-MM-DD"T"HH24') as hour
                    FROM spend_logs
                    WHERE body_archived = FALSE
                      AND messages IS NOT NULL
                      AND start_time < $1
                    ORDER BY hour
                    LIMIT 24
                "#;
                sqlx::query_scalar::<_, String>(pg_query)
                    .bind(&cutoff_str)
                    .fetch_all(pool)
                    .await?
            }
        };

        if hours.is_empty() {
            return Ok(None);
        }

        let steps: Vec<NewStep> = hours
            .into_iter()
            .map(|hour| NewStep {
                key: format!("hour={}", hour),
                payload: serde_json::json!({"hour": hour, "batch_size": self.config.archive.batch_size}),
            })
            .collect();

        info!(count = steps.len(), "body_archive tick: discovered unarchived hours");
        Ok(Some(steps))
    }

    fn tick_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.config.archive.check_interval_secs)
    }

    /// Execute: archive body data for one hour.
    async fn execute(&self, db: &Database, step: &crate::async_task::StepRecord) -> Result<StepOutput> {
        let hour = step.payload["hour"].as_str().unwrap_or("unknown");
        let batch_size = step.payload["batch_size"].as_u64().unwrap_or(5000) as usize;

        info!(%hour, batch_size, "body_archive: executing archive for hour");

        // 1. Query unarchived rows for this hour
        let rows = query_unarchived_rows(db, hour, batch_size).await?;
        let row_count = rows.len();

        if rows.is_empty() {
            return Ok(StepOutput {
                result: serde_json::json!({"rows_archived": 0, "hour": hour, "message": "no unarchived rows"}),
            });
        }

        // 2. Build storage path
        let storage_path = build_storage_path(&self.config.s3.prefix, hour);

        // 3. Write Parquet + upload to storage
        let object_store = build_object_store(&self.config.s3)
            .map_err(|e| DbError::Other(format!("storage init: {}", e)))?;

        let bytes_written = write_parquet_to_store(
            &*object_store,
            &storage_path,
            &rows,
            self.config.archive.row_group_size,
        )
        .map_err(|e| DbError::Other(format!("parquet write: {}", e)))?;

        // 4. Mark rows as archived in DB
        let request_ids: Vec<String> = rows.iter().map(|r| r.request_id.clone()).collect();
        mark_rows_archived(db, &request_ids, &storage_path).await?;

        info!(%hour, row_count, bytes_written, path = %storage_path, "body_archive: hour archived");

        Ok(StepOutput {
            result: serde_json::json!({
                "rows_archived": row_count,
                "bytes_written": bytes_written,
                "storage_path": storage_path,
                "hour": hour,
            }),
        })
    }

    fn concurrency(&self) -> usize {
        2
    }

    /// Finalize: null body columns for rows past retention period.
    async fn finalize(&self, db: &Database, _job: &crate::async_task::JobRecord) -> Result<()> {
        if !self.config.archive.null_body_after_archive {
            return Ok(());
        }

        let cutoff = Utc::now() - Duration::days(self.config.archive.null_body_after_days as i64);
        let cutoff_str = cutoff.format("%Y-%m-%dT%H:%M:%S+00:00").to_string();

        let affected = null_expired_bodies(db, &cutoff_str).await?;
        if affected > 0 {
            info!(affected, "body_archive: nulled expired body fields");

            // SQLite VACUUM
            if self.config.archive.vacuum_after_null {
                if let Database::Sqlite(pool) = db {
                    sqlx::query("VACUUM").execute(pool).await.ok();
                }
            }
        }

        Ok(())
    }

    /// Manual trigger: convert date range to hour steps.
    async fn steps_from_payload(&self, payload: &serde_json::Value) -> Result<Vec<NewStep>> {
        let start = payload["start_date"]
            .as_str()
            .ok_or_else(|| DbError::Other("start_date required".into()))?;
        let end = payload["end_date"]
            .as_str()
            .unwrap_or(start);

        let start_dt = DateTime::parse_from_rfc3339(start)
            .map_err(|e| DbError::Other(format!("invalid start_date: {}", e)))?;
        let end_dt = DateTime::parse_from_rfc3339(end)
            .map_err(|e| DbError::Other(format!("invalid end_date: {}", e)))?;

        let mut steps = Vec::new();
        let mut current = start_dt;
        while current < end_dt {
            let hour = current.format("%Y-%m-%dT%H").to_string();
            steps.push(NewStep {
                key: format!("hour={}", hour),
                payload: serde_json::json!({
                    "hour": hour,
                    "batch_size": self.config.archive.batch_size,
                }),
            });
            current = current + Duration::hours(1);
        }

        Ok(steps)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Internal helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A row of body data from spend_logs, ready for archival.
#[derive(Debug, Clone)]
pub struct BodyRow {
    pub request_id: String,
    pub start_time: String,
    pub model: String,
    pub status: Option<String>,
    pub cache_hit: Option<String>,
    pub session_id: Option<String>,
    pub messages: Option<String>,
    pub response: Option<String>,
    pub proxy_server_request: Option<String>,
}

fn build_storage_path(prefix: &str, hour: &str) -> String {
    // Parse "2026-07-22T14" → year=2026/month=07/day=22/hour=14/data.parquet
    let parts: Vec<&str> = hour.split('T').collect();
    if parts.len() != 2 {
        return format!("{}/{}/data.parquet", prefix, hour);
    }
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    if date_parts.len() != 3 {
        return format!("{}/{}/data.parquet", prefix, hour);
    }
    format!(
        "{}/year={}/month={}/day={}/hour={}/data.parquet",
        prefix, date_parts[0], date_parts[1], date_parts[2], parts[1]
    )
}

async fn query_unarchived_rows(
    db: &Database,
    hour: &str,
    limit: usize,
) -> Result<Vec<BodyRow>> {
    let query_base = r#"
        SELECT request_id, start_time, model, status, cache_hit, session_id,
               messages, response, proxy_server_request
        FROM spend_logs
        WHERE body_archived = FALSE
          AND messages IS NOT NULL
          AND strftime('%Y-%m-%dT%H', start_time) = ?
        ORDER BY request_id
        LIMIT ?
    "#;

    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>, Option<String>, Option<serde_json::Value>, Option<serde_json::Value>, Option<serde_json::Value>)>(query_base)
                .bind(hour)
                .bind(limit as i32)
                .fetch_all(pool)
                .await?;
            Ok(rows.into_iter().map(|(rid, st, m, s, ch, sid, msg, resp, psr)| BodyRow {
                request_id: rid,
                start_time: st,
                model: m,
                status: s,
                cache_hit: ch,
                session_id: sid,
                messages: msg.map(|v| v.to_string()),
                response: resp.map(|v| v.to_string()),
                proxy_server_request: psr.map(|v| v.to_string()),
            }).collect())
        }
        Database::Mysql(pool) => {
            let mysql_query = r#"
                SELECT request_id, start_time, model, status, cache_hit, session_id,
                       messages, response, proxy_server_request
                FROM spend_logs
                WHERE body_archived = FALSE
                  AND messages IS NOT NULL
                  AND DATE_FORMAT(start_time, '%Y-%m-%dT%H') = ?
                ORDER BY request_id
                LIMIT ?
            "#;
            let rows = sqlx::query_as::<_, (String, chrono::NaiveDateTime, String, Option<String>, Option<String>, Option<String>, Option<serde_json::Value>, Option<serde_json::Value>, Option<serde_json::Value>)>(mysql_query)
                .bind(hour)
                .bind(limit as i32)
                .fetch_all(pool)
                .await?;
            Ok(rows.into_iter().map(|(rid, st, m, s, ch, sid, msg, resp, psr)| BodyRow {
                request_id: rid,
                start_time: st.to_string(),
                model: m,
                status: s,
                cache_hit: ch,
                session_id: sid,
                messages: msg.map(|v| v.to_string()),
                response: resp.map(|v| v.to_string()),
                proxy_server_request: psr.map(|v| v.to_string()),
            }).collect())
        }
        Database::Postgres(pool) => {
            let pg_query = r#"
                SELECT request_id,
                       to_char(start_time, 'YYYY-MM-DD"T"HH24:MI:SS') as start_time,
                       model, status, cache_hit, session_id,
                       messages::text, response::text, proxy_server_request::text
                FROM spend_logs
                WHERE body_archived = FALSE
                  AND messages IS NOT NULL
                  AND to_char(start_time, 'YYYY-MM-DD"T"HH24') = $1
                ORDER BY request_id
                LIMIT $2
            "#;
            let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>)>(pg_query)
                .bind(hour)
                .bind(limit as i64)
                .fetch_all(pool)
                .await?;
            Ok(rows.into_iter().map(|(rid, st, m, s, ch, sid, msg, resp, psr)| BodyRow {
                request_id: rid,
                start_time: st,
                model: m,
                status: s,
                cache_hit: ch,
                session_id: sid,
                messages: msg,
                response: resp,
                proxy_server_request: psr,
            }).collect())
        }
    }
}

async fn mark_rows_archived(db: &Database, request_ids: &[String], path: &str) -> Result<()> {
    if request_ids.is_empty() {
        return Ok(());
    }

    // Build placeholders for IN clause
    let placeholders = request_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "UPDATE spend_logs SET body_archived = TRUE, parquet_path = ? WHERE request_id IN ({})",
        placeholders
    );

    match db {
        Database::Sqlite(pool) => {
            let mut query = sqlx::query(&sql).bind(path);
            for id in request_ids {
                query = query.bind(id);
            }
            query.execute(pool).await?;
        }
        Database::Mysql(pool) => {
            let mut query = sqlx::query(&sql).bind(path);
            for id in request_ids {
                query = query.bind(id);
            }
            query.execute(pool).await?;
        }
        Database::Postgres(pool) => {
            let pg_placeholders: Vec<String> = (2..=request_ids.len()+1).map(|i| format!("${}", i)).collect();
            let pg_sql = format!(
                "UPDATE spend_logs SET body_archived = TRUE, parquet_path = $1 WHERE request_id IN ({})",
                pg_placeholders.join(",")
            );
            let mut query = sqlx::query(&pg_sql).bind(path);
            for id in request_ids {
                query = query.bind(id);
            }
            query.execute(pool).await?;
        }
    }

    Ok(())
}

async fn null_expired_bodies(db: &Database, cutoff: &str) -> Result<u64> {
    let rows_affected = match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                "UPDATE spend_logs SET messages = NULL, response = NULL, proxy_server_request = NULL
                 WHERE body_archived = TRUE AND start_time < ? AND messages IS NOT NULL"
            )
            .bind(cutoff)
            .execute(pool).await?.rows_affected()
        }
        Database::Mysql(pool) => {
            sqlx::query(
                "UPDATE spend_logs SET messages = NULL, response = NULL, proxy_server_request = NULL
                 WHERE body_archived = TRUE AND start_time < ? AND messages IS NOT NULL"
            )
            .bind(cutoff)
            .execute(pool).await?.rows_affected()
        }
        Database::Postgres(pool) => {
            sqlx::query(
                "UPDATE spend_logs SET messages = NULL, response = NULL, proxy_server_request = NULL
                 WHERE body_archived = TRUE AND start_time < $1 AND messages IS NOT NULL"
            )
            .bind(cutoff)
            .execute(pool).await?.rows_affected()
        }
    };
    Ok(rows_affected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_storage_path() {
        let path = build_storage_path("logs", "2026-07-22T14");
        assert_eq!(path, "logs/year=2026/month=07/day=22/hour=14/data.parquet");
    }

    #[test]
    fn test_build_storage_path_invalid_format() {
        let path = build_storage_path("logs", "invalid");
        assert_eq!(path, "logs/invalid/data.parquet");
    }
}
