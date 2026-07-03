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
