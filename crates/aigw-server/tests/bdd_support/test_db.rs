//! Test database lifecycle manager — create/drop databases for BDD tests.
//!
//! Supports SQLite, PostgreSQL, and MySQL backends.
//! Controlled by env var `AIGW_TEST_DB_DRIVER`.

use chrono::Utc;

/// Info about the created test database.
#[derive(Debug, Clone)]
pub struct DbInfo {
    /// Connection URL for the test database (passed to aigw --database-url).
    pub database_url: String,
    /// Database name (for drop).
    pub db_name: String,
    /// Driver type.
    #[allow(dead_code)]
    pub driver: String,
}

/// Manages the lifecycle of a test database.
pub enum TestDatabaseManager {
    Sqlite,
    Postgres {
        admin_url: String,
        host: String,
        port: u16,
        user: String,
        password: String,
    },
    Mysql {
        admin_url: String,
        host: String,
        port: u16,
        user: String,
        password: String,
    },
}

impl TestDatabaseManager {
    /// Read configuration from environment variables.
    /// Returns `None` if `AIGW_TEST_DB_DRIVER` is not set (opt-in).
    pub fn from_env() -> Option<Self> {
        let driver = std::env::var("AIGW_TEST_DB_DRIVER").ok()?;
        match driver.to_lowercase().as_str() {
            "sqlite" => Some(Self::Sqlite),
            "postgres" | "postgresql" | "pg" => {
                let host = std::env::var("AIGW_TEST_DB_HOST").unwrap_or_else(|_| "localhost".into());
                let port: u16 = std::env::var("AIGW_TEST_DB_PORT")
                    .ok()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(5432);
                let user = std::env::var("AIGW_TEST_DB_USER")
                    .unwrap_or_else(|_| "postgres".into());
                let password =
                    std::env::var("AIGW_TEST_DB_PASS").unwrap_or_else(|_| "postgres".into());
                let clean_host = host.trim_end_matches('/');
                let admin_url = format!(
                    "postgres://{}:{}@{}:{}/postgres",
                    user, password, clean_host, port
                );
                Some(Self::Postgres {
                    admin_url,
                    host: clean_host.to_string(),
                    port,
                    user,
                    password,
                })
            }
            "mysql" | "mariadb" => {
                let host = std::env::var("AIGW_TEST_DB_HOST").unwrap_or_else(|_| "localhost".into());
                let port: u16 = std::env::var("AIGW_TEST_DB_PORT")
                    .ok()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(3306);
                let user =
                    std::env::var("AIGW_TEST_DB_USER").unwrap_or_else(|_| "root".into());
                let password =
                    std::env::var("AIGW_TEST_DB_PASS").unwrap_or_else(|_| "root".into());
                let clean_host = host.trim_end_matches('/');
                let admin_url =
                    format!("mysql://{}:{}@{}:{}/mysql", user, password, clean_host, port);
                Some(Self::Mysql {
                    admin_url,
                    host: clean_host.to_string(),
                    port,
                    user,
                    password,
                })
            }
            _ => {
                eprintln!(
                    "WARN: unknown AIGW_TEST_DB_DRIVER={}, disabling auto lifecycle",
                    driver
                );
                None
            }
        }
    }

    /// Create a new test database and return its info.
    pub async fn create_db(&self) -> Result<DbInfo, String> {
        let db_name = gen_db_name();
        match self {
            Self::Sqlite => {
                let dir = std::env::temp_dir().join("aigw_bdd_tests");
                std::fs::create_dir_all(&dir)
                    .map_err(|e| format!("create temp dir: {e}"))?;
                let path = dir.join(format!("{db_name}.db"));
                let database_url = format!("sqlite:///{}", path.display());
                Ok(DbInfo {
                    database_url,
                    db_name: path.to_string_lossy().to_string(),
                    driver: "sqlite".into(),
                })
            }
            Self::Postgres {
                admin_url, host, port, user, password, ..
            } => {
                use sqlx::postgres::PgConnectOptions;

                let admin_opts: PgConnectOptions = admin_url
                    .parse()
                    .map_err(|e| format!("parse admin url: {e}"))?;
                let admin_opts = admin_opts.database("postgres");
                // CREATE DATABASE cannot run inside a transaction (PG limitation).
                // Use a raw query without starting a transaction.
                let admin_pool = sqlx::PgPool::connect_with(admin_opts)
                    .await
                    .map_err(|e| format!("connect to postgres admin db: {e}"))?;
                let quoted = quote_identifier(&db_name);
                let sql = format!("CREATE DATABASE {}", quoted);
                sqlx::query(&sql)
                    .execute(&admin_pool)
                    .await
                    .map_err(|e| format!("CREATE DATABASE {}: {e}", db_name))?;
                admin_pool.close().await;

                let database_url = format!(
                    "postgres://{}:{}@{}:{}/{}",
                    user, password, host, port, db_name
                );
                Ok(DbInfo {
                    database_url,
                    db_name,
                    driver: "postgres".into(),
                })
            }
            Self::Mysql {
                admin_url, host, port, user, password, ..
            } => {
                let admin_pool =
                    sqlx::MySqlPool::connect(admin_url)
                        .await
                        .map_err(|e| format!("connect to mysql admin db: {e}"))?;
                let quoted = format!("`{db_name}`");
                let sql = format!("CREATE DATABASE {}", quoted);
                sqlx::query(&sql)
                    .execute(&admin_pool)
                    .await
                    .map_err(|e| format!("CREATE DATABASE {}: {e}", db_name))?;
                admin_pool.close().await;

                let database_url = format!(
                    "mysql://{}:{}@{}:{}/{}",
                    user, password, host, port, db_name
                );
                Ok(DbInfo {
                    database_url,
                    db_name,
                    driver: "mysql".into(),
                })
            }
        }
    }

    /// Drop the test database.
    pub async fn drop_db(&self, info: &DbInfo) -> Result<(), String> {
        match self {
            Self::Sqlite => {
                // Delete the .db file and any WAL/SHM companions.
                let _ = std::fs::remove_file(&info.db_name);
                let _ = std::fs::remove_file(format!("{}-wal", info.db_name));
                let _ = std::fs::remove_file(format!("{}-shm", info.db_name));
                Ok(())
            }
            Self::Postgres { admin_url, .. } => {
                let admin_opts: sqlx::postgres::PgConnectOptions = admin_url
                    .parse()
                    .map_err(|e| format!("parse admin url: {e}"))?;
                let admin_opts = admin_opts.database("postgres");
                let admin_pool = sqlx::PgPool::connect_with(admin_opts)
                    .await
                    .map_err(|e| format!("connect to postgres admin db: {e}"))?;

                let db_name = &info.db_name;
                // Terminate existing connections before dropping.
                let term_sql = format!(
                    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{}' AND pid <> pg_backend_pid()",
                    db_name
                );
                sqlx::query(&term_sql).execute(&admin_pool).await.ok();

                let drop_sql = format!("DROP DATABASE {}", quote_identifier(db_name));
                sqlx::query(&drop_sql)
                    .execute(&admin_pool)
                    .await
                    .map_err(|e| format!("DROP DATABASE {}: {e}", db_name))?;
                admin_pool.close().await;
                Ok(())
            }
            Self::Mysql { admin_url, .. } => {
                let admin_pool =
                    sqlx::MySqlPool::connect(admin_url)
                        .await
                        .map_err(|e| format!("connect to mysql admin db: {e}"))?;

                let sql = format!("DROP DATABASE IF EXISTS `{}`", info.db_name);
                sqlx::query(&sql)
                    .execute(&admin_pool)
                    .await
                    .map_err(|e| format!("DROP DATABASE {}: {e}", info.db_name))?;
                admin_pool.close().await;
                Ok(())
            }
        }
    }
}

fn gen_db_name() -> String {
    let date = Utc::now().format("%Y%m%d");
    let rand: String = (0..8).map(|_| fastrand::lowercase()).collect();
    format!("aigw_test_{}_{}", date, rand)
}

/// Surround a PostgreSQL identifier with double quotes, escaping any embedded quotes.
fn quote_identifier(id: &str) -> String {
    let escaped = id.replace('"', "\"\"");
    format!("\"{}\"", escaped)
}
