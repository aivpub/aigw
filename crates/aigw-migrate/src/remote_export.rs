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
async fn extract_target_master_key(target: &AnyPool) -> anyhow::Result<Option<String>> {
    let row = sqlx::query(
        "SELECT param_value FROM \"LiteLLM_Config\" WHERE param_name = 'litellm_master_key'",
    )
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
        .map(|c| c.name().to_string())
        .collect();

    let mut inserted = 0usize;
    for row in &rows {
        let col_count = row.columns().len();
        let placeholders: Vec<String> = (0..col_count).map(|_| "?".to_string()).collect();

        let insert_sql = format!(
            "INSERT INTO \"{}\" ({}) VALUES ({})",
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

/// Migrate credentials: aigw → litellm with key rotation.
async fn migrate_credentials(
    source: &AnyPool,
    target: &AnyPool,
    source_key: &str,
    target_key: &str,
) -> anyhow::Result<usize> {
    let query = "SELECT * FROM \"credentials\"";
    let rows = match sqlx::query(query).fetch_all(source).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] credentials: {}", e);
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

    let values_col = columns
        .iter()
        .position(|c| c == "credential_values")
        .unwrap_or_else(|| {
            eprintln!("  [WARN] credential_values column not found, using index 2");
            2
        });

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    for row in &rows {
        let col_count = row.columns().len();
        let placeholders: Vec<String> = (0..col_count).map(|_| "?".to_string()).collect();

        let insert_sql = format!(
            "INSERT INTO \"LiteLLM_CredentialsTable\" ({}) VALUES ({})",
            columns.join(", "),
            placeholders.join(", ")
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
        eprintln!("  [WARN] Skipped {} credential rows", skipped);
    }

    Ok(inserted)
}

/// Migrate proxy_models: aigw → litellm with key rotation.
async fn migrate_proxy_models(
    source: &AnyPool,
    target: &AnyPool,
    source_key: &str,
    target_key: &str,
) -> anyhow::Result<usize> {
    let query = "SELECT * FROM \"proxy_models\"";
    let rows = match sqlx::query(query).fetch_all(source).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] proxy_models: {}", e);
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

    let params_col = columns
        .iter()
        .position(|c| c == "litellm_params")
        .unwrap_or_else(|| {
            eprintln!("  [WARN] litellm_params column not found, using index 2");
            2
        });

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    for row in &rows {
        let col_count = row.columns().len();
        let placeholders: Vec<String> = (0..col_count).map(|_| "?".to_string()).collect();

        let insert_sql = format!(
            "INSERT INTO \"LiteLLM_ProxyModelTable\" ({}) VALUES ({})",
            columns.join(", "),
            placeholders.join(", ")
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
        eprintln!("  [WARN] Skipped {} model rows", skipped);
    }

    Ok(inserted)
}

/// Batch migrate spend_logs (no crypto).
async fn migrate_spend_logs(source: &AnyPool, target: &AnyPool) -> anyhow::Result<usize> {
    let query = "SELECT * FROM \"spend_logs\"";
    let rows = match sqlx::query(query).fetch_all(source).await {
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
    for row in &rows {
        let col_count = row.columns().len();
        let placeholders: Vec<String> = (0..col_count).map(|_| "?".to_string()).collect();

        let insert_sql = format!(
            "INSERT INTO \"LiteLLM_SpendLogs\" ({}) VALUES ({})",
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
    source_master_key: &str,
    target_master_key: Option<&str>,
) -> anyhow::Result<bool> {
    sqlx::any::install_default_drivers();
    let source = connect(source_url).await?;
    let target = connect(target_url).await?;

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
        let count = migrate_plain_table(&source, &target, src, tgt).await?;
        println!("  {} -> {} ({} rows)", src, tgt, count);
    }

    // Step 3: Migrate credentials with key rotation (aigw→litellm)
    println!("Step 3: Exporting credentials (with key rotation)...");
    let cred_count =
        migrate_credentials(&source, &target, source_master_key, &target_key).await?;
    println!("  credentials -> LiteLLM_CredentialsTable ({} rows)", cred_count);

    // Step 4: Migrate proxy_models with key rotation
    println!("Step 4: Exporting proxy_models (with key rotation)...");
    let model_count =
        migrate_proxy_models(&source, &target, source_master_key, &target_key).await?;
    println!(
        "  proxy_models -> LiteLLM_ProxyModelTable ({} rows)",
        model_count
    );

    // Step 5: Migrate spend_logs
    println!("Step 5: Exporting spend_logs...");
    let spend_count = migrate_spend_logs(&source, &target).await?;
    println!(
        "  spend_logs -> LiteLLM_SpendLogs ({} rows)",
        spend_count
    );

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
        let decrypted_params =
            aigw_core::decrypt_litellm_value(&model_row.0, litellm_key).unwrap();
        assert_eq!(
            decrypted_params, plain_params,
            "model params should decrypt with litellm key"
        );

        tgt_pool.close().await;
    }
}
