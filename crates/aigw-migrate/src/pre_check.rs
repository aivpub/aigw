//! Pre-migration checks: verify source/target connectivity, keys, and data integrity.
//!
//! Runs 6 automated checks before migration:
//!   1. Source DB connectivity + all 12 required tables exist
//!   2. Source core tables have data (row counts > 0)
//!   3. Target DB connectivity
//!   4. Source master_key extractable from LiteLLM_Config
//!   5. Target master key valid (>= 32 chars)
//!   6. Encryption/decryption spot check on first credential

use sqlx::any::AnyPoolOptions;
use sqlx::{AnyPool, Row};

fn is_pg(url: &str) -> bool {
    url.starts_with("postgres://") || url.starts_with("postgresql://")
}

fn is_mysql(url: &str) -> bool {
    url.starts_with("mysql://") || url.starts_with("mariadb://")
}

fn quote_table(name: &str, db_url: &str) -> String {
    if is_mysql(db_url) {
        format!("`{}`", name)
    } else {
        format!("\"{}\"", name)
    }
}

async fn connect(url: &str) -> anyhow::Result<AnyPool> {
    sqlx::any::install_default_drivers();
    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await?;
    Ok(pool)
}

const REQUIRED_TABLES: &[&str] = &[
    "LiteLLM_VerificationToken",
    "LiteLLM_SpendLogs",
    "LiteLLM_OrganizationTable",
    "LiteLLM_TeamTable",
    "LiteLLM_UserTable",
    "LiteLLM_ProjectTable",
    "LiteLLM_BudgetTable",
    "LiteLLM_OrganizationMembership",
    "LiteLLM_TeamMembership",
    "LiteLLM_ProxyModelTable",
    "LiteLLM_Config",
    "LiteLLM_CredentialsTable",
];

const CORE_TABLES: &[&str] = &[
    "LiteLLM_VerificationToken",
    "LiteLLM_SpendLogs",
    "LiteLLM_OrganizationTable",
    "LiteLLM_ProxyModelTable",
];

pub async fn run(source_url: &str, target_url: &str, target_master_key: &str) -> anyhow::Result<bool> {
    let mut passed = 0u32;
    let total = 6u32;

    // Check 1: Source DB connectivity + tables exist
    print!("[ 1/6] Source DB connectivity... ");
    let source = match connect(source_url).await {
        Ok(p) => {
            println!("[PASS] connected");
            p
        }
        Err(e) => {
            println!("[FAIL] {e}");
            println!("{passed}/{total} checks passed");
            return Ok(false);
        }
    };

    print!("       Source tables... ");
    let mut missing = Vec::new();
    for table in REQUIRED_TABLES {
        let quoted = quote_table(table, source_url);
        let result = sqlx::query(&format!("SELECT 1 FROM {quoted} LIMIT 0"))
            .fetch_optional(&source)
            .await;
        if result.is_err() {
            missing.push(*table);
        }
    }
    if missing.is_empty() {
        println!("[PASS] all 12 tables present");
        passed += 1;
    } else {
        println!("[FAIL] missing: {missing:?}");
    }

    // Check 2: Source core tables have data
    print!("[ 2/6] Source tables have data... ");
    let mut empty_tables = Vec::new();
    for table in CORE_TABLES {
        let quoted = quote_table(table, source_url);
        let count: i64 = sqlx::query(&format!("SELECT COUNT(*) FROM {quoted}"))
            .fetch_one(&source)
            .await
            .map(|row| row.get(0))
            .unwrap_or(0);
        if count == 0 {
            empty_tables.push(*table);
        }
    }
    if empty_tables.is_empty() {
        println!("[PASS]");
        passed += 1;
    } else {
        println!("[FAIL] 0 rows: {empty_tables:?}");
    }

    // Check 3: Target DB connectivity
    print!("[ 3/6] Target DB connectivity... ");
    match connect(target_url).await {
        Ok(p) => {
            println!("[PASS] connected");
            passed += 1;
            p.close().await;
        }
        Err(e) => {
            println!("[FAIL] {e}");
            println!("{passed}/{total} checks passed");
            return Ok(false);
        }
    }

    // Check 4: Source master_key extractable
    print!("[ 4/6] Source master_key... ");
    let col = if is_pg(source_url) {
        "param_value::text"
    } else {
        "param_value"
    };
    let config_table = quote_table("LiteLLM_Config", source_url);
    let master_key_row = sqlx::query(&format!(
        "SELECT {col} FROM {config_table} WHERE param_name = 'litellm_master_key'"
    ))
    .fetch_optional(&source)
    .await;

    let source_key: Option<String> = match master_key_row {
        Ok(Some(row)) => {
            let key: String = row.try_get::<String, _>(0).unwrap_or_default();
            if key.is_empty() {
                println!("[FAIL] master_key is empty");
                None
            } else {
                println!("[PASS] found ({} chars)", key.len());
                passed += 1;
                Some(key)
            }
        }
        Ok(None) => {
            println!("[FAIL] param_name='litellm_master_key' not found in LiteLLM_Config");
            None
        }
        Err(e) => {
            println!("[FAIL] {e}");
            None
        }
    };

    // Check 5: Target master key valid
    print!("[ 5/6] Target master key... ");
    if target_master_key.len() >= 32 {
        println!("[PASS] {} chars", target_master_key.len());
        passed += 1;
    } else {
        println!(
            "[FAIL] too short: {} chars (need >= 32)",
            target_master_key.len()
        );
    }

    // Check 6: Encryption/decryption spot check
    print!("[ 6/6] Decryption spot check... ");

    match source_key {
        Some(key) => {
            let cred_table = quote_table("LiteLLM_CredentialsTable", source_url);
            let cred_row = sqlx::query(&format!(
                "SELECT credential_name, credential_values FROM {cred_table} LIMIT 1"
            ))
            .fetch_optional(&source)
            .await;

            match cred_row {
                Ok(Some(row)) => {
                    let name: String = row.get(0);
                    let encrypted: String = row.try_get(1).unwrap_or_default();
                    match aigw_core::crypto::decrypt_litellm_value(&encrypted, &key) {
                        Ok(_) => {
                            println!("[PASS] decrypted '{name}'");
                            passed += 1;
                        }
                        Err(e) => println!("[FAIL] cannot decrypt '{name}': {e}"),
                    }
                }
                Ok(None) => {
                    println!("[PASS] no credentials to check (skipped)");
                    passed += 1;
                }
                Err(e) => println!("[FAIL] {e}"),
            }
        }
        None => println!("[FAIL] no source master_key available"),
    }

    println!("{passed}/{total} checks passed");

    source.close().await;
    Ok(passed == total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn create_db_file(path: &str) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(path)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        pool.close().await;
    }

    async fn setup_source_db(path: &str, url: &str) -> AnyPool {
        create_db_file(path).await;
        let pool = connect(url).await.unwrap();

        for table in REQUIRED_TABLES {
            sqlx::query(&format!(
                "CREATE TABLE IF NOT EXISTS \"{table}\" (
                    id INTEGER PRIMARY KEY,
                    credential_name TEXT,
                    credential_values TEXT,
                    param_name TEXT,
                    param_value TEXT
                )"
            ))
            .execute(&pool)
            .await
            .unwrap();
            // Insert a row for row-count check tables EXCEPT credentials (let Check 6 skip it)
            if *table != "LiteLLM_CredentialsTable" {
                sqlx::query(&format!("INSERT INTO \"{table}\" (id) VALUES (1)"))
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        }

        // Insert master_key in LiteLLM_Config
        sqlx::query(
            "INSERT INTO \"LiteLLM_Config\" (param_name, param_value) VALUES ('litellm_master_key', 'sk-test-source-key-for-precheck-32chars')"
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    #[tokio::test]
    async fn test_pre_check_all_pass_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.db");
        let tgt_path = dir.path().join("tgt.db");
        let src_url = format!("sqlite://{}", src_path.display());
        let tgt_url = format!("sqlite://{}", tgt_path.display());

        let source = setup_source_db(
            src_path.to_str().unwrap(),
            &src_url,
        )
        .await;
        source.close().await;

        // Create target DB (empty is fine for connectivity check)
        create_db_file(tgt_path.to_str().unwrap()).await;

        let result = run(
            &src_url,
            &tgt_url,
            "sk-aigw-target-key-for-precheck-32chars+",
        )
        .await
        .unwrap();

        assert!(result, "All pre-checks should pass");
    }

    #[tokio::test]
    async fn test_pre_check_missing_table_fails() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src_missing.db");
        let tgt_path = dir.path().join("tgt_missing.db");
        let src_url = format!("sqlite://{}", src_path.display());
        let tgt_url = format!("sqlite://{}", tgt_path.display());

        // Create source with only a subset of tables (missing most)
        create_db_file(src_path.to_str().unwrap()).await;
        let pool = connect(&src_url).await.unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS \"LiteLLM_VerificationToken\" (id INTEGER PRIMARY KEY)"
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO \"LiteLLM_VerificationToken\" (id) VALUES (1)")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        create_db_file(tgt_path.to_str().unwrap()).await;

        let result = run(
            &src_url,
            &tgt_url,
            "sk-aigw-target-key-for-precheck-32chars+",
        )
        .await
        .unwrap();

        assert!(!result, "Should fail when tables are missing");
    }

    #[tokio::test]
    async fn test_pre_check_bad_target_key_fails() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src_badkey.db");
        let tgt_path = dir.path().join("tgt_badkey.db");
        let src_url = format!("sqlite://{}", src_path.display());
        let tgt_url = format!("sqlite://{}", tgt_path.display());

        let source = setup_source_db(
            src_path.to_str().unwrap(),
            &src_url,
        )
        .await;
        source.close().await;

        create_db_file(tgt_path.to_str().unwrap()).await;

        let result = run(&src_url, &tgt_url, "short")
            .await
            .unwrap();

        assert!(!result, "Should fail with short target key");
    }

    #[tokio::test]
    async fn test_pre_check_empty_source_table_reports() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src_empty.db");
        let tgt_path = dir.path().join("tgt_empty.db");
        let src_url = format!("sqlite://{}", src_path.display());
        let tgt_url = format!("sqlite://{}", tgt_path.display());

        // Create all tables but don't insert data in core tables
        create_db_file(src_path.to_str().unwrap()).await;
        let pool = connect(&src_url).await.unwrap();
        for table in REQUIRED_TABLES {
            sqlx::query(&format!(
                "CREATE TABLE IF NOT EXISTS \"{table}\" (id INTEGER PRIMARY KEY, credential_name TEXT, credential_values TEXT, param_name TEXT, param_value TEXT)"
            ))
            .execute(&pool)
            .await
            .unwrap();
        }
        // Insert master_key into LiteLLM_Config (which has param_name/param_value columns)
        sqlx::query(
            "INSERT INTO \"LiteLLM_Config\" (id, param_name, param_value) VALUES (1, 'litellm_master_key', 'sk-test-source-key-for-precheck-32chars')"
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        create_db_file(tgt_path.to_str().unwrap()).await;

        let result = run(
            &src_url,
            &tgt_url,
            "sk-aigw-target-key-for-precheck-32chars+",
        )
        .await
        .unwrap();

        assert!(!result, "Should fail when core tables are empty");
    }
}
