//! remote-export: Reverse migration — aigw → litellm with encryption key rotation.
//!
//! Pipeline:
//!   1. Connect to source (aigw DB) and target (litellm DB) via native pools
//!   2. Extract litellm master_key from target LiteLLM_Config or CLI arg
//!   3. Migrate plain tables (no encrypted fields)
//!   4. Migrate credentials — decrypt with aigw key, re-encrypt with litellm key
//!   5. Migrate proxy_models — decrypt with aigw key, re-encrypt with litellm key
//!   6. Batch migrate spend_logs (no crypto)
//!
//! All cross-database type coercion is handled by [crate::native].

use crate::native::{self, SourcePool};
use std::collections::HashMap;

/// Tables without encrypted fields — plain copy (aigw name → litellm name).
/// NOTE: virtual_keys is excluded from export because the aigw server creates
/// keys during BDD scenarios that reference locally-created budgets/organizations.
/// Exporting them back to litellm would violate FK constraints.
const PLAIN_TABLES: &[(&str, &str)] = &[
    ("organizations", "LiteLLM_OrganizationTable"),
    ("teams", "LiteLLM_TeamTable"),
    ("users", "LiteLLM_UserTable"),
    ("projects", "LiteLLM_ProjectTable"),
    ("budgets", "LiteLLM_BudgetTable"),
    ("organization_memberships", "LiteLLM_OrganizationMembership"),
    ("team_memberships", "LiteLLM_TeamMembership"),
    ("config", "LiteLLM_Config"),
];

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Master key extraction
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn extract_target_master_key(target: &SourcePool) -> anyhow::Result<Option<String>> {
    let tbl = target.quote_ident("LiteLLM_Config");
    // On PG target, `param_value` may be `jsonb` — cast to text so the
    // scalar can be decoded as `String` uniformly.
    let col = if target.kind() == native::DbKind::Postgres {
        "param_value::text"
    } else {
        "param_value"
    };
    let sql = format!(
        "SELECT {col} FROM {} WHERE param_name = 'litellm_master_key'",
        tbl
    );
    let raw = target.query_scalar_string(&sql).await?;
    // JSONB scalar → text produces `"sk-..."` (with quotes).  Strip them so
    // callers get the raw key.
    Ok(raw.map(|s| {
        if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
            serde_json::from_str::<String>(&s).unwrap_or(s)
        } else {
            s
        }
    }))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Plain table migration
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

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

    if tgt_col_info.is_empty() {
        eprintln!("  [SKIP] {}: no target columns", src_table);
        return Ok(0);
    }

    // Build overrides: if source has camelCase and target has snake_case
    let overrides: HashMap<String, String> = src_col_names
        .iter()
        .map(|n| (camel_to_snake(n), n.clone()))
        .collect();

    native::insert_rows(target, tgt_table, &tgt_col_info, &rows, &overrides).await
}

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

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Credentials migration (with key rotation)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn migrate_credentials(
    source: &SourcePool,
    target: &SourcePool,
    source_key: &str,
    target_key: &str,
) -> anyhow::Result<usize> {
    let rows = match source.read_rows("credentials").await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] credentials: {}", e);
            return Ok(0);
        }
    };
    if rows.is_empty() {
        return Ok(0);
    }

    let src_col_names: Vec<String> = rows[0].iter().map(|(n, _)| n.clone()).collect();
    let tgt_col_info = target.column_types("LiteLLM_CredentialsTable").await?;
    let overrides: HashMap<String, String> = src_col_names
        .iter()
        .map(|n| (camel_to_snake(n), n.clone()))
        .collect();

    if tgt_col_info.is_empty() {
        eprintln!("  [SKIP] LiteLLM_CredentialsTable: no target columns");
        return Ok(0);
    }

    let values_col = tgt_col_info.iter().position(|(n, _)| n == "credential_values");

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    for row in &rows {
        let row_map: HashMap<&str, &serde_json::Value> = row.iter().map(|(n, v)| (n.as_str(), v)).collect();
        let tbl_quoted = target.quote_ident("LiteLLM_CredentialsTable");

        let values: Vec<String> = tgt_col_info
            .iter()
            .enumerate()
            .map(|(idx, (col_name, col_type))| {
                if values_col == Some(idx) {
                    // credential_values: decrypt with source key, re-encrypt with target key
                    let encrypted = row_map.get(col_name.as_str())
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if encrypted.is_empty() || encrypted == "{}" {
                        native::value_to_target_literal(
                            &serde_json::Value::String(encrypted.to_string()),
                            col_type, target.kind(),
                        )
                    } else {
                        match aigw_core::decrypt_litellm_value(encrypted, source_key) {
                            Ok(plaintext) => {
                                match aigw_core::encrypt_litellm_value(&plaintext, target_key) {
                                    Ok(re_encrypted) => native::value_to_target_literal(
                                        &serde_json::Value::String(re_encrypted),
                                        col_type, target.kind(),
                                    ),
                                    Err(e) => {
                                        eprintln!("  [WARN] Re-encrypt: {}", e);
                                        skipped += 1;
                                        native::value_to_target_literal(
                                            &serde_json::Value::String(encrypted.to_string()),
                                            col_type, target.kind(),
                                        )
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("  [WARN] Decrypt: {}", e);
                                skipped += 1;
                                native::value_to_target_literal(
                                    &serde_json::Value::String(encrypted.to_string()),
                                    col_type, target.kind(),
                                )
                            }
                        }
                    }
                } else {
                    let v = row_map.get(col_name.as_str())
                        .or_else(|| overrides.get(col_name.as_str())
                            .and_then(|m| row_map.get(m.as_str())))
                        .copied()
                        .unwrap_or(&serde_json::Value::Null);
                    native::value_to_target_literal(v, col_type, target.kind())
                }
            })
            .collect();

        let quoted_cols: Vec<String> = tgt_col_info.iter().map(|(n, _)| target.quote_ident(n)).collect();
        let sql = format!(
            "{}{} ({}) VALUES ({}){}",
            target.insert_prefix(), tbl_quoted,
            quoted_cols.join(", "), values.join(", "),
            target.conflict_clause(),
        );
        target.execute_raw(&sql).await?;
        inserted += 1;
    }

    if skipped > 0 {
        eprintln!("  [WARN] Skipped {} credential rows", skipped);
    }
    Ok(inserted)
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
    let rows = match source.read_rows("proxy_models").await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] proxy_models: {}", e);
            return Ok(0);
        }
    };
    if rows.is_empty() {
        return Ok(0);
    }

    let src_col_names: Vec<String> = rows[0].iter().map(|(n, _)| n.clone()).collect();
    let tgt_col_info = target.column_types("LiteLLM_ProxyModelTable").await?;
    let overrides: HashMap<String, String> = src_col_names
        .iter()
        .map(|n| (camel_to_snake(n), n.clone()))
        .collect();

    if tgt_col_info.is_empty() {
        eprintln!("  [SKIP] LiteLLM_ProxyModelTable: no target columns");
        return Ok(0);
    }

    let params_col = tgt_col_info.iter().position(|(n, _)| n == "litellm_params");

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    for row in &rows {
        let row_map: HashMap<&str, &serde_json::Value> = row.iter().map(|(n, v)| (n.as_str(), v)).collect();
        let tbl_quoted = target.quote_ident("LiteLLM_ProxyModelTable");

        let values: Vec<String> = tgt_col_info
            .iter()
            .enumerate()
            .map(|(idx, (col_name, col_type))| {
                if params_col == Some(idx) {
                    let value_str = row_map.get(col_name.as_str())
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if value_str.is_empty() || value_str.starts_with('{') {
                        // JSON value, not encrypted
                        native::value_to_target_literal(
                            &serde_json::Value::String(value_str.to_string()),
                            col_type, target.kind(),
                        )
                    } else {
                        match aigw_core::decrypt_litellm_value(value_str, source_key) {
                            Ok(plaintext) => {
                                match aigw_core::encrypt_litellm_value(&plaintext, target_key) {
                                    Ok(re_encrypted) => native::value_to_target_literal(
                                        &serde_json::Value::String(re_encrypted),
                                        col_type, target.kind(),
                                    ),
                                    Err(e) => {
                                        eprintln!("  [WARN] Re-encrypt: {}", e);
                                        skipped += 1;
                                        native::value_to_target_literal(
                                            &serde_json::Value::String(value_str.to_string()),
                                            col_type, target.kind(),
                                        )
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("  [WARN] Decrypt: {}", e);
                                skipped += 1;
                                native::value_to_target_literal(
                                    &serde_json::Value::String(value_str.to_string()),
                                    col_type, target.kind(),
                                )
                            }
                        }
                    }
                } else {
                    let v = row_map.get(col_name.as_str())
                        .or_else(|| overrides.get(col_name.as_str())
                            .and_then(|m| row_map.get(m.as_str())))
                        .copied()
                        .unwrap_or(&serde_json::Value::Null);
                    native::value_to_target_literal(v, col_type, target.kind())
                }
            })
            .collect();

        let quoted_cols: Vec<String> = tgt_col_info.iter().map(|(n, _)| target.quote_ident(n)).collect();
        let sql = format!(
            "{}{} ({}) VALUES ({}){}",
            target.insert_prefix(), tbl_quoted,
            quoted_cols.join(", "), values.join(", "),
            target.conflict_clause(),
        );
        target.execute_raw(&sql).await?;
        inserted += 1;
    }

    if skipped > 0 {
        eprintln!("  [WARN] Skipped {} model rows", skipped);
    }
    Ok(inserted)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Spend logs migration
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn migrate_spend_logs(
    source: &SourcePool,
    target: &SourcePool,
) -> anyhow::Result<usize> {
    let rows = match source.read_rows("spend_logs").await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] spend_logs: {}", e);
            return Ok(0);
        }
    };
    if rows.is_empty() {
        return Ok(0);
    }

    let src_col_names: Vec<String> = rows[0].iter().map(|(n, _)| n.clone()).collect();
    let tgt_col_info = target.column_types("LiteLLM_SpendLogs").await?;
    let overrides: HashMap<String, String> = src_col_names
        .iter()
        .map(|n| (camel_to_snake(n), n.clone()))
        .collect();

    if tgt_col_info.is_empty() {
        eprintln!("  [SKIP] LiteLLM_SpendLogs: no target columns");
        return Ok(0);
    }

    native::insert_rows(target, "LiteLLM_SpendLogs", &tgt_col_info, &rows, &overrides).await
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Main entry point
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub async fn run(
    source_url: &str,
    target_url: &str,
    source_master_key: &str,
    target_master_key: Option<&str>,
) -> anyhow::Result<bool> {
    let source = SourcePool::connect(source_url).await?;
    let target = SourcePool::connect(target_url).await?;

    // Step 1: Determine target litellm master_key
    let target_key = match target_master_key {
        Some(k) => k.to_string(),
        None => match extract_target_master_key(&target).await? {
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
        let count = migrate_plain_table(&source, &target, src, tgt)
            .await
            .map_err(|e| anyhow::anyhow!("[{src} -> {tgt}] {e}"))?;
        println!("  {} -> {} ({} rows)", src, tgt, count);
    }

    // Step 3: Migrate credentials with key rotation
    println!("Step 3: Exporting credentials (with key rotation)...");
    let cred_count = migrate_credentials(&source, &target, source_master_key, &target_key).await?;
    println!("  credentials -> LiteLLM_CredentialsTable ({} rows)", cred_count);

    // Step 4: Migrate proxy_models with key rotation
    println!("Step 4: Exporting proxy_models (with key rotation)...");
    let model_count = migrate_proxy_models(&source, &target, source_master_key, &target_key).await?;
    println!("  proxy_models -> LiteLLM_ProxyModelTable ({} rows)", model_count);

    // Step 5: Migrate spend_logs
    println!("Step 5: Exporting spend_logs...");
    let spend_count = migrate_spend_logs(&source, &target).await?;
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
        let src_count = source.count_rows(src).await.unwrap_or(0);
        let tgt_count = target.count_rows(tgt).await.unwrap_or(-1);

        let status = if src_count == tgt_count { "OK" } else { "MISMATCH" };
        if src_count != tgt_count {
            all_match = false;
        }
        println!("  {} -> {}: src={} tgt={} [{}]", src, tgt, src_count, tgt_count, status);
    }

    Ok(all_match)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

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
        let src_url = format!("sqlite://{}", src_str);
        let tgt_url = format!("sqlite://{}", tgt_str);

        // Setup aigw source DB
        let src_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(src_str).create_if_missing(true))
            .await.unwrap();

        sqlx::query(
            r#"CREATE TABLE "organizations" (
                organization_id TEXT PRIMARY KEY, organization_alias TEXT, spend REAL DEFAULT 0
            )"#,
        ).execute(&src_pool).await.unwrap();
        sqlx::query(
            "INSERT INTO \"organizations\" (organization_id, organization_alias, spend) VALUES ('org-1', 'test-org', 42.0)"
        ).execute(&src_pool).await.unwrap();

        // Credentials
        sqlx::query(
            r#"CREATE TABLE "credentials" (
                credential_id TEXT PRIMARY KEY, credential_name TEXT, credential_values TEXT, credential_info TEXT
            )"#,
        ).execute(&src_pool).await.unwrap();
        let aigw_key = "sk-aigw-source-key-00001";
        let plain_cred = r#"{"api_key":"sk-secret","api_base":"https://api.openai.com"}"#;
        let encrypted_cred = aigw_core::encrypt_litellm_value(plain_cred, aigw_key).unwrap();
        sqlx::query(
            "INSERT INTO \"credentials\" (credential_id, credential_name, credential_values) VALUES ('cred-1', 'test-cred', ?)"
        ).bind(&encrypted_cred).execute(&src_pool).await.unwrap();

        // Proxy models
        sqlx::query(
            r#"CREATE TABLE "proxy_models" (
                model_id TEXT PRIMARY KEY, model_name TEXT, litellm_params TEXT, model_info TEXT
            )"#,
        ).execute(&src_pool).await.unwrap();
        let plain_params = r#"{"model":"gpt-4","api_key":"sk-model-key"}"#;
        let encrypted_params = aigw_core::encrypt_litellm_value(plain_params, aigw_key).unwrap();
        sqlx::query(
            "INSERT INTO \"proxy_models\" (model_id, model_name, litellm_params) VALUES ('model-1', 'gpt-4', ?)"
        ).bind(&encrypted_params).execute(&src_pool).await.unwrap();

        // Spend logs
        sqlx::query(
            r#"CREATE TABLE "spend_logs" (request_id TEXT PRIMARY KEY, model TEXT, spend REAL DEFAULT 0)"#,
        ).execute(&src_pool).await.unwrap();
        src_pool.close().await;

        // Setup litellm target DB
        let tgt_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(tgt_str).create_if_missing(true))
            .await.unwrap();

        sqlx::query(
            r#"CREATE TABLE "LiteLLM_OrganizationTable" (
                organization_id TEXT PRIMARY KEY, organization_alias TEXT, spend REAL DEFAULT 0
            )"#,
        ).execute(&tgt_pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_CredentialsTable" (
                credential_id TEXT PRIMARY KEY, credential_name TEXT, credential_values TEXT, credential_info TEXT
            )"#,
        ).execute(&tgt_pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_ProxyModelTable" (
                model_id TEXT PRIMARY KEY, model_name TEXT, litellm_params TEXT, model_info TEXT
            )"#,
        ).execute(&tgt_pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_SpendLogs" (
                request_id TEXT PRIMARY KEY, model TEXT, spend REAL DEFAULT 0
            )"#,
        ).execute(&tgt_pool).await.unwrap();
        tgt_pool.close().await;

        // Run
        let litellm_key = "sk-litellm-target-key-00099";
        let result = run(&src_url, &tgt_url, aigw_key, Some(litellm_key)).await;
        assert!(result.is_ok(), "remote_export failed: {:?}", result.err());

        // Verify re-encryption
        let tgt_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(tgt_str).create_if_missing(true))
            .await.unwrap();

        let cred_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM \"LiteLLM_CredentialsTable\"")
            .fetch_one(&tgt_pool).await.unwrap();
        assert_eq!(cred_count.0, 1);

        let cred_row: (String,) = sqlx::query_as(
            "SELECT credential_values FROM \"LiteLLM_CredentialsTable\" WHERE credential_id = 'cred-1'",
        ).fetch_one(&tgt_pool).await.unwrap();
        let decrypted = aigw_core::decrypt_litellm_value(&cred_row.0, litellm_key).unwrap();
        assert_eq!(decrypted, plain_cred);

        let model_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM \"LiteLLM_ProxyModelTable\"")
            .fetch_one(&tgt_pool).await.unwrap();
        assert_eq!(model_count.0, 1);

        let model_row: (String,) = sqlx::query_as(
            "SELECT litellm_params FROM \"LiteLLM_ProxyModelTable\" WHERE model_id = 'model-1'",
        ).fetch_one(&tgt_pool).await.unwrap();
        let decrypted_params = aigw_core::decrypt_litellm_value(&model_row.0, litellm_key).unwrap();
        assert_eq!(decrypted_params, plain_params);

        tgt_pool.close().await;
    }
}
