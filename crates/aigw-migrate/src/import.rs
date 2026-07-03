use crate::TABLE_MAPPINGS;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::Column;
use sqlx::Row;

pub async fn run(source_path: &str, target_path: &str) -> anyhow::Result<()> {
    let source = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(source_path)
                .create_if_missing(true),
        )
        .await?;
    let target = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(target_path)
                .create_if_missing(true),
        )
        .await?;

    for &(litellm_table, aigw_table) in TABLE_MAPPINGS {
        let query = format!("SELECT * FROM \"{}\"", litellm_table);
        let rows: Vec<sqlx::sqlite::SqliteRow> = match sqlx::query(&query).fetch_all(&source).await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  [SKIP] {}: {}", litellm_table, e);
                continue;
            }
        };

        if rows.is_empty() {
            println!("  {} -> {} (0 rows)", litellm_table, aigw_table);
            continue;
        }

        let columns: Vec<String> = rows[0]
            .columns()
            .iter()
            .map(|c| format!("\"{}\"", c.name()))
            .collect();
        let placeholders: Vec<String> = columns.iter().map(|_| "?".to_string()).collect();

        let insert_sql = format!(
            "INSERT INTO \"{}\" ({}) VALUES ({})",
            aigw_table,
            columns.join(", "),
            placeholders.join(", ")
        );

        let mut inserted = 0usize;
        for row in &rows {
            let col_count = row.columns().len();
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
            q.execute(&target).await?;
            inserted += 1;
        }

        println!("  {} -> {} ({} rows)", litellm_table, aigw_table, inserted);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    fn open_db(path: &str) -> SqliteConnectOptions {
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
    }

    #[tokio::test]
    async fn test_import_single_table() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.db");
        let tgt_path = dir.path().join("tgt.db");
        let src_str = src_path.to_str().unwrap();
        let tgt_str = tgt_path.to_str().unwrap();

        let src_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(open_db(src_str))
            .await
            .unwrap();
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_OrganizationTable" (
            organization_id TEXT PRIMARY KEY, organization_alias TEXT, spend REAL DEFAULT 0,
            models TEXT, max_budget REAL, created_at DATETIME, updated_at DATETIME
        )"#,
        )
        .execute(&src_pool)
        .await
        .unwrap();
        sqlx::query(
            r#"INSERT INTO "LiteLLM_OrganizationTable" (organization_id, organization_alias, spend)
            VALUES ('org-1', 'test', 42.0)"#,
        )
        .execute(&src_pool)
        .await
        .unwrap();
        src_pool.close().await;

        let tgt_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(open_db(tgt_str))
            .await
            .unwrap();
        sqlx::query(
            r#"CREATE TABLE "organizations" (
            organization_id TEXT PRIMARY KEY, organization_alias TEXT, spend REAL DEFAULT 0,
            models TEXT, max_budget REAL, created_at DATETIME, updated_at DATETIME
        )"#,
        )
        .execute(&tgt_pool)
        .await
        .unwrap();
        tgt_pool.close().await;

        let result = run(src_str, tgt_str).await;
        assert!(result.is_ok(), "import failed: {:?}", result.err());

        let tgt_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(open_db(tgt_str))
            .await
            .unwrap();
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM organizations")
            .fetch_one(&tgt_pool)
            .await
            .unwrap();
        assert_eq!(count.0, 1);
        tgt_pool.close().await;
    }
}
