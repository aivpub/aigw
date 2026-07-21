//! Native DB pools — connect, read source rows, write to target.
//!
//! Each native pool (PgPool / SqlitePool / MySqlPool) decodes its own
//! column types correctly. We unify them into Vec<(col_name, Value)>
//! so migration logic works on neutral representation, then convert back
//! to target DB types on write.

use futures::stream::{BoxStream, StreamExt};
use serde_json::{json, Value};
use sqlx::mysql::MySqlPoolOptions;
use sqlx::postgres::PgPoolOptions;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Column, MySqlPool, PgPool, Row, SqlitePool};
use std::collections::HashMap;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A row from any source DB, uniformly represented as (column_name, JSON Value).
pub type UnifiedRow = Vec<(String, Value)>;

/// Cursor range for paginated spend_logs reads.
/// Uses `startTime` as anchor — the litellm source table has `@@index([startTime])`.
/// Same-second overlap is harmless because target inserts are idempotent on `request_id`.
#[derive(Debug, Clone, Default)]
pub struct CursorRange {
    /// ISO 8601 datetime. `WHERE startTime >= resume_after`.
    pub resume_after: Option<String>,
    /// ISO 8601 datetime. `WHERE startTime < end_before`.
    pub end_before: Option<String>,
}

/// Which database we're connected to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbKind {
    Postgres,
    Sqlite,
    Mysql,
}

impl DbKind {
    pub fn from_url(url: &str) -> Self {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            DbKind::Postgres
        } else if url.starts_with("mysql://") || url.starts_with("mariadb://") {
            DbKind::Mysql
        } else {
            DbKind::Sqlite
        }
    }
}

/// A concrete connection pool to a source database.
pub enum SourcePool {
    Postgres(PgPool),
    Sqlite(SqlitePool),
    Mysql(MySqlPool),
}

impl SourcePool {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        sqlx::any::install_default_drivers();
        match DbKind::from_url(url) {
            DbKind::Postgres => {
                let pool = PgPoolOptions::new().max_connections(5).connect(url).await?;
                Ok(SourcePool::Postgres(pool))
            }
            DbKind::Sqlite => {
                // Normalize: collapse sqlite://// → sqlite:// (2 slashes).
                // sqlx SqliteConnectOptions treats sqlite:///abs/path as absolute,
                // and sqlite:///abs/path is the standard form. Any 4-slash variant
                // must be collapsed to avoid divergent parsing between drivers.
                let normalized = url.replacen("sqlite:////", "sqlite://", 1);
                let pool = if normalized.starts_with("sqlite:") || normalized.contains("://") {
                    SqlitePoolOptions::new().max_connections(5).connect(&normalized).await?
                } else {
                    let sqlite_url = format!("sqlite://{}", normalized);
                    SqlitePoolOptions::new().max_connections(5).connect(&sqlite_url).await?
                };
                Ok(SourcePool::Sqlite(pool))
            }
            DbKind::Mysql => {
                let pool = MySqlPoolOptions::new().max_connections(5).connect(url).await?;
                Ok(SourcePool::Mysql(pool))
            }
        }
    }

    pub fn kind(&self) -> DbKind {
        match self {
            SourcePool::Postgres(_) => DbKind::Postgres,
            SourcePool::Sqlite(_) => DbKind::Sqlite,
            SourcePool::Mysql(_) => DbKind::Mysql,
        }
    }

    /// Quote an identifier for this database.
    pub fn quote_ident(&self, name: &str) -> String {
        match self.kind() {
            DbKind::Mysql => format!("`{}`", name),
            _ => format!("\"{}\"", name),
        }
    }

    /// Get the INSERT conflict clause for idempotent inserts.
    pub fn conflict_clause(&self) -> &'static str {
        match self.kind() {
            DbKind::Postgres => " ON CONFLICT DO NOTHING",
            _ => "",
        }
    }

    /// Get the INSERT prefix (OR IGNORE for sqlite, IGNORE for mysql).
    pub fn insert_prefix(&self) -> &'static str {
        match self.kind() {
            DbKind::Postgres => "INSERT INTO",
            DbKind::Sqlite => "INSERT OR IGNORE INTO",
            DbKind::Mysql => "INSERT IGNORE INTO",
        }
    }

    /// Count rows in a table.
    pub async fn count_rows(&self, table: &str) -> anyhow::Result<i64> {
        let quoted = self.quote_ident(table);
        let sql = format!("SELECT COUNT(*) FROM {}", quoted);
        let count: i64 = match self {
            SourcePool::Postgres(p) => {
                sqlx::query_scalar(&sql).fetch_one(p).await?
            }
            SourcePool::Sqlite(p) => {
                sqlx::query_scalar(&sql).fetch_one(p).await?
            }
            SourcePool::Mysql(p) => {
                sqlx::query_scalar(&sql).fetch_one(p).await?
            }
        };
        Ok(count)
    }

    /// Execute raw SQL (no parameters). Returns affected rows.
    pub async fn execute_raw(&self, sql: &str) -> anyhow::Result<u64> {
        let rows = match self {
            SourcePool::Postgres(p) => sqlx::query(sql).execute(p).await?.rows_affected(),
            SourcePool::Sqlite(p) => sqlx::query(sql).execute(p).await?.rows_affected(),
            SourcePool::Mysql(p) => sqlx::query(sql).execute(p).await?.rows_affected(),
        };
        Ok(rows)
    }

    /// Read a single optional value from the source (used for config extraction).
    ///
    /// Callers pass PG SQL with `::text` casts when the underlying column may be
    /// `jsonb` (see `extract_source_master_key` in `remote_import.rs`) — this
    /// keeps decoding uniform as `String` across TEXT and JSONB.
    pub async fn query_scalar_string(&self, sql: &str) -> anyhow::Result<Option<String>> {
        let opt: Option<String> = match self {
            SourcePool::Postgres(p) => {
                sqlx::query_scalar(sql).fetch_optional(p).await?
            }
            SourcePool::Sqlite(p) => {
                sqlx::query_scalar(sql).fetch_optional(p).await?
            }
            SourcePool::Mysql(p) => {
                sqlx::query_scalar(sql).fetch_optional(p).await?
            }
        };
        Ok(opt)
    }

    /// Read all rows from a table, converting each column to serde_json::Value.
    ///
    /// Native drivers handle their own types:
    ///   PG:  JSONB→Value, BOOLEAN→bool, DOUBLE PRECISION→f64, TIMESTAMPTZ→String
    ///   SQLite: BLOB→Vec<u8>→parse JSON, INTEGER→i64, REAL→f64, DATETIME→String
    ///   MySQL: JSON→Value, TINYINT(1)→i64, DOUBLE→f64, DATETIME→String
    pub async fn read_rows(&self, table: &str) -> anyhow::Result<Vec<UnifiedRow>> {
        self.read_rows_with_limit(table, None).await
    }

    /// Read rows with an optional SQL LIMIT clause.
    /// When `limit` is Some(N), appends `LIMIT N` to avoid full table scans.
    pub async fn read_rows_with_limit(&self, table: &str, limit: Option<usize>) -> anyhow::Result<Vec<UnifiedRow>> {
        let quoted = self.quote_ident(table);
        let sql = if let Some(n) = limit {
            format!("SELECT * FROM {} LIMIT {}", quoted, n)
        } else {
            format!("SELECT * FROM {}", quoted)
        };

        match self {
            SourcePool::Postgres(p) => read_pg_rows(p, &sql).await,
            SourcePool::Sqlite(p) => read_sqlite_rows(p, &sql).await,
            SourcePool::Mysql(p) => read_mysql_rows(p, &sql).await,
        }
    }

    /// Read spend_logs with `startTime`-based cursor pagination.
    ///
    /// Generates SQL like:
    /// ```sql
    /// SELECT * FROM "LiteLLM_SpendLogs"
    ///   WHERE "startTime" >= '2026-07-15 10:30:00'
    ///     AND "startTime" < '2026-08-01 00:00:00'
    ///   ORDER BY "startTime" ASC
    ///   LIMIT 10000
    /// ```
    ///
    /// The source litellm table has `@@index([startTime])` so both the WHERE
    /// filter and ORDER BY hit the index.
    ///
    /// Retained for BDD / integration tests that want a simple "read
    /// everything into memory" cursor path.  Production migrations use
    /// [`stream_rows_with_cursor`] instead.
    #[allow(dead_code)]
    pub async fn read_rows_with_cursor(
        &self,
        table: &str,
        cursor: &CursorRange,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<UnifiedRow>> {
        let sql = self.build_cursor_sql(table, cursor, limit, None);
        match self {
            SourcePool::Postgres(p) => read_pg_rows(p, &sql).await,
            SourcePool::Sqlite(p) => read_sqlite_rows(p, &sql).await,
            SourcePool::Mysql(p) => read_mysql_rows(p, &sql).await,
        }
    }

    /// Build the cursor SQL with an optional column projection.
    ///
    /// `select_columns` supplies the *source-side* column names (upstream
    /// litellm uses camelCase like `startTime`; the migration layer maps
    /// them to target snake_case).  Callers that want to skip large columns
    /// (e.g. `--skip-body`) should exclude those column names here so the
    /// SELECT never reads them from disk / ships them over the network.
    pub fn build_cursor_sql(
        &self,
        table: &str,
        cursor: &CursorRange,
        limit: Option<usize>,
        select_columns: Option<&[String]>,
    ) -> String {
        let quoted = self.quote_ident(table);
        let projection = match select_columns {
            Some(cols) if !cols.is_empty() => cols
                .iter()
                .map(|c| self.quote_ident(c))
                .collect::<Vec<_>>()
                .join(", "),
            _ => "*".to_string(),
        };
        let mut parts = vec![format!("SELECT {} FROM {}", projection, quoted)];

        let mut conditions: Vec<String> = Vec::new();
        if let Some(ref t) = cursor.resume_after {
            let lit = self.time_literal(t);
            conditions.push(format!("\"startTime\" >= {}", lit));
        }
        if let Some(ref end) = cursor.end_before {
            let lit = self.time_literal(end);
            conditions.push(format!("\"startTime\" < {}", lit));
        }
        if !conditions.is_empty() {
            parts.push(format!("WHERE {}", conditions.join(" AND ")));
        }
        parts.push("ORDER BY \"startTime\" ASC".to_string());
        if let Some(n) = limit {
            parts.push(format!("LIMIT {}", n));
        }
        parts.join(" ")
    }


    /// Stream rows from a paginated cursor query (used by pipelined migrations).
    ///
    /// Producer-side of the pipeline: yields one `UnifiedRow` at a time driven
    /// by the driver's server-side cursor.  Callers typically forward these
    /// into a bounded channel and feed a target-side consumer that batches
    /// INSERTs inside a transaction — see `migrate_spend_logs`.
    ///
    /// See `build_cursor_sql` for the semantics of `select_columns`.
    pub fn stream_rows_with_cursor<'a>(
        &'a self,
        table: &'a str,
        cursor: &CursorRange,
        limit: Option<usize>,
        select_columns: Option<&'a [String]>,
        batch_size: usize,
    ) -> BoxStream<'a, anyhow::Result<UnifiedRow>> {
        match self {
            SourcePool::Postgres(p) => {
                stream_pg_rows_keyset(p, table, cursor.clone(), limit, select_columns, batch_size)
            }
            SourcePool::Sqlite(p) => {
                let sql = self.build_cursor_sql(table, cursor, limit, select_columns);
                stream_sqlite_rows(p, sql, batch_size)
            }
            SourcePool::Mysql(p) => {
                let sql = self.build_cursor_sql(table, cursor, limit, select_columns);
                stream_mysql_rows(p, sql, batch_size)
            }
        }
    }

    /// Convert an ISO 8601 datetime string to a SQL literal accepted by this DB.
    ///
    /// | DB    | Output                                       |
    /// |-------|----------------------------------------------|
    /// | PG    | `'2026-07-15 10:30:00+00'::timestamptz`     |
    /// | MySQL | `'2026-07-15 10:30:00'`                     |
    /// | SQLite| `'2026-07-15 10:30:00'`                     |
    pub fn time_literal(&self, iso8601: &str) -> String {
        // Accept both "2026-07-15T10:30:00Z" and "2026-07-15 10:30:00"
        let normalized = iso8601.replace('T', " ").replace('Z', "");
        match self.kind() {
            DbKind::Postgres => {
                format!("'{}'::timestamptz", normalized)
            }
            _ => {
                format!("'{}'", normalized)
            }
        }
    }

    /// Get target column names and types for INSERT generation.
    pub async fn column_types(&self, table: &str) -> anyhow::Result<Vec<(String, String, bool)>> {
        match self {
            SourcePool::Postgres(p) => pg_column_types(p, table).await,
            SourcePool::Sqlite(p) => sqlite_column_types(p, table).await,
            SourcePool::Mysql(p) => mysql_column_types(p, table).await,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// PG reader
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn read_pg_rows(pool: &PgPool, sql: &str) -> anyhow::Result<Vec<UnifiedRow>> {
    let rows = sqlx::query(sql).fetch_all(pool).await?;
    let mut result = Vec::with_capacity(rows.len());
    for row in &rows {
        let cols = row.columns();
        let mut unified = Vec::with_capacity(cols.len());
        for col in cols {
            let name = col.name().to_string();
            let val = try_pg_get(row, &name);
            unified.push((name, val));
        }
        result.push(unified);
    }
    Ok(result)
}

fn try_pg_get(row: &sqlx::postgres::PgRow, col: &str) -> Value {
    // ── String BEFORE Value ──────────────────────────────────
    // PG JSONB columns arrive as wire-protocol text.  Decoding as
    // `serde_json::Value` builds an entire object tree (128 KB × 800k =
    // 10+ GB of transient allocations per batch), while `String` is a
    // single allocation that round-trips cleanly through PG→PG migration
    // without any JSON manipulation.
    if let Ok(v) = row.try_get::<String, _>(col) {
        if v.is_empty() { return Value::Null; }
        return Value::String(v);
    }
    if let Ok(v) = row.try_get::<Value, _>(col) { return v; }
    if let Ok(v) = row.try_get::<bool, _>(col) { return Value::Bool(v); }
    if let Ok(v) = row.try_get::<f64, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<f32, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<i64, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<i32, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<chrono::DateTime<chrono::Utc>, _>(col) {
        return Value::String(v.to_rfc3339());
    }
    if let Ok(v) = row.try_get::<chrono::NaiveDateTime, _>(col) {
        return Value::String(
            chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(v, chrono::Utc)
                .to_rfc3339(),
        );
    }
    if let Ok(v) = row.try_get::<chrono::NaiveDate, _>(col) {
        return Value::String(v.format("%Y-%m-%d").to_string());
    }
    Value::Null
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SQLite reader — BLOB → parse JSON, INTEGER → i64
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn read_sqlite_rows(pool: &SqlitePool, sql: &str) -> anyhow::Result<Vec<UnifiedRow>> {
    let rows = sqlx::query(sql).fetch_all(pool).await?;
    let mut result = Vec::with_capacity(rows.len());
    for row in &rows {
        let cols = row.columns();
        let mut unified = Vec::with_capacity(cols.len());
        for col in cols {
            let name = col.name().to_string();
            let val = try_sqlite_get(row, &name);
            unified.push((name, val));
        }
        result.push(unified);
    }
    Ok(result)
}

fn try_sqlite_get(row: &sqlx::sqlite::SqliteRow, col: &str) -> Value {
    // BLOB: try parse as UTF-8 JSON, fallback to base64-like hex
    if let Ok(v) = row.try_get::<Vec<u8>, _>(col) {
        return match String::from_utf8(v) {
            Ok(s) if !s.is_empty() => {
                serde_json::from_str(&s).unwrap_or(Value::String(s))
            }
            _ => Value::Null,
        };
    }
    if let Ok(v) = row.try_get::<f64, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<f32, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<i64, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<i32, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<String, _>(col) {
        if v.is_empty() { return Value::Null; }
        // SQLite TEXT column — try parse as JSON (covers the case where
        // a value was written as text but the column is logically JSON/BLOB)
        if let Ok(json_val) = serde_json::from_str(&v) {
            return json_val;
        }
        return Value::String(v);
    }
    Value::Null
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// MySQL reader
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn read_mysql_rows(pool: &MySqlPool, sql: &str) -> anyhow::Result<Vec<UnifiedRow>> {
    let rows = sqlx::query(sql).fetch_all(pool).await?;
    let mut result = Vec::with_capacity(rows.len());
    for row in &rows {
        let cols = row.columns();
        let mut unified = Vec::with_capacity(cols.len());
        for col in cols {
            let name = col.name().to_string();
            let val = try_mysql_get(row, &name);
            unified.push((name, val));
        }
        result.push(unified);
    }
    Ok(result)
}


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Row streaming (for pipelined migrations — see migrate_spend_logs)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Stream rows from a PG source using keyset pagination on `(startTime, request_id)`.
///
/// Each iteration issues a standalone query with bind parameters ($1, $2, $3),
/// which forces the Extended Query Protocol — PG streams rows one at a time
/// without server-side materialisation.  After the batch is consumed the
/// keyset anchor is advanced to the last row seen, so the next iteration
/// picks up where the previous one left off.
///
/// This replaces the DECLARE/FETCH cursor approach which materialised the
/// entire result set on the PG server (119 GB text → 16 GB RAM on a 2C6G
/// instance) and kept an open transaction for hours.
fn stream_pg_rows_keyset<'a>(
    pool: &'a PgPool,
    table: &'a str,
    cursor: CursorRange,
    limit: Option<usize>,
    select_columns: Option<&'a [String]>,
    batch_size: usize,
) -> BoxStream<'a, anyhow::Result<UnifiedRow>> {
    async_stream::try_stream! {
        // ── Build projection with ::text cast for JSONB columns ──────
        //
        // sqlx refuses to decode PG JSONB (OID 3802) as String — even with
        // the "json" feature — because its type-check compares OIDs and only
        // allows TEXT (25) / VARCHAR (1043) → String.  By casting on the PG
        // side we make every JSONB column arrive as wire-protocol text,
        // bypassing the serde_json::Value object-tree decode entirely.
        let src_col_info: Vec<(String, String, bool)> = sqlx::query_as(
            "SELECT column_name::text, \
                    CASE WHEN data_type = 'ARRAY' THEN udt_name ELSE data_type END::text, \
                    is_nullable::text \
             FROM information_schema.columns \
             WHERE lower(table_name) = lower($1) \
             ORDER BY ordinal_position",
        )
        .bind(table)
        .fetch_all(pool)
        .await?;

        // Set of jsonb column names for fast projection lookup.
        let jsonb_cols: std::collections::HashSet<&str> = src_col_info
            .iter()
            .filter(|(_, ty, _)| ty == "jsonb")
            .map(|(n, _, _)| n.as_str())
            .collect();

        // Build the projection once — it never changes across batches.
        let projection: String = match select_columns {
            Some(cols) if !cols.is_empty() => cols
                .iter()
                .map(|c| {
                    let quoted = format!("\"{}\"", c);
                    if jsonb_cols.contains(c.as_str()) {
                        format!("{}::text", quoted)
                    } else {
                        quoted
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
            _ => {
                // SELECT * — build explicit projection with ::text casts.
                let col_names: Vec<&str> = src_col_info.iter().map(|(n, _, _)| n.as_str()).collect();
                col_names
                    .iter()
                    .map(|c| {
                        let quoted = format!("\"{}\"", c);
                        if jsonb_cols.contains(c) {
                            format!("{}::text", quoted)
                        } else {
                            quoted
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        };
        let quoted_table = format!("\"{}\"", table);
        let end_before_lit = cursor.end_before.as_ref().map(|t| format!("'{}'::timestamptz", t.replace('T', " ").replace('Z', "")));

        // Anchor: (last_start_time, last_request_id).
        // Initial value comes from --spend-log-resume-after, defaulting to epoch.
        let mut anchor_time: String = cursor.resume_after
            .map(|t| t.replace('T', " ").replace('Z', ""))
            .unwrap_or_else(|| "1970-01-01 00:00:00".to_string());
        let mut anchor_id: String = String::new();

        // If the caller set a global limit, track how many rows remain.
        let mut remaining = limit;

        loop {
            // Build the keyset SQL with bind parameters.
            // Use expanded comparison instead of row-constructor so we can
            // explicitly cast $1 to timestamp (startTime is `timestamp
            // without time zone` in upstream litellm, not `timestamptz`).
            let mut sql = format!(
                "SELECT {} FROM {} WHERE (\"startTime\" > $1::timestamp OR (\"startTime\" = $1::timestamp AND \"request_id\" > $2))",
                projection, quoted_table,
            );
            if let Some(ref end) = end_before_lit {
                sql.push_str(&format!(" AND \"startTime\" < {}", end));
            }
            sql.push_str(" ORDER BY \"startTime\" ASC, \"request_id\" ASC");

            // Clamp batch size to remaining limit when one is set.
            let limit_clause = match remaining {
                Some(rem) if (rem as usize) < batch_size => rem as usize,
                _ => batch_size,
            };
            sql.push_str(&format!(" LIMIT {}", limit_clause));

            // Bind parameters: $1=anchor_time, $2=anchor_id.
            let rows = sqlx::query(&sql)
                .bind(&anchor_time)
                .bind(&anchor_id)
                .fetch_all(pool)
                .await?;

            if rows.is_empty() {
                break;
            }

            // Track the last row for the next keyset anchor.
            for row in &rows {
                let cols = row.columns();
                let mut unified = Vec::with_capacity(cols.len());
                for col in cols {
                    let name = col.name().to_string();
                    let val = try_pg_get(row, &name);
                    if name == "startTime" {
                        if let Value::String(ref s) = val {
                            anchor_time = s.replace('T', " ").replace('Z', "");
                        }
                    }
                    if name == "request_id" {
                        if let Value::String(ref s) = val {
                            anchor_id.clone_from(s);
                        }
                    }
                    unified.push((name, val));
                }
                yield unified;
            }

            // Decrement remaining if a global limit was set.
            if let Some(ref mut rem) = remaining {
                let count = rows.len();
                if count >= *rem {
                    break;
                }
                *rem -= count;
            }

            // If this batch was smaller than requested, we've hit the end.
            if rows.len() < limit_clause {
                break;
            }
        }
    }
    .boxed()
}

/// Stream rows from a SQLite source one at a time.
fn stream_sqlite_rows<'a>(
    pool: &'a SqlitePool,
    sql: String,
    _batch_size: usize,
) -> BoxStream<'a, anyhow::Result<UnifiedRow>> {
    async_stream::try_stream! {
        let mut stream = sqlx::query(&sql).fetch(pool);
        while let Some(row_res) = stream.next().await {
            let row = row_res?;
            let cols = row.columns();
            let mut unified = Vec::with_capacity(cols.len());
            for col in cols {
                let name = col.name().to_string();
                let val = try_sqlite_get(&row, &name);
                unified.push((name, val));
            }
            yield unified;
        }
    }
    .boxed()
}

/// Stream rows from a MySQL source one at a time.
fn stream_mysql_rows<'a>(
    pool: &'a MySqlPool,
    sql: String,
    _batch_size: usize,
) -> BoxStream<'a, anyhow::Result<UnifiedRow>> {
    async_stream::try_stream! {
        let mut stream = sqlx::query(&sql).fetch(pool);
        while let Some(row_res) = stream.next().await {
            let row = row_res?;
            let cols = row.columns();
            let mut unified = Vec::with_capacity(cols.len());
            for col in cols {
                let name = col.name().to_string();
                let val = try_mysql_get(&row, &name);
                unified.push((name, val));
            }
            yield unified;
        }
    }
    .boxed()
}

fn try_mysql_get(row: &sqlx::mysql::MySqlRow, col: &str) -> Value {
    if let Ok(v) = row.try_get::<Value, _>(col) { return v; }
    if let Ok(v) = row.try_get::<bool, _>(col) { return Value::Bool(v); }
    if let Ok(v) = row.try_get::<f64, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<f32, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<i64, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<i32, _>(col) { return json!(v); }
    // MySQL datetime/timestamp columns don't decode straight into String.
    // Same treatment as PG: format uniformly as RFC 3339 UTC.
    if let Ok(v) = row.try_get::<chrono::DateTime<chrono::Utc>, _>(col) {
        return Value::String(v.to_rfc3339());
    }
    if let Ok(v) = row.try_get::<chrono::NaiveDateTime, _>(col) {
        return Value::String(
            chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(v, chrono::Utc)
                .to_rfc3339(),
        );
    }
    if let Ok(v) = row.try_get::<chrono::NaiveDate, _>(col) {
        return Value::String(v.format("%Y-%m-%d").to_string());
    }
    if let Ok(v) = row.try_get::<String, _>(col) {
        if v.is_empty() { return Value::Null; }
        return Value::String(v);
    }
    Value::Null
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Column type metadata readers (target DB)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn pg_column_types(pool: &PgPool, table: &str) -> anyhow::Result<Vec<(String, String, bool)>> {
    let rows = sqlx::query(
        "SELECT column_name::text, \
                CASE WHEN data_type = 'ARRAY' THEN udt_name ELSE data_type END::text, \
                is_nullable::text \
         FROM information_schema.columns \
         WHERE lower(table_name) = lower($1) \
         ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let nullable: String = r.get(2);
            (r.get::<String, _>(0), r.get::<String, _>(1), nullable == "YES")
        })
        .collect())
}

async fn sqlite_column_types(pool: &SqlitePool, table: &str) -> anyhow::Result<Vec<(String, String, bool)>> {
    let sql = format!("PRAGMA table_info(\"{}\")", table);
    let rows = sqlx::query(&sql).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .map(|r| {
            let name: String = r.get(1);
            let ty: String = r.try_get::<String, _>(2).unwrap_or_default();
            let notnull: bool = r.try_get::<i32, _>(3).unwrap_or(0) != 0;
            (name, ty, !notnull)
        })
        .collect())
}

async fn mysql_column_types(pool: &MySqlPool, table: &str) -> anyhow::Result<Vec<(String, String, bool)>> {
    let rows = sqlx::query(
        "SELECT COLUMN_NAME, DATA_TYPE, IS_NULLABLE \
         FROM INFORMATION_SCHEMA.COLUMNS \
         WHERE TABLE_NAME = ? AND TABLE_SCHEMA = DATABASE() \
         ORDER BY ORDINAL_POSITION",
    )
    .bind(table)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let name: String = r.get(0);
            let ty: String = r
                .try_get::<Vec<u8>, _>(1)
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .unwrap_or_default();
            let nullable: String = r
                .try_get::<Vec<u8>, _>(2)
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .unwrap_or_default();
            (name, normalize_mysql_type(&ty), nullable == "YES")
        })
        .collect())
}

/// Normalize MySQL type names to SQL-standard equivalents.
fn normalize_mysql_type(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if lower.starts_with("varchar") || lower.starts_with("char") || lower.starts_with("text")
        || lower.starts_with("longtext") || lower.starts_with("mediumtext")
    { "text".into() }
    else if lower.starts_with("int") || lower.starts_with("tinyint") { "integer".into() }
    else if lower.starts_with("bigint") { "bigint".into() }
    else if lower.starts_with("smallint") { "smallint".into() }
    else if lower.starts_with("float") { "real".into() }
    else if lower.starts_with("double") { "double precision".into() }
    else if lower.starts_with("decimal") || lower.starts_with("numeric") { "numeric".into() }
    else if lower.starts_with("datetime") || lower.starts_with("timestamp") { "timestamp".into() }
    else if lower == "json" { "json".into() }
    else if lower.starts_with("blob") || lower.starts_with("binary") { "blob".into() }
    else { "text".into() }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// MySQL JSON helper — hex-encoded literal to avoid escaping issues
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Encode a string as a MySQL hex-encoded JSON literal.
/// Bypasses all SQL-string-escaping problems — PG JSONB can contain
/// arbitrary characters that MySQL's JSON validator rejects when
/// passed via '…'-quoting.  `X'<hex>'` is a raw binary → `CAST(… AS JSON)`
/// feeds it directly to MySQL's JSON parser without shell-quoting.
fn mysql_json_hex_literal(s: &str) -> String {
    let hex: String = s.as_bytes().iter().map(|b| format!("{:02X}", b)).collect();
    format!("CAST(X'{}' AS JSON)", hex)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Value → target DB literal
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Convert a unified Value to a SQL literal string that the target DB accepts.
///
/// This is the core type coercion layer:
///   - JSON/JSONB: reject empty strings (→ NULL), wrap with ::jsonb for PG
///   - BOOLEAN: SQLite INTEGER(0/1) → PG true/false; PG bool → SQLite 1/0
///   - Numeric: native decode gives us f64/i64, direct string conversion
pub fn value_to_target_literal(v: &Value, col_type: &str, target: DbKind, is_nullable: bool) -> String {
    let ty = col_type.to_lowercase();
    // Determine if this PG column is an array type.
    // information_schema returns udt_name like `_text`, `_varchar` for array columns.
    let is_pg_array = || {
        target == DbKind::Postgres
            && (ty.ends_with("[]") || ty.starts_with("_") || ty == "array" || ty == "arary")
    };
    match v {
        Value::Null => {
            // If the target column is nullable, NULL is the correct value.
            if is_nullable {
                return "NULL".to_string();
            }
            // For NOT NULL columns, provide a type-appropriate default
            // instead of NULL — otherwise INSERT OR IGNORE (SQLite)
            // silently drops the entire row.
            match target {
                DbKind::Postgres if !ty.is_empty() => match ty.as_str() {
                    "boolean" | "bool" => "false".to_string(),
                    "integer" | "int" | "int4" | "smallint" | "int2" | "bigint" | "int8" => "0".to_string(),
                    "jsonb" | "json" => "'{}'::jsonb".to_string(),
                    "timestamp with time zone" | "timestamptz" => "'1970-01-01 00:00:00+00'::timestamptz".to_string(),
                    "timestamp without time zone" | "timestamp" => "'1970-01-01 00:00:00'::timestamp".to_string(),
                    "double precision" | "float8" | "real" | "float4" | "numeric" | "decimal" => "0".to_string(),
                    _ if ty.ends_with("[]") || ty.starts_with("_") => "'{}'".to_string(),
                    "text" | "character varying" | "varchar" => "''".to_string(),
                    // Any time/date type not matched above (e.g. "timestamp with local time zone")
                    _ if ty.contains("time") => "'1970-01-01 00:00:00+00'::timestamptz".to_string(),
                    _ if ty.contains("date") => "'1970-01-01'::date".to_string(),
                    _ => "''".to_string(),
                },
                // PG fallback: safe default for unknown types instead of NULL
                DbKind::Postgres => "''".to_string(),
                // MySQL: safe fallback for unknown types
                DbKind::Mysql if !ty.is_empty() => match ty.as_str() {
                    "json" | "blob" | "binary" | "varbinary" => "'{}'".to_string(),
                    "integer" | "int" | "bigint" | "smallint" | "tinyint" | "float" | "double" | "decimal" | "numeric" | "real" => "0".to_string(),
                    "timestamp" | "datetime" => "'1970-01-01 00:00:00'".to_string(),
                    "date" => "'1970-01-01'".to_string(),
                    "time" => "'00:00:00'".to_string(),
                    _ => "''".to_string(),
                },
                DbKind::Mysql => "''".to_string(),
                // SQLite INSERT OR IGNORE silently drops rows with NULL in
                // NOT NULL columns — provide safe defaults so rows land.
                DbKind::Sqlite if !ty.is_empty() => match ty.as_str() {
                    "blob" | "binary" | "varbinary" => "'{}'".to_string(),
                    "integer" | "int" | "bigint" | "smallint" | "tinyint" => "0".to_string(),
                    "real" | "float" | "double" | "numeric" | "decimal" | "number" => "0".to_string(),
                    // FK columns: SQLite allows NULL in FK columns, so keep NULL.
                    // TEXT / DATETIME / empty-type columns: empty string is safe.
                    _ => "''".to_string(),
                },
                _ => "NULL".to_string(),
            }
        }

        Value::Bool(b) => {
            match target {
                DbKind::Postgres => {
                    if ty == "boolean" || ty == "bool" {
                        if *b { "true".into() } else { "false".into() }
                    } else if ty.contains("int") || ty == "smallint" || ty == "bigint" {
                        (if *b { "1" } else { "0" }).into()
                    } else if ty.contains("timestamp") || ty.contains("date") || ty.contains("time") {
                        // SQLite stores "false"/"true" as BLOB→JSON→Bool →
                        // won't convert to timestamp.  Use NULL.
                        "NULL".to_string()
                    } else {
                        format!("'{}'", if *b { "true" } else { "false" })
                    }
                }
                DbKind::Sqlite => {
                    // SQLite has no bool — always use INTEGER
                    (if *b { "1" } else { "0" }).into()
                }
                DbKind::Mysql => {
                    if ty.contains("int") || ty == "tinyint" || ty.contains("bool") {
                        (if *b { "1" } else { "0" }).into()
                    } else if ty.contains("timestamp") || ty.contains("date") || ty.contains("time") || ty.contains("datetime") {
                        "NULL".to_string()
                    } else {
                        format!("'{}'", if *b { "true" } else { "false" })
                    }
                }
            }
        }

        Value::Number(n) => {
            match target {
                DbKind::Postgres => {
                    if ty == "boolean" || ty == "bool" {
                        if n.as_i64().map(|i| i != 0).unwrap_or(false) { "true".into() } else { "false".into() }
                    } else if ty.contains("int") || ty == "smallint" || ty == "bigint" {
                        n.as_i64().map(|i| i.to_string()).unwrap_or_else(|| n.to_string())
                    } else if ty.contains("double") || ty.contains("real") || ty.contains("float") || ty.contains("numeric") || ty.contains("decimal") {
                        n.as_f64().map(|f| f.to_string()).unwrap_or_else(|| n.to_string())
                    } else {
                        n.to_string()
                    }
                }
                DbKind::Sqlite => {
                    // SQLite is flexible — numbers are fine as-is
                    n.to_string()
                }
                DbKind::Mysql => {
                    if ty.contains("json") {
                        // JSON column shouldn't get a bare number
                        format!("'{}'", n)
                    } else {
                        n.to_string()
                    }
                }
            }
        }

        Value::String(s) => {
            if s.is_empty() {
                // Empty string → type-appropriate default instead of NULL.
                // NULL causes INSERT OR IGNORE to drop rows (SQLite) and NOT NULL
                // violations (PG). Use the same defaults as the Value::Null arm.
                match target {
                    DbKind::Postgres if !ty.is_empty() => {
                        return match ty.as_str() {
                            "jsonb" | "json" => "'{}'::jsonb".to_string(),
                            "boolean" | "bool" => "false".to_string(),
                            "timestamp with time zone" | "timestamptz"
                                => "'1970-01-01 00:00:00+00'::timestamptz".to_string(),
                            "timestamp without time zone" | "timestamp"
                                => "'1970-01-01 00:00:00'::timestamp".to_string(),
                            "date" => "'1970-01-01'::date".to_string(),
                            "time without time zone" | "time" => "'00:00:00'::time".to_string(),
                            _ if ty.contains("time") || ty.contains("date") => "'1970-01-01 00:00:00+00'::timestamptz".to_string(),
                            _ if would_reject_empty_string(&ty, target) => "0".to_string(),
                            _ => "''".to_string(),
                        };
                    }
                    DbKind::Sqlite if !ty.is_empty() => {
                        return match ty.as_str() {
                            "blob" | "binary" | "varbinary" => "'{}'".to_string(),
                            "integer" | "int" | "bigint" | "smallint" | "tinyint" => "0".to_string(),
                            "real" | "float" | "double" | "numeric" | "decimal" | "number" => "0".to_string(),
                            _ => "''".to_string(),
                        };
                    }
                    _ => {
                        return match ty.as_str() {
                            "" => "''".to_string(),
                            _ if target == DbKind::Mysql && ty.contains("json") => "'{}'".to_string(),
                            _ if target == DbKind::Mysql && (ty.contains("int") || ty.contains("double") || ty.contains("float")) => "0".to_string(),
                            _ => "''".to_string(),
                        };
                    }
                }
            }

            match target {
                DbKind::Postgres => {
                    match ty.as_str() {
                        "jsonb" | "json" => {
                            // PG→PG migration: the source JSONB is already
                            // valid JSON text (try_pg_get returns raw String).
                            // Skip the serde_json::from_str() → to_string()
                            // round-trip — for 128KB proxy_server_request blobs
                            // this saves millions of allocs per batch.
                            //
                            // Health-check: if the text starts with '{' or '['
                            // it's good to go.  Opaque encrypted blobs
                            // (gAAAAAB…) will match neither and fall through to
                            // JSON-scalar wrapping, preserving credential_values
                            // / litellm_params rotation.
                            let trimmed = s.trim();
                            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                                let escaped = trimmed.replace('\'', "''");
                                format!("'{}'::jsonb", escaped)
                            } else {
                                let wrapped = serde_json::to_string(s)
                                    .unwrap_or_else(|_| "\"\"".to_string());
                                let escaped = wrapped.replace('\'', "''");
                                format!("'{}'::jsonb", escaped)
                            }
                        }
                        "boolean" | "bool" => {
                            match s.to_lowercase().as_str() {
                                "true" | "1" | "t" | "yes" | "y" => "true".into(),
                                _ => "false".into(),
                            }
                        }
                        _ => {
                            if is_pg_array() {
                                // Try parse JSON string as array, convert to PG array literal
                                match serde_json::from_str::<Value>(s) {
                                    Ok(v) if v.is_array() => value_to_pg_array_literal(&v),
                                    _ => format!("'{}'", s.replace('\'', "''")),
                                }
                            } else {
                                format!("'{}'", s.replace('\'', "''"))
                            }
                        }
                    }
                }
                DbKind::Sqlite => {
                    if ty.contains("json") || ty == "blob" || ty.contains("blob") {
                        // Store as JSON-compatible string: if the value is NOT
                        // valid JSON, wrap it in JSON double quotes so sqlx can
                        // decode it as serde_json::Value::String.  Otherwise
                        // bare encrypted strings like "v2:gcm:..." cause
                        // serde_json::from_str to fail when reading back,
                        // breaking /spend/providers decryption.
                        let escaped = if serde_json::from_str::<Value>(s).is_ok() {
                            s.replace('\'', "''")
                        } else {
                            // Wrap non-JSON strings as JSON strings
                            serde_json::to_string(s).unwrap_or_else(|_| s.to_string())
                                .replace('\'', "''")
                        };
                        format!("'{}'", escaped)
                    } else {
                        let escaped = s.replace('\'', "''");
                        format!("'{}'", escaped)
                    }
                }
                DbKind::Mysql => {
                    if ty == "json" || ty.contains("json") {
                        // MySQL strict JSON: validate AND normalize via serde_json
                        // round-trip to clean up PG-jsonb-specific quirks that
                        // MySQL's JSON validator rejects.  Use hex encoding to
                        // avoid any SQL-string-escaping problems.
                        match serde_json::from_str::<Value>(s) {
                            Ok(json_val) => {
                                mysql_json_hex_literal(&json_val.to_string())
                            }
                            Err(_) => "'{}'".to_string(),
                        }
                    } else {
                        let escaped = s.replace('\'', "''");
                        format!("'{}'", escaped)
                    }
                }
            }
        }

        Value::Array(_) | Value::Object(_) => {
            let json_str = v.to_string();
            // MySQL JSON is stricter than PG JSONB — validate before inserting.
            // Invalid JSON (e.g. PG jsonb with trailing garbage) → fall back to '{}'.
            if target == DbKind::Mysql && (ty == "json" || ty.contains("json"))
                && serde_json::from_str::<Value>(&json_str).is_err()
            {
                return "'{}'".to_string();
            }
            let escaped = json_str.replace('\'', "''");
            match target {
                DbKind::Postgres => {
                    if ty == "jsonb" || ty == "json" {
                        format!("'{}'::jsonb", escaped)
                    } else if is_pg_array() {
                        value_to_pg_array_literal(v)
                    } else {
                        format!("'{}'", escaped)
                    }
                }
                DbKind::Sqlite => {
                    format!("'{}'", escaped)
                }
                DbKind::Mysql => {
                    if ty == "json" || ty.contains("json") {
                        mysql_json_hex_literal(&json_str)
                    } else {
                        format!("'{}'", escaped)
                    }
                }
            }
        }
    }
}

/// Convert a JSON array value to a PostgreSQL array literal.
/// e.g. ["a","b"] → '{"a","b"}'
fn value_to_pg_array_literal(v: &Value) -> String {
    match v {
        Value::Array(arr) => {
            if arr.is_empty() {
                return "'{}'".to_string();
            }
            let elems: Vec<String> = arr.iter().map(|elem| {
                match elem {
                    Value::String(s) => format!("\"{}\"", s.replace('"', "\\\"")),
                    Value::Null => "NULL".to_string(),
                    other => format!("\"{}\"", other.to_string().replace('"', "\\\"")),
                }
            }).collect();
            format!("'{{{}}}'", elems.join(","))
        }
        Value::String(s) => {
            // JSON string from SQLite TEXT→String→try parse as JSON arr
            match serde_json::from_str::<Value>(s) {
                Ok(Value::Array(arr)) => {
                    value_to_pg_array_literal(&Value::Array(arr))
                }
                Ok(_) => {
                    // Non-array JSON (object, string, number) — wrap as single element
                    let escaped = s.replace('\'', "''");
                    format!("'{{{}}}'", escaped)
                }
                Err(_) => {
                    let escaped = s.replace('\'', "''");
                    format!("'{{{}}}'", escaped)
                }
            }
        }
        _ => {
            // Non-array value: wrap as single-element array literal
            let s = v.to_string();
            let escaped = s.replace('\'', "''");
            format!("'{{\"{}\"}}'", escaped.replace('"', "\\\""))
        }
    }
}

/// Would this target column type reject an empty string ''?
fn would_reject_empty_string(col_type: &str, target: DbKind) -> bool {
    let ty = col_type.to_lowercase();
    match target {
        DbKind::Postgres => {
            ty == "jsonb" || ty == "json"
                || ty.contains("int") || ty == "integer" || ty == "smallint" || ty == "bigint"
                || ty.contains("double") || ty.contains("real") || ty.contains("float")
                || ty.contains("numeric") || ty.contains("decimal")
                || ty == "boolean" || ty == "bool"
                || ty.contains("timestamp")
        }
        DbKind::Mysql => {
            ty == "json" || ty.contains("json")
        }
        DbKind::Sqlite => false, // SQLite is permissive
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Build INSERT + execute
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Insert a batch of rows into the target table, using column type coercion.
///
/// `target_cols`: target column names + types from column_types()
/// `rows`: unified rows read from the source
/// `column_override`: optional map to rename source columns (camelCase→snake_case)
pub async fn insert_rows(
    target: &SourcePool,
    table: &str,
    target_cols: &[(String, String, bool)],
    rows: &[UnifiedRow],
    column_override: &HashMap<String, String>,
) -> anyhow::Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }

    let target_kind = target.kind();
    let tbl_quoted = target.quote_ident(table);
    let conflict = target.conflict_clause();

    // Build column list for INSERT (target columns that exist in source)
    let col_names: Vec<&str> = target_cols.iter().map(|(n, _, _)| n.as_str()).collect();
    let quoted_cols: Vec<String> = col_names.iter().map(|n| target.quote_ident(n)).collect();

    let mut inserted = 0;
    // Debug-only counters: how many rows the DB ignored (unique/NOT-NULL/CHECK
    // violations under INSERT OR IGNORE / INSERT IGNORE / ON CONFLICT DO
    // NOTHING).  This surfaces silent drops that used to make rows vanish
    // without any error.
    let mut ignored = 0usize;
    let mut first_ignored_sql: Option<String> = None;
    for row in rows {
        let row_map: HashMap<&str, &Value> = row.iter().map(|(n, v)| (n.as_str(), v)).collect();

        let values: Vec<String> = target_cols
            .iter()
            .map(|(col_name, col_type, is_nullable)| {
                // Try source column name first, then override mapping
                let v = row_map
                    .get(col_name.as_str())
                    .or_else(|| {
                        column_override
                            .get(col_name.as_str())
                            .and_then(|mapped| row_map.get(mapped.as_str()))
                    })
                    .copied()
                    .unwrap_or(&Value::Null);
                value_to_target_literal(v, col_type, target_kind, *is_nullable)
            })
            .collect();

        let sql = format!(
            "{}{} ({}) VALUES ({}){}",
            target.insert_prefix(),
            tbl_quoted,
            quoted_cols.join(", "),
            values.join(", "),
            conflict,
        );

        let affected = target.execute_raw(&sql).await?;
        if affected == 0 {
            ignored += 1;
            if first_ignored_sql.is_none() {
                first_ignored_sql = Some(sql.clone());
            }
        }
        // INSERT OR IGNORE / ON CONFLICT DO NOTHING: count only actual inserts.
        inserted += affected as usize;
    }

    if ignored > 0 {
        eprintln!(
            "  [WARN] {}: {}/{} rows ignored by target (INSERT OR IGNORE / ON CONFLICT DO NOTHING)",
            table,
            ignored,
            rows.len()
        );
        if let Some(sql) = first_ignored_sql {
            let preview: String = sql.chars().take(500).collect();
            eprintln!("  [WARN] first ignored SQL preview: {}", preview);
        }
    }

    Ok(inserted)
}


/// Build the (comma-separated column list, comma-separated VALUES tuple)
/// for a single unified row, given the target column schema and any
/// source→target column-name overrides.
fn build_row_values(
    target_kind: DbKind,
    target_cols: &[(String, String, bool)],
    row: &UnifiedRow,
    column_override: &HashMap<String, String>,
) -> String {
    let row_map: HashMap<&str, &Value> = row.iter().map(|(n, v)| (n.as_str(), v)).collect();
    let values: Vec<String> = target_cols
        .iter()
        .map(|(col_name, col_type, is_nullable)| {
            let v = row_map
                .get(col_name.as_str())
                .or_else(|| {
                    column_override
                        .get(col_name.as_str())
                        .and_then(|mapped| row_map.get(mapped.as_str()))
                })
                .copied()
                .unwrap_or(&Value::Null);
            value_to_target_literal(v, col_type, target_kind, *is_nullable)
        })
        .collect();
    format!("({})", values.join(", "))
}

/// Insert a batch of rows inside a single transaction using ONE
/// multi-row INSERT statement.  Returns (rows_affected, ignored_rows).
///
/// `rows_affected` counts real inserts (target-side `rows_affected()`),
/// `ignored_rows = rows.len() - rows_affected` covers UNIQUE / NOT NULL
/// / CHECK collisions swallowed by INSERT OR IGNORE / ON CONFLICT DO NOTHING.
///
/// This is a strictly better shape than the row-at-a-time `insert_rows` for
/// pipelined workloads: one round trip per batch instead of `batch` round
/// trips, and a single implicit fsync per commit.
pub async fn insert_rows_batch(
    target: &SourcePool,
    table: &str,
    target_cols: &[(String, String, bool)],
    rows: &[UnifiedRow],
    column_override: &HashMap<String, String>,
) -> anyhow::Result<(usize, usize)> {
    if rows.is_empty() {
        return Ok((0, 0));
    }

    let target_kind = target.kind();
    let tbl_quoted = target.quote_ident(table);
    let conflict = target.conflict_clause();
    let quoted_cols: Vec<String> = target_cols
        .iter()
        .map(|(n, _, _)| target.quote_ident(n))
        .collect();

    let tuples: Vec<String> = rows
        .iter()
        .map(|row| build_row_values(target_kind, target_cols, row, column_override))
        .collect();

    let sql = format!(
        "{}{} ({}) VALUES {}{}",
        target.insert_prefix(),
        tbl_quoted,
        quoted_cols.join(", "),
        tuples.join(", "),
        conflict,
    );

    let affected: u64 = match target {
        SourcePool::Postgres(p) => {
            let mut tx = p.begin().await?;
            let r = sqlx::query(&sql).execute(&mut *tx).await?.rows_affected();
            tx.commit().await?;
            r
        }
        SourcePool::Sqlite(p) => {
            let mut tx = p.begin().await?;
            let r = sqlx::query(&sql).execute(&mut *tx).await?.rows_affected();
            tx.commit().await?;
            r
        }
        SourcePool::Mysql(p) => {
            let mut tx = p.begin().await?;
            let r = sqlx::query(&sql).execute(&mut *tx).await?.rows_affected();
            tx.commit().await?;
            r
        }
    };
    let inserted = affected as usize;
    let ignored = rows.len().saturating_sub(inserted);
    Ok((inserted, ignored))
}


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Unit tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    // ── value_to_target_literal — JSON coercion for encrypted opaque blobs ──
    //
    // The `credentials.credential_values` and `proxy_models.litellm_params`
    // columns hold values the runtime decodes as `serde_json::Value`.  When
    // upstream litellm stores them as a single encrypted string (`"gAAAAAB..."`),
    // the migration layer normalises it back to `Value::String(...)` and asks
    // `value_to_target_literal` to render a valid `jsonb` literal.  The
    // previous PG jsonb `Err(_)` branch dropped the value as `'{}'::jsonb`,
    // silently blanking credential_values in PG targets.

    #[test]
    fn pg_jsonb_wraps_encrypted_string_as_json_scalar() {
        // Typical NaCl-encrypted litellm value (base64-ish, no braces).
        let v = Value::String("gAAAAABmR1KzLmxk-notjson".to_string());
        let out = value_to_target_literal(&v, "jsonb", DbKind::Postgres, false);
        // Must NOT collapse to '{}'::jsonb — that's the bug we just fixed.
        assert_ne!(out, "'{}'::jsonb", "encrypted blob must not be lost");
        // Must be a valid jsonb scalar string literal so `sqlx` decodes it
        // back as `serde_json::Value::String(_)` at read time.
        assert_eq!(out, "'\"gAAAAABmR1KzLmxk-notjson\"'::jsonb");
    }

    #[test]
    fn pg_jsonb_passes_through_valid_json_object() {
        // Object payload — keys preserved in source order (no longer
        // sorted via serde_json round-trip).
        let v = Value::String(r#"{"api_key":"k","api_base":"http://x"}"#.to_string());
        let out = value_to_target_literal(&v, "jsonb", DbKind::Postgres, false);
        assert_eq!(out, "'{\"api_key\":\"k\",\"api_base\":\"http://x\"}'::jsonb");
    }

    #[test]
    fn pg_jsonb_empty_string_becomes_empty_object_default() {
        // Empty strings for a NOT-NULL jsonb column should still get a sane
        // default (the existing empty-string arm handles this — we're just
        // pinning the behaviour so it doesn't regress alongside the fix.).
        let v = Value::String(String::new());
        let out = value_to_target_literal(&v, "jsonb", DbKind::Postgres, false);
        assert_eq!(out, "'{}'::jsonb");
    }

    #[test]
    fn pg_jsonb_object_value_inlined() {
        // Value::Object should serialize inline as jsonb — separate arm from
        // the Value::String path, but sharing the same "must not be empty" rule.
        let v: Value = serde_json::json!({"k": "v"});
        let out = value_to_target_literal(&v, "jsonb", DbKind::Postgres, false);
        assert_eq!(out, "'{\"k\":\"v\"}'::jsonb");
    }

    #[test]
    fn sqlite_json_column_wraps_non_json_string() {
        // Parity check: the SQLite branch already wraps non-JSON strings as
        // JSON scalars, ensuring parity with the PG fix above.
        let v = Value::String("gAAAAABmR1KzLmxk".to_string());
        let out = value_to_target_literal(&v, "json", DbKind::Sqlite, false);
        assert_eq!(out, "'\"gAAAAABmR1KzLmxk\"'");
    }

    // ── nullable column handling ──
    //
    // Regression: `value_to_target_literal` used to unconditionally convert
    // `Value::Null` to type-appropriate defaults (e.g. epoch 0 for timestamps)
    // even for nullable columns, corrupting migration data.

    #[test]
    fn null_to_nullable_timestamptz_returns_null() {
        let v = Value::Null;
        let out = value_to_target_literal(&v, "timestamptz", DbKind::Postgres, true);
        assert_eq!(out, "NULL");
    }

    #[test]
    fn null_to_notnull_timestamptz_returns_epoch() {
        let v = Value::Null;
        let out = value_to_target_literal(&v, "timestamptz", DbKind::Postgres, false);
        assert_eq!(out, "'1970-01-01 00:00:00+00'::timestamptz");
    }

    #[test]
    fn null_to_nullable_text_returns_null() {
        let v = Value::Null;
        let out = value_to_target_literal(&v, "text", DbKind::Postgres, true);
        assert_eq!(out, "NULL");
    }

    #[test]
    fn null_to_notnull_text_returns_empty_string() {
        let v = Value::Null;
        let out = value_to_target_literal(&v, "text", DbKind::Postgres, false);
        assert_eq!(out, "''");
    }

    // ── build_cursor_sql — column projection ──
    //
    // Regression: `--skip-body` used to keep `SELECT *`, causing 1.6 GB of
    // response text to be read then discarded.  Ensuring the projection is
    // respected keeps that fix locked down.

    #[tokio::test]
    async fn build_cursor_sql_projects_selected_columns() {
        // In-memory sqlite is enough — build_cursor_sql is pure string work
        // that only depends on the pool's DbKind + identifier quoting.
        let pool = SourcePool::connect("sqlite::memory:").await.unwrap();
        let cursor = CursorRange {
            resume_after: Some("2026-01-01T00:00:00Z".into()),
            end_before: None,
        };
        let cols: Vec<String> = vec![
            "startTime".into(),
            "request_id".into(),
            "spend".into(),
        ];

        let sql = pool.build_cursor_sql("LiteLLM_SpendLogs", &cursor, Some(10), Some(&cols));

        assert!(
            sql.starts_with(
                "SELECT \"startTime\", \"request_id\", \"spend\" FROM \"LiteLLM_SpendLogs\""
            ),
            "projection must include exactly the requested columns in order; got: {sql}"
        );
        assert!(!sql.contains(" * "), "must not fall back to SELECT *; got: {sql}");
        assert!(sql.contains("ORDER BY \"startTime\" ASC"));
        assert!(sql.contains("LIMIT 10"));
        assert!(sql.contains("\"startTime\" >="));
    }

    #[tokio::test]
    async fn build_cursor_sql_defaults_to_star() {
        let pool = SourcePool::connect("sqlite::memory:").await.unwrap();
        let sql =
            pool.build_cursor_sql("LiteLLM_SpendLogs", &CursorRange::default(), None, None);
        assert!(sql.starts_with("SELECT * FROM \"LiteLLM_SpendLogs\""));
    }
}
