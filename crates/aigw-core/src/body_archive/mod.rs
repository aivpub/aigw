//! Body Archiver — archives spend_logs body fields to Parquet cold storage.
//!
//! Implements AsyncTask for cron-based and manually triggered archive jobs.
//!
//! # Architecture
//! - tick(): discovers hours with unarchived data, returns NewStep[]
//! - execute(): reads unarchived rows for a given hour → writes Parquet → uploads → marks archived
//! - finalize(): nulls body columns for rows past retention period

pub mod cache;
pub mod config;
pub mod query;
pub mod storage;
pub mod writer;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::sync::{Arc, Mutex};
use tracing::info;

use crate::async_task::{AsyncTask, NewStep, StepOutput};
use crate::body_archive::cache::FooterCache;
use crate::body_archive::config::BodyArchiveConfig;
use crate::body_archive::query::{query_parquet_with_cache, BodyPayload};
use crate::body_archive::storage::{build_object_store_for_backend, resolve_env_placeholders};
use crate::body_archive::writer::write_parquet_shards;
use crate::db::{Database, DbError, Result};

/// BodyArchiver: archives spend_logs body fields to Parquet on object storage.
pub struct BodyArchiver {
    config: BodyArchiveConfig,
    footer_cache: FooterCache,
    /// Lazily-initialized object store. Built on first read and reused
    /// for the lifetime of the archiver. Avoids rebuilding S3/LocalFS
    /// client on every request. Mutex used to avoid unstable OnceLock::get_or_try_init.
    store: Mutex<Option<Arc<dyn object_store::ObjectStore>>>,
}

// Manual Debug: Mutex<Option<Arc<dyn ObjectStore>>> doesn't impl Debug
impl std::fmt::Debug for BodyArchiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let has_store = self.store.lock().unwrap().is_some();
        f.debug_struct("BodyArchiver")
            .field("config", &self.config)
            .field("footer_cache", &self.footer_cache)
            .field("store_initialized", &has_store)
            .finish()
    }
}

impl BodyArchiver {
    pub fn new(config: BodyArchiveConfig) -> Self {
        Self {
            footer_cache: FooterCache::default(),
            config,
            store: Mutex::new(None),
        }
    }

    /// Lazily build (on first call) or return the cached ObjectStore.
    fn get_or_init_store(&self) -> std::result::Result<Arc<dyn object_store::ObjectStore>, String> {
        // Fast path: store already initialized
        {
            let guard = self.store.lock().unwrap();
            if let Some(ref store) = *guard {
                return Ok(Arc::clone(store));
            }
        }
        // Slow path: build the store
        let resolved = resolve_env_placeholders(&self.config.storage);
        let new_store = build_object_store_for_backend(&resolved)?;
        let mut guard = self.store.lock().unwrap();
        // Double-check: another thread might have initialized it while we
        // were building
        if let Some(ref existing) = *guard {
            return Ok(Arc::clone(existing));
        }
        *guard = Some(Arc::clone(&new_store));
        Ok(new_store)
    }

    pub fn config(&self) -> &BodyArchiveConfig {
        &self.config
    }

    /// Return a copy of the storage backend with `${ENV_VAR}` placeholders
    /// resolved from the environment (Stage 83 credential safety).
    pub fn resolved_storage(&self) -> BodyArchiveConfig {
        let mut cfg = self.config.clone();
        cfg.storage = resolve_env_placeholders(&self.config.storage);
        cfg
    }

    /// Check whether storage is properly configured for archiving.
    /// For S3: requires non-empty bucket. For FileSystem: requires non-empty path.
    pub fn storage_configured(&self) -> bool {
        use crate::body_archive::config::StorageBackend;
        match &self.config.storage {
            StorageBackend::S3 {
                bucket,
                access_key_id,
                ..
            } => !bucket.is_empty() && !access_key_id.is_empty(),
            StorageBackend::FileSystem { path } => !path.as_os_str().is_empty(),
        }
    }
}

#[async_trait]
impl AsyncTask for BodyArchiver {
    fn step_type(&self) -> &'static str {
        "body_archive"
    }

    /// Cron: discover hours with unarchived body data.
    async fn tick(&self, db: &Database) -> Result<Option<Vec<NewStep>>> {
        if !self.config.auto_archive {
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

        info!(
            count = steps.len(),
            "body_archive tick: discovered unarchived hours"
        );
        Ok(Some(steps))
    }

    fn tick_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.config.archive.check_interval_secs)
    }

    /// Execute: archive body data for one hour.
    async fn execute(
        &self,
        db: &Database,
        step: &crate::async_task::StepRecord,
    ) -> Result<StepOutput> {
        let hour = step.payload["hour"].as_str().unwrap_or("unknown");
        let batch_size = step.payload["batch_size"].as_u64().unwrap_or(5000) as usize;

        info!(%hour, batch_size, "body_archive: executing archive for hour");

        // 0. Validate storage is properly configured.
        if !self.storage_configured() {
            return Err(DbError::Other("body archive storage not configured".into()));
        }

        // 1. Query unarchived rows for this hour
        let rows = query_unarchived_rows(db, hour, batch_size).await?;
        let row_count = rows.len();

        if rows.is_empty() {
            return Ok(StepOutput {
                result: serde_json::json!({"rows_archived": 0, "hour": hour, "message": "no unarchived rows"}),
            });
        }

        // 2. Build storage path
        let prefix = match &self.config.storage {
            crate::body_archive::config::StorageBackend::S3 { prefix, .. } => prefix.as_str(),
            crate::body_archive::config::StorageBackend::FileSystem { .. } => "",
        };
        let storage_path = build_storage_path(prefix, hour);

        // 3. Write Parquet + upload to storage (S3 or FileSystem). For hours
        //    that exceed `max_parquet_body_mb`, this writes multiple
        //    `data-N.parquet` shards; each shard's rows must be marked archived
        //    with the EXACT shard path so cold reads resolve the right object.
        let resolved = resolve_env_placeholders(&self.config.storage);
        let object_store = build_object_store_for_backend(&resolved)
            .map_err(|e| DbError::Other(format!("storage init: {}", e)))?;

        let shards = write_parquet_shards(
            &*object_store,
            &storage_path,
            &rows,
            self.config.archive.row_group_size,
            self.config.archive.bloom_min_rows,
            &self.config.archive.compression,
            self.config.archive.compression_level,
            self.config.archive.multipart_part_size_mb,
            self.config.archive.max_parquet_body_mb,
        )
        .await
        .map_err(|e| DbError::Other(format!("parquet write: {}", e)))?;

        let bytes_written: usize = shards.iter().map(|s| s.bytes).sum();

        // 4. Mark rows as archived in DB — per-shard path so cold reads of a
        //    sharded hour find the correct object.
        let mut archived = 0usize;
        for shard in &shards {
            let shard_rows: Vec<BodyRow> = rows
                .iter()
                .take(shard.row_count)
                .skip(archived)
                .cloned()
                .collect();
            let call_ids: Vec<String> = shard_rows.iter().map(|r| r.call_id.clone()).collect();
            mark_rows_archived(db, &call_ids, &shard.path).await?;
            archived += shard.row_count;
        }
        // Safety: every row must be covered by a shard.
        if archived != row_count {
            return Err(DbError::Other(format!(
                "body archive: shard row accounting mismatch: archived {archived} of {row_count}"
            )));
        }

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
        if !self.storage_configured() {
            return Err(DbError::Other("body archive storage not configured".into()));
        }

        let start = payload["start_date"]
            .as_str()
            .ok_or_else(|| DbError::Other("start_date required".into()))?;
        let end = payload["end_date"].as_str().unwrap_or(start);

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
// Query router — get_message_body()
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl BodyArchiver {
    /// Get message body for a spend_log record.
    ///
    /// Hot path: body exists in DB → return directly.
    /// Cold path: body is NULL + body_archived=true → read from Parquet.
    pub async fn get_message_body(
        &self,
        db: &Database,
        call_id: &str,
    ) -> Result<Option<BodyPayload>> {
        // Use the existing GetSpendLog trait method to fetch the row
        let log = db.get_spend_log_by_call_id(call_id).await?;

        match log {
            Some(ref entry) if entry.messages.is_some() => {
                // Hot path: body exists in DB
                Ok(Some(BodyPayload {
                    messages: entry.messages.clone(),
                    response: entry.response.clone(),
                    proxy_server_request: entry.proxy_server_request.clone(),
                }))
            }
            Some(ref entry) if entry.body_archived && entry.parquet_path.is_some() => {
                // Cold path: body archived to Parquet
                let path = entry.parquet_path.as_ref().unwrap();
                self.read_body_from_storage(path, call_id).await
            }
            Some(_) => Ok(None), // DB row exists but body is null and not archived
            None => Ok(None),    // Record not found
        }
    }

    /// Read body from Parquet file at the given storage path, using the
    /// cached object store (lazy-init on first call). Uses the footer cache
    /// so repeated reads of the same hour file skip the footer round-trip.
    ///
    /// Error semantics (Stage 83 P1-2):
    /// - Object not found → `Ok(None)` (cold data legitimately absent)
    /// - Store unreachable / decode failure → `Err`
    pub async fn read_body_from_storage(
        &self,
        parquet_path: &str,
        call_id: &str,
    ) -> Result<Option<BodyPayload>> {
        let store = self
            .get_or_init_store()
            .map_err(|e| DbError::Other(format!("storage init: {}", e)))?;
        self.read_body_from_storage_with_store(&store, parquet_path, call_id)
            .await
    }

    /// Read body directly from a known `parquet_path` without any DB query.
    ///
    /// The caller already knows the body is in cold storage (e.g. handler
    /// has the `SpendLog` row and sees `body_archived=true` + `messages=None`).
    /// This avoids the redundant DB round-trip that `get_message_body` performs.
    ///
    /// Debug builds emit per-phase trace logs with latency breakdowns.
    pub async fn read_body_from_parquet_path(
        &self,
        parquet_path: &str,
        call_id: &str,
    ) -> Result<Option<BodyPayload>> {
        let store = self
            .get_or_init_store()
            .map_err(|e| DbError::Other(format!("storage init: {}", e)))?;
        self.read_body_from_storage_with_store(&store, parquet_path, call_id)
            .await
    }

    /// Read body from Parquet at `parquet_path` using a caller-provided store.
    /// Split out so tests (and the FS round-trip) can inject an InMemory or
    /// LocalFileSystem store without touching env config.
    pub async fn read_body_from_storage_with_store(
        &self,
        store: &std::sync::Arc<dyn object_store::ObjectStore>,
        parquet_path: &str,
        call_id: &str,
    ) -> Result<Option<BodyPayload>> {
        // Footer-cached range read: NotFound → Ok(None), other errors → Err.
        match query_parquet_with_cache(store, &self.footer_cache, parquet_path, call_id).await {
            Ok(body) => Ok(body),
            Err(msg) => {
                // Distinguish "object not found" from genuine failures.
                if msg.contains("not found") || msg.contains("NotFound") {
                    Ok(None)
                } else {
                    Err(DbError::Other(format!("cold read: {}", msg)))
                }
            }
        }
    }

    /// Footer-cached range read over an arbitrary object store. Thin wrapper
    /// over [`query_parquet_with_cache`] that exposes the archiver's footer
    /// cache so callers don't have to construct one themselves.
    pub async fn query_parquet_with_cache(
        &self,
        store: &std::sync::Arc<dyn object_store::ObjectStore>,
        path_str: &str,
        target_call_id: &str,
    ) -> std::result::Result<Option<BodyPayload>, String> {
        query_parquet_with_cache(store, &self.footer_cache, path_str, target_call_id).await
    }

    /// Archive a slice of body rows to object storage for the given hour,
    /// returning the storage path the parquet was written to. Public so tests
    /// (and a future admin "dry-run") can exercise the write path without the
    /// full Engine + DB pipeline.
    pub async fn archive_rows_to_storage(&self, rows: &[BodyRow], hour: &str) -> Result<String> {
        if !self.storage_configured() {
            return Err(DbError::Other("body archive storage not configured".into()));
        }
        if rows.is_empty() {
            return Ok(String::new());
        }

        let prefix = match &self.config.storage {
            crate::body_archive::config::StorageBackend::S3 { prefix, .. } => prefix.as_str(),
            crate::body_archive::config::StorageBackend::FileSystem { path } => {
                // FileSystem stores under its root; LocalFileSystem resolves
                // keys relative to the configured root, so the object-store
                // key must NOT include the FS root path (that would double it).
                let _ = path;
                ""
            }
        };
        let storage_path = build_storage_path(prefix, hour);

        let resolved = resolve_env_placeholders(&self.config.storage);
        let store = build_object_store_for_backend(&resolved)
            .map_err(|e| DbError::Other(format!("storage init: {}", e)))?;

        write_parquet_shards(
            &*store,
            &storage_path,
            rows,
            self.config.archive.row_group_size,
            self.config.archive.bloom_min_rows,
            &self.config.archive.compression,
            self.config.archive.compression_level,
            self.config.archive.multipart_part_size_mb,
            self.config.archive.max_parquet_body_mb,
        )
        .await
        .map_err(|e| DbError::Other(format!("parquet write: {}", e)))?;

        Ok(storage_path)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Admin stats
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl BodyArchiver {
    /// Get archive statistics — count of archived and pending rows.
    pub async fn get_archive_stats(&self, db: &Database) -> Result<serde_json::Value> {
        let (total_archived_rows, pending_rows) = match db {
            Database::Sqlite(pool) => {
                let archived: (i64,) =
                    sqlx::query_as("SELECT COUNT(*) FROM spend_logs WHERE body_archived = TRUE")
                        .fetch_one(pool)
                        .await?;
                let pending: (i64,) = sqlx::query_as(
                    "SELECT COUNT(*) FROM spend_logs WHERE body_archived = FALSE AND messages IS NOT NULL",
                )
                .fetch_one(pool)
                .await?;
                (archived.0, pending.0)
            }
            Database::Mysql(pool) => {
                let archived: (i64,) =
                    sqlx::query_as("SELECT COUNT(*) FROM spend_logs WHERE body_archived = TRUE")
                        .fetch_one(pool)
                        .await?;
                let pending: (i64,) = sqlx::query_as(
                    "SELECT COUNT(*) FROM spend_logs WHERE body_archived = FALSE AND messages IS NOT NULL",
                )
                .fetch_one(pool)
                .await?;
                (archived.0, pending.0)
            }
            Database::Postgres(pool) => {
                let archived: (i64,) =
                    sqlx::query_as("SELECT COUNT(*) FROM spend_logs WHERE body_archived = TRUE")
                        .fetch_one(pool)
                        .await?;
                let pending: (i64,) = sqlx::query_as(
                    "SELECT COUNT(*) FROM spend_logs WHERE body_archived = FALSE AND messages IS NOT NULL",
                )
                .fetch_one(pool)
                .await?;
                (archived.0, pending.0)
            }
        };

        Ok(serde_json::json!({
            "total_archived_rows": total_archived_rows,
            "pending_rows": pending_rows,
            "auto_archive": self.config.auto_archive,
            "storage_configured": self.storage_configured(),
        }))
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Internal helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A row of body data from spend_logs, ready for archival.
#[derive(Debug, Clone)]
pub struct BodyRow {
    pub call_id: String,
    pub start_time: String,
    pub model: String,
    pub status: Option<String>,
    pub cache_hit: Option<String>,
    pub session_id: Option<String>,
    pub messages: Option<String>,
    pub response: Option<String>,
    pub proxy_server_request: Option<String>,
    pub request_id: Option<String>,
    pub spend: f64,
    pub total_tokens: i32,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub end_time: String,
    pub model_group: Option<String>,
}

/// Build the object-store key for a given hour. The key is always relative
/// (no leading slash): `year=YYYY/month=MM/day=DD/hour=HH/data.parquet`.
/// `prefix` is optional (S3 bucket key prefix); for FileSystem it must be
/// empty because the FS root is already configured on the store.
fn build_storage_path(prefix: &str, hour: &str) -> String {
    // Parse "2026-07-22T14" → year=2026/month=07/day=22/hour=14/data.parquet
    let parts: Vec<&str> = hour.split('T').collect();
    let rel = if parts.len() == 2 {
        let date_parts: Vec<&str> = parts[0].split('-').collect();
        if date_parts.len() == 3 {
            format!(
                "year={}/month={}/day={}/hour={}/data.parquet",
                date_parts[0], date_parts[1], date_parts[2], parts[1]
            )
        } else {
            format!("{}/data.parquet", hour)
        }
    } else {
        format!("{}/data.parquet", hour)
    };

    let prefix = prefix.trim_start_matches('/');
    if prefix.is_empty() {
        rel
    } else {
        format!("{}/{}", prefix, rel)
    }
}

async fn query_unarchived_rows(db: &Database, hour: &str, limit: usize) -> Result<Vec<BodyRow>> {
    let query_base = r#"
        SELECT call_id, start_time, model, status, cache_hit, session_id,
               messages, response, proxy_server_request,
               request_id, spend, total_tokens, prompt_tokens,
               completion_tokens, end_time, model_group
        FROM spend_logs
        WHERE body_archived = FALSE
          AND messages IS NOT NULL
          AND strftime('%Y-%m-%dT%H', start_time) = ?
        ORDER BY call_id
        LIMIT ?
    "#;

    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query_as::<
                _,
                (
                    String,
                    String,
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<serde_json::Value>,
                    Option<serde_json::Value>,
                    Option<serde_json::Value>,
                    Option<String>,
                    f64,
                    i32,
                    i32,
                    i32,
                    String,
                    Option<String>,
                ),
            >(query_base)
            .bind(hour)
            .bind(limit as i32)
            .fetch_all(pool)
            .await?;
            Ok(rows
                .into_iter()
                .map(
                    |(rid, st, m, s, ch, sid, msg, resp, psr, req_id, sp, tt, pt, ct, et, mg)| {
                        BodyRow {
                            call_id: rid,
                            start_time: st,
                            model: m,
                            status: s,
                            cache_hit: ch,
                            session_id: sid,
                            messages: msg.map(|v| v.to_string()),
                            response: resp.map(|v| v.to_string()),
                            proxy_server_request: psr.map(|v| v.to_string()),
                            request_id: req_id,
                            spend: sp,
                            total_tokens: tt,
                            prompt_tokens: pt,
                            completion_tokens: ct,
                            end_time: et,
                            model_group: mg,
                        }
                    },
                )
                .collect())
        }
        Database::Mysql(pool) => {
            let mysql_query = r#"
                SELECT call_id, start_time, model, status, cache_hit, session_id,
                       messages, response, proxy_server_request,
                       request_id, spend, total_tokens, prompt_tokens,
                       completion_tokens, end_time, model_group
                FROM spend_logs
                WHERE body_archived = FALSE
                  AND messages IS NOT NULL
                  AND DATE_FORMAT(start_time, '%Y-%m-%dT%H') = ?
                ORDER BY call_id
                LIMIT ?
            "#;
            let rows = sqlx::query_as::<
                _,
                (
                    String,
                    chrono::NaiveDateTime,
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<serde_json::Value>,
                    Option<serde_json::Value>,
                    Option<serde_json::Value>,
                    Option<String>,
                    f64,
                    i32,
                    i32,
                    i32,
                    chrono::NaiveDateTime,
                    Option<String>,
                ),
            >(mysql_query)
            .bind(hour)
            .bind(limit as i32)
            .fetch_all(pool)
            .await?;
            Ok(rows
                .into_iter()
                .map(
                    |(rid, st, m, s, ch, sid, msg, resp, psr, req_id, sp, tt, pt, ct, et, mg)| {
                        BodyRow {
                            call_id: rid,
                            start_time: st.to_string(),
                            model: m,
                            status: s,
                            cache_hit: ch,
                            session_id: sid,
                            messages: msg.map(|v| v.to_string()),
                            response: resp.map(|v| v.to_string()),
                            proxy_server_request: psr.map(|v| v.to_string()),
                            request_id: req_id,
                            spend: sp,
                            total_tokens: tt,
                            prompt_tokens: pt,
                            completion_tokens: ct,
                            end_time: et.to_string(),
                            model_group: mg,
                        }
                    },
                )
                .collect())
        }
        Database::Postgres(pool) => {
            let pg_query = r#"
                SELECT call_id,
                       to_char(start_time, 'YYYY-MM-DD"T"HH24:MI:SS') as start_time,
                       model, status, cache_hit, session_id,
                       messages::text, response::text, proxy_server_request::text,
                       request_id, spend, total_tokens, prompt_tokens,
                       completion_tokens,
                       to_char(end_time, 'YYYY-MM-DD"T"HH24:MI:SS') as end_time,
                       model_group
                FROM spend_logs
                WHERE body_archived = FALSE
                  AND messages IS NOT NULL
                  AND to_char(start_time, 'YYYY-MM-DD"T"HH24') = $1
                  ORDER BY call_id
                  LIMIT $2
            "#;
            let rows = sqlx::query_as::<
                _,
                (
                    String,
                    String,
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    f64,
                    i32,
                    i32,
                    i32,
                    String,
                    Option<String>,
                ),
            >(pg_query)
            .bind(hour)
            .bind(limit as i64)
            .fetch_all(pool)
            .await?;
            Ok(rows
                .into_iter()
                .map(
                    |(rid, st, m, s, ch, sid, msg, resp, psr, req_id, sp, tt, pt, ct, et, mg)| {
                        BodyRow {
                            call_id: rid,
                            start_time: st,
                            model: m,
                            status: s,
                            cache_hit: ch,
                            session_id: sid,
                            messages: msg,
                            response: resp,
                            proxy_server_request: psr,
                            request_id: req_id,
                            spend: sp,
                            total_tokens: tt,
                            prompt_tokens: pt,
                            completion_tokens: ct,
                            end_time: et,
                            model_group: mg,
                        }
                    },
                )
                .collect())
        }
    }
}

async fn mark_rows_archived(db: &Database, call_ids: &[String], path: &str) -> Result<()> {
    if call_ids.is_empty() {
        return Ok(());
    }

    // Build placeholders for IN clause
    let placeholders = call_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "UPDATE spend_logs SET body_archived = TRUE, parquet_path = ? WHERE call_id IN ({})",
        placeholders
    );

    match db {
        Database::Sqlite(pool) => {
            let mut query = sqlx::query(&sql).bind(path);
            for id in call_ids {
                query = query.bind(id);
            }
            query.execute(pool).await?;
        }
        Database::Mysql(pool) => {
            let mut query = sqlx::query(&sql).bind(path);
            for id in call_ids {
                query = query.bind(id);
            }
            query.execute(pool).await?;
        }
        Database::Postgres(pool) => {
            let pg_placeholders: Vec<String> = (2..=call_ids.len() + 1)
                .map(|i| format!("${}", i))
                .collect();
            let pg_sql = format!(
                "UPDATE spend_logs SET body_archived = TRUE, parquet_path = $1 WHERE call_id IN ({})",
                pg_placeholders.join(",")
            );
            let mut query = sqlx::query(&pg_sql).bind(path);
            for id in call_ids {
                query = query.bind(id);
            }
            query.execute(pool).await?;
        }
    }

    Ok(())
}

async fn null_expired_bodies(db: &Database, cutoff: &str) -> Result<u64> {
    let rows_affected = match db {
        Database::Sqlite(pool) => sqlx::query(
            "UPDATE spend_logs SET messages = NULL, response = NULL, proxy_server_request = NULL
                 WHERE body_archived = TRUE AND start_time < ? AND messages IS NOT NULL",
        )
        .bind(cutoff)
        .execute(pool)
        .await?
        .rows_affected(),
        Database::Mysql(pool) => sqlx::query(
            "UPDATE spend_logs SET messages = NULL, response = NULL, proxy_server_request = NULL
                 WHERE body_archived = TRUE AND start_time < ? AND messages IS NOT NULL",
        )
        .bind(cutoff)
        .execute(pool)
        .await?
        .rows_affected(),
        Database::Postgres(pool) => sqlx::query(
            "UPDATE spend_logs SET messages = NULL, response = NULL, proxy_server_request = NULL
                 WHERE body_archived = TRUE AND start_time < $1 AND messages IS NOT NULL",
        )
        .bind(cutoff)
        .execute(pool)
        .await?
        .rows_affected(),
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
