//! Database module — SQLite with litellm-compatible schema
//!
//! aigw uses its own table names (see `docs/litellm-diff-baseline.md` §5 for mapping).
//! Column names, types, and defaults match litellm schema.prisma at the column level.
//! Data portability is ensured via the `aigw-migrate` tool, not by sharing table names.
//!
//! # Table name mapping (aigw → litellm)
//!
//! | aigw table                  | litellm table                     |
//! |-----------------------------|-----------------------------------|
//! | `virtual_keys`              | `LiteLLM_VerificationToken`       |
//! | `spend_logs`                | `LiteLLM_SpendLogs`               |
//! | `organizations`             | `LiteLLM_OrganizationTable`       |
//! | `teams`                     | `LiteLLM_TeamTable`               |
//! | `users`                     | `LiteLLM_UserTable`               |
//! | `projects`                  | `LiteLLM_ProjectTable`            |
//! | `budgets`                   | `LiteLLM_BudgetTable`             |
//! | `organization_memberships`  | `LiteLLM_OrganizationMembership`  |
//! | `team_memberships`          | `LiteLLM_TeamMembership`          |
//! | `deprecated_keys`           | `LiteLLM_DeprecatedVerificationToken` |
//! | `deleted_keys`              | `LiteLLM_DeletedVerificationToken`    |

use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use thiserror::Error;

use crate::models::*;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("SQL error: {0}")]
    Sql(#[from] sqlx::Error),
    #[error("Not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, DbError>;

/// Initialize SQLite pool and run migrations
pub async fn init_pool(database_url: &str) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    run_migrations(&pool).await?;
    Ok(pool)
}

/// Execute all CREATE TABLE IF NOT EXISTS statements
pub async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    // All tables are created with the exact column definitions from litellm schema.prisma
    // Stage 1 will produce the full, precise migration SQL.
    // For now: placeholder for the full migration script.

    // TODO (Stage 1): Full migration SQL for all 11 tables
    // - budgets
    // - virtual_keys (55 columns, 1:1 column mapping to LiteLLM_VerificationToken)
    // - spend_logs (24 columns, 1:1 column mapping to LiteLLM_SpendLogs)
    // - organizations (matches LiteLLM_OrganizationTable)
    // - teams (matches LiteLLM_TeamTable)
    // - users (matches LiteLLM_UserTable)
    // - projects (matches LiteLLM_ProjectTable)
    // - organization_memberships
    // - team_memberships
    // - deprecated_keys (key rotation grace period)
    // - deleted_keys (key audit trail)

    let _ = pool;
    Ok(())
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Key CRUD operations (placeholder signatures for Stage 2)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub async fn insert_key(pool: &SqlitePool, key: &VirtualKey) -> Result<()> {
    let _ = (pool, key);
    todo!("Stage 2: implement key insert with all 55 columns")
}

pub async fn get_key_by_token(pool: &SqlitePool, token_hash: &str) -> Result<Option<VirtualKey>> {
    let _ = (pool, token_hash);
    todo!("Stage 2: implement key lookup")
}

pub async fn list_keys(pool: &SqlitePool) -> Result<Vec<VirtualKey>> {
    let _ = pool;
    todo!("Stage 2: implement key listing")
}

pub async fn delete_key(pool: &SqlitePool, token_hash: &str) -> Result<()> {
    let _ = (pool, token_hash);
    todo!("Stage 2: implement key deletion")
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SpendLog CRUD (placeholder signatures for Stage 3)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub async fn insert_spend_log(pool: &SqlitePool, log: &SpendLog) -> Result<()> {
    let _ = (pool, log);
    todo!("Stage 3: implement spendlog insert with all 24 columns")
}

pub async fn query_spend_logs(
    pool: &SqlitePool,
    api_key: Option<&str>,
    limit: Option<i32>,
) -> Result<Vec<SpendLog>> {
    let _ = (pool, api_key, limit);
    todo!("Stage 3: implement spendlog query")
}
