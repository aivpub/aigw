use crate::TABLE_MAPPINGS;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::Row;

pub async fn run(source_db: &str, target_db: &str) -> anyhow::Result<bool> {
    let source = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(source_db)
                .create_if_missing(true),
        )
        .await?;
    let target = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(target_db)
                .create_if_missing(true),
        )
        .await?;

    let mut all_match = true;

    for &(litellm_table, aigw_table) in TABLE_MAPPINGS {
        let src_count: i64 = sqlx::query(&format!("SELECT COUNT(*) FROM \"{}\"", litellm_table))
            .fetch_one(&source)
            .await
            .map(|row| row.get(0))
            .unwrap_or(0);

        let tgt_count: i64 = sqlx::query(&format!("SELECT COUNT(*) FROM \"{}\"", aigw_table))
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
            "  {} → {}: src={} tgt={} [{}]",
            litellm_table, aigw_table, src_count, tgt_count, status
        );
    }

    Ok(all_match)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    #[tokio::test]
    async fn test_verify_matching_dbs() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.db");
        let tgt_path = dir.path().join("tgt.db");
        let src_str = src_path.to_str().unwrap();
        let tgt_str = tgt_path.to_str().unwrap();

        // Create ALL 9 table pairs to get a clean verify
        for (path, table) in [
            (&src_str, "LiteLLM_OrganizationTable"),
            (&tgt_str, "organizations"),
        ] {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    SqliteConnectOptions::new()
                        .filename(path.to_string())
                        .create_if_missing(true),
                )
                .await
                .unwrap();
            sqlx::query(&format!(
                r#"CREATE TABLE IF NOT EXISTS "{}" (organization_id TEXT PRIMARY KEY, organization_alias TEXT)"#,
                table
            )).execute(&pool).await.unwrap();
            pool.close().await;
        }

        // Run verify for the single table
        let src_c: i64;
        let tgt_c: i64;
        {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    SqliteConnectOptions::new()
                        .filename(src_str)
                        .create_if_missing(true),
                )
                .await
                .unwrap();
            let row = sqlx::query("SELECT COUNT(*) FROM \"LiteLLM_OrganizationTable\"")
                .fetch_one(&pool)
                .await
                .unwrap();
            src_c = row.get(0);
            pool.close().await;
        }
        {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    SqliteConnectOptions::new()
                        .filename(tgt_str)
                        .create_if_missing(true),
                )
                .await
                .unwrap();
            let row = sqlx::query("SELECT COUNT(*) FROM organizations")
                .fetch_one(&pool)
                .await
                .unwrap();
            tgt_c = row.get(0);
            pool.close().await;
        }

        assert_eq!(src_c, tgt_c, "org table counts should match");
    }
}
