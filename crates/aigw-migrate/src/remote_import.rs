//! remote-import: Full litellm → aigw migration with encryption key rotation.
//!
//! Pipeline:
//!   1. Connect to source (litellm) and target (aigw) via native pools
//!   2. Extract litellm master_key from LiteLLM_Config or CLI arg
//!   3. Migrate plain tables (no encrypted fields)
//!   4. Migrate credentials — decrypt credential_values, re-encrypt with aigw key
//!   5. Migrate proxy_models — decrypt litellm_params, re-encrypt with aigw key
//!   6. Batch migrate spend_logs
//!
//! All cross-database type coercion is handled by [crate::native].

use crate::native::{self, SourcePool};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

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

/// Build a column name override mapping (camelCase → snake_case) from source columns.
fn build_snake_overrides(src_columns: &[String]) -> HashMap<String, String> {
    src_columns
        .iter()
        .map(|n| (camel_to_snake(n), n.clone()))
        .collect()
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Master key extraction
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn extract_source_master_key(source: &SourcePool) -> anyhow::Result<Option<String>> {
    let tbl = source.quote_ident("LiteLLM_Config");

    // Strategy 1: legacy flat key
    let sql = format!(
        "SELECT param_value FROM {} WHERE param_name = 'litellm_master_key'",
        tbl
    );
    if let Some(val) = source.query_scalar_string(&sql).await? {
        if !val.is_empty() {
            return Ok(Some(val));
        }
    }

    // Strategy 2: general_settings JSON
    let sql = format!(
        "SELECT param_value FROM {} WHERE param_name = 'general_settings'",
        tbl
    );
    if let Some(val) = source.query_scalar_string(&sql).await? {
        if let Ok(parsed) = serde_json::from_str::<Value>(&val) {
            if let Some(mk) = parsed.get("master_key").and_then(|v| v.as_str()) {
                if !mk.is_empty() {
                    return Ok(Some(mk.to_string()));
                }
            }
        }
    }

    Ok(None)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Plain table migration
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

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

async fn migrate_plain_table(
    source: &SourcePool,
    target: &SourcePool,
    src_table: &str,
    tgt_table: &str,
) -> anyhow::Result<usize> {
    let rows = match source.read_rows(src_table).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] {}: {}", src_table, e);
            return Ok(0);
        }
    };
    if rows.is_empty() {
        return Ok(0);
    }

    let src_col_names: Vec<String> = rows[0].iter().map(|(n, _)| n.clone()).collect();
    let tgt_col_info = target.column_types(tgt_table).await?;
    let overrides = build_snake_overrides(&src_col_names);

    if tgt_col_info.is_empty() {
        eprintln!("  [SKIP] {}: no target columns", src_table);
        return Ok(0);
    }

    let count = native::insert_rows(target, tgt_table, &tgt_col_info, &rows, &overrides).await?;
    Ok(count)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Credentials migration (with key rotation)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn migrate_credentials(
    source: &SourcePool,
    target: &SourcePool,
    source_key: &str,
    target_key: &str,
) -> anyhow::Result<usize> {
    let rows = match source.read_rows("LiteLLM_CredentialsTable").await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] LiteLLM_CredentialsTable: {}", e);
            return Ok(0);
        }
    };
    if rows.is_empty() {
        return Ok(0);
    }

    let src_col_names: Vec<String> = rows[0].iter().map(|(n, _)| n.clone()).collect();
    let tgt_col_info = target.column_types("credentials").await?;
    let overrides = build_snake_overrides(&src_col_names);

    if tgt_col_info.is_empty() {
        eprintln!("  [SKIP] credentials: no target columns");
        return Ok(0);
    }

    let values_col = tgt_col_info
        .iter()
        .position(|(n, _)| n == "credential_values");

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    for row in &rows {
        let row_map: HashMap<&str, &Value> = row.iter().map(|(n, v)| (n.as_str(), v)).collect();

        let values: Vec<String> = tgt_col_info
            .iter()
            .enumerate()
            .map(|(idx, (col_name, col_type))| {
                let v: Value = if values_col == Some(idx) {
                    // credential_values: decrypt with source key, re-encrypt with target key
                    let encrypted = row_map.get(col_name.as_str()).copied().unwrap_or(&Value::Null);
                    let encrypted_str = encrypted.as_str().unwrap_or("");
                    if encrypted_str.is_empty() || encrypted_str == "{}" {
                        Value::String(encrypted_str.to_string())
                    } else {
                        let rotated = rotate_field(encrypted_str, source_key, target_key, &mut skipped);
                        Value::String(rotated.unwrap_or_else(|| encrypted_str.to_string()))
                    }
                } else {
                    row_map
                        .get(col_name.as_str())
                        .or_else(|| overrides.get(col_name.as_str())
                            .and_then(|m| row_map.get(m.as_str())))
                        .copied()
                        .unwrap_or(&Value::Null)
                        .clone()
                };
                native::value_to_target_literal(&v, col_type, target.kind())
            })
            .collect();

        let tbl_quoted = target.quote_ident("credentials");
        let quoted_cols: Vec<String> = tgt_col_info.iter().map(|(n, _)| target.quote_ident(n)).collect();
        let sql = format!(
            "{}{} ({}) VALUES ({}){}",
            target.insert_prefix(),
            tbl_quoted,
            quoted_cols.join(", "),
            values.join(", "),
            target.conflict_clause(),
        );
        target.execute_raw(&sql).await?;
        inserted += 1;
    }

    if skipped > 0 {
        eprintln!("  [WARN] Skipped {} credential rows due to crypto errors", skipped);
    }
    Ok(inserted)
}

fn rotate_field(encrypted: &str, source_key: &str, target_key: &str, skipped: &mut usize) -> Option<String> {
    if encrypted.starts_with('{') {
        // JSON object — rotate individual encrypted fields
        match serde_json::from_str::<Value>(encrypted) {
            Ok(json_val) => {
                match aigw_core::rotate_json_fields(&json_val, source_key, target_key) {
                    Ok(rotated) => {
                        match aigw_core::encrypt_litellm_value(&rotated, target_key) {
                            Ok(re_encrypted) => return Some(re_encrypted),
                            Err(_) => { *skipped += 1; }
                        }
                    }
                    Err(_) => { *skipped += 1; }
                }
            }
            Err(_) => {}
        }
    } else {
        // Simple encrypted string
        if let Ok(plaintext) = aigw_core::decrypt_litellm_value(encrypted, source_key) {
            if let Ok(re_encrypted) = aigw_core::encrypt_litellm_value(&plaintext, target_key) {
                return Some(re_encrypted);
            }
        }
        *skipped += 1;
    }
    None
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Proxy models migration (with key rotation)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn migrate_proxy_models(
    source: &SourcePool,
    target: &SourcePool,
    source_key: &str,
    target_key: &str,
) -> anyhow::Result<usize> {
    let rows = match source.read_rows("LiteLLM_ProxyModelTable").await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] LiteLLM_ProxyModelTable: {}", e);
            return Ok(0);
        }
    };
    if rows.is_empty() {
        return Ok(0);
    }

    let src_col_names: Vec<String> = rows[0].iter().map(|(n, _)| n.clone()).collect();
    let tgt_col_info = target.column_types("proxy_models").await?;
    let overrides = build_snake_overrides(&src_col_names);

    if tgt_col_info.is_empty() {
        eprintln!("  [SKIP] proxy_models: no target columns");
        return Ok(0);
    }

    let params_col = tgt_col_info.iter().position(|(n, _)| n == "litellm_params");

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    for row in &rows {
        let row_map: HashMap<&str, &Value> = row.iter().map(|(n, v)| (n.as_str(), v)).collect();

        let values: Vec<String> = tgt_col_info
            .iter()
            .enumerate()
            .map(|(idx, (col_name, col_type))| {
                if params_col == Some(idx) {
                    let value_str = row_map.get(col_name.as_str()).and_then(|v| v.as_str()).unwrap_or("");
                    if value_str.is_empty() {
                        return native::value_to_target_literal(&Value::String("".into()), col_type, target.kind());
                    }
                    let rotated = rotate_field(value_str, source_key, target_key, &mut skipped);
                    let v = Value::String(rotated.unwrap_or_else(|| value_str.to_string()));
                    native::value_to_target_literal(&v, col_type, target.kind())
                } else {
                    let v = row_map
                        .get(col_name.as_str())
                        .or_else(|| overrides.get(col_name.as_str())
                            .and_then(|m| row_map.get(m.as_str())))
                        .copied()
                        .unwrap_or(&Value::Null);
                    native::value_to_target_literal(v, col_type, target.kind())
                }
            })
            .collect();

        let tbl_quoted = target.quote_ident("proxy_models");
        let quoted_cols: Vec<String> = tgt_col_info.iter().map(|(n, _)| target.quote_ident(n)).collect();
        let sql = format!(
            "{}{} ({}) VALUES ({}){}",
            target.insert_prefix(),
            tbl_quoted,
            quoted_cols.join(", "),
            values.join(", "),
            target.conflict_clause(),
        );
        target.execute_raw(&sql).await?;
        inserted += 1;
    }

    if skipped > 0 {
        eprintln!("  [WARN] Skipped {} model rows due to crypto errors", skipped);
    }
    Ok(inserted)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Spend logs migration (batch, no crypto)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn migrate_spend_logs(
    source: &SourcePool,
    target: &SourcePool,
    limit: Option<usize>,
    _skip_body: bool,
    skip_columns_set: &HashSet<(String, String)>,
) -> anyhow::Result<usize> {
    let t_fetch = std::time::Instant::now();
    let rows = match source.read_rows_with_limit("LiteLLM_SpendLogs", limit).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] LiteLLM_SpendLogs: {}", e);
            return Ok(0);
        }
    };

    eprintln!("  [TIMING] spend_logs fetch: {:?} ({} rows)", t_fetch.elapsed(), rows.len());
    if rows.is_empty() {
        return Ok(0);
    }

    let src_col_names: Vec<String> = rows[0].iter().map(|(n, _)| n.clone()).collect();
    let tgt_col_info = target.column_types("spend_logs").await?;

    // Filter columns to skip
    let skipped_list: Vec<String> = tgt_col_info
        .iter()
        .filter(|(col, _)| skip_columns_set.contains(&("spend_logs".to_string(), col.clone())))
        .map(|(col, _)| format!("spend_logs.{}", col))
        .collect();
    if !skipped_list.is_empty() {
        eprintln!("  [SKIP-COLUMNS] spend_logs: {:?}", skipped_list);
    }

    let filtered_cols: Vec<(String, String)> = tgt_col_info
        .into_iter()
        .filter(|(col, _)| !skip_columns_set.contains(&("spend_logs".to_string(), col.clone())))
        .collect();

    if filtered_cols.is_empty() {
        eprintln!("  [SKIP] spend_logs: all columns filtered out");
        return Ok(0);
    }

    let overrides = build_snake_overrides(&src_col_names);

    let t_insert = std::time::Instant::now();
    let count = native::insert_rows(target, "spend_logs", &filtered_cols, &rows, &overrides).await?;
    eprintln!(
        "  [TIMING] spend_logs insert: {:?} ({} rows, avg {:?}/row)",
        t_insert.elapsed(),
        count,
        t_insert.elapsed() / count.max(1) as u32
    );

    Ok(count)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Main entry point
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub async fn run_filtered(
    source_url: &str,
    target_url: &str,
    source_master_key: Option<&str>,
    target_master_key: &str,
    spend_log_limit: Option<usize>,
    step_filter: Option<u8>,
    skip_body: bool,
    skip_columns_set: &HashSet<(String, String)>,
) -> anyhow::Result<bool> {
    let total_start = std::time::Instant::now();

    let t0 = std::time::Instant::now();
    let source = SourcePool::connect(source_url).await?;
    let target = SourcePool::connect(target_url).await?;
    eprintln!("  [TIMING] connect: {:?}", t0.elapsed());

    // Step 1: Extract source master_key
    let t0 = std::time::Instant::now();
    let source_key = match source_master_key {
        Some(k) => k.to_string(),
        None => match extract_source_master_key(&source).await? {
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
            let count = migrate_plain_table(&source, &target, src, tgt).await?;
            eprintln!("  {} -> {} ({} rows, {:?})", src, tgt, count, t_tbl.elapsed());
        }
        eprintln!("Step 2: plain tables done ({:?})", t0.elapsed());
    } else {
        eprintln!("Step 2: [SKIP]");
    }

    // Step 3: Migrate credentials
    if run_step(3) {
        eprintln!("Step 3: Migrating credentials (with key rotation)...");
        let t0 = std::time::Instant::now();
        let cred_count = migrate_credentials(&source, &target, &source_key, target_master_key).await?;
        eprintln!("  LiteLLM_CredentialsTable -> credentials ({} rows, {:?})", cred_count, t0.elapsed());
    } else {
        eprintln!("Step 3: [SKIP]");
    }

    // Step 4: Migrate proxy_models
    if run_step(4) {
        eprintln!("Step 4: Migrating proxy_models (with key rotation)...");
        let t0 = std::time::Instant::now();
        let model_count = migrate_proxy_models(&source, &target, &source_key, target_master_key).await?;
        eprintln!("  LiteLLM_ProxyModelTable -> proxy_models ({} rows, {:?})", model_count, t0.elapsed());
    } else {
        eprintln!("Step 4: [SKIP]");
    }

    // Step 5: Migrate spend_logs
    if run_step(5) {
        eprintln!("Step 5: Migrating spend_logs...");
        let t0 = std::time::Instant::now();
        let spend_count = migrate_spend_logs(&source, &target, spend_log_limit, skip_body, skip_columns_set).await?;
        eprintln!("  LiteLLM_SpendLogs -> spend_logs ({} rows, {:?})", spend_count, t0.elapsed());
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
        let src_count = source.count_rows(src).await.unwrap_or(0);
        let tgt_count = target.count_rows(tgt).await.unwrap_or(-1);

        let status = if src_count == tgt_count { "OK" } else { "MISMATCH" };
        if src_count != tgt_count {
            all_match = false;
        }
        eprintln!("  {} -> {}: src={} tgt={} [{}]", src, tgt, src_count, tgt_count, status);
    }
    eprintln!("Step 6: verify done ({:?})", t0.elapsed());

    eprintln!("[TIMING] total migration: {:?}", total_start.elapsed());
    Ok(all_match)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

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

        let src_str_sqlite = format!("sqlite://{}", src_str);
        let tgt_str_sqlite = format!("sqlite://{}", tgt_str);

        // Setup source DB
        let src_pool = create_pool(src_str).await;
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_OrganizationTable" (
                organization_id TEXT PRIMARY KEY, organization_alias TEXT, spend REAL DEFAULT 0
            )"#,
        ).execute(&src_pool).await.unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_OrganizationTable\" (organization_id, organization_alias, spend) VALUES ('org-1', 'test', 42.0)"
        ).execute(&src_pool).await.unwrap();

        sqlx::query(
            r#"CREATE TABLE "LiteLLM_Config" (param_name TEXT PRIMARY KEY, param_value TEXT)"#,
        ).execute(&src_pool).await.unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_Config\" (param_name, param_value) VALUES ('litellm_master_key', 'sk-test-source-key-12345')"
        ).execute(&src_pool).await.unwrap();

        // Credentials
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_CredentialsTable" (
                credential_id TEXT PRIMARY KEY, credential_name TEXT NOT NULL,
                credential_values TEXT, credential_info TEXT
            )"#,
        ).execute(&src_pool).await.unwrap();
        let source_key = "sk-test-source-key-12345";
        let plain_cred = r#"{"api_key":"sk-secret-123","api_base":"https://api.openai.com"}"#;
        let encrypted_cred = aigw_core::encrypt_litellm_value(plain_cred, source_key).unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_CredentialsTable\" (credential_id, credential_name, credential_values) VALUES ('cred-1', 'openai-key', ?)"
        ).bind(&encrypted_cred).execute(&src_pool).await.unwrap();

        // Proxy models
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_ProxyModelTable" (
                model_id TEXT PRIMARY KEY, model_name TEXT, litellm_params TEXT, model_info TEXT
            )"#,
        ).execute(&src_pool).await.unwrap();
        let plain_params = r#"{"model":"gpt-4","api_key":"sk-model-key-456"}"#;
        let encrypted_params = aigw_core::encrypt_litellm_value(plain_params, source_key).unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_ProxyModelTable\" (model_id, model_name, litellm_params) VALUES ('model-1', 'gpt-4', ?)"
        ).bind(&encrypted_params).execute(&src_pool).await.unwrap();

        // Spend logs
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_SpendLogs" (request_id TEXT PRIMARY KEY, model TEXT, spend REAL DEFAULT 0)"#,
        ).execute(&src_pool).await.unwrap();
        src_pool.close().await;

        // Setup target DB
        let tgt_pool = create_pool(tgt_str).await;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "organizations" (
                organization_id TEXT PRIMARY KEY, organization_alias TEXT, spend REAL DEFAULT 0
            )"#,
        ).execute(&tgt_pool).await.unwrap();

        for table in &["teams", "users", "projects", "budgets", "organization_memberships",
            "team_memberships", "virtual_keys", "spend_logs"] {
            sqlx::query(&format!("CREATE TABLE IF NOT EXISTS \"{}\" (id TEXT PRIMARY KEY)", table))
                .execute(&tgt_pool).await.unwrap();
        }

        sqlx::query(r#"CREATE TABLE IF NOT EXISTS "config" (param_name TEXT PRIMARY KEY, param_value TEXT)"#)
            .execute(&tgt_pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "credentials" (
                credential_id TEXT PRIMARY KEY, credential_name TEXT NOT NULL,
                credential_values TEXT, credential_info TEXT
            )"#,
        ).execute(&tgt_pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "proxy_models" (
                model_id TEXT PRIMARY KEY, model_name TEXT, litellm_params TEXT, model_info TEXT
            )"#,
        ).execute(&tgt_pool).await.unwrap();
        tgt_pool.close().await;

        // Run
        let target_key = "sk-aigw-target-key-99999";
        let result = run_filtered(
            &src_str_sqlite, &tgt_str_sqlite,
            None, target_key, None, None, false,
            &HashSet::new(),
        ).await;
        assert!(result.is_ok(), "remote_import failed: {:?}", result.err());

        // Verify re-encryption
        let tgt_pool = create_pool(tgt_str).await;
        let cred_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM credentials")
            .fetch_one(&tgt_pool).await.unwrap();
        assert_eq!(cred_count.0, 1);

        let cred_row: (String,) = sqlx::query_as(
            "SELECT credential_values FROM credentials WHERE credential_id = 'cred-1'",
        ).fetch_one(&tgt_pool).await.unwrap();
        let decrypted = aigw_core::decrypt_litellm_value(&cred_row.0, target_key).unwrap();
        assert_eq!(decrypted, plain_cred);

        let model_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM proxy_models")
            .fetch_one(&tgt_pool).await.unwrap();
        assert_eq!(model_count.0, 1);

        let model_row: (String,) = sqlx::query_as(
            "SELECT litellm_params FROM proxy_models WHERE model_id = 'model-1'",
        ).fetch_one(&tgt_pool).await.unwrap();
        let decrypted_params = aigw_core::decrypt_litellm_value(&model_row.0, target_key).unwrap();
        assert_eq!(decrypted_params, plain_params);

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
        ).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_Config\" (param_name, param_value) VALUES ('litellm_master_key', 'sk-extracted-key')"
        ).execute(&pool).await.unwrap();
        pool.close().await;

        let source = SourcePool::connect(db_str).await.unwrap();
        let key = extract_source_master_key(&source).await.unwrap();
        assert_eq!(key, Some("sk-extracted-key".to_string()));
    }

    #[tokio::test]
    async fn test_migrate_plain_table_empty() {
        sqlx::any::install_default_drivers();
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.db");
        let tgt_path = dir.path().join("tgt.db");
        let src_str = format!("sqlite://{}", src_path.to_str().unwrap());
        let tgt_str = format!("sqlite://{}", tgt_path.to_str().unwrap());

        let src_pool = create_pool(src_path.to_str().unwrap()).await;
        sqlx::query(r#"CREATE TABLE "LiteLLM_OrganizationTable" (organization_id TEXT)"#)
            .execute(&src_pool).await.unwrap();
        src_pool.close().await;

        let tgt_pool = create_pool(tgt_path.to_str().unwrap()).await;
        sqlx::query(r#"CREATE TABLE "organizations" (organization_id TEXT)"#)
            .execute(&tgt_pool).await.unwrap();
        tgt_pool.close().await;

        let source = SourcePool::connect(&src_str).await.unwrap();
        let target = SourcePool::connect(&tgt_str).await.unwrap();
        let count = migrate_plain_table(&source, &target, "LiteLLM_OrganizationTable", "organizations").await.unwrap();
        assert_eq!(count, 0, "empty table should migrate 0 rows");
    }
}
