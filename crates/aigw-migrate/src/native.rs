//! Native DB pools — connect, read source rows, write to target.
//!
//! Each native pool (PgPool / SqlitePool / MySqlPool) decodes its own
//! column types correctly. We unify them into Vec<(col_name, Value)>
//! so migration logic works on neutral representation, then convert back
//! to target DB types on write.

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

    /// Get target column names and types for INSERT generation.
    pub async fn column_types(&self, table: &str) -> anyhow::Result<Vec<(String, String)>> {
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
    if let Ok(v) = row.try_get::<Value, _>(col) { return v; }
    if let Ok(v) = row.try_get::<bool, _>(col) { return Value::Bool(v); }
    if let Ok(v) = row.try_get::<f64, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<f32, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<i64, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<i32, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<String, _>(col) {
        if v.is_empty() { return Value::Null; }
        return Value::String(v);
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

fn try_mysql_get(row: &sqlx::mysql::MySqlRow, col: &str) -> Value {
    if let Ok(v) = row.try_get::<Value, _>(col) { return v; }
    if let Ok(v) = row.try_get::<bool, _>(col) { return Value::Bool(v); }
    if let Ok(v) = row.try_get::<f64, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<f32, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<i64, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<i32, _>(col) { return json!(v); }
    if let Ok(v) = row.try_get::<String, _>(col) {
        if v.is_empty() { return Value::Null; }
        return Value::String(v);
    }
    Value::Null
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Column type metadata readers (target DB)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn pg_column_types(pool: &PgPool, table: &str) -> anyhow::Result<Vec<(String, String)>> {
    let rows = sqlx::query(
        "SELECT column_name::text, \
                CASE WHEN data_type = 'ARRAY' THEN udt_name ELSE data_type END::text \
         FROM information_schema.columns \
         WHERE lower(table_name) = lower($1) \
         ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get::<String, _>(0), r.get::<String, _>(1)))
        .collect())
}

async fn sqlite_column_types(pool: &SqlitePool, table: &str) -> anyhow::Result<Vec<(String, String)>> {
    let sql = format!("PRAGMA table_info(\"{}\")", table);
    let rows = sqlx::query(&sql).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .map(|r| {
            let name: String = r.get(1);
            let ty: String = r.try_get::<String, _>(2).unwrap_or_default();
            (name, ty)
        })
        .collect())
}

async fn mysql_column_types(pool: &MySqlPool, table: &str) -> anyhow::Result<Vec<(String, String)>> {
    let rows = sqlx::query(
        "SELECT COLUMN_NAME, DATA_TYPE \
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
            (name, normalize_mysql_type(&ty))
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
pub fn value_to_target_literal(v: &Value, col_type: &str, target: DbKind) -> String {
    let ty = col_type.to_lowercase();
    // Determine if this PG column is an array type.
    // information_schema returns udt_name like `_text`, `_varchar` for array columns.
    let is_pg_array = || {
        target == DbKind::Postgres
            && (ty.ends_with("[]") || ty.starts_with("_") || ty == "array" || ty == "arary")
    };
    match v {
        Value::Null => {
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
                            // For JSON columns: try parse as JSON first
                            // (SQLite BLOB→JSON arrives as String of JSON text)
                            match serde_json::from_str::<Value>(s) {
                                Ok(json_val) => {
                                    let escaped = json_val.to_string().replace('\'', "''");
                                    format!("'{}'::jsonb", escaped)
                                }
                                Err(_) => {
                                    // Not valid JSON (e.g. empty blob, binary data) —
                                    // use empty JSON object instead of raw text
                                    "'{}'::jsonb".to_string()
                                }
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
    target_cols: &[(String, String)],
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
    let col_names: Vec<&str> = target_cols.iter().map(|(n, _)| n.as_str()).collect();
    let quoted_cols: Vec<String> = col_names.iter().map(|n| target.quote_ident(n)).collect();

    let mut inserted = 0;
    for row in rows {
        let row_map: HashMap<&str, &Value> = row.iter().map(|(n, v)| (n.as_str(), v)).collect();

        let values: Vec<String> = target_cols
            .iter()
            .map(|(col_name, col_type)| {
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
                value_to_target_literal(v, col_type, target_kind)
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
        // INSERT OR IGNORE / ON CONFLICT DO NOTHING: count only actual inserts.
        inserted += affected as usize;
    }

    Ok(inserted)
}
