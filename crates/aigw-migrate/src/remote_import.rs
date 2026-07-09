//! remote-import: Full litellm → aigw migration with encryption key rotation.
//!
//! Pipeline:
//!   1. Connect to source (litellm PG/MySQL) and target (aigw DB)
//!   2. Extract litellm master_key from LiteLLM_Config or CLI arg
//!   3. Migrate plain tables (no encrypted fields)
//!   4. Migrate credentials — decrypt credential_values, re-encrypt with aigw key
//!   5. Migrate proxy_models — decrypt litellm_params, re-encrypt with aigw key
//!   6. Batch migrate spend_logs (10k per batch)
//!
//! For PostgreSQL sources, we use native PgPool to avoid AnyPool's limited
//! support for PG-native types (TextArray, Jsonb, Timestamp, Name, etc.).

use serde_json::Value;
use sqlx::any::AnyPoolOptions;
use sqlx::Row as _;
use sqlx::{AnyPool, Column};

/// Generate database-appropriate placeholder for positional parameters.
/// sqlx::AnyPool doesn't reliably translate `?` for all backends (particularly PG),
/// so we explicitly use `$N` for PostgreSQL and `?` for everything else.
fn placeholder(i: usize, db_url: &str) -> String {
    if is_pg(db_url) {
        format!("${}", i + 1)
    } else {
        "?".to_string()
    }
}

/// Generate placeholders for `col_count` columns.
fn placeholders(col_count: usize, db_url: &str) -> Vec<String> {
    (0..col_count).map(|i| placeholder(i, db_url)).collect()
}

/// Generate database-appropriate conflict handling clause.
/// PostgreSQL uses `ON CONFLICT DO NOTHING`; SQLite/MySQL don't need it
/// Return the appropriate INSERT prefix for the target database,
/// including conflict handling (idempotent re-import).
///
/// PG: `INSERT INTO` + `ON CONFLICT DO NOTHING` suffix (handled per call site)
/// SQLite: `INSERT OR IGNORE INTO`
/// MySQL: `INSERT IGNORE INTO`
fn insert_prefix(db_url: &str) -> &'static str {
    if is_pg(db_url) {
        "INSERT INTO"
    } else if is_mysql(db_url) {
        "INSERT IGNORE INTO"
    } else {
        "INSERT OR IGNORE INTO"
    }
}

/// Check if a URL targets PostgreSQL.
fn is_pg(url: &str) -> bool {
    url.starts_with("postgres://") || url.starts_with("postgresql://")
}

/// Quote an SQL identifier for the given database URL.
/// PG/SQLite: double-quotes; MySQL: backticks.
fn quote_ident(name: &str, db_url: &str) -> String {
    if is_mysql(db_url) {
        format!("`{}`", name)
    } else {
        format!("\"{}\"", name)
    }
}

/// Connect to a database from either a file path (SQLite) or a URL (any DB).
async fn connect(source_or_url: &str) -> anyhow::Result<AnyPool> {
    if source_or_url.starts_with("sqlite:") || source_or_url.contains("://") {
        let pool = AnyPoolOptions::new()
            .max_connections(5)
            .connect(source_or_url)
            .await?;
        return Ok(pool);
    }
    let url = format!("sqlite://{}", source_or_url);
    let pool = AnyPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await?;
    Ok(pool)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// PG type-aware helpers — work around AnyPool's inability to decode
// PostgreSQL native types (jsonb, timestamp, text[], Name, etc.).
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Get column names and data types for the source database.
/// Supports PG (information_schema), MySQL (INFORMATION_SCHEMA), and SQLite (PRAGMA table_info).
async fn source_column_info(pool: &AnyPool, table: &str, db_url: &str) -> Vec<(String, String)> {
    if is_pg(db_url) {
        let rows = sqlx::query(
            "SELECT column_name::text, data_type::text \
             FROM information_schema.columns \
             WHERE table_name = $1 AND table_schema = 'public' \
             ORDER BY ordinal_position",
        )
        .bind(table)
        .fetch_all(pool)
        .await;
        match rows {
            Ok(r) => r
                .iter()
                .map(|row| {
                    let name: String = row.get(0);
                    let ty: String = row.get(1);
                    (name, ty)
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    } else if is_mysql(db_url) {
        // Use INFORMATION_SCHEMA instead of DESCRIBE because sqlx Any driver
        // cannot decode the Type column from DESCRIBE results on MySQL.
        // Decode DATA_TYPE as Vec<u8> and convert to String because mysql
        // driver may return it as BLOB even after CAST(… AS CHAR).
        let rows = sqlx::query(
            "SELECT COLUMN_NAME, DATA_TYPE \
             FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_NAME = ? AND TABLE_SCHEMA = DATABASE() \
             ORDER BY ORDINAL_POSITION",
        )
        .bind(table)
        .fetch_all(pool)
        .await;
        match rows {
            Ok(r) => r
                .iter()
                .map(|row| {
                    let name: String = row.get(0);
                    let ty: String = row
                        .try_get::<Vec<u8>, _>(1)
                        .map(|b| String::from_utf8_lossy(&b).to_string())
                        .unwrap_or_default();
                    (name, normalize_mysql_type(&ty))
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    } else {
        // SQLite: PRAGMA table_info returns (cid, name, type, notnull, dflt_value, pk)
        let rows = sqlx::query(&format!("PRAGMA table_info(\"{}\")", table))
            .fetch_all(pool)
            .await;
        match rows {
            Ok(r) => r
                .iter()
                .map(|row| {
                    let name: String = row.get(1);
                    let ty: String = row.try_get::<String, _>(2).unwrap_or_default();
                    (name, ty)
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Build a SELECT query for PG sources, casting every column to ::text
/// so AnyPool can decode the values.
///
/// For text[] / ARRAY columns, `col::text` produces PG array-literal format
/// (`{elem1,elem2}`) which is NOT valid JSON. Use `array_to_json()` instead
/// to produce `["elem1","elem2"]`, which is valid JSON and can be inserted
/// into jsonb columns via CAST.
fn build_pg_select(
    table: &str,
    cols: &[(String, String)],
    order_by: Option<&str>,
    limit: Option<usize>,
) -> String {
    let select_cols: Vec<String> = cols
        .iter()
        .map(|(name, ty)| {
            if ty == "text[]" || ty == "ARRAY" {
                format!("COALESCE(array_to_json(\"{name}\")::text, '[]') AS \"{name}\"",)
            } else {
                format!("COALESCE(\"{name}\"::text, '') AS \"{name}\"")
            }
        })
        .collect();
    let mut query = format!("SELECT {} FROM \"{}\"", select_cols.join(", "), table);
    if let Some(ob) = order_by {
        query.push_str(&format!(" ORDER BY \"{}\" ASC", ob));
    }
    if let Some(lim) = limit {
        query.push_str(&format!(" LIMIT {}", lim));
    }
    query
}

/// Get column names and data types from the **target** database.
///
/// PG:  uses information_schema.columns
/// SQLite: uses PRAGMA table_info (returns cid, name, type, notnull, dflt_value, pk)
/// MySQL: uses INFORMATION_SCHEMA.COLUMNS (sqlx Any driver cannot decode DESCRIBE Type column).
async fn target_column_info(pool: &AnyPool, table: &str, db_url: &str) -> Vec<(String, String)> {
    if is_pg(db_url) {
        let rows = sqlx::query(
            "SELECT column_name::text, data_type::text \
             FROM information_schema.columns \
             WHERE table_name = $1 AND table_schema = 'public' \
             ORDER BY ordinal_position",
        )
        .bind(table)
        .fetch_all(pool)
        .await;
        match rows {
            Ok(r) => r
                .iter()
                .map(|row| {
                    let name: String = row.get(0);
                    let ty: String = row.get(1);
                    (name, ty)
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    } else if is_mysql(db_url) {
        // Use INFORMATION_SCHEMA instead of DESCRIBE because sqlx Any driver
        // cannot decode the Type column from DESCRIBE results on MySQL.
        // Decode DATA_TYPE as Vec<u8> and convert to String because mysql
        // driver may return it as BLOB even after CAST(… AS CHAR).
        let rows = sqlx::query(
            "SELECT COLUMN_NAME, DATA_TYPE \
             FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_NAME = ? AND TABLE_SCHEMA = DATABASE() \
             ORDER BY ORDINAL_POSITION",
        )
        .bind(table)
        .fetch_all(pool)
        .await;
        match rows {
            Ok(r) => r
                .iter()
                .map(|row| {
                    let name: String = row.get(0);
                    let ty: String = row
                        .try_get::<Vec<u8>, _>(1)
                        .map(|b| String::from_utf8_lossy(&b).to_string())
                        .unwrap_or_default();
                    (name, normalize_mysql_type(&ty))
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    } else {
        // SQLite: PRAGMA table_info returns (cid, name, type, notnull, dflt_value, pk)
        let rows = sqlx::query(&format!("PRAGMA table_info(\"{}\")", table))
            .fetch_all(pool)
            .await;
        match rows {
            Ok(r) => r
                .iter()
                .map(|row| {
                    let name: String = row.get(1);
                    let ty: String = row.try_get::<String, _>(2).unwrap_or_default();
                    (name, ty)
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Map MySQL type names to SQL-standard equivalents used in pg_type_for_cast.
fn normalize_mysql_type(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if lower.starts_with("varchar")
        || lower.starts_with("char")
        || lower.starts_with("text")
        || lower.starts_with("longtext")
        || lower.starts_with("mediumtext")
    {
        "text".to_string()
    } else if lower.starts_with("int") || lower.starts_with("tinyint") {
        "integer".to_string()
    } else if lower.starts_with("bigint") {
        "bigint".to_string()
    } else if lower.starts_with("smallint") {
        "smallint".to_string()
    } else if lower.starts_with("float") {
        "real".to_string()
    } else if lower.starts_with("double") {
        "double precision".to_string()
    } else if lower.starts_with("decimal") || lower.starts_with("numeric") {
        "numeric".to_string()
    } else if lower.starts_with("datetime") || lower.starts_with("timestamp") {
        "timestamp".to_string()
    } else if lower == "json" {
        "json".to_string()
    } else if lower.starts_with("blob") || lower.starts_with("binary") {
        "blob".to_string()
    } else {
        "text".to_string()
    }
}

/// Check if a URL targets MySQL.
fn is_mysql(url: &str) -> bool {
    url.starts_with("mysql://") || url.starts_with("mariadb://")
}

/// Build the VALUES ( ... ) expression for INSERT, using **target** column
/// types so the cast suffix matches the destination column's actual type.
///
/// For columns with a source index (Some), uses placeholder + CAST + NULLIF.
/// For target-only columns (None), uses a type-appropriate literal default.
///
/// Returns (values_expr, placeholder_count) — placeholder count is needed to
/// know how many binds to supply, since target-only columns use literals.
fn pg_insert_values_expr(
    cols: &[(String, String, Option<usize>)],
    db_url: &str,
) -> (String, usize) {
    let mut ph_count = 0usize;
    let expr = cols
        .iter()
        .map(|(_, ty, src_idx)| {
            if src_idx.is_none() {
                // Target-only column: use literal default
                return pg_default_literal(ty).to_string();
            }
            let ph = placeholder(ph_count, db_url);
            ph_count += 1;
            let pg_type = pg_type_for_cast(ty);
            match pg_type {
                "" => {
                    // Text-compatible: no cast needed, empty string is fine
                    ph
                }
                _ => {
                    // CAST(NULLIF(…, '') AS type) handles empty strings from NULL source
                    // values, but if the target column is NOT NULL we need a fallback.
                    // COALESCE ensures we always provide a valid value.
                    format!(
                        "COALESCE(CAST(NULLIF({}, '') AS {}), {}::{})",
                        ph,
                        pg_type,
                        pg_default_literal(ty),
                        pg_type
                    )
                }
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    (expr, ph_count)
}

/// Build the column list and a mapping from target column → source row index.
/// Returns (target_col_names, target_col_types, source_index_opt) where
/// source_index_opt is Some(idx) for columns present in source, None for
/// target-only columns that need type-appropriate literal defaults.
/// Convert camelCase to snake_case.
fn camel_to_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap_or(c));
        } else {
            result.push(c);
        }
    }
    result
}

fn build_column_merge(
    src_cols: &[(String, String)],
    tgt_cols: &[(String, String)],
) -> Vec<(String, String, Option<usize>)> {
    // Build source name → index map
    let src_index: std::collections::HashMap<&str, usize> = src_cols
        .iter()
        .enumerate()
        .map(|(i, (name, _))| (name.as_str(), i))
        .collect();

    // Build camelCase → snake_case fallback map for sources with camelCase columns
    let src_snake_map: std::collections::HashMap<String, usize> = src_cols
        .iter()
        .enumerate()
        .map(|(i, (name, _))| (camel_to_snake(name), i))
        .collect();

    // For each target column, look up its index in the source (None if missing)
    tgt_cols
        .iter()
        .map(|(name, ty)| {
            let idx = src_index
                .get(name.as_str())
                .copied()
                .or_else(|| src_snake_map.get(name.as_str()).copied());
            (name.clone(), ty.clone(), idx)
        })
        .collect()
}

/// Return a type-appropriate default literal for a column not present in the source.
/// These are used when the target has a NOT NULL column the source doesn't have.
fn pg_default_literal(col_type: &str) -> &'static str {
    match col_type {
        "jsonb" | "json" => "'{}'::jsonb",
        "text[]" | "ARRAY" => "'{}'::text[]",
        "boolean" | "bool" => "false",
        "integer" | "int" | "int4" | "smallint" | "int2" | "bigint" | "int8" => "0",
        "double precision" | "float8" | "real" | "float4" | "numeric" | "decimal" => "0",
        "timestamp with time zone" | "timestamptz" => "'1970-01-01 00:00:00+00'::timestamptz",
        "timestamp without time zone" | "timestamp" => "'1970-01-01 00:00:00'::timestamp",
        _ => "''",
    }
}

/// Return the PG cast suffix for a given data type.
/// All non-text types need explicit casts because `::text` SELECT values
/// don't implicitly cast to jsonb/numeric/timestamp/etc.
fn pg_type_for_cast(data_type: &str) -> &'static str {
    match data_type {
        "jsonb" => "jsonb",
        // json pseudotype: text inserts work but empty string isn't valid JSON →
        // needs NULLIF(…, '') in pg_insert_values_expr
        "json" => "json",
        "text[]" | "ARRAY" => "text[]",
        "boolean" | "bool" => "boolean",
        "integer" | "int" | "int4" => "integer",
        "bigint" | "int8" => "bigint",
        "smallint" | "int2" => "smallint",
        "double precision" | "float8" => "double precision",
        "real" | "float4" => "real",
        "numeric" | "decimal" => "numeric",
        "text" | "character varying" | "varchar" | "char" | "character" => "",
        "uuid" => "uuid",
        "timestamp without time zone" | "timestamp" => "timestamp",
        "timestamp with time zone" | "timestamptz" => "timestamptz",
        _ => "",
    }
}

/// Bind a row value at `idx` to a query, trying String → i64 → f64 → empty string.
/// When `col_type` is provided and target is MySQL, JSON-typed columns are sanitized:
/// empty/invalid strings are replaced with `'{}'` to satisfy MySQL's strict JSON validation.
fn bind_value_from_row<'q>(
    mut q: sqlx::query::Query<'q, sqlx::Any, <sqlx::Any as sqlx::Database>::Arguments<'q>>,
    row: &<sqlx::Any as sqlx::Database>::Row,
    idx: usize,
    col_type: Option<&str>,
    is_mysql: bool,
) -> sqlx::query::Query<'q, sqlx::Any, <sqlx::Any as sqlx::Database>::Arguments<'q>> {
    if let Ok(v) = row.try_get::<String, _>(idx) {
        if is_mysql {
            if let Some(ct) = col_type {
                let ct_lower = ct.to_lowercase();
                if ct_lower == "json" || ct_lower.contains("json") {
                    let trimmed = v.trim();
                    if trimmed.is_empty() || trimmed == "null" {
                        return q.bind("{}".to_string());
                    }
                    // Also handle the case where the value looks like an incomplete/truncated JSON
                    // (e.g. empty after trimming, or not starting with { or [)
                    if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
                        return q.bind("{}".to_string());
                    }
                }
            }
        }
        q = q.bind(v);
    } else if let Ok(v) = row.try_get::<i64, _>(idx) {
        q = q.bind(v);
    } else if let Ok(v) = row.try_get::<f64, _>(idx) {
        q = q.bind(v);
    } else {
        if is_mysql {
            if let Some(ct) = col_type {
                let ct_lower = ct.to_lowercase();
                if ct_lower == "json" || ct_lower.contains("json") {
                    return q.bind("{}".to_string());
                }
            }
        }
        q = q.bind(String::new());
    }
    q
}

/// Extract litellm master_key from the LiteLLM_Config table.
///
/// Tries two strategies:
/// 1. `param_name = 'litellm_master_key'` — legacy flat key (old litellm versions)
/// 2. `param_name = 'general_settings'` — JSON with `master_key` field (current litellm)
async fn extract_source_master_key(
    source: &AnyPool,
    source_url: &str,
) -> anyhow::Result<Option<String>> {
    let col = if is_pg(source_url) {
        "param_value::text"
    } else {
        "param_value"
    };

    // Strategy 1: legacy flat key
    let query = format!(
        "SELECT {} FROM \"LiteLLM_Config\" WHERE param_name = 'litellm_master_key'",
        col
    );
    if let Some(row) = sqlx::query(&query).fetch_optional(source).await? {
        if let Ok(val) = row.try_get::<String, _>(0) {
            if !val.is_empty() {
                return Ok(Some(val));
            }
        }
    }

    // Strategy 2: general_settings JSON
    let query = format!(
        "SELECT {} FROM \"LiteLLM_Config\" WHERE param_name = 'general_settings'",
        col
    );
    if let Some(row) = sqlx::query(&query).fetch_optional(source).await? {
        if let Ok(val) = row.try_get::<String, _>(0) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&val) {
                if let Some(mk) = parsed.get("master_key").and_then(|v| v.as_str()) {
                    if !mk.is_empty() {
                        return Ok(Some(mk.to_string()));
                    }
                }
            }
        }
    }

    Ok(None)
}

/// Tables without encrypted fields (plain copy).
const PLAIN_TABLES: &[(&str, &str)] = &[
    ("LiteLLM_OrganizationTable", "organizations"),
    ("LiteLLM_TeamTable", "teams"),
    ("LiteLLM_UserTable", "users"),
    ("LiteLLM_ProjectTable", "projects"),
    ("LiteLLM_BudgetTable", "budgets"),
    ("LiteLLM_OrganizationMembership", "organization_memberships"),
    ("LiteLLM_TeamMembership", "team_memberships"),
    ("LiteLLM_VerificationToken", "virtual_keys"),
    ("LiteLLM_Config", "config"),
];

/// Build a simple INSERT for non-PG targets using target column info + column merge.
/// Target-only columns get `''` or `'{}'` literal defaults.
fn non_pg_insert_values_expr(merged: &[(String, String, Option<usize>)]) -> (String, usize) {
    let mut ph_count = 0usize;
    let expr = merged
        .iter()
        .map(|(_, ty, src_idx)| {
            if src_idx.is_some() {
                let s = format!("?");
                ph_count += 1;
                s
            } else {
                // Target-only column: use a type-appropriate literal default
                let ty_lower = ty.to_lowercase();
                if ty_lower == "blob" || ty_lower.contains("blob") {
                    "X''".to_string()
                } else if ty_lower == "json" || ty_lower.contains("json") || ty_lower == "jsonb" {
                    "'{}'".to_string()
                } else if ty_lower.contains("int")
                    || ty_lower == "integer"
                    || ty_lower == "real"
                    || ty_lower == "float"
                    || ty_lower.contains("double")
                    || ty_lower.contains("numeric")
                {
                    "0".to_string()
                } else {
                    "''".to_string()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    (expr, ph_count)
}

/// Copy all rows from src_table to tgt_table without transformation.
async fn migrate_plain_table(
    source: &AnyPool,
    target: &AnyPool,
    src_table: &str,
    tgt_table: &str,
    source_url: &str,
    target_url: &str,
) -> anyhow::Result<usize> {
    let col_info = source_column_info(source, src_table, source_url).await;
    let tgt_col_info = target_column_info(target, tgt_table, target_url).await;

    let query = if !col_info.is_empty() && is_pg(source_url) {
        build_pg_select(src_table, &col_info, None, None)
    } else {
        format!("SELECT * FROM \"{}\"", src_table)
    };

    let rows = match sqlx::query(&query).fetch_all(source).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] {}: {}", src_table, e);
            return Ok(0);
        }
    };

    if rows.is_empty() {
        return Ok(0);
    }

    let columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    // Build column merge: map target columns to source column indices
    let merged: Vec<(String, String, Option<usize>)> = if !tgt_col_info.is_empty() {
        build_column_merge(&col_info, &tgt_col_info)
    } else {
        Vec::new()
    };

    let do_merge = !merged.is_empty();
    let use_pg = do_merge && is_pg(target_url);
    let insert_cols: Vec<String> = merged
        .iter()
        .map(|(n, _, _)| quote_ident(n, target_url))
        .collect();

    // Build a column-name→type lookup from target column info for MySQL JSON sanitization.
    // Needed even when do_merge is false: PG sources produce empty strings for NULL JSON
    // columns (via COALESCE(col::text, '')), and MySQL rejects empty strings in JSON columns.
    let tgt_type_lookup: std::collections::HashMap<&str, &str> =
        if !do_merge && is_mysql(target_url) {
            tgt_col_info
                .iter()
                .map(|(n, t)| (n.as_str(), t.as_str()))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

    let mut inserted = 0usize;
    for (row_idx, row) in rows.iter().enumerate() {
        if row_idx == 0 && tgt_table.contains("teams") {
            for (name, ty, idx) in &merged {
                if name == "default_team_member_models" {
                    eprintln!("  [DEBUG-MERGED] {name}: ty={ty}, src_idx={idx:?}");
                }
            }
        }
        let col_count = row.columns().len();

        let pg_conflict = if is_pg(target_url) {
            " ON CONFLICT DO NOTHING"
        } else {
            ""
        };
        let tgt_quoted = quote_ident(tgt_table, target_url);
        let insert_sql = if do_merge {
            if use_pg {
                let (values_expr, _ph_count) = pg_insert_values_expr(&merged, target_url);
                format!(
                    "{} {} ({}) VALUES ({}){}",
                    insert_prefix(target_url),
                    tgt_quoted,
                    insert_cols.join(", "),
                    values_expr,
                    pg_conflict
                )
            } else {
                let (values_expr, _ph_count) = non_pg_insert_values_expr(&merged);
                format!(
                    "{} {} ({}) VALUES ({}){}",
                    insert_prefix(target_url),
                    tgt_quoted,
                    insert_cols.join(", "),
                    values_expr,
                    pg_conflict
                )
            }
        } else {
            let phs = placeholders(col_count, target_url);
            format!(
                "{} {} ({}) VALUES ({}){}",
                insert_prefix(target_url),
                tgt_quoted,
                columns.join(", "),
                phs.join(", "),
                pg_conflict
            )
        };

        let mut q = sqlx::query(&insert_sql);
        if do_merge {
            for (_col_name, col_ty, src_idx) in &merged {
                if let Some(idx) = src_idx {
                    q = bind_value_from_row(q, row, *idx, Some(col_ty), is_mysql(target_url));
                }
                // target-only columns use literal defaults in VALUES, no bind needed
            }
        } else {
            for i in 0..col_count {
                let col_name = &columns[i];
                let col_type = tgt_type_lookup.get(col_name.as_str()).copied();
                q = bind_value_from_row(q, row, i, col_type, is_mysql(target_url));
            }
        }
        match q.execute(target).await {
            Ok(_) => inserted += 1,
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "INSERT error for table '{}': {}",
                    tgt_table,
                    e
                ));
            }
        }
    }

    Ok(inserted)
}

/// Migrate credentials table with encryption key rotation.
async fn migrate_credentials(
    source: &AnyPool,
    target: &AnyPool,
    source_key: &str,
    target_key: &str,
    source_url: &str,
    target_url: &str,
) -> anyhow::Result<usize> {
    let src_table = "LiteLLM_CredentialsTable";
    let col_info = source_column_info(source, src_table, source_url).await;
    let tgt_col_info = target_column_info(target, "credentials", target_url).await;

    let query = if !col_info.is_empty() && is_pg(source_url) {
        build_pg_select(src_table, &col_info, None, None)
    } else {
        format!("SELECT * FROM \"{}\"", src_table)
    };

    let rows = match sqlx::query(&query).fetch_all(source).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] {}: {}", src_table, e);
            return Ok(0);
        }
    };

    if rows.is_empty() {
        return Ok(0);
    }

    let columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    // Build column merge: target columns → source indices
    let merged: Vec<(String, String, Option<usize>)> = if !tgt_col_info.is_empty() {
        build_column_merge(&col_info, &tgt_col_info)
    } else {
        Vec::new()
    };

    let do_merge = !merged.is_empty();
    let use_pg = do_merge && is_pg(target_url);
    let insert_cols: Vec<String> = merged
        .iter()
        .map(|(n, _, _)| quote_ident(n, target_url))
        .collect();

    // Find the credential_values index in the MERGED ordering (i.e. target column position)
    let values_merged_idx = merged.iter().position(|(n, _, _)| n == "credential_values");

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    for row in &rows {
        let pg_conflict = if is_pg(target_url) {
            " ON CONFLICT DO NOTHING"
        } else {
            ""
        };
        let insert_sql = if do_merge {
            if use_pg {
                let (values_expr, _ph_count) = pg_insert_values_expr(&merged, target_url);
                format!(
                    "{} {} ({}) VALUES ({}){}",
                    insert_prefix(target_url),
                    quote_ident("credentials", target_url),
                    insert_cols.join(", "),
                    values_expr,
                    pg_conflict
                )
            } else {
                let (values_expr, _ph_count) = non_pg_insert_values_expr(&merged);
                format!(
                    "{} {} ({}) VALUES ({}){}",
                    insert_prefix(target_url),
                    quote_ident("credentials", target_url),
                    insert_cols.join(", "),
                    values_expr,
                    pg_conflict
                )
            }
        } else {
            let phs = placeholders(columns.len(), target_url);
            format!(
                "{} {} ({}) VALUES ({}){}",
                insert_prefix(target_url),
                quote_ident("credentials", target_url),
                columns.join(", "),
                phs.join(", "),
                pg_conflict
            )
        };

        let mut q = sqlx::query(&insert_sql);
        if do_merge {
            for (mi, (_, _, src_idx)) in merged.iter().enumerate() {
                if let Some(idx) = src_idx {
                    if values_merged_idx == Some(mi) {
                        // Decrypt credential_values with source key, re-encrypt with target key
                        if let Ok(encrypted) = row.try_get::<String, _>(*idx) {
                            if encrypted.is_empty() || encrypted == "{}" {
                                q = q.bind(encrypted);
                            } else if encrypted.starts_with('{') {
                                // JSON object with potentially individually encrypted fields —
                                // rotate each encrypted field then re-encrypt the whole thing
                                match serde_json::from_str::<Value>(&encrypted) {
                                    Ok(json_val) => {
                                        match aigw_core::rotate_json_fields(
                                            &json_val, source_key, target_key,
                                        ) {
                                            Ok(rotated) => {
                                                match aigw_core::encrypt_litellm_value(
                                                    &rotated, target_key,
                                                ) {
                                                    Ok(re_encrypted) => q = q.bind(re_encrypted),
                                                    Err(_e) => {
                                                        q = q.bind(encrypted);
                                                        skipped += 1;
                                                    }
                                                }
                                            }
                                            Err(_e) => {
                                                q = q.bind(encrypted);
                                                skipped += 1;
                                            }
                                        }
                                    }
                                    Err(_) => {
                                        q = q.bind(encrypted);
                                    }
                                }
                            } else {
                                match aigw_core::decrypt_litellm_value(&encrypted, source_key) {
                                    Ok(plaintext) => {
                                        match aigw_core::encrypt_litellm_value(
                                            &plaintext, target_key,
                                        ) {
                                            Ok(re_encrypted) => q = q.bind(re_encrypted),
                                            Err(_e) => {
                                                q = q.bind(encrypted);
                                                skipped += 1;
                                            }
                                        }
                                    }
                                    Err(_e) => {
                                        q = q.bind(encrypted);
                                        skipped += 1;
                                    }
                                }
                            }
                        } else {
                            let ty = merged[mi].1.as_str();
                            q = bind_value_from_row(q, row, *idx, Some(ty), is_mysql(target_url));
                        }
                    } else {
                        let ty = merged[mi].1.as_str();
                        q = bind_value_from_row(q, row, *idx, Some(ty), is_mysql(target_url));
                    }
                }
                // target-only column: literal default used in VALUES, no bind needed
            }
        } else {
            let values_col = columns
                .iter()
                .position(|c| c == "credential_values")
                .unwrap_or(2);
            for i in 0..columns.len() {
                if i == values_col {
                    if let Ok(encrypted) = row.try_get::<String, _>(i) {
                        if encrypted.is_empty() || encrypted == "{}" {
                            q = q.bind(encrypted);
                        } else if encrypted.starts_with('{') {
                            match serde_json::from_str::<Value>(&encrypted) {
                                Ok(json_val) => {
                                    match aigw_core::rotate_json_fields(
                                        &json_val, source_key, target_key,
                                    ) {
                                        Ok(rotated) => {
                                            match aigw_core::encrypt_litellm_value(
                                                &rotated, target_key,
                                            ) {
                                                Ok(re_encrypted) => q = q.bind(re_encrypted),
                                                Err(_e) => {
                                                    q = q.bind(encrypted);
                                                    skipped += 1;
                                                }
                                            }
                                        }
                                        Err(_e) => {
                                            q = q.bind(encrypted);
                                            skipped += 1;
                                        }
                                    }
                                }
                                Err(_) => {
                                    q = q.bind(encrypted);
                                }
                            }
                        } else {
                            match aigw_core::decrypt_litellm_value(&encrypted, source_key) {
                                Ok(plaintext) => {
                                    match aigw_core::encrypt_litellm_value(&plaintext, target_key) {
                                        Ok(re_encrypted) => q = q.bind(re_encrypted),
                                        Err(_e) => {
                                            q = q.bind(encrypted);
                                            skipped += 1;
                                        }
                                    }
                                }
                                Err(_e) => {
                                    q = q.bind(encrypted);
                                    skipped += 1;
                                }
                            }
                        }
                    } else {
                        q = bind_value_from_row(q, row, i, None, is_mysql(target_url));
                    }
                } else {
                    q = bind_value_from_row(q, row, i, None, is_mysql(target_url));
                }
            }
        }
        q.execute(target).await?;
        inserted += 1;
    }

    if skipped > 0 {
        eprintln!(
            "  [WARN] Skipped {} credential rows due to crypto errors",
            skipped
        );
    }

    Ok(inserted)
}

/// Migrate proxy_models table with encryption key rotation on litellm_params.
async fn migrate_proxy_models(
    source: &AnyPool,
    target: &AnyPool,
    source_key: &str,
    target_key: &str,
    source_url: &str,
    target_url: &str,
) -> anyhow::Result<usize> {
    let src_table = "LiteLLM_ProxyModelTable";
    let col_info = source_column_info(source, src_table, source_url).await;
    let tgt_col_info = target_column_info(target, "proxy_models", target_url).await;

    let query = if !col_info.is_empty() && is_pg(source_url) {
        build_pg_select(src_table, &col_info, None, None)
    } else {
        format!("SELECT * FROM \"{}\"", src_table)
    };

    let rows = match sqlx::query(&query).fetch_all(source).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] {}: {}", src_table, e);
            return Ok(0);
        }
    };

    if rows.is_empty() {
        return Ok(0);
    }

    let columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    // Build column merge: target columns → source indices
    let merged: Vec<(String, String, Option<usize>)> = if !tgt_col_info.is_empty() {
        build_column_merge(&col_info, &tgt_col_info)
    } else {
        Vec::new()
    };

    let do_merge = !merged.is_empty();
    let use_pg = do_merge && is_pg(target_url);
    let insert_cols: Vec<String> = merged
        .iter()
        .map(|(n, _, _)| quote_ident(n, target_url))
        .collect();

    // Find the litellm_params index in the MERGED ordering
    let params_merged_idx = merged.iter().position(|(n, _, _)| n == "litellm_params");

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    for row in &rows {
        let pg_conflict = if is_pg(target_url) {
            " ON CONFLICT DO NOTHING"
        } else {
            ""
        };
        let tgt_quoted = quote_ident("proxy_models", target_url);
        let insert_sql = if do_merge {
            if use_pg {
                let (values_expr, _ph_count) = pg_insert_values_expr(&merged, target_url);
                format!(
                    "{} {} ({}) VALUES ({}){}",
                    insert_prefix(target_url),
                    tgt_quoted,
                    insert_cols.join(", "),
                    values_expr,
                    pg_conflict
                )
            } else {
                let (values_expr, _ph_count) = non_pg_insert_values_expr(&merged);
                format!(
                    "{} {} ({}) VALUES ({}){}",
                    insert_prefix(target_url),
                    tgt_quoted,
                    insert_cols.join(", "),
                    values_expr,
                    pg_conflict
                )
            }
        } else {
            let phs = placeholders(columns.len(), target_url);
            format!(
                "{} {} ({}) VALUES ({}){}",
                insert_prefix(target_url),
                tgt_quoted,
                columns.join(", "),
                phs.join(", "),
                pg_conflict
            )
        };

        let mut q = sqlx::query(&insert_sql);
        if do_merge {
            for (mi, (_, _, src_idx)) in merged.iter().enumerate() {
                if let Some(idx) = src_idx {
                    if params_merged_idx == Some(mi) {
                        if let Ok(value) = row.try_get::<String, _>(*idx) {
                            if value.is_empty() {
                                q = q.bind(value);
                            } else if value.starts_with('{') {
                                // JSON object with potentially individually encrypted fields —
                                // rotate each encrypted field then re-encrypt the whole thing
                                match serde_json::from_str::<Value>(&value) {
                                    Ok(json_val) => {
                                        match aigw_core::rotate_json_fields(
                                            &json_val, source_key, target_key,
                                        ) {
                                            Ok(rotated) => {
                                                match aigw_core::encrypt_litellm_value(
                                                    &rotated, target_key,
                                                ) {
                                                    Ok(re_encrypted) => q = q.bind(re_encrypted),
                                                    Err(_e) => {
                                                        q = q.bind(value);
                                                        skipped += 1;
                                                    }
                                                }
                                            }
                                            Err(_e) => {
                                                q = q.bind(value);
                                                skipped += 1;
                                            }
                                        }
                                    }
                                    Err(_) => {
                                        q = q.bind(value);
                                    }
                                }
                            } else {
                                match aigw_core::decrypt_litellm_value(&value, source_key) {
                                    Ok(plaintext) => {
                                        match aigw_core::encrypt_litellm_value(
                                            &plaintext, target_key,
                                        ) {
                                            Ok(re_encrypted) => q = q.bind(re_encrypted),
                                            Err(_e) => {
                                                q = q.bind(value);
                                                skipped += 1;
                                            }
                                        }
                                    }
                                    Err(_e) => {
                                        q = q.bind(value);
                                        skipped += 1;
                                    }
                                }
                            }
                        } else {
                            let ty = merged[mi].1.as_str();
                            q = bind_value_from_row(q, row, *idx, Some(ty), is_mysql(target_url));
                        }
                    } else {
                        let ty = merged[mi].1.as_str();
                        q = bind_value_from_row(q, row, *idx, Some(ty), is_mysql(target_url));
                    }
                }
                // target-only column: literal default used in VALUES, no bind needed
            }
        } else {
            let params_col = columns
                .iter()
                .position(|c| c == "litellm_params")
                .unwrap_or(4);
            for i in 0..columns.len() {
                if i == params_col {
                    if let Ok(value) = row.try_get::<String, _>(i) {
                        if value.is_empty() {
                            q = q.bind(value);
                        } else if value.starts_with('{') {
                            match serde_json::from_str::<Value>(&value) {
                                Ok(json_val) => {
                                    match aigw_core::rotate_json_fields(
                                        &json_val, source_key, target_key,
                                    ) {
                                        Ok(rotated) => {
                                            match aigw_core::encrypt_litellm_value(
                                                &rotated, target_key,
                                            ) {
                                                Ok(re_encrypted) => q = q.bind(re_encrypted),
                                                Err(_e) => {
                                                    q = q.bind(value);
                                                    skipped += 1;
                                                }
                                            }
                                        }
                                        Err(_e) => {
                                            q = q.bind(value);
                                            skipped += 1;
                                        }
                                    }
                                }
                                Err(_) => {
                                    q = q.bind(value);
                                }
                            }
                        } else {
                            match aigw_core::decrypt_litellm_value(&value, source_key) {
                                Ok(plaintext) => {
                                    match aigw_core::encrypt_litellm_value(&plaintext, target_key) {
                                        Ok(re_encrypted) => q = q.bind(re_encrypted),
                                        Err(_e) => {
                                            q = q.bind(value);
                                            skipped += 1;
                                        }
                                    }
                                }
                                Err(_e) => {
                                    q = q.bind(value);
                                    skipped += 1;
                                }
                            }
                        }
                    } else {
                        q = bind_value_from_row(q, row, i, None, is_mysql(target_url));
                    }
                } else {
                    q = bind_value_from_row(q, row, i, None, is_mysql(target_url));
                }
            }
        }
        q.execute(target).await?;
        inserted += 1;
    }

    if skipped > 0 {
        eprintln!(
            "  [WARN] Skipped {} model rows due to crypto errors",
            skipped
        );
    }

    Ok(inserted)
}

/// Batch migrate spend_logs (no crypto, just large table optimization).
/// When `limit` is Some(n), only the first n rows from the source are imported.
async fn migrate_spend_logs(
    source: &AnyPool,
    target: &AnyPool,
    limit: Option<usize>,
    source_url: &str,
    target_url: &str,
) -> anyhow::Result<usize> {
    let src_table = "LiteLLM_SpendLogs";
    let col_info = source_column_info(source, src_table, source_url).await;
    let tgt_col_info = target_column_info(target, "spend_logs", target_url).await;

    let t_fetch = std::time::Instant::now();
    let query = if !col_info.is_empty() && is_pg(source_url) {
        build_pg_select(src_table, &col_info, None, limit)
    } else {
        let mut q = format!("SELECT * FROM \"{}\"", src_table);
        if let Some(lim) = limit {
            q.push_str(&format!(" LIMIT {}", lim));
        }
        q
    };

    let rows = match sqlx::query(&query).fetch_all(source).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] {}: {}", src_table, e);
            return Ok(0);
        }
    };

    let row_count = rows.len();
    eprintln!(
        "  [TIMING] spend_logs fetch: {:?} ({} rows)",
        t_fetch.elapsed(),
        row_count
    );

    if row_count == 0 {
        return Ok(0);
    }

    let columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    // Build column merge: target columns → source indices
    let merged: Vec<(String, String, Option<usize>)> = if !tgt_col_info.is_empty() {
        build_column_merge(&col_info, &tgt_col_info)
    } else {
        Vec::new()
    };

    let do_merge = !merged.is_empty();
    let use_pg = do_merge && is_pg(target_url);
    let insert_cols: Vec<String> = merged
        .iter()
        .map(|(n, _, _)| quote_ident(n, target_url))
        .collect();

    // Build a column-name→type lookup from target column info for MySQL JSON sanitization.
    let tgt_type_lookup: std::collections::HashMap<&str, &str> =
        if !do_merge && is_mysql(target_url) {
            tgt_col_info
                .iter()
                .map(|(n, t)| (n.as_str(), t.as_str()))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

    let t_insert = std::time::Instant::now();
    let mut inserted = 0usize;
    for row in &rows {
        let pg_conflict = if is_pg(target_url) {
            " ON CONFLICT DO NOTHING"
        } else {
            ""
        };
        let tgt_quoted = quote_ident("spend_logs", target_url);
        let insert_sql = if do_merge {
            if use_pg {
                let (values_expr, _ph_count) = pg_insert_values_expr(&merged, target_url);
                format!(
                    "{} {} ({}) VALUES ({}){}",
                    insert_prefix(target_url),
                    tgt_quoted,
                    insert_cols.join(", "),
                    values_expr,
                    pg_conflict
                )
            } else {
                let (values_expr, _ph_count) = non_pg_insert_values_expr(&merged);
                format!(
                    "{} {} ({}) VALUES ({}){}",
                    insert_prefix(target_url),
                    tgt_quoted,
                    insert_cols.join(", "),
                    values_expr,
                    pg_conflict
                )
            }
        } else {
            let phs = placeholders(columns.len(), target_url);
            format!(
                "{} {} ({}) VALUES ({}){}",
                insert_prefix(target_url),
                tgt_quoted,
                columns.join(", "),
                phs.join(", "),
                pg_conflict
            )
        };

        let mut q = sqlx::query(&insert_sql);
        if do_merge {
            for (_, col_ty, src_idx) in &merged {
                if let Some(idx) = src_idx {
                    q = bind_value_from_row(q, row, *idx, Some(col_ty), is_mysql(target_url));
                }
                // target-only columns use literal defaults in VALUES, no bind needed
            }
        } else {
            for i in 0..columns.len() {
                let col_name = &columns[i];
                let col_type = tgt_type_lookup.get(col_name.as_str()).copied();
                q = bind_value_from_row(q, row, i, col_type, is_mysql(target_url));
            }
        }
        q.execute(target).await?;
        inserted += 1;
    }
    eprintln!(
        "  [TIMING] spend_logs insert: {:?} ({} rows, avg {:?}/row)",
        t_insert.elapsed(),
        inserted,
        t_insert.elapsed() / inserted.max(1) as u32
    );

    Ok(inserted)
}

/// Run remote import. When `spend_log_limit` is Some(n), only import the first n
/// spend_log rows (ordered by start_time ASC). None = import all rows.
/// `step_filter` (2=plain, 3=credentials, 4=proxy_models, 5=spend_logs) runs only that step;
/// steps 1 (master_key extraction) and 6 (verification) always execute.
pub async fn run_filtered(
    source_url: &str,
    target_url: &str,
    source_master_key: Option<&str>,
    target_master_key: &str,
    spend_log_limit: Option<usize>,
    step_filter: Option<u8>,
) -> anyhow::Result<bool> {
    let total_start = std::time::Instant::now();
    sqlx::any::install_default_drivers();

    let t0 = std::time::Instant::now();
    let source = connect(source_url).await?;
    let target = connect(target_url).await?;
    eprintln!("  [TIMING] connect: {:?}", t0.elapsed());

    // Step 1: Extract source master_key
    let t0 = std::time::Instant::now();
    let source_key = match source_master_key {
        Some(k) => k.to_string(),
        None => match extract_source_master_key(&source, source_url).await? {
            Some(k) => {
                eprintln!("  Extracted master_key from LiteLLM_Config");
                k
            }
            None => {
                anyhow::bail!(
                    "No source master_key found. Provide --source-master-key or \
                     ensure LiteLLM_Config has param_name='general_settings' with master_key field"
                );
            }
        },
    };
    eprintln!("Step 1: Source master_key obtained ({:?})", t0.elapsed());

    let run_step = |s: u8| step_filter.map_or(true, |f| f == s);

    // Step 2: Migrate plain tables
    if run_step(2) {
        eprintln!("Step 2: Migrating plain tables...");
        let t0 = std::time::Instant::now();
        for &(src, tgt) in PLAIN_TABLES {
            let t_tbl = std::time::Instant::now();
            let count =
                migrate_plain_table(&source, &target, src, tgt, source_url, target_url).await?;
            eprintln!(
                "  {} -> {} ({} rows, {:?})",
                src,
                tgt,
                count,
                t_tbl.elapsed()
            );
        }
        eprintln!("Step 2: plain tables done ({:?})", t0.elapsed());
    } else {
        eprintln!("Step 2: [SKIP]");
    }

    // Step 3: Migrate credentials with key rotation
    if run_step(3) {
        eprintln!("Step 3: Migrating credentials (with key rotation)...");
        let t0 = std::time::Instant::now();
        let cred_count = migrate_credentials(
            &source,
            &target,
            &source_key,
            target_master_key,
            source_url,
            target_url,
        )
        .await?;
        eprintln!(
            "  LiteLLM_CredentialsTable -> credentials ({} rows, {:?})",
            cred_count,
            t0.elapsed()
        );
    } else {
        eprintln!("Step 3: [SKIP]");
    }

    // Step 4: Migrate proxy_models with key rotation
    if run_step(4) {
        eprintln!("Step 4: Migrating proxy_models (with key rotation)...");
        let t0 = std::time::Instant::now();
        let model_count = migrate_proxy_models(
            &source,
            &target,
            &source_key,
            target_master_key,
            source_url,
            target_url,
        )
        .await?;
        eprintln!(
            "  LiteLLM_ProxyModelTable -> proxy_models ({} rows, {:?})",
            model_count,
            t0.elapsed()
        );
    } else {
        eprintln!("Step 4: [SKIP]");
    }

    // Step 5: Migrate spend_logs
    if run_step(5) {
        eprintln!("Step 5: Migrating spend_logs...");
        let t0 = std::time::Instant::now();
        let spend_count =
            migrate_spend_logs(&source, &target, spend_log_limit, source_url, target_url).await?;
        eprintln!(
            "  LiteLLM_SpendLogs -> spend_logs ({} rows, {:?})",
            spend_count,
            t0.elapsed()
        );
    } else {
        eprintln!("Step 5: [SKIP]");
    }

    // Step 6: Verify
    eprintln!("Step 6: Verifying row counts...");
    let t0 = std::time::Instant::now();
    let mut all_match = true;
    let all_tables: &[(&str, &str)] = &[
        ("LiteLLM_OrganizationTable", "organizations"),
        ("LiteLLM_TeamTable", "teams"),
        ("LiteLLM_UserTable", "users"),
        ("LiteLLM_ProjectTable", "projects"),
        ("LiteLLM_BudgetTable", "budgets"),
        ("LiteLLM_OrganizationMembership", "organization_memberships"),
        ("LiteLLM_TeamMembership", "team_memberships"),
        ("LiteLLM_VerificationToken", "virtual_keys"),
        ("LiteLLM_Config", "config"),
        ("LiteLLM_CredentialsTable", "credentials"),
        ("LiteLLM_ProxyModelTable", "proxy_models"),
        ("LiteLLM_SpendLogs", "spend_logs"),
    ];

    for &(src, tgt) in all_tables {
        let src_quoted = quote_ident(src, source_url);
        let src_count: i64 = sqlx::query(&format!("SELECT COUNT(*) FROM {}", src_quoted))
            .fetch_one(&source)
            .await
            .map(|row| row.get(0))
            .unwrap_or(0);

        let tgt_quoted = quote_ident(tgt, target_url);
        let tgt_count: i64 = sqlx::query(&format!("SELECT COUNT(*) FROM {}", tgt_quoted))
            .fetch_one(&target)
            .await
            .map(|row| row.get(0))
            .unwrap_or(-1);

        let status = if src_count == tgt_count {
            "OK"
        } else {
            "MISMATCH"
        };
        if src_count != tgt_count {
            all_match = false;
        }
        eprintln!(
            "  {} -> {}: src={} tgt={} [{}]",
            src, tgt, src_count, tgt_count, status
        );
    }
    eprintln!("Step 6: verify done ({:?})", t0.elapsed());

    eprintln!("[TIMING] total migration: {:?}", total_start.elapsed());
    Ok(all_match)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    /// Helper: Create a SQLite pool with a given file.
    async fn create_pool(path: &str) -> sqlx::SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(path.to_string())
                    .create_if_missing(true),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_remote_import_plain_tables() {
        sqlx::any::install_default_drivers();
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.db");
        let tgt_path = dir.path().join("tgt.db");
        let src_str = src_path.to_str().unwrap();
        let tgt_str = tgt_path.to_str().unwrap();

        // Setup source DB with organizations table
        let src_pool = create_pool(src_str).await;
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_OrganizationTable" (
                organization_id TEXT PRIMARY KEY,
                organization_alias TEXT,
                spend REAL DEFAULT 0
            )"#,
        )
        .execute(&src_pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_OrganizationTable\" (organization_id, organization_alias, spend) VALUES ('org-1', 'test', 42.0)"
        )
        .execute(&src_pool)
        .await
        .unwrap();

        // Setup source DB with config table containing master_key
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_Config" (param_name TEXT PRIMARY KEY, param_value TEXT)"#,
        )
        .execute(&src_pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_Config\" (param_name, param_value) VALUES ('litellm_master_key', 'sk-test-source-key-12345')"
        )
        .execute(&src_pool)
        .await
        .unwrap();

        // Setup source DB with credentials table
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_CredentialsTable" (
                credential_id TEXT PRIMARY KEY,
                credential_name TEXT NOT NULL,
                credential_values TEXT,
                credential_info TEXT
            )"#,
        )
        .execute(&src_pool)
        .await
        .unwrap();

        // Insert a credential with encrypted values
        let source_key = "sk-test-source-key-12345";
        let plain_cred = r#"{"api_key":"sk-secret-123","api_base":"https://api.openai.com"}"#;
        let encrypted_cred = aigw_core::encrypt_litellm_value(plain_cred, source_key).unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_CredentialsTable\" (credential_id, credential_name, credential_values) VALUES ('cred-1', 'openai-key', ?)"
        )
        .bind(&encrypted_cred)
        .execute(&src_pool)
        .await
        .unwrap();

        // Setup source DB with proxy_models table
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_ProxyModelTable" (
                model_id TEXT PRIMARY KEY,
                model_name TEXT,
                litellm_params TEXT,
                model_info TEXT
            )"#,
        )
        .execute(&src_pool)
        .await
        .unwrap();

        let plain_params = r#"{"model":"gpt-4","api_key":"sk-model-key-456"}"#;
        let encrypted_params = aigw_core::encrypt_litellm_value(plain_params, source_key).unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_ProxyModelTable\" (model_id, model_name, litellm_params) VALUES ('model-1', 'gpt-4', ?)"
        )
        .bind(&encrypted_params)
        .execute(&src_pool)
        .await
        .unwrap();

        // Also add spend_logs table
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_SpendLogs" (
                request_id TEXT PRIMARY KEY,
                model TEXT,
                spend REAL DEFAULT 0
            )"#,
        )
        .execute(&src_pool)
        .await
        .unwrap();
        src_pool.close().await;

        // Setup target DB with all required tables (matching source columns)
        let tgt_pool = create_pool(tgt_str).await;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "organizations" (
                organization_id TEXT PRIMARY KEY, organization_alias TEXT, spend REAL DEFAULT 0
            )"#,
        )
        .execute(&tgt_pool)
        .await
        .unwrap();

        for table in &[
            "teams",
            "users",
            "projects",
            "budgets",
            "organization_memberships",
            "team_memberships",
            "virtual_keys",
            "spend_logs",
        ] {
            sqlx::query(&format!(
                "CREATE TABLE IF NOT EXISTS \"{}\" (id TEXT PRIMARY KEY)",
                table
            ))
            .execute(&tgt_pool)
            .await
            .unwrap();
        }

        // config table with matching columns
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "config" (param_name TEXT PRIMARY KEY, param_value TEXT)"#,
        )
        .execute(&tgt_pool)
        .await
        .unwrap();

        // credentials table with matching columns
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "credentials" (
                credential_id TEXT PRIMARY KEY,
                credential_name TEXT NOT NULL,
                credential_values TEXT,
                credential_info TEXT
            )"#,
        )
        .execute(&tgt_pool)
        .await
        .unwrap();

        // proxy_models table with matching columns
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "proxy_models" (
                model_id TEXT PRIMARY KEY,
                model_name TEXT,
                litellm_params TEXT,
                model_info TEXT
            )"#,
        )
        .execute(&tgt_pool)
        .await
        .unwrap();
        tgt_pool.close().await;

        // Run remote_import (source key from config, not CLI)
        let target_key = "sk-aigw-target-key-99999";
        let result = run_filtered(src_str, tgt_str, None, target_key, None, None).await;
        assert!(result.is_ok(), "remote_import failed: {:?}", result.err());

        // Verify: credentials should be migrated with re-encryption
        let tgt_pool = create_pool(tgt_str).await;
        let cred_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM credentials")
            .fetch_one(&tgt_pool)
            .await
            .unwrap();
        assert_eq!(cred_count.0, 1, "should have 1 credential");

        // Verify credential_values was re-encrypted with target key
        let cred_row: (String,) = sqlx::query_as(
            "SELECT credential_values FROM credentials WHERE credential_id = 'cred-1'",
        )
        .fetch_one(&tgt_pool)
        .await
        .unwrap();
        let decrypted = aigw_core::decrypt_litellm_value(&cred_row.0, target_key).unwrap();
        assert_eq!(
            decrypted, plain_cred,
            "credential should decrypt with target key"
        );

        // Verify proxy_models litellm_params was re-encrypted
        let model_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM proxy_models")
            .fetch_one(&tgt_pool)
            .await
            .unwrap();
        assert_eq!(model_count.0, 1, "should have 1 proxy model");

        let model_row: (String,) =
            sqlx::query_as("SELECT litellm_params FROM proxy_models WHERE model_id = 'model-1'")
                .fetch_one(&tgt_pool)
                .await
                .unwrap();
        let decrypted_params = aigw_core::decrypt_litellm_value(&model_row.0, target_key).unwrap();
        assert_eq!(
            decrypted_params, plain_params,
            "model params should decrypt with target key"
        );

        tgt_pool.close().await;
    }

    #[tokio::test]
    async fn test_extract_master_key_from_config() {
        sqlx::any::install_default_drivers();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();

        let pool = create_pool(db_str).await;
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_Config" (param_name TEXT PRIMARY KEY, param_value TEXT)"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_Config\" (param_name, param_value) VALUES ('litellm_master_key', 'sk-extracted-key')"
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        let source = connect(db_str).await.unwrap();
        let key = extract_source_master_key(&source, db_str).await.unwrap();
        assert_eq!(key, Some("sk-extracted-key".to_string()));
    }

    #[tokio::test]
    async fn test_migrate_plain_table_empty() {
        sqlx::any::install_default_drivers();
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.db");
        let tgt_path = dir.path().join("tgt.db");
        let src_str = src_path.to_str().unwrap();
        let tgt_str = tgt_path.to_str().unwrap();

        let src_pool = create_pool(src_str).await;
        sqlx::query(r#"CREATE TABLE "LiteLLM_OrganizationTable" (organization_id TEXT)"#)
            .execute(&src_pool)
            .await
            .unwrap();
        src_pool.close().await;

        let tgt_pool = create_pool(tgt_str).await;
        sqlx::query(r#"CREATE TABLE "organizations" (organization_id TEXT)"#)
            .execute(&tgt_pool)
            .await
            .unwrap();
        tgt_pool.close().await;

        let source = connect(src_str).await.unwrap();
        let target = connect(tgt_str).await.unwrap();
        let count = migrate_plain_table(
            &source,
            &target,
            "LiteLLM_OrganizationTable",
            "organizations",
            tgt_str, // source_url (SQLite, no ::text casting needed)
            tgt_str, // target_url
        )
        .await
        .unwrap();
        assert_eq!(count, 0, "empty table should migrate 0 rows");
    }
}
