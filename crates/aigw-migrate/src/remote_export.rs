//! remote-export: Reverse migration — aigw → litellm with encryption key rotation.
//!
//! Pipeline (reverse of remote_import):
//!   1. Connect to source (aigw DB) and target (litellm DB)
//!   2. Extract litellm master_key from target LiteLLM_Config or CLI arg
//!   3. Migrate plain tables (no encrypted fields)
//!   4. Migrate credentials — decrypt with aigw key, re-encrypt with litellm key
//!   5. Migrate proxy_models — decrypt with aigw key, re-encrypt with litellm key
//!   6. Batch migrate spend_logs (no crypto)

use sqlx::any::AnyPoolOptions;
use sqlx::{AnyPool, Column, Row};
use std::collections::HashSet;

fn is_pg(url: &str) -> bool {
    url.starts_with("postgres://") || url.starts_with("postgresql://")
}

fn is_mysql(url: &str) -> bool {
    url.starts_with("mysql://") || url.starts_with("mariadb://")
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

/// Get column names and data types for a table in the target database.
async fn target_column_info(target: &AnyPool, table: &str, db_url: &str) -> Vec<(String, String)> {
    if is_pg(db_url) {
        // Use udt_name for user-defined types (arrays, enums) + data_type for built-in.
        // information_schema.columns.data_type returns 'ARRAY' for array columns;
        // udt_name gives the actual type name like '_text' (internal name for text[]).
        // PG stores unquoted table names in lowercase in information_schema.
        let pg_table = table.to_lowercase();
        // Try current schema and PUBLIC fallback (litellm sometimes uses mixed case schema).
        let query = r#"SELECT column_name::text,
               CASE WHEN data_type = 'ARRAY' THEN udt_name ELSE data_type END::text
        FROM information_schema.columns
        WHERE lower(table_name) = $1
        ORDER BY ordinal_position"#;
        let rows = sqlx::query(query)
        .bind(&pg_table)
        .fetch_all(target)
        .await;
        let result: Vec<(String, String)> = match rows {
            Ok(ref r) if !r.is_empty() => r
                .iter()
                .map(|row| (row.get::<String, _>(0), row.get::<String, _>(1)))
                .collect(),
            Ok(_) => { eprintln!("  [DEBUG] target_column_info({table}): empty result"); Vec::new() }
            Err(ref e) => { eprintln!("  [DEBUG] target_column_info({table}) error: {e}"); Vec::new() }
        };
        result
    } else if is_mysql(db_url) {
        // Use INFORMATION_SCHEMA instead of SHOW COLUMNS because sqlx Any driver
        // cannot decode the Type column from SHOW COLUMNS results on MySQL.
        // Decode DATA_TYPE as Vec<u8> and convert to String because mysql
        // driver may return it as BLOB.
        let rows = sqlx::query(
            "SELECT COLUMN_NAME, DATA_TYPE \
             FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_NAME = ? AND TABLE_SCHEMA = DATABASE() \
             ORDER BY ORDINAL_POSITION",
        )
        .bind(table)
        .fetch_all(target)
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
                    (name, ty)
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    } else {
        // SQLite
        let rows = sqlx::query(&format!("PRAGMA table_info(\"{}\")", table))
            .fetch_all(target)
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

fn placeholder(i: usize, target_url: &str) -> String {
    if is_pg(target_url) {
        format!("${}", i + 1)
    } else {
        "?".to_string()
    }
}

/// Build a CAST expression for PostgreSQL columns that need explicit type casts
/// (jsonb, timestamp, timestamptz). Returns the placeholder unchanged for non-PG
/// or non-cast-needing columns.
fn cast_expr(col_name: &str, col_ty: Option<&str>, ph: &str, target_url: &str) -> String {
    if !is_pg(target_url) {
        return ph.to_string();
    }
    let ty = col_ty.unwrap_or("").to_lowercase();
    // Numeric columns: bind_cell() handles type coercion (empty string → NULL::f64,
    // "100.0" → f64), so no SQL-level cast needed. Plain placeholder is correct.
    if is_numeric(&ty) && !col_name.starts_with("user_api_key_hash") {
        return ph.to_string();
    }
    if ty == "boolean" {
        return format!("CAST(NULLIF({}, '') AS boolean)", ph);
    }
    if (ty == "jsonb" || ty == "json") && !col_name.starts_with("user_api_key_hash") {
        format!("NULLIF({}, '')::jsonb", ph)
    } else if ty.contains("timestamp") {
        if ty.contains("with time zone") || ty == "timestamptz" {
            format!("CAST(NULLIF({}, '') AS timestamptz)", ph)
        } else {
            format!("CAST(NULLIF({}, '') AS timestamp)", ph)
        }
    } else if (ty.ends_with("[]") || ty.starts_with("_")) && !col_name.starts_with("user_api_key_hash") {
        format!("string_to_array(NULLIF({}, ''), ',')", ph)
    } else {
        ph.to_string()
    }
}

/// Get source column info for a table and build a SELECT expression that casts
/// non-text columns to TEXT for sqlx::Any compatibility (it cannot decode SQLite DATETIME).
/// `source_url` is the URL of the DB being queried (source/sqlite DB for plain tables).
async fn build_select_cols(source: &AnyPool, table: &str, _source_url: &str) -> String {
    // Probe source table columns. Try SQLite PRAGMA first; fall back to
    // information_schema for PG/MySQL. sqlx::Any cannot decode SQLite DATETIME
    // or BOOLEAN — CAST them to TEXT.
    let sql = format!("PRAGMA table_info(\"{}\")", table);
    let rows_res = sqlx::query(&sql).fetch_all(source).await;

    let (names, types): (Vec<String>, Vec<String>) = match rows_res {
        Ok(ref rows) if !rows.is_empty() => {
            let names: Vec<String> = rows.iter().map(|r| r.try_get::<String, _>(1).unwrap_or_default()).collect();
            let types: Vec<String> = rows.iter().map(|r| r.try_get::<String, _>(2).unwrap_or_default()).collect();
            (names, types)
        }
        _ => return "*".to_string(),
    };

    let cols: Vec<String> = names.iter().zip(types.iter()).map(|(name, ty)| {
        let ty_lower = ty.to_lowercase();
        if ty_lower.contains("datetime")
            || ty_lower.contains("timestamp")
            || ty_lower.contains("date")
            || ty_lower.contains("bool")
        {
            format!("CAST(\"{}\" AS TEXT) AS \"{}\"", name, name)
        } else {
            format!("\"{}\"", name)
        }
    }).collect();

    cols.join(", ")
}

/// Normalize a string value for the target column type.
/// - Empty strings → None for numeric / timestamp / array PG columns
/// - Empty strings → "{}" for JSON columns
/// - Otherwise pass through.
fn normalize_string_value(val: String, col_ty: &str) -> Option<String> {
    if val.is_empty() {
        let ty = col_ty.to_lowercase();
        if ty == "jsonb" || ty == "json" {
            Some("{}".to_string())
        } else if ty == "text" || ty == "varchar" || ty == "character varying" || ty == "" {
            Some(String::new())
        } else {
            // Numeric, timestamp, boolean, array, etc. — convert to NULL
            None
        }
    } else {
        Some(val)
    }
}

/// Returns true if the PG column type is numeric (int/float/etc.).
fn is_numeric(col_ty: &str) -> bool {
    let ty = col_ty.to_lowercase();
    ty.contains("int") || ty == "integer" || ty == "bigint" || ty == "smallint"
        || ty == "tinyint" || ty == "serial" || ty == "bigserial"
        || ty.contains("numeric") || ty.contains("decimal")
        || ty.contains("double") || ty.contains("real") || ty.contains("float")
}

/// Bind a source row value to a sqlx query parameter, with PostgreSQL
/// numeric-column awareness so that text source values (empty strings,
/// "100.0") don't cause "text vs double precision" PREPARE errors.
fn bind_cell<'q>(
    q: sqlx::query::Query<'q, sqlx::Any, <sqlx::Any as sqlx::Database>::Arguments<'q>>,
    value: &sqlx::any::AnyRow,
    idx: usize,
    col_ty: Option<&str>,
    target_url: &str,
) -> sqlx::query::Query<'q, sqlx::Any, <sqlx::Any as sqlx::Database>::Arguments<'q>> {
    let ty_lower = col_ty.unwrap_or("").to_lowercase();
    if is_pg(target_url) && is_numeric(&ty_lower) {
        if let Ok(v) = value.try_get::<String, _>(idx) {
            if v.is_empty() { return q.bind(None::<f64>); }
            return q.bind(v.parse::<f64>().ok());
        }
        if let Ok(v) = value.try_get::<i64, _>(idx) { return q.bind(v as f64); }
        if let Ok(v) = value.try_get::<f64, _>(idx) { return q.bind(v); }
        return q.bind(None::<f64>);
    }
    if is_pg(target_url) && ty_lower == "boolean" {
        if let Ok(v) = value.try_get::<String, _>(idx) {
            if v.is_empty() { return q.bind(None::<bool>); }
            let v = v.to_lowercase();
            return q.bind(v == "true" || v == "1");
        }
        if let Ok(v) = value.try_get::<i64, _>(idx) { return q.bind(v != 0); }
        if let Ok(v) = value.try_get::<bool, _>(idx) { return q.bind(v); }
        return q.bind(None::<bool>);
    }
    if is_pg(target_url) && (ty_lower == "jsonb" || ty_lower == "json") {
        // PG jsonb/json — bind as String, but cast_expr adds ::jsonb
        if let Ok(v) = value.try_get::<String, _>(idx) {
            if v.is_empty() { return q.bind("{}"); }
            return q.bind(v);
        }
        return q.bind("{}");
    }
    if is_pg(target_url) && ty_lower.contains("timestamp") {
        // PG timestamp — bind as String, cast_expr adds CAST(... AS timestamp)
        if let Ok(v) = value.try_get::<String, _>(idx) {
            if v.is_empty() { return q.bind(None::<String>); }
            return q.bind(v);
        }
        return q.bind(None::<String>);
    }
    // non-PG or text/varchar: bind as-is
    if let Ok(v) = value.try_get::<String, _>(idx) {
        if v.is_empty() {
            return q.bind(None::<String>);
        }
        return q.bind(v);
    }
    if let Ok(v) = value.try_get::<i64, _>(idx) { return q.bind(v); }
    if let Ok(v) = value.try_get::<f64, _>(idx) { return q.bind(v); }
    q.bind(None::<String>)
}

/// Connect to a database from a file path or URL.
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

/// Extract litellm master_key from the LiteLLM_Config table in the target DB.
async fn extract_target_master_key(
    target: &AnyPool,
    target_url: &str,
) -> anyhow::Result<Option<String>> {
    let table = quote_ident("LiteLLM_Config", target_url);
    let row = sqlx::query(&format!(
        "SELECT param_value FROM {} WHERE param_name = 'litellm_master_key'",
        table,
    ))
    .fetch_optional(target)
    .await?;

    Ok(row.and_then(|r| r.try_get::<String, _>(0).ok()))
}

/// Tables without encrypted fields — plain copy (aigw name → litellm name).
const PLAIN_TABLES: &[(&str, &str)] = &[
    ("organizations", "LiteLLM_OrganizationTable"),
    ("teams", "LiteLLM_TeamTable"),
    ("users", "LiteLLM_UserTable"),
    ("projects", "LiteLLM_ProjectTable"),
    ("budgets", "LiteLLM_BudgetTable"),
    ("organization_memberships", "LiteLLM_OrganizationMembership"),
    ("team_memberships", "LiteLLM_TeamMembership"),
    ("virtual_keys", "LiteLLM_VerificationToken"),
    ("config", "LiteLLM_Config"),
];

/// Copy all rows from src_table (aigw) to tgt_table (litellm).
/// Only inserts columns that exist in the target table (intersection of source & target columns).
async fn migrate_plain_table(
    source: &AnyPool,
    target: &AnyPool,
    target_url: &str,
    src_table: &str,
    tgt_table: &str,
) -> anyhow::Result<usize> {
    let src_quoted = quote_ident(src_table, target_url);
    // Build SELECT with CAST(TEXT) for non-TEXT source columns.
    // The source pool may be SQLite or PG; probe its column types.
    // sqlx::Any cannot decode SQLite DATETIME/BOOLEAN types — CAST them to TEXT.
    // Use a simple PRAGMA/INFORMATION_SCHEMA probe to get column types from source.
    let select_cols = build_select_cols(source, src_table, target_url).await;
    let query = format!("SELECT {} FROM {}", select_cols, src_quoted);
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

    // Get source column names from the first row
    let src_columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    // Get target column info (name + type) from metadata
    let tgt_col_info = target_column_info(target, tgt_table, target_url).await;
    let tgt_set: HashSet<&str> = tgt_col_info.iter().map(|(s, _)| s.as_str()).collect();
    let tgt_type_map: std::collections::HashMap<&str, &str> = tgt_col_info
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();

    // Filter to columns present in both source and target (intersection)
    let insert_cols: Vec<(&str, Option<&str>, usize)> = src_columns
        .iter()
        .enumerate()
        .filter(|(_, name)| tgt_set.contains(name.as_str()))
        .map(|(idx, name)| (name.as_str(), tgt_type_map.get(name.as_str()).copied(), idx))
        .collect();

    if insert_cols.is_empty() {
        eprintln!(
            "  [SKIP] {}: no intersecting columns with target",
            src_table
        );
        return Ok(0);
    }

    let mut inserted = 0usize;
    let pg_conflict = if is_pg(target_url) {
        " ON CONFLICT DO NOTHING"
    } else {
        ""
    };
    for row in &rows {
        let values_expr: Vec<String> = insert_cols
            .iter()
            .enumerate()
            .map(|(ph_idx, (name, col_ty, _src_idx))| {
                let ph = placeholder(ph_idx, target_url);
                cast_expr(name, *col_ty, &ph, target_url)
            })
            .collect();

        let tgt_quoted = quote_ident(tgt_table, target_url);
        let insert_sql = format!(
            "INSERT INTO {} ({}) VALUES ({}){}",
            tgt_quoted,
            insert_cols
                .iter()
                .map(|(n, _, _)| quote_ident(n, target_url))
                .collect::<Vec<_>>()
                .join(", "),
            values_expr.join(", "),
            pg_conflict,
        );

        let mut q = sqlx::query(&insert_sql);
        for &(_name, col_ty, src_idx) in &insert_cols {
            q = bind_cell(q, row, src_idx, col_ty, target_url);
        }
        q.execute(target).await?;
        inserted += 1;
    }

    Ok(inserted)
}

/// Migrate credentials: aigw → litellm with key rotation.
async fn migrate_credentials(
    source: &AnyPool,
    target: &AnyPool,
    target_url: &str,
    source_key: &str,
    target_key: &str,
) -> anyhow::Result<usize> {
    let cred_quoted = quote_ident("credentials", target_url);
    let query = format!("SELECT * FROM {}", cred_quoted);
    let rows = match sqlx::query(&query).fetch_all(source).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] credentials: {}", e);
            return Ok(0);
        }
    };

    if rows.is_empty() {
        return Ok(0);
    }

    let src_columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    // Get target column types for proper CAST (jsonb columns need explicit CAST)
    let tgt_col_info = target_column_info(target, "LiteLLM_CredentialsTable", target_url).await;
    let tgt_type_map: std::collections::HashMap<&str, &str> = tgt_col_info
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();

    let values_col = src_columns
        .iter()
        .position(|c| c == "credential_values")
        .unwrap_or_else(|| {
            eprintln!("  [WARN] credential_values column not found, using index 2");
            2
        });

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    let pg_conflict = if is_pg(target_url) {
        " ON CONFLICT DO NOTHING"
    } else {
        ""
    };
    for row in &rows {
        let col_count = row.columns().len();
        let values_expr: Vec<String> = (0..col_count)
            .map(|i| {
                let ph = placeholder(i, target_url);
                let col_name = &src_columns[i];
                cast_expr(
                    col_name,
                    tgt_type_map.get(col_name.as_str()).copied(),
                    &ph,
                    target_url,
                )
            })
            .collect();

        let cred_cols_quoted: Vec<String> = src_columns
            .iter()
            .map(|c| quote_ident(c, target_url))
            .collect();
        let insert_sql = format!(
            "INSERT INTO {} ({}) VALUES ({}){}",
            quote_ident("LiteLLM_CredentialsTable", target_url),
            cred_cols_quoted.join(", "),
            values_expr.join(", "),
            pg_conflict,
        );

        let mut q = sqlx::query(&insert_sql);
        for i in 0..col_count {
            if i == values_col {
                if let Ok(encrypted) = row.try_get::<String, _>(i) {
                    if encrypted.is_empty() || encrypted == "{}" {
                        q = q.bind(encrypted);
                    } else {
                        match aigw_core::decrypt_litellm_value(&encrypted, source_key) {
                            Ok(plaintext) => {
                                match aigw_core::encrypt_litellm_value(&plaintext, target_key) {
                                    Ok(re_encrypted) => q = q.bind(re_encrypted),
                                    Err(e) => {
                                        eprintln!("  [WARN] Re-encrypt: {}", e);
                                        q = q.bind(encrypted);
                                        skipped += 1;
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("  [WARN] Decrypt: {}", e);
                                q = q.bind(encrypted);
                                skipped += 1;
                            }
                        }
                    }
                } else {
                    q = bind_cell(q, row, i, tgt_type_map.get(src_columns[i].as_str()).copied(), target_url);
                }
            } else if i == values_col && is_pg(target_url) {
                // credential_values is always jsonb in litellm PG schema.
                // bind_cell handles it as text → PG will reject unless we use
                // explicit ::jsonb cast.  But bind_cell doesn't do jsonb casts;
                // this is handled by the encrypted branch above.
                q = bind_cell(q, row, i, tgt_type_map.get(src_columns[i].as_str()).copied(), target_url);
            } else {
                q = bind_cell(q, row, i, tgt_type_map.get(src_columns[i].as_str()).copied(), target_url);
            }
        }
        q.execute(target).await?;
        inserted += 1;
    }

    if skipped > 0 {
        eprintln!("  [WARN] Skipped {} credential rows", skipped);
    }

    Ok(inserted)
}

/// Migrate proxy_models: aigw → litellm with key rotation.
async fn migrate_proxy_models(
    source: &AnyPool,
    target: &AnyPool,
    target_url: &str,
    source_key: &str,
    target_key: &str,
) -> anyhow::Result<usize> {
    let models_quoted = quote_ident("proxy_models", target_url);
    let query = format!("SELECT * FROM {}", models_quoted);
    let rows = match sqlx::query(&query).fetch_all(source).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] proxy_models: {}", e);
            return Ok(0);
        }
    };

    if rows.is_empty() {
        return Ok(0);
    }

    let src_columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    // Get target column types for jsonb CAST
    let tgt_col_info = target_column_info(target, "LiteLLM_ProxyModelTable", target_url).await;
    let tgt_type_map: std::collections::HashMap<&str, &str> = tgt_col_info
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();

    let params_col = src_columns
        .iter()
        .position(|c| c == "litellm_params")
        .unwrap_or_else(|| {
            eprintln!("  [WARN] litellm_params column not found, using index 2");
            2
        });

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    let pg_conflict = if is_pg(target_url) {
        " ON CONFLICT DO NOTHING"
    } else {
        ""
    };
    for row in &rows {
        let col_count = row.columns().len();
        let values_expr: Vec<String> = (0..col_count)
            .map(|i| {
                let ph = placeholder(i, target_url);
                let col_name = &src_columns[i];
                cast_expr(
                    col_name,
                    tgt_type_map.get(col_name.as_str()).copied(),
                    &ph,
                    target_url,
                )
            })
            .collect();

        let proxy_table = quote_ident("LiteLLM_ProxyModelTable", target_url);
        let insert_sql = format!(
            "INSERT INTO {} ({}) VALUES ({}){}",
            proxy_table,
            src_columns.join(", "),
            values_expr.join(", "),
            pg_conflict,
        );

        let mut q = sqlx::query(&insert_sql);
        for i in 0..col_count {
            if i == params_col {
                if let Ok(value) = row.try_get::<String, _>(i) {
                    if value.is_empty() || value.starts_with('{') {
                        q = q.bind(value);
                    } else {
                        match aigw_core::decrypt_litellm_value(&value, source_key) {
                            Ok(plaintext) => {
                                match aigw_core::encrypt_litellm_value(&plaintext, target_key) {
                                    Ok(re_encrypted) => q = q.bind(re_encrypted),
                                    Err(e) => {
                                        eprintln!("  [WARN] Re-encrypt: {}", e);
                                        q = q.bind(value);
                                        skipped += 1;
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("  [WARN] Decrypt: {}", e);
                                q = q.bind(value);
                                skipped += 1;
                            }
                        }
                    }
                } else {
                    q = bind_cell(q, row, i, tgt_type_map.get(src_columns[i].as_str()).copied(), target_url);
                }
            } else {
                q = bind_cell(q, row, i, tgt_type_map.get(src_columns[i].as_str()).copied(), target_url);
            }
        }
        q.execute(target).await?;
        inserted += 1;
    }

    if skipped > 0 {
        eprintln!("  [WARN] Skipped {} model rows", skipped);
    }

    Ok(inserted)
}

/// Batch migrate spend_logs (no crypto).
async fn migrate_spend_logs(
    source: &AnyPool,
    target: &AnyPool,
    target_url: &str,
) -> anyhow::Result<usize> {
    let logs_quoted = quote_ident("spend_logs", target_url);
    let query = format!("SELECT * FROM {}", logs_quoted);
    let rows = match sqlx::query(&query).fetch_all(source).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] spend_logs: {}", e);
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

    let mut inserted = 0usize;
    let pg_conflict = if is_pg(target_url) {
        " ON CONFLICT DO NOTHING"
    } else {
        ""
    };
    for row in &rows {
        let col_count = row.columns().len();
        let placeholders: Vec<String> =
            (0..col_count).map(|i| placeholder(i, target_url)).collect();

        let spend_table = quote_ident("LiteLLM_SpendLogs", target_url);
        let insert_sql = format!(
            "INSERT INTO {} ({}) VALUES ({}){}",
            spend_table,
            columns.join(", "),
            placeholders.join(", "),
            pg_conflict,
        );

        let mut q = sqlx::query(&insert_sql);
        for i in 0..col_count {
            q = bind_cell(q, row, i, None, target_url);
        }
        q.execute(target).await?;
        inserted += 1;
    }

    Ok(inserted)
}

pub async fn run(
    source_url: &str,
    target_url: &str,
    source_master_key: &str,
    target_master_key: Option<&str>,
) -> anyhow::Result<bool> {
    sqlx::any::install_default_drivers();
    let source = connect(source_url).await?;
    let target = connect(target_url).await?;

    // Step 1: Determine target litellm master_key
    let target_key = match target_master_key {
        Some(k) => k.to_string(),
        None => match extract_target_master_key(&target, target_url).await? {
            Some(k) => {
                println!("  Extracted master_key from LiteLLM_Config in target DB");
                k
            }
            None => {
                anyhow::bail!(
                    "No target master_key found. Provide --target-master-key or \
                     ensure target DB has LiteLLM_Config with param_name='litellm_master_key'"
                );
            }
        },
    };

    println!("Step 1: Source master_key = AIGW_MASTER_KEY, Target master_key obtained");

    // Step 2: Migrate plain tables
    println!("Step 2: Exporting plain tables (aigw → litellm)...");
    for &(src, tgt) in PLAIN_TABLES {
        let count = migrate_plain_table(&source, &target, target_url, src, tgt).await?;
        println!("  {} -> {} ({} rows)", src, tgt, count);
    }

    // Step 3: Migrate credentials with key rotation (aigw→litellm)
    println!("Step 3: Exporting credentials (with key rotation)...");
    let cred_count =
        migrate_credentials(&source, &target, target_url, source_master_key, &target_key).await?;
    println!(
        "  credentials -> LiteLLM_CredentialsTable ({} rows)",
        cred_count
    );

    // Step 4: Migrate proxy_models with key rotation
    println!("Step 4: Exporting proxy_models (with key rotation)...");
    let model_count =
        migrate_proxy_models(&source, &target, target_url, source_master_key, &target_key).await?;
    println!(
        "  proxy_models -> LiteLLM_ProxyModelTable ({} rows)",
        model_count
    );

    // Step 5: Migrate spend_logs
    println!("Step 5: Exporting spend_logs...");
    let spend_count = migrate_spend_logs(&source, &target, target_url).await?;
    println!("  spend_logs -> LiteLLM_SpendLogs ({} rows)", spend_count);

    // Step 6: Verify
    println!("Step 6: Verifying row counts...");
    let mut all_match = true;
    let all_tables: &[(&str, &str)] = &[
        ("organizations", "LiteLLM_OrganizationTable"),
        ("teams", "LiteLLM_TeamTable"),
        ("users", "LiteLLM_UserTable"),
        ("projects", "LiteLLM_ProjectTable"),
        ("budgets", "LiteLLM_BudgetTable"),
        ("organization_memberships", "LiteLLM_OrganizationMembership"),
        ("team_memberships", "LiteLLM_TeamMembership"),
        ("virtual_keys", "LiteLLM_VerificationToken"),
        ("config", "LiteLLM_Config"),
        ("credentials", "LiteLLM_CredentialsTable"),
        ("proxy_models", "LiteLLM_ProxyModelTable"),
        ("spend_logs", "LiteLLM_SpendLogs"),
    ];

    for &(src, tgt) in all_tables {
        let src_quoted = if is_mysql(target_url) {
            format!("`{}`", src)
        } else {
            format!("\"{}\"", src)
        };
        let src_count: i64 = sqlx::query(&format!("SELECT COUNT(*) FROM {}", src_quoted))
            .fetch_one(&source)
            .await
            .map(|row| row.get(0))
            .unwrap_or(0);

        let tgt_quoted = if is_mysql(target_url) {
            format!("`{}`", tgt)
        } else {
            format!("\"{}\"", tgt)
        };
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
        println!(
            "  {} -> {}: src={} tgt={} [{}]",
            src, tgt, src_count, tgt_count, status
        );
    }

    Ok(all_match)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    #[tokio::test]
    async fn test_remote_export_roundtrip() {
        sqlx::any::install_default_drivers();
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.db");
        let tgt_path = dir.path().join("tgt.db");
        let src_str = src_path.to_str().unwrap();
        let tgt_str = tgt_path.to_str().unwrap();

        // Setup aigw source DB
        let src_pool: sqlx::SqlitePool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(src_str)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        sqlx::query(
            r#"CREATE TABLE "organizations" (
                organization_id TEXT PRIMARY KEY, organization_alias TEXT, spend REAL DEFAULT 0
            )"#,
        )
        .execute(&src_pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO \"organizations\" (organization_id, organization_alias, spend) VALUES ('org-1', 'test-org', 42.0)"
        )
        .execute(&src_pool)
        .await
        .unwrap();

        // Add credentials table with encrypted values
        sqlx::query(
            r#"CREATE TABLE "credentials" (
                credential_id TEXT PRIMARY KEY, credential_name TEXT, credential_values TEXT, credential_info TEXT
            )"#,
        )
        .execute(&src_pool)
        .await
        .unwrap();
        let aigw_key = "sk-aigw-source-key-00001";
        let plain_cred = r#"{"api_key":"sk-secret","api_base":"https://api.openai.com"}"#;
        let encrypted_cred = aigw_core::encrypt_litellm_value(plain_cred, aigw_key).unwrap();
        sqlx::query(
            "INSERT INTO \"credentials\" (credential_id, credential_name, credential_values) VALUES ('cred-1', 'test-cred', ?)"
        )
        .bind(&encrypted_cred)
        .execute(&src_pool)
        .await
        .unwrap();

        // Add proxy_models with encrypted params
        sqlx::query(
            r#"CREATE TABLE "proxy_models" (
                model_id TEXT PRIMARY KEY, model_name TEXT, litellm_params TEXT, model_info TEXT
            )"#,
        )
        .execute(&src_pool)
        .await
        .unwrap();
        let plain_params = r#"{"model":"gpt-4","api_key":"sk-model-key"}"#;
        let encrypted_params = aigw_core::encrypt_litellm_value(plain_params, aigw_key).unwrap();
        sqlx::query(
            "INSERT INTO \"proxy_models\" (model_id, model_name, litellm_params) VALUES ('model-1', 'gpt-4', ?)"
        )
        .bind(&encrypted_params)
        .execute(&src_pool)
        .await
        .unwrap();

        // Add spend_logs
        sqlx::query(
            r#"CREATE TABLE "spend_logs" (request_id TEXT PRIMARY KEY, model TEXT, spend REAL DEFAULT 0)"#,
        )
        .execute(&src_pool)
        .await
        .unwrap();
        src_pool.close().await;

        // Setup litellm target DB
        let tgt_pool: sqlx::SqlitePool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(tgt_str)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_OrganizationTable" (
                organization_id TEXT PRIMARY KEY, organization_alias TEXT, spend REAL DEFAULT 0
            )"#,
        )
        .execute(&tgt_pool)
        .await
        .unwrap();

        // Create litellm target tables with correct columns
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_CredentialsTable" (
                credential_id TEXT PRIMARY KEY, credential_name TEXT, credential_values TEXT, credential_info TEXT
            )"#,
        )
        .execute(&tgt_pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_ProxyModelTable" (
                model_id TEXT PRIMARY KEY, model_name TEXT, litellm_params TEXT, model_info TEXT
            )"#,
        )
        .execute(&tgt_pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_SpendLogs" (
                request_id TEXT PRIMARY KEY, model TEXT, spend REAL DEFAULT 0
            )"#,
        )
        .execute(&tgt_pool)
        .await
        .unwrap();
        tgt_pool.close().await;

        let litellm_key = "sk-litellm-target-key-00099";
        let result = run(src_str, tgt_str, aigw_key, Some(litellm_key)).await;
        assert!(result.is_ok(), "remote_export failed: {:?}", result.err());

        // Verify credentials were re-encrypted with litellm key
        let tgt_pool: sqlx::SqlitePool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(tgt_str)
                    .create_if_missing(true),
            )
            .await
            .unwrap();

        let cred_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM \"LiteLLM_CredentialsTable\"")
                .fetch_one(&tgt_pool)
                .await
                .unwrap();
        assert_eq!(cred_count.0, 1, "should have 1 credential in litellm DB");

        let cred_row: (String,) = sqlx::query_as(
            "SELECT credential_values FROM \"LiteLLM_CredentialsTable\" WHERE credential_id = 'cred-1'",
        )
        .fetch_one(&tgt_pool)
        .await
        .unwrap();
        let decrypted = aigw_core::decrypt_litellm_value(&cred_row.0, litellm_key).unwrap();
        assert_eq!(
            decrypted, plain_cred,
            "credential should decrypt with litellm key"
        );

        // Verify proxy_models
        let model_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM \"LiteLLM_ProxyModelTable\"")
                .fetch_one(&tgt_pool)
                .await
                .unwrap();
        assert_eq!(model_count.0, 1, "should have 1 proxy model in litellm DB");

        let model_row: (String,) = sqlx::query_as(
            "SELECT litellm_params FROM \"LiteLLM_ProxyModelTable\" WHERE model_id = 'model-1'",
        )
        .fetch_one(&tgt_pool)
        .await
        .unwrap();
        let decrypted_params = aigw_core::decrypt_litellm_value(&model_row.0, litellm_key).unwrap();
        assert_eq!(
            decrypted_params, plain_params,
            "model params should decrypt with litellm key"
        );

        tgt_pool.close().await;
    }
}
