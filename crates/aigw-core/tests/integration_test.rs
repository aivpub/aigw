//! Integration tests using testcontainers
//!
//! These tests require Docker and are gated behind the `integration` feature flag.
//! Run with: `cargo test --features integration`

#[cfg(feature = "integration")]
use aigw_core::db::Database;

#[cfg(feature = "integration")]
const EXPECTED_TABLES: &[&str] = &[
    "virtual_keys",
    "spend_logs",
    "organizations",
    "teams",
    "users",
    "projects",
    "budgets",
    "organization_memberships",
    "team_memberships",
    "deprecated_keys",
    "deleted_keys",
];

#[cfg(feature = "integration")]
mod postgres_tests {
    use super::*;
    use aigw_core::models::Credential;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    #[tokio::test]
    async fn test_postgres_migration() {
        // Spin up PostgreSQL container
        let container = Postgres::default()
            .start()
            .await
            .expect("failed to start postgres container");

        let port = container
            .get_host_port_ipv4(5432)
            .await
            .expect("failed to get host port");
        let url = format!("postgresql://postgres:postgres@localhost:{}/postgres", port);

        // Initialize database and run migrations
        let db = Database::init(&url).await.expect("postgres init");

        match db {
            Database::Postgres(pool) => {
                // Verify all 11 tables exist
                for table_name in EXPECTED_TABLES {
                    let row: (String,) = sqlx::query_as(
                        "SELECT table_name FROM information_schema.tables WHERE table_name = $1 AND table_schema = 'public'",
                    )
                    .bind(table_name)
                    .fetch_one(&pool)
                    .await
                    .unwrap_or_else(|_| {
                        panic!("table '{}' should exist after postgres migration", table_name)
                    });
                    assert_eq!(row.0, *table_name);
                }
            }
            _ => panic!("expected PostgreSQL"),
        }
    }

    /// Regression: `credentials.credential_values` / `credential_info` used to be
    /// TEXT in PG, which made `list_credentials` explode with:
    ///
    ///   mismatched types; Rust type `serde_json::value::Value`
    ///   (as SQL type `JSONB`) is not compatible with SQL type `TEXT`
    ///
    /// Migration 019 flips them to JSONB.  This test:
    ///   1. Confirms migrations succeed against a real PG.
    ///   2. Inserts a credential whose `credential_values` is a *JSON scalar
    ///      string* (mimicking what remote-import writes for opaque encrypted
    ///      blobs like `"gAAAAAB..."`).
    ///   3. Reads it back via the store trait — this is the path that used
    ///      to fail — and asserts we get `Value::String(_)` verbatim.
    #[tokio::test]
    async fn test_postgres_credentials_jsonb_roundtrip() {
        use aigw_core::db::CredentialsStore;

        let container = Postgres::default()
            .start()
            .await
            .expect("failed to start postgres container");
        let port = container
            .get_host_port_ipv4(5432)
            .await
            .expect("failed to get host port");
        let url = format!("postgresql://postgres:postgres@localhost:{}/postgres", port);
        let db = Database::init(&url).await.expect("postgres init");
        let pool = match db {
            Database::Postgres(p) => p,
            _ => panic!("expected PostgreSQL"),
        };

        // Verify column type is jsonb (not text) after migration 019.
        let (values_ty, info_ty): (String, String) = sqlx::query_as(
            "SELECT
               (SELECT data_type FROM information_schema.columns
                WHERE table_name='credentials' AND column_name='credential_values'),
               (SELECT data_type FROM information_schema.columns
                WHERE table_name='credentials' AND column_name='credential_info')",
        )
        .fetch_one(&pool)
        .await
        .expect("column type lookup");
        assert_eq!(values_ty, "jsonb", "credential_values should be JSONB after 019");
        assert_eq!(info_ty, "jsonb", "credential_info should be JSONB after 019");

        // Insert an opaque encrypted-blob credential.  This mirrors the
        // remote-import output when litellm's LiteLLM_CredentialsTable stores
        // a single encrypted string rather than a JSON object.
        let cred = Credential {
            credential_id: "id-1".to_string(),
            credential_name: "acme".to_string(),
            credential_values: serde_json::Value::String("gAAAAABopaqueBase64Blob".to_string()),
            credential_info: serde_json::Value::Object(Default::default()),
            created_at: "2026-07-20T00:00:00Z".to_string(),
            created_by: None,
            updated_at: "2026-07-20T00:00:00Z".to_string(),
            updated_by: None,
        };
        pool.insert_credential(&cred).await.expect("insert");

        // Read back via the same code path the /credential/list handler uses.
        let listed = pool.list_credentials().await.expect("list");
        assert_eq!(listed.len(), 1);
        let got = &listed[0];
        assert_eq!(got.credential_name, "acme");
        assert_eq!(
            got.credential_values,
            serde_json::Value::String("gAAAAABopaqueBase64Blob".to_string()),
            "opaque encrypted blob must round-trip as Value::String, not Value::Object({{}})",
        );
    }
}

#[cfg(feature = "integration")]
mod mysql_tests {
    use super::*;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::mysql::Mysql;

    #[tokio::test]
    async fn test_mysql_migration() {
        // Spin up MySQL container
        let container = Mysql::default()
            .start()
            .await
            .expect("failed to start mysql container");

        let port = container
            .get_host_port_ipv4(3306)
            .await
            .expect("failed to get host port");
        let url = format!("mysql://root:test@localhost:{}/test", port);

        // Initialize database and run migrations
        let db = Database::init(&url).await.expect("mysql init");

        match db {
            Database::Mysql(pool) => {
                // Verify all 11 tables exist
                for table_name in EXPECTED_TABLES {
                    let row: (String,) = sqlx::query_as(
                        "SELECT TABLE_NAME FROM information_schema.tables WHERE TABLE_NAME = ? AND TABLE_SCHEMA = DATABASE()",
                    )
                    .bind(table_name)
                    .fetch_one(&pool)
                    .await
                    .unwrap_or_else(|_| {
                        panic!("table '{}' should exist after mysql migration", table_name)
                    });
                    assert_eq!(row.0, *table_name);
                }
            }
            _ => panic!("expected MySQL"),
        }
    }
}
