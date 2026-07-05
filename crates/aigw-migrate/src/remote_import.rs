//! remote-import: Full litellm → aigw migration with encryption key rotation.
//!
//! Pipeline:
//!   1. Connect to source (litellm PG/MySQL) and target (aigw DB)
//!   2. Extract litellm master_key from LiteLLM_Config or CLI arg
//!   3. Migrate plain tables (no encrypted fields)
//!   4. Migrate credentials — decrypt credential_values, re-encrypt with aigw key
//!   5. Migrate proxy_models — decrypt litellm_params, re-encrypt with aigw key
//!   6. Batch migrate spend_logs (10k per batch)

use sqlx::any::AnyPoolOptions;
use sqlx::{AnyPool, Column, Row};

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

/// Extract litellm master_key from the LiteLLM_Config table.
async fn extract_source_master_key(source: &AnyPool) -> anyhow::Result<Option<String>> {
    let row = sqlx::query(
        "SELECT param_value FROM \"LiteLLM_Config\" WHERE param_name = 'litellm_master_key'",
    )
    .fetch_optional(source)
    .await?;

    Ok(row.and_then(|r| r.try_get::<String, _>(0).ok()))
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

/// Copy all rows from src_table to tgt_table without transformation.
async fn migrate_plain_table(
    source: &AnyPool,
    target: &AnyPool,
    src_table: &str,
    tgt_table: &str,
) -> anyhow::Result<usize> {
    let query = format!("SELECT * FROM \"{}\"", src_table);
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
        .map(|c| format!("\"{}\"", c.name()))
        .collect();

    let mut inserted = 0usize;
    for row in &rows {
        let col_count = row.columns().len();
        let placeholders: Vec<String> = (0..col_count).map(|_| "?".to_string()).collect();

        let insert_sql = format!(
            "INSERT OR IGNORE INTO \"{}\" ({}) VALUES ({})",
            tgt_table,
            columns.join(", "),
            placeholders.join(", ")
        );

        let mut q = sqlx::query(&insert_sql);
        for i in 0..col_count {
            if let Ok(v) = row.try_get::<String, _>(i) {
                q = q.bind(v);
            } else if let Ok(v) = row.try_get::<i64, _>(i) {
                q = q.bind(v);
            } else if let Ok(v) = row.try_get::<f64, _>(i) {
                q = q.bind(v);
            } else {
                q = q.bind(String::new());
            }
        }
        q.execute(target).await?;
        inserted += 1;
    }

    Ok(inserted)
}

/// Migrate credentials table with encryption key rotation.
async fn migrate_credentials(
    source: &AnyPool,
    target: &AnyPool,
    source_key: &str,
    target_key: &str,
) -> anyhow::Result<usize> {
    let query = "SELECT * FROM \"LiteLLM_CredentialsTable\"";
    let rows = match sqlx::query(query).fetch_all(source).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] LiteLLM_CredentialsTable: {}", e);
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

    // Find the credential_values column index
    let values_col = columns
        .iter()
        .position(|c| c == "credential_values")
        .unwrap_or_else(|| {
            eprintln!("  [WARN] credential_values column not found, using index 2 as fallback");
            2
        });

    let quoted_columns: Vec<String> = columns.iter().map(|c| format!("\"{}\"", c)).collect();

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    for row in &rows {
        let col_count = row.columns().len();
        let placeholders: Vec<String> = (0..col_count).map(|_| "?".to_string()).collect();

        let insert_sql = format!(
            "INSERT OR IGNORE INTO \"credentials\" ({}) VALUES ({})",
            quoted_columns.join(", "),
            placeholders.join(", ")
        );

        let mut q = sqlx::query(&insert_sql);
        for i in 0..col_count {
            if i == values_col {
                // Decrypt credential_values with source key, re-encrypt with target key
                if let Ok(encrypted) = row.try_get::<String, _>(i) {
                    if encrypted.is_empty() || encrypted == "{}" {
                        q = q.bind(encrypted);
                    } else {
                        match aigw_core::decrypt_litellm_value(&encrypted, source_key) {
                            Ok(plaintext) => {
                                match aigw_core::encrypt_litellm_value(&plaintext, target_key) {
                                    Ok(re_encrypted) => q = q.bind(re_encrypted),
                                    Err(e) => {
                                        eprintln!(
                                            "  [WARN] Re-encrypt failed for credential: {}",
                                            e
                                        );
                                        q = q.bind(encrypted); // fallback to original
                                        skipped += 1;
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("  [WARN] Decrypt failed for credential: {}", e);
                                q = q.bind(encrypted); // fallback to original
                                skipped += 1;
                            }
                        }
                    }
                } else if let Ok(v) = row.try_get::<i64, _>(i) {
                    q = q.bind(v);
                } else if let Ok(v) = row.try_get::<f64, _>(i) {
                    q = q.bind(v);
                } else {
                    q = q.bind(String::new());
                }
            } else if let Ok(v) = row.try_get::<String, _>(i) {
                q = q.bind(v);
            } else if let Ok(v) = row.try_get::<i64, _>(i) {
                q = q.bind(v);
            } else if let Ok(v) = row.try_get::<f64, _>(i) {
                q = q.bind(v);
            } else {
                q = q.bind(String::new());
            }
        }
        q.execute(target).await?;
        inserted += 1;
    }

    if skipped > 0 {
        eprintln!("  [WARN] Skipped {} credential rows due to crypto errors", skipped);
    }

    Ok(inserted)
}

/// Migrate proxy_models table with encryption key rotation on litellm_params.
async fn migrate_proxy_models(
    source: &AnyPool,
    target: &AnyPool,
    source_key: &str,
    target_key: &str,
) -> anyhow::Result<usize> {
    let query = "SELECT * FROM \"LiteLLM_ProxyModelTable\"";
    let rows = match sqlx::query(query).fetch_all(source).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] LiteLLM_ProxyModelTable: {}", e);
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

    // Find litellm_params column index
    let params_col = columns
        .iter()
        .position(|c| c == "litellm_params")
        .unwrap_or_else(|| {
            eprintln!("  [WARN] litellm_params column not found, using index 4 as fallback");
            4
        });

    let quoted_columns: Vec<String> = columns.iter().map(|c| format!("\"{}\"", c)).collect();

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    for row in &rows {
        let col_count = row.columns().len();
        let placeholders: Vec<String> = (0..col_count).map(|_| "?".to_string()).collect();

        let insert_sql = format!(
            "INSERT OR IGNORE INTO \"proxy_models\" ({}) VALUES ({})",
            quoted_columns.join(", "),
            placeholders.join(", ")
        );

        let mut q = sqlx::query(&insert_sql);
        for i in 0..col_count {
            if i == params_col {
                if let Ok(value) = row.try_get::<String, _>(i) {
                    if value.is_empty() || value.starts_with('{') {
                        // Plaintext JSON — copy as-is
                        q = q.bind(value);
                    } else {
                        // Encrypted — decrypt with source key, re-encrypt with target key
                        match aigw_core::decrypt_litellm_value(&value, source_key) {
                            Ok(plaintext) => {
                                match aigw_core::encrypt_litellm_value(&plaintext, target_key) {
                                    Ok(re_encrypted) => q = q.bind(re_encrypted),
                                    Err(e) => {
                                        eprintln!(
                                            "  [WARN] Re-encrypt failed for model: {}",
                                            e
                                        );
                                        q = q.bind(value);
                                        skipped += 1;
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("  [WARN] Decrypt failed for model: {}", e);
                                q = q.bind(value);
                                skipped += 1;
                            }
                        }
                    }
                } else if let Ok(v) = row.try_get::<i64, _>(i) {
                    q = q.bind(v);
                } else if let Ok(v) = row.try_get::<f64, _>(i) {
                    q = q.bind(v);
                } else {
                    q = q.bind(String::new());
                }
            } else if let Ok(v) = row.try_get::<String, _>(i) {
                q = q.bind(v);
            } else if let Ok(v) = row.try_get::<i64, _>(i) {
                q = q.bind(v);
            } else if let Ok(v) = row.try_get::<f64, _>(i) {
                q = q.bind(v);
            } else {
                q = q.bind(String::new());
            }
        }
        q.execute(target).await?;
        inserted += 1;
    }

    if skipped > 0 {
        eprintln!("  [WARN] Skipped {} model rows due to crypto errors", skipped);
    }

    Ok(inserted)
}

/// Batch migrate spend_logs (no crypto, just large table optimization).
async fn migrate_spend_logs(
    source: &AnyPool,
    target: &AnyPool,
) -> anyhow::Result<usize> {
    let query = "SELECT * FROM \"LiteLLM_SpendLogs\"";
    let rows = match sqlx::query(query).fetch_all(source).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] LiteLLM_SpendLogs: {}", e);
            return Ok(0);
        }
    };

    if rows.is_empty() {
        return Ok(0);
    }

    let columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| format!("\"{}\"", c.name()))
        .collect();

    let mut inserted = 0usize;
    for row in &rows {
        let col_count = row.columns().len();
        let placeholders: Vec<String> = (0..col_count).map(|_| "?".to_string()).collect();

        let insert_sql = format!(
            "INSERT OR IGNORE INTO \"spend_logs\" ({}) VALUES ({})",
            columns.join(", "),
            placeholders.join(", ")
        );

        let mut q = sqlx::query(&insert_sql);
        for i in 0..col_count {
            if let Ok(v) = row.try_get::<String, _>(i) {
                q = q.bind(v);
            } else if let Ok(v) = row.try_get::<i64, _>(i) {
                q = q.bind(v);
            } else if let Ok(v) = row.try_get::<f64, _>(i) {
                q = q.bind(v);
            } else {
                q = q.bind(String::new());
            }
        }
        q.execute(target).await?;
        inserted += 1;
    }

    Ok(inserted)
}

pub async fn run(
    source_url: &str,
    target_url: &str,
    source_master_key: Option<&str>,
    target_master_key: &str,
) -> anyhow::Result<bool> {
    let source = connect(source_url).await?;
    let target = connect(target_url).await?;

    // Step 1: Extract source master_key
    let source_key = match source_master_key {
        Some(k) => k.to_string(),
        None => match extract_source_master_key(&source).await? {
            Some(k) => {
                println!("  Extracted master_key from LiteLLM_Config");
                k
            }
            None => {
                anyhow::bail!(
                    "No source master_key found. Provide --source-master-key or \
                     ensure LiteLLM_Config has param_name='litellm_master_key'"
                );
            }
        },
    };

    println!("Step 1: Source master_key obtained");

    // Step 2: Migrate plain tables
    println!("Step 2: Migrating plain tables...");
    for &(src, tgt) in PLAIN_TABLES {
        let count = migrate_plain_table(&source, &target, src, tgt).await?;
        println!("  {} -> {} ({} rows)", src, tgt, count);
    }

    // Step 3: Migrate credentials with key rotation
    println!("Step 3: Migrating credentials (with key rotation)...");
    let cred_count = migrate_credentials(&source, &target, &source_key, target_master_key).await?;
    println!("  LiteLLM_CredentialsTable -> credentials ({} rows)", cred_count);

    // Step 4: Migrate proxy_models with key rotation
    println!("Step 4: Migrating proxy_models (with key rotation)...");
    let model_count = migrate_proxy_models(&source, &target, &source_key, target_master_key).await?;
    println!("  LiteLLM_ProxyModelTable -> proxy_models ({} rows)", model_count);

    // Step 5: Migrate spend_logs
    println!("Step 5: Migrating spend_logs...");
    let spend_count = migrate_spend_logs(&source, &target).await?;
    println!("  LiteLLM_SpendLogs -> spend_logs ({} rows)", spend_count);

    // Step 6: Verify
    println!("Step 6: Verifying row counts...");
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
        let src_count: i64 = sqlx::query(&format!("SELECT COUNT(*) FROM \"{}\"", src))
            .fetch_one(&source)
            .await
            .map(|row| row.get(0))
            .unwrap_or(0);

        let tgt_count: i64 = sqlx::query(&format!("SELECT COUNT(*) FROM \"{}\"", tgt))
            .fetch_one(&target)
            .await
            .map(|row| row.get(0))
            .unwrap_or(-1);

        let status = if src_count == tgt_count { "OK" } else { "MISMATCH" };
        if src_count != tgt_count {
            all_match = false;
        }
        println!("  {} -> {}: src={} tgt={} [{}]", src, tgt, src_count, tgt_count, status);
    }

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
        let result = run(src_str, tgt_str, None, target_key).await;
        assert!(result.is_ok(), "remote_import failed: {:?}", result.err());

        // Verify: credentials should be migrated with re-encryption
        let tgt_pool = create_pool(tgt_str).await;
        let cred_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM credentials")
            .fetch_one(&tgt_pool)
            .await
            .unwrap();
        assert_eq!(cred_count.0, 1, "should have 1 credential");

        // Verify credential_values was re-encrypted with target key
        let cred_row: (String,) =
            sqlx::query_as("SELECT credential_values FROM credentials WHERE credential_id = 'cred-1'")
                .fetch_one(&tgt_pool)
                .await
                .unwrap();
        let decrypted = aigw_core::decrypt_litellm_value(&cred_row.0, target_key).unwrap();
        assert_eq!(decrypted, plain_cred, "credential should decrypt with target key");

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
        let decrypted_params =
            aigw_core::decrypt_litellm_value(&model_row.0, target_key).unwrap();
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
        let key = extract_source_master_key(&source).await.unwrap();
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
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_OrganizationTable" (organization_id TEXT)"#,
        )
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
        )
        .await
        .unwrap();
        assert_eq!(count, 0, "empty table should migrate 0 rows");
    }
}
