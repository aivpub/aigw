//! Database module — multi-DB support with litellm-compatible schema
//!
//! aigw uses its own table names (see `docs/litellm-diff-baseline.md` §5 for mapping).
#![allow(deprecated)] // chrono::DateTime::from_utc — chrono 0.4.x deprecated but still widely used
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

use async_trait::async_trait;
use chrono::Utc;
use sqlx::mysql::MySqlPoolOptions;
use sqlx::postgres::PgPoolOptions;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{MySqlPool, PgPool, SqlitePool};
use std::str::FromStr;
use thiserror::Error;

use crate::models::*;

// re-export uuid for test use
#[cfg(test)]
use uuid::Uuid;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Error types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Error, Debug)]
pub enum DbError {
    #[error("SQL error: {0}")]
    Sql(#[from] sqlx::Error),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Invalid database URL: {0}")]
    InvalidUrl(String),
}

pub type Result<T> = std::result::Result<T, DbError>;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Database enum — runtime dispatch across SQLite, MySQL, PostgreSQL
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Multi-database pool supporting SQLite, MySQL, PostgreSQL
#[derive(Debug)]
pub enum Database {
    Sqlite(SqlitePool),
    Mysql(MySqlPool),
    Postgres(PgPool),
}

impl Database {
    /// Initialize database from DATABASE_URL and run migrations
    pub async fn init(database_url: &str) -> std::result::Result<Self, DbError> {
        if database_url.starts_with("sqlite:") {
            // Normalize to sqlx-compatible formats:
            //   sqlite::memory:     -> keep as-is
            //   sqlite:///abs/path  -> keep as-is (3 slashes, absolute)
            //   sqlite://relative   -> sqlite:relative (2 slashes, relative -> 1 colon)
            //   sqlite:/abs/path    -> sqlite:///abs/path (1 slash, absolute -> 3 slashes)
            let url = if database_url.starts_with("sqlite:///") {
                database_url.to_string()
            } else if database_url.starts_with("sqlite://") {
                database_url.replacen("sqlite://", "sqlite:", 1)
            } else if database_url.starts_with("sqlite:/") {
                database_url.replacen("sqlite:/", "sqlite:///", 1)
            } else {
                database_url.to_string()
            };
            let options = if url == "sqlite::memory:" || url == "sqlite:memory:" {
                SqliteConnectOptions::from_str(&url)?.create_if_missing(true)
            } else {
                // Ensure parent directory exists for file-based databases
                let path = url
                    .strip_prefix("sqlite:")
                    .and_then(|p| p.strip_prefix("///"))
                    .or_else(|| url.strip_prefix("sqlite:///"))
                    .or_else(|| url.strip_prefix("sqlite://"))
                    .unwrap_or(&url);
                if let Some(parent) = std::path::Path::new(path).parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent).ok();
                    }
                }
                SqliteConnectOptions::from_str(&url)?.create_if_missing(true)
            };
            let pool = SqlitePoolOptions::new()
                .max_connections(5)
                .connect_with(options)
                .await?;
            run_migrations_sqlite(&pool).await?;
            Ok(Database::Sqlite(pool))
        } else if database_url.starts_with("mysql://") {
            let pool = MySqlPoolOptions::new()
                .max_connections(10)
                .connect(database_url)
                .await?;
            run_migrations_mysql(&pool).await?;
            Ok(Database::Mysql(pool))
        } else if database_url.starts_with("postgresql://")
            || database_url.starts_with("postgres://")
        {
            let pool = PgPoolOptions::new()
                .max_connections(10)
                .connect(database_url)
                .await?;
            run_migrations_postgres(&pool).await?;
            Ok(Database::Postgres(pool))
        } else {
            Err(DbError::InvalidUrl(database_url.to_string()))
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Pool initialization
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Initialize SQLite pool and run migrations
pub async fn init_pool(database_url: &str) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    run_migrations_sqlite(&pool).await?;
    Ok(pool)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Migrations — per-database type
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Run SQLite migrations from `migrations/sqlite/`
pub async fn run_migrations_sqlite(pool: &SqlitePool) -> Result<()> {
    sqlx::migrate!("./migrations/sqlite")
        .run(pool)
        .await
        .map_err(|e| DbError::Sql(e.into()))?;
    Ok(())
}

/// Run PostgreSQL migrations from `migrations/postgres/`
pub async fn run_migrations_postgres(pool: &PgPool) -> Result<()> {
    sqlx::migrate!("./migrations/postgres")
        .run(pool)
        .await
        .map_err(|e| DbError::Sql(e.into()))?;
    Ok(())
}

/// Run MySQL migrations from `migrations/mysql/`
pub async fn run_migrations_mysql(pool: &MySqlPool) -> Result<()> {
    sqlx::migrate!("./migrations/mysql")
        .run(pool)
        .await
        .map_err(|e| DbError::Sql(e.into()))?;
    Ok(())
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// KeyStore trait — Virtual Key CRUD across all DB backends
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Trait for key operations across all DB backends.
/// Implemented for SqlitePool, MySqlPool, and PgPool individually.
#[async_trait]
pub trait KeyStore {
    /// Insert a new VirtualKey. The token field must already contain the SHA256 hash.
    async fn insert_key(&self, key: &VirtualKey) -> Result<()>;

    /// Lookup a VirtualKey by its SHA256 token hash.
    /// Returns None if the key does not exist, is blocked, or has expired.
    async fn get_key_by_token(&self, token_hash: &str) -> Result<Option<VirtualKey>>;

    /// List all virtual_keys, with optional team_id and user_id filters.
    async fn list_keys(
        &self,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<VirtualKey>>;

    /// Soft-delete a key: move to deleted_keys table, then remove from virtual_keys.
    async fn delete_key(&self, token_hash: &str) -> Result<()>;

    /// Update specific fields on a key (spend, models, max_budget, tpm_limit, rpm_limit, blocked, metadata, tags).
    async fn update_key(&self, token_hash: &str, key: &VirtualKey) -> Result<()>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// KeyStore implementation for SqlitePool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const INSERT_KEY_SQLITE: &str = r#"
INSERT INTO virtual_keys (
    token, key_name, key_alias, soft_budget_cooldown, spend, expires,
    models, aliases, config, router_settings,
    user_id, team_id, agent_id, project_id, permissions, max_parallel_requests,
    metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at,
    allowed_cache_controls, allowed_routes, policies, access_group_ids,
    model_spend, model_max_budget, budget_id, organization_id, object_permission_id,
    created_at, created_by, updated_at, updated_by,
    last_active, rotation_count, auto_rotate, rotation_interval,
    last_rotation_at, key_rotation_at, budget_limits
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#;

const GET_KEY_SQLITE: &str = r#"
SELECT
    token, key_name, key_alias, soft_budget_cooldown, spend, expires,
    models, aliases, config, router_settings,
    user_id, team_id, agent_id, project_id, permissions, max_parallel_requests,
    metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at,
    allowed_cache_controls, allowed_routes, policies, access_group_ids,
    model_spend, model_max_budget, budget_id, organization_id, object_permission_id,
    created_at, created_by, updated_at, updated_by,
    last_active, rotation_count, auto_rotate, rotation_interval,
    last_rotation_at, key_rotation_at, budget_limits
FROM virtual_keys
WHERE token = ?
"#;

const LIST_KEYS_SQLITE: &str = r#"
SELECT
    token, key_name, key_alias, soft_budget_cooldown, spend, expires,
    models, aliases, config, router_settings,
    user_id, team_id, agent_id, project_id, permissions, max_parallel_requests,
    metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at,
    allowed_cache_controls, allowed_routes, policies, access_group_ids,
    model_spend, model_max_budget, budget_id, organization_id, object_permission_id,
    created_at, created_by, updated_at, updated_by,
    last_active, rotation_count, auto_rotate, rotation_interval,
    last_rotation_at, key_rotation_at, budget_limits
FROM virtual_keys
"#;

const LIST_KEYS_TEAM_SQLITE: &str = r#"
SELECT
    token, key_name, key_alias, soft_budget_cooldown, spend, expires,
    models, aliases, config, router_settings,
    user_id, team_id, agent_id, project_id, permissions, max_parallel_requests,
    metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at,
    allowed_cache_controls, allowed_routes, policies, access_group_ids,
    model_spend, model_max_budget, budget_id, organization_id, object_permission_id,
    created_at, created_by, updated_at, updated_by,
    last_active, rotation_count, auto_rotate, rotation_interval,
    last_rotation_at, key_rotation_at, budget_limits
FROM virtual_keys
WHERE team_id = ?
"#;

const LIST_KEYS_USER_SQLITE: &str = r#"
SELECT
    token, key_name, key_alias, soft_budget_cooldown, spend, expires,
    models, aliases, config, router_settings,
    user_id, team_id, agent_id, project_id, permissions, max_parallel_requests,
    metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at,
    allowed_cache_controls, allowed_routes, policies, access_group_ids,
    model_spend, model_max_budget, budget_id, organization_id, object_permission_id,
    created_at, created_by, updated_at, updated_by,
    last_active, rotation_count, auto_rotate, rotation_interval,
    last_rotation_at, key_rotation_at, budget_limits
FROM virtual_keys
WHERE user_id = ?
"#;

const LIST_KEYS_TEAM_USER_SQLITE: &str = r#"
SELECT
    token, key_name, key_alias, soft_budget_cooldown, spend, expires,
    models, aliases, config, router_settings,
    user_id, team_id, agent_id, project_id, permissions, max_parallel_requests,
    metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at,
    allowed_cache_controls, allowed_routes, policies, access_group_ids,
    model_spend, model_max_budget, budget_id, organization_id, object_permission_id,
    created_at, created_by, updated_at, updated_by,
    last_active, rotation_count, auto_rotate, rotation_interval,
    last_rotation_at, key_rotation_at, budget_limits
FROM virtual_keys
WHERE team_id = ? AND user_id = ?
"#;

const INSERT_DELETED_KEY_SQLITE: &str = r#"
INSERT INTO deleted_keys (
    token, key_name, key_alias, soft_budget_cooldown, spend, expires,
    models, aliases, config, router_settings,
    user_id, team_id, agent_id, project_id, permissions, max_parallel_requests,
    metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at,
    allowed_cache_controls, allowed_routes, policies, access_group_ids,
    model_spend, model_max_budget, budget_id, organization_id, object_permission_id,
    created_at, created_by, updated_at, updated_by,
    last_active, rotation_count, auto_rotate, rotation_interval,
    last_rotation_at, key_rotation_at, budget_limits
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#;

const UPDATE_KEY_SQLITE: &str = r#"
UPDATE virtual_keys SET
    key_name = ?, key_alias = ?, spend = ?, models = ?, max_budget = ?,
    tpm_limit = ?, rpm_limit = ?, blocked = ?, metadata = ?,
    permissions = ?, budget_duration = ?, budget_reset_at = ?,
    max_parallel_requests = ?, aliases = ?, config = ?, router_settings = ?,
    user_id = ?, team_id = ?, agent_id = ?, project_id = ?,
    allowed_cache_controls = ?, allowed_routes = ?, policies = ?, access_group_ids = ?,
    model_spend = ?, model_max_budget = ?, budget_id = ?, organization_id = ?,
    object_permission_id = ?, updated_at = ?, updated_by = ?,
    soft_budget_cooldown = ?, expires = ?,
    auto_rotate = ?, rotation_interval = ?, budget_limits = ?
WHERE token = ?
"#;

#[async_trait]
impl KeyStore for SqlitePool {
    async fn insert_key(&self, key: &VirtualKey) -> Result<()> {
        sqlx::query(INSERT_KEY_SQLITE)
            .bind(&key.token)
            .bind(&key.key_name)
            .bind(&key.key_alias)
            .bind(key.soft_budget_cooldown)
            .bind(key.spend)
            .bind(key.expires)
            .bind(&key.models)
            .bind(&key.aliases)
            .bind(&key.config)
            .bind(&key.router_settings)
            .bind(&key.user_id)
            .bind(&key.team_id)
            .bind(&key.agent_id)
            .bind(&key.project_id)
            .bind(&key.permissions)
            .bind(key.max_parallel_requests)
            .bind(&key.metadata)
            .bind(key.blocked)
            .bind(key.tpm_limit)
            .bind(key.rpm_limit)
            .bind(key.max_budget)
            .bind(&key.budget_duration)
            .bind(key.budget_reset_at)
            .bind(&key.allowed_cache_controls)
            .bind(&key.allowed_routes)
            .bind(&key.policies)
            .bind(&key.access_group_ids)
            .bind(&key.model_spend)
            .bind(&key.model_max_budget)
            .bind(&key.budget_id)
            .bind(&key.organization_id)
            .bind(&key.object_permission_id)
            .bind(key.created_at)
            .bind(&key.created_by)
            .bind(key.updated_at)
            .bind(&key.updated_by)
            .bind(key.last_active)
            .bind(key.rotation_count)
            .bind(key.auto_rotate)
            .bind(&key.rotation_interval)
            .bind(key.last_rotation_at)
            .bind(key.key_rotation_at)
            .bind(&key.budget_limits)
            .execute(self)
            .await?;
        Ok(())
    }

    async fn get_key_by_token(&self, token_hash: &str) -> Result<Option<VirtualKey>> {
        let key: Option<VirtualKey> = sqlx::query_as(GET_KEY_SQLITE)
            .bind(token_hash)
            .fetch_optional(self)
            .await?;

        match key {
            Some(k) => {
                // Check blocked
                if k.blocked == Some(true) {
                    return Ok(None);
                }
                // Check expiry
                if let Some(expires) = k.expires {
                    if expires <= Utc::now() {
                        return Ok(None);
                    }
                }
                Ok(Some(k))
            }
            None => Ok(None),
        }
    }

    async fn list_keys(
        &self,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<VirtualKey>> {
        let keys = match (team_id, user_id) {
            (Some(tid), Some(uid)) => {
                sqlx::query_as(LIST_KEYS_TEAM_USER_SQLITE)
                    .bind(tid)
                    .bind(uid)
                    .fetch_all(self)
                    .await?
            }
            (Some(tid), None) => {
                sqlx::query_as(LIST_KEYS_TEAM_SQLITE)
                    .bind(tid)
                    .fetch_all(self)
                    .await?
            }
            (None, Some(uid)) => {
                sqlx::query_as(LIST_KEYS_USER_SQLITE)
                    .bind(uid)
                    .fetch_all(self)
                    .await?
            }
            (None, None) => sqlx::query_as(LIST_KEYS_SQLITE).fetch_all(self).await?,
        };
        Ok(keys)
    }

    async fn delete_key(&self, token_hash: &str) -> Result<()> {
        // 1. Move the key to deleted_keys
        let key = self.get_key_by_token(token_hash).await?;
        if let Some(k) = key {
            sqlx::query(INSERT_DELETED_KEY_SQLITE)
                .bind(&k.token)
                .bind(&k.key_name)
                .bind(&k.key_alias)
                .bind(k.soft_budget_cooldown)
                .bind(k.spend)
                .bind(k.expires)
                .bind(&k.models)
                .bind(&k.aliases)
                .bind(&k.config)
                .bind(&k.router_settings)
                .bind(&k.user_id)
                .bind(&k.team_id)
                .bind(&k.agent_id)
                .bind(&k.project_id)
                .bind(&k.permissions)
                .bind(k.max_parallel_requests)
                .bind(&k.metadata)
                .bind(k.blocked)
                .bind(k.tpm_limit)
                .bind(k.rpm_limit)
                .bind(k.max_budget)
                .bind(&k.budget_duration)
                .bind(k.budget_reset_at)
                .bind(&k.allowed_cache_controls)
                .bind(&k.allowed_routes)
                .bind(&k.policies)
                .bind(&k.access_group_ids)
                .bind(&k.model_spend)
                .bind(&k.model_max_budget)
                .bind(&k.budget_id)
                .bind(&k.organization_id)
                .bind(&k.object_permission_id)
                .bind(k.created_at)
                .bind(&k.created_by)
                .bind(k.updated_at)
                .bind(&k.updated_by)
                .bind(k.last_active)
                .bind(k.rotation_count)
                .bind(k.auto_rotate)
                .bind(&k.rotation_interval)
                .bind(k.last_rotation_at)
                .bind(k.key_rotation_at)
                .bind(&k.budget_limits)
                .execute(self)
                .await?;
        }

        // 2. Delete from virtual_keys
        sqlx::query("DELETE FROM virtual_keys WHERE token = ?")
            .bind(token_hash)
            .execute(self)
            .await?;
        Ok(())
    }

    async fn update_key(&self, token_hash: &str, key: &VirtualKey) -> Result<()> {
        let now = Utc::now();
        sqlx::query(UPDATE_KEY_SQLITE)
            .bind(&key.key_name)
            .bind(&key.key_alias)
            .bind(key.spend)
            .bind(&key.models)
            .bind(key.max_budget)
            .bind(key.tpm_limit)
            .bind(key.rpm_limit)
            .bind(key.blocked)
            .bind(&key.metadata)
            .bind(&key.permissions)
            .bind(&key.budget_duration)
            .bind(key.budget_reset_at)
            .bind(key.max_parallel_requests)
            .bind(&key.aliases)
            .bind(&key.config)
            .bind(&key.router_settings)
            .bind(&key.user_id)
            .bind(&key.team_id)
            .bind(&key.agent_id)
            .bind(&key.project_id)
            .bind(&key.allowed_cache_controls)
            .bind(&key.allowed_routes)
            .bind(&key.policies)
            .bind(&key.access_group_ids)
            .bind(&key.model_spend)
            .bind(&key.model_max_budget)
            .bind(&key.budget_id)
            .bind(&key.organization_id)
            .bind(&key.object_permission_id)
            .bind(now)
            .bind(&key.updated_by)
            .bind(key.soft_budget_cooldown)
            .bind(key.expires)
            .bind(key.auto_rotate)
            .bind(&key.rotation_interval)
            .bind(&key.budget_limits)
            .bind(token_hash)
            .execute(self)
            .await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// KeyStore implementation for MySqlPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const INSERT_KEY_MYSQL: &str = r#"
INSERT INTO virtual_keys (
    token, key_name, key_alias, soft_budget_cooldown, spend, expires,
    models, aliases, config, router_settings,
    user_id, team_id, agent_id, project_id, permissions, max_parallel_requests,
    metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at,
    allowed_cache_controls, allowed_routes, policies, access_group_ids,
    model_spend, model_max_budget, budget_id, organization_id, object_permission_id,
    created_at, created_by, updated_at, updated_by,
    last_active, rotation_count, auto_rotate, rotation_interval,
    last_rotation_at, key_rotation_at, budget_limits
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#;

#[async_trait]
impl KeyStore for MySqlPool {
    async fn insert_key(&self, key: &VirtualKey) -> Result<()> {
        sqlx::query(INSERT_KEY_MYSQL)
            .bind(&key.token)
            .bind(&key.key_name)
            .bind(&key.key_alias)
            .bind(key.soft_budget_cooldown as i8)
            .bind(key.spend)
            .bind(key.expires)
            .bind(&key.models)
            .bind(&key.aliases)
            .bind(&key.config)
            .bind(&key.router_settings)
            .bind(&key.user_id)
            .bind(&key.team_id)
            .bind(&key.agent_id)
            .bind(&key.project_id)
            .bind(&key.permissions)
            .bind(key.max_parallel_requests)
            .bind(&key.metadata)
            .bind(key.blocked.map(|b| b as i8))
            .bind(key.tpm_limit)
            .bind(key.rpm_limit)
            .bind(key.max_budget)
            .bind(&key.budget_duration)
            .bind(key.budget_reset_at)
            .bind(&key.allowed_cache_controls)
            .bind(&key.allowed_routes)
            .bind(&key.policies)
            .bind(&key.access_group_ids)
            .bind(&key.model_spend)
            .bind(&key.model_max_budget)
            .bind(&key.budget_id)
            .bind(&key.organization_id)
            .bind(&key.object_permission_id)
            .bind(key.created_at)
            .bind(&key.created_by)
            .bind(key.updated_at)
            .bind(&key.updated_by)
            .bind(key.last_active)
            .bind(key.rotation_count)
            .bind(key.auto_rotate.map(|b| b as i8))
            .bind(&key.rotation_interval)
            .bind(key.last_rotation_at)
            .bind(key.key_rotation_at)
            .bind(&key.budget_limits)
            .execute(self)
            .await?;
        Ok(())
    }

    async fn get_key_by_token(&self, token_hash: &str) -> Result<Option<VirtualKey>> {
        let key: Option<VirtualKey> = sqlx::query_as(GET_KEY_SQLITE)
            .bind(token_hash)
            .fetch_optional(self)
            .await?;

        match key {
            Some(k) => {
                if k.blocked == Some(true) {
                    return Ok(None);
                }
                if let Some(expires) = k.expires {
                    if expires <= Utc::now() {
                        return Ok(None);
                    }
                }
                Ok(Some(k))
            }
            None => Ok(None),
        }
    }

    async fn list_keys(
        &self,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<VirtualKey>> {
        let keys = match (team_id, user_id) {
            (Some(tid), Some(uid)) => {
                sqlx::query_as(LIST_KEYS_TEAM_USER_SQLITE)
                    .bind(tid)
                    .bind(uid)
                    .fetch_all(self)
                    .await?
            }
            (Some(tid), None) => {
                sqlx::query_as(LIST_KEYS_TEAM_SQLITE)
                    .bind(tid)
                    .fetch_all(self)
                    .await?
            }
            (None, Some(uid)) => {
                sqlx::query_as(LIST_KEYS_USER_SQLITE)
                    .bind(uid)
                    .fetch_all(self)
                    .await?
            }
            (None, None) => sqlx::query_as(LIST_KEYS_SQLITE).fetch_all(self).await?,
        };
        Ok(keys)
    }

    async fn delete_key(&self, token_hash: &str) -> Result<()> {
        let key = self.get_key_by_token(token_hash).await?;
        if let Some(k) = key {
            sqlx::query(INSERT_DELETED_KEY_SQLITE)
                .bind(&k.token)
                .bind(&k.key_name)
                .bind(&k.key_alias)
                .bind(k.soft_budget_cooldown as i8)
                .bind(k.spend)
                .bind(k.expires)
                .bind(&k.models)
                .bind(&k.aliases)
                .bind(&k.config)
                .bind(&k.router_settings)
                .bind(&k.user_id)
                .bind(&k.team_id)
                .bind(&k.agent_id)
                .bind(&k.project_id)
                .bind(&k.permissions)
                .bind(k.max_parallel_requests)
                .bind(&k.metadata)
                .bind(k.blocked.map(|b| b as i8))
                .bind(k.tpm_limit)
                .bind(k.rpm_limit)
                .bind(k.max_budget)
                .bind(&k.budget_duration)
                .bind(k.budget_reset_at)
                .bind(&k.allowed_cache_controls)
                .bind(&k.allowed_routes)
                .bind(&k.policies)
                .bind(&k.access_group_ids)
                .bind(&k.model_spend)
                .bind(&k.model_max_budget)
                .bind(&k.budget_id)
                .bind(&k.organization_id)
                .bind(&k.object_permission_id)
                .bind(k.created_at)
                .bind(&k.created_by)
                .bind(k.updated_at)
                .bind(&k.updated_by)
                .bind(k.last_active)
                .bind(k.rotation_count)
                .bind(k.auto_rotate.map(|b| b as i8))
                .bind(&k.rotation_interval)
                .bind(k.last_rotation_at)
                .bind(k.key_rotation_at)
                .bind(&k.budget_limits)
                .execute(self)
                .await?;
        }
        sqlx::query("DELETE FROM virtual_keys WHERE token = ?")
            .bind(token_hash)
            .execute(self)
            .await?;
        Ok(())
    }

    async fn update_key(&self, token_hash: &str, key: &VirtualKey) -> Result<()> {
        let now = Utc::now();
        sqlx::query("UPDATE virtual_keys SET key_name = ?, key_alias = ?, spend = ?, models = ?, max_budget = ?, tpm_limit = ?, rpm_limit = ?, blocked = ?, metadata = ?, permissions = ?, budget_duration = ?, budget_reset_at = ?, max_parallel_requests = ?, aliases = ?, config = ?, router_settings = ?, user_id = ?, team_id = ?, agent_id = ?, project_id = ?, allowed_cache_controls = ?, allowed_routes = ?, policies = ?, access_group_ids = ?, model_spend = ?, model_max_budget = ?, budget_id = ?, organization_id = ?, object_permission_id = ?, updated_at = ?, updated_by = ?, soft_budget_cooldown = ?, expires = ?, auto_rotate = ?, rotation_interval = ?, budget_limits = ? WHERE token = ?")
            .bind(&key.key_name).bind(&key.key_alias).bind(key.spend).bind(&key.models)
            .bind(key.max_budget).bind(key.tpm_limit).bind(key.rpm_limit)
            .bind(key.blocked.map(|b| b as i8)).bind(&key.metadata).bind(&key.permissions)
            .bind(&key.budget_duration).bind(key.budget_reset_at).bind(key.max_parallel_requests)
            .bind(&key.aliases).bind(&key.config).bind(&key.router_settings)
            .bind(&key.user_id).bind(&key.team_id).bind(&key.agent_id).bind(&key.project_id)
            .bind(&key.allowed_cache_controls).bind(&key.allowed_routes)
            .bind(&key.policies).bind(&key.access_group_ids)
            .bind(&key.model_spend).bind(&key.model_max_budget)
            .bind(&key.budget_id).bind(&key.organization_id).bind(&key.object_permission_id)
            .bind(now).bind(&key.updated_by)
            .bind(key.soft_budget_cooldown as i8).bind(key.expires)
            .bind(key.auto_rotate.map(|b| b as i8)).bind(&key.rotation_interval)
            .bind(&key.budget_limits)
            .bind(token_hash)
            .execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// KeyStore implementation for PgPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const INSERT_KEY_PG: &str = r#"
INSERT INTO virtual_keys (
    token, key_name, key_alias, soft_budget_cooldown, spend, expires,
    models, aliases, config, router_settings,
    user_id, team_id, agent_id, project_id, permissions, max_parallel_requests,
    metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at,
    allowed_cache_controls, allowed_routes, policies, access_group_ids,
    model_spend, model_max_budget, budget_id, organization_id, object_permission_id,
    created_at, created_by, updated_at, updated_by,
    last_active, rotation_count, auto_rotate, rotation_interval,
    last_rotation_at, key_rotation_at, budget_limits
) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,$38,$39,$40,$41,$42,$43)
"#;

const GET_KEY_PG: &str = r#"
SELECT
    token, key_name, key_alias, soft_budget_cooldown, spend, expires,
    models, aliases, config, router_settings,
    user_id, team_id, agent_id, project_id, permissions, max_parallel_requests,
    metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at,
    allowed_cache_controls, allowed_routes, policies, access_group_ids,
    model_spend, model_max_budget, budget_id, organization_id, object_permission_id,
    created_at, created_by, updated_at, updated_by,
    last_active, rotation_count, auto_rotate, rotation_interval,
    last_rotation_at, key_rotation_at, budget_limits
FROM virtual_keys
WHERE token = $1
"#;

const LIST_KEYS_PG: &str = r#"
SELECT
    token, key_name, key_alias, soft_budget_cooldown, spend, expires,
    models, aliases, config, router_settings,
    user_id, team_id, agent_id, project_id, permissions, max_parallel_requests,
    metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at,
    allowed_cache_controls, allowed_routes, policies, access_group_ids,
    model_spend, model_max_budget, budget_id, organization_id, object_permission_id,
    created_at, created_by, updated_at, updated_by,
    last_active, rotation_count, auto_rotate, rotation_interval,
    last_rotation_at, key_rotation_at, budget_limits
FROM virtual_keys
"#;

const INSERT_DELETED_KEY_PG: &str = r#"
INSERT INTO deleted_keys (
    token, key_name, key_alias, soft_budget_cooldown, spend, expires,
    models, aliases, config, router_settings,
    user_id, team_id, agent_id, project_id, permissions, max_parallel_requests,
    metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at,
    allowed_cache_controls, allowed_routes, policies, access_group_ids,
    model_spend, model_max_budget, budget_id, organization_id, object_permission_id,
    created_at, created_by, updated_at, updated_by,
    last_active, rotation_count, auto_rotate, rotation_interval,
    last_rotation_at, key_rotation_at, budget_limits
) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,$38,$39,$40,$41,$42,$43)
"#;

#[async_trait]
impl KeyStore for PgPool {
    async fn insert_key(&self, key: &VirtualKey) -> Result<()> {
        sqlx::query(INSERT_KEY_PG)
            .bind(&key.token)
            .bind(&key.key_name)
            .bind(&key.key_alias)
            .bind(key.soft_budget_cooldown)
            .bind(key.spend)
            .bind(key.expires)
            .bind(&key.models)
            .bind(&key.aliases)
            .bind(&key.config)
            .bind(&key.router_settings)
            .bind(&key.user_id)
            .bind(&key.team_id)
            .bind(&key.agent_id)
            .bind(&key.project_id)
            .bind(&key.permissions)
            .bind(key.max_parallel_requests)
            .bind(&key.metadata)
            .bind(key.blocked)
            .bind(key.tpm_limit)
            .bind(key.rpm_limit)
            .bind(key.max_budget)
            .bind(&key.budget_duration)
            .bind(key.budget_reset_at)
            .bind(&key.allowed_cache_controls)
            .bind(&key.allowed_routes)
            .bind(&key.policies)
            .bind(&key.access_group_ids)
            .bind(&key.model_spend)
            .bind(&key.model_max_budget)
            .bind(&key.budget_id)
            .bind(&key.organization_id)
            .bind(&key.object_permission_id)
            .bind(key.created_at)
            .bind(&key.created_by)
            .bind(key.updated_at)
            .bind(&key.updated_by)
            .bind(key.last_active)
            .bind(key.rotation_count)
            .bind(key.auto_rotate)
            .bind(&key.rotation_interval)
            .bind(key.last_rotation_at)
            .bind(key.key_rotation_at)
            .bind(&key.budget_limits)
            .execute(self)
            .await?;
        Ok(())
    }

    async fn get_key_by_token(&self, token_hash: &str) -> Result<Option<VirtualKey>> {
        let key: Option<VirtualKey> = sqlx::query_as(GET_KEY_PG)
            .bind(token_hash)
            .fetch_optional(self)
            .await?;

        match key {
            Some(k) => {
                if k.blocked == Some(true) {
                    return Ok(None);
                }
                if let Some(expires) = k.expires {
                    if expires <= Utc::now() {
                        return Ok(None);
                    }
                }
                Ok(Some(k))
            }
            None => Ok(None),
        }
    }

    async fn list_keys(
        &self,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<VirtualKey>> {
        let keys = match (team_id, user_id) {
            (Some(tid), Some(uid)) => {
                sqlx::query_as("SELECT token, key_name, key_alias, soft_budget_cooldown, spend, expires, models, aliases, config, router_settings, user_id, team_id, agent_id, project_id, permissions, max_parallel_requests, metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, budget_id, organization_id, object_permission_id, created_at, created_by, updated_at, updated_by, last_active, rotation_count, auto_rotate, rotation_interval, last_rotation_at, key_rotation_at, budget_limits FROM virtual_keys WHERE team_id = $1 AND user_id = $2")
                    .bind(tid).bind(uid).fetch_all(self).await?
            }
            (Some(tid), None) => {
                sqlx::query_as("SELECT token, key_name, key_alias, soft_budget_cooldown, spend, expires, models, aliases, config, router_settings, user_id, team_id, agent_id, project_id, permissions, max_parallel_requests, metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, budget_id, organization_id, object_permission_id, created_at, created_by, updated_at, updated_by, last_active, rotation_count, auto_rotate, rotation_interval, last_rotation_at, key_rotation_at, budget_limits FROM virtual_keys WHERE team_id = $1")
                    .bind(tid).fetch_all(self).await?
            }
            (None, Some(uid)) => {
                sqlx::query_as("SELECT token, key_name, key_alias, soft_budget_cooldown, spend, expires, models, aliases, config, router_settings, user_id, team_id, agent_id, project_id, permissions, max_parallel_requests, metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, budget_id, organization_id, object_permission_id, created_at, created_by, updated_at, updated_by, last_active, rotation_count, auto_rotate, rotation_interval, last_rotation_at, key_rotation_at, budget_limits FROM virtual_keys WHERE user_id = $1")
                    .bind(uid).fetch_all(self).await?
            }
            (None, None) => {
                sqlx::query_as(LIST_KEYS_PG).fetch_all(self).await?
            }
        };
        Ok(keys)
    }

    async fn delete_key(&self, token_hash: &str) -> Result<()> {
        let key = self.get_key_by_token(token_hash).await?;
        if let Some(k) = key {
            sqlx::query(INSERT_DELETED_KEY_PG)
                .bind(&k.token)
                .bind(&k.key_name)
                .bind(&k.key_alias)
                .bind(k.soft_budget_cooldown)
                .bind(k.spend)
                .bind(k.expires)
                .bind(&k.models)
                .bind(&k.aliases)
                .bind(&k.config)
                .bind(&k.router_settings)
                .bind(&k.user_id)
                .bind(&k.team_id)
                .bind(&k.agent_id)
                .bind(&k.project_id)
                .bind(&k.permissions)
                .bind(k.max_parallel_requests)
                .bind(&k.metadata)
                .bind(k.blocked)
                .bind(k.tpm_limit)
                .bind(k.rpm_limit)
                .bind(k.max_budget)
                .bind(&k.budget_duration)
                .bind(k.budget_reset_at)
                .bind(&k.allowed_cache_controls)
                .bind(&k.allowed_routes)
                .bind(&k.policies)
                .bind(&k.access_group_ids)
                .bind(&k.model_spend)
                .bind(&k.model_max_budget)
                .bind(&k.budget_id)
                .bind(&k.organization_id)
                .bind(&k.object_permission_id)
                .bind(k.created_at)
                .bind(&k.created_by)
                .bind(k.updated_at)
                .bind(&k.updated_by)
                .bind(k.last_active)
                .bind(k.rotation_count)
                .bind(k.auto_rotate)
                .bind(&k.rotation_interval)
                .bind(k.last_rotation_at)
                .bind(k.key_rotation_at)
                .bind(&k.budget_limits)
                .execute(self)
                .await?;
        }
        sqlx::query("DELETE FROM virtual_keys WHERE token = $1")
            .bind(token_hash)
            .execute(self)
            .await?;
        Ok(())
    }

    async fn update_key(&self, token_hash: &str, key: &VirtualKey) -> Result<()> {
        let now = Utc::now();
        sqlx::query("UPDATE virtual_keys SET key_name = $1, key_alias = $2, spend = $3, models = $4, max_budget = $5, tpm_limit = $6, rpm_limit = $7, blocked = $8, metadata = $9, permissions = $10, budget_duration = $11, budget_reset_at = $12, max_parallel_requests = $13, aliases = $14, config = $15, router_settings = $16, user_id = $17, team_id = $18, agent_id = $19, project_id = $20, allowed_cache_controls = $21, allowed_routes = $22, policies = $23, access_group_ids = $24, model_spend = $25, model_max_budget = $26, budget_id = $27, organization_id = $28, object_permission_id = $29, updated_at = $30, updated_by = $31, soft_budget_cooldown = $32, expires = $33, auto_rotate = $34, rotation_interval = $35, budget_limits = $36 WHERE token = $37")
            .bind(&key.key_name).bind(&key.key_alias).bind(key.spend).bind(&key.models)
            .bind(key.max_budget).bind(key.tpm_limit).bind(key.rpm_limit)
            .bind(key.blocked).bind(&key.metadata).bind(&key.permissions)
            .bind(&key.budget_duration).bind(key.budget_reset_at).bind(key.max_parallel_requests)
            .bind(&key.aliases).bind(&key.config).bind(&key.router_settings)
            .bind(&key.user_id).bind(&key.team_id).bind(&key.agent_id).bind(&key.project_id)
            .bind(&key.allowed_cache_controls).bind(&key.allowed_routes)
            .bind(&key.policies).bind(&key.access_group_ids)
            .bind(&key.model_spend).bind(&key.model_max_budget)
            .bind(&key.budget_id).bind(&key.organization_id).bind(&key.object_permission_id)
            .bind(now).bind(&key.updated_by)
            .bind(key.soft_budget_cooldown).bind(key.expires)
            .bind(key.auto_rotate).bind(&key.rotation_interval)
            .bind(&key.budget_limits)
            .bind(token_hash)
            .execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Database enum key dispatch
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl Database {
    /// Returns the database backend type as a static string
    pub fn database_type(&self) -> &'static str {
        match self {
            Database::Sqlite(_) => "sqlite",
            Database::Mysql(_) => "mysql",
            Database::Postgres(_) => "postgres",
        }
    }

    pub async fn insert_key(&self, key: &VirtualKey) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.insert_key(key).await,
            Database::Mysql(pool) => pool.insert_key(key).await,
            Database::Postgres(pool) => pool.insert_key(key).await,
        }
    }

    pub async fn get_key_by_token(&self, token_hash: &str) -> Result<Option<VirtualKey>> {
        match self {
            Database::Sqlite(pool) => pool.get_key_by_token(token_hash).await,
            Database::Mysql(pool) => pool.get_key_by_token(token_hash).await,
            Database::Postgres(pool) => pool.get_key_by_token(token_hash).await,
        }
    }

    pub async fn list_keys(
        &self,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<VirtualKey>> {
        match self {
            Database::Sqlite(pool) => pool.list_keys(team_id, user_id).await,
            Database::Mysql(pool) => pool.list_keys(team_id, user_id).await,
            Database::Postgres(pool) => pool.list_keys(team_id, user_id).await,
        }
    }

    pub async fn delete_key(&self, token_hash: &str) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.delete_key(token_hash).await,
            Database::Mysql(pool) => pool.delete_key(token_hash).await,
            Database::Postgres(pool) => pool.delete_key(token_hash).await,
        }
    }

    pub async fn update_key(&self, token_hash: &str, key: &VirtualKey) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.update_key(token_hash, key).await,
            Database::Mysql(pool) => pool.update_key(token_hash, key).await,
            Database::Postgres(pool) => pool.update_key(token_hash, key).await,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SpendLogStore trait — Spend log CRUD across all DB backends
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Trait for spend log operations across all DB backends.
#[async_trait]
pub trait SpendLogStore {
    async fn insert_spend_log(&self, log: &SpendLog) -> Result<()>;
    async fn query_spend_logs(
        &self,
        api_key: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<SpendLog>>;
    async fn get_spend_by_key(&self, api_key: &str) -> Result<f64>;
    async fn get_spend_by_user(&self, user_id: &str) -> Result<f64>;
    async fn get_spend_by_tag(&self, tag: &str) -> Result<f64>;
    async fn get_global_spend(&self) -> Result<f64>;
    async fn aggregate_spend_by_model(&self, api_key: Option<&str>) -> Result<Vec<SpendModelAgg>>;
    async fn aggregate_spend_by_provider(&self) -> Result<Vec<SpendProviderAgg>>;
    async fn query_spend_logs_filtered(
        &self,
        api_key: Option<&str>,
        model: Option<&str>,
        provider: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<SpendLog>>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SpendLogStore implementation for SqlitePool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const INSERT_SPEND_LOG_SQLITE: &str = r#"
INSERT INTO spend_logs (
    request_id, call_type, api_key, spend, total_tokens,
    prompt_tokens, completion_tokens, start_time, end_time,
    request_duration_ms, completion_start_time, model, model_id, model_group,
    custom_llm_provider, api_base, "user", metadata,
    cache_hit, cache_key, request_tags, team_id, organization_id,
    end_user, requester_ip_address, messages, response,
    session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request
) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
"#;

const QUERY_SPEND_LOGS_ALL_SQLITE: &str = r#"
SELECT
    request_id, call_type, api_key, spend, total_tokens,
    prompt_tokens, completion_tokens, start_time, end_time,
    request_duration_ms, completion_start_time, model, model_id, model_group,
    custom_llm_provider, api_base, "user", metadata,
    cache_hit, cache_key, request_tags, team_id, organization_id,
    end_user, requester_ip_address, messages, response,
    session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request
FROM spend_logs
ORDER BY start_time DESC
LIMIT ?
"#;

const QUERY_SPEND_LOGS_BY_KEY_SQLITE: &str = r#"
SELECT
    request_id, call_type, api_key, spend, total_tokens,
    prompt_tokens, completion_tokens, start_time, end_time,
    request_duration_ms, completion_start_time, model, model_id, model_group,
    custom_llm_provider, api_base, "user", metadata,
    cache_hit, cache_key, request_tags, team_id, organization_id,
    end_user, requester_ip_address, messages, response,
    session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request
FROM spend_logs
WHERE api_key = ?
ORDER BY start_time DESC
LIMIT ?
"#;

#[async_trait]
impl SpendLogStore for SqlitePool {
    async fn insert_spend_log(&self, log: &SpendLog) -> Result<()> {
        sqlx::query(INSERT_SPEND_LOG_SQLITE)
            .bind(&log.request_id)
            .bind(&log.call_type)
            .bind(&log.api_key)
            .bind(log.spend)
            .bind(log.total_tokens)
            .bind(log.prompt_tokens)
            .bind(log.completion_tokens)
            .bind(log.start_time)
            .bind(log.end_time)
            .bind(log.request_duration_ms)
            .bind(log.completion_start_time)
            .bind(&log.model)
            .bind(&log.model_id)
            .bind(&log.model_group)
            .bind(&log.custom_llm_provider)
            .bind(&log.api_base)
            .bind(&log.user)
            .bind(&log.metadata)
            .bind(&log.cache_hit)
            .bind(&log.cache_key)
            .bind(&log.request_tags)
            .bind(&log.team_id)
            .bind(&log.organization_id)
            .bind(&log.end_user)
            .bind(&log.requester_ip_address)
            .bind(&log.messages)
            .bind(&log.response)
            .bind(&log.session_id)
            .bind(&log.status)
            .bind(&log.mcp_namespaced_tool_name)
            .bind(&log.agent_id)
            .bind(&log.proxy_server_request)
            .execute(self)
            .await?;
        Ok(())
    }

    async fn query_spend_logs(
        &self,
        api_key: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        let limit_val = limit.unwrap_or(100);
        match api_key {
            Some(key) => sqlx::query_as(QUERY_SPEND_LOGS_BY_KEY_SQLITE)
                .bind(key)
                .bind(limit_val)
                .fetch_all(self)
                .await
                .map_err(DbError::from),
            None => sqlx::query_as(QUERY_SPEND_LOGS_ALL_SQLITE)
                .bind(limit_val)
                .fetch_all(self)
                .await
                .map_err(DbError::from),
        }
    }

    async fn get_spend_by_key(&self, api_key: &str) -> Result<f64> {
        let row: (Option<f64>,) =
            sqlx::query_as("SELECT SUM(spend) FROM spend_logs WHERE api_key = ?")
                .bind(api_key)
                .fetch_one(self)
                .await?;
        Ok(row.0.unwrap_or(0.0))
    }

    async fn get_spend_by_user(&self, user_id: &str) -> Result<f64> {
        let row: (Option<f64>,) =
            sqlx::query_as(r#"SELECT SUM(spend) FROM spend_logs WHERE "user" = ?"#)
                .bind(user_id)
                .fetch_one(self)
                .await?;
        Ok(row.0.unwrap_or(0.0))
    }

    async fn get_spend_by_tag(&self, tag: &str) -> Result<f64> {
        let pattern = format!("%{}%", tag);
        let row: (Option<f64>,) =
            sqlx::query_as("SELECT SUM(spend) FROM spend_logs WHERE request_tags LIKE ?")
                .bind(&pattern)
                .fetch_one(self)
                .await?;
        Ok(row.0.unwrap_or(0.0))
    }

    async fn get_global_spend(&self) -> Result<f64> {
        let row: (Option<f64>,) = sqlx::query_as("SELECT SUM(spend) FROM spend_logs")
            .fetch_one(self)
            .await?;
        Ok(row.0.unwrap_or(0.0))
    }

    async fn aggregate_spend_by_model(&self, api_key: Option<&str>) -> Result<Vec<SpendModelAgg>> {
        match api_key {
            Some(_key) => {
                sqlx::query_as(
                    "SELECT model, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests \
                     FROM spend_logs WHERE api_key = ? GROUP BY model ORDER BY total_tokens DESC"
                )
                .bind(_key)
                .fetch_all(self)
                .await
                .map_err(DbError::from)
            }
            None => {
                sqlx::query_as(
                    "SELECT model, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests \
                     FROM spend_logs GROUP BY model ORDER BY total_tokens DESC"
                )
                .fetch_all(self)
                .await
                .map_err(DbError::from)
            }
        }
    }

    async fn aggregate_spend_by_provider(&self) -> Result<Vec<SpendProviderAgg>> {
        // Join spend_logs with proxy_models to get provider info from litellm_params JSON
        let rows: Vec<(String, i64, f64, i64)> = sqlx::query_as(
            r#"SELECT COALESCE(json_extract(pm.litellm_params, '$.model'), sl.model) as provider,
               COALESCE(SUM(sl.total_tokens), 0) as total_tokens,
               COALESCE(SUM(sl.spend), 0) as total_spend,
               COUNT(sl.request_id) as requests
               FROM spend_logs sl
               LEFT JOIN proxy_models pm ON sl.model = pm.model_name
               GROUP BY provider
               ORDER BY total_tokens DESC"#
        )
        .fetch_all(self)
        .await
        .map_err(DbError::from)?;

        Ok(rows.into_iter().map(|(provider, total_tokens, total_spend, requests)| {
            SpendProviderAgg { provider, total_tokens, total_spend, requests }
        }).collect())
    }

    async fn query_spend_logs_filtered(
        &self,
        api_key: Option<&str>,
        model: Option<&str>,
        _provider: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        let limit_val = limit.unwrap_or(100);
        let mut sql = String::from(
            r#"SELECT
                request_id, call_type, api_key, spend, total_tokens,
                prompt_tokens, completion_tokens, start_time, end_time,
                request_duration_ms, completion_start_time, model, model_id, model_group,
                custom_llm_provider, api_base, "user", metadata,
                cache_hit, cache_key, request_tags, team_id, organization_id,
                end_user, requester_ip_address, messages, response,
                session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request
            FROM spend_logs WHERE 1=1"#
        );

        if api_key.is_some() { sql.push_str(" AND api_key = ?"); }
        if model.is_some() { sql.push_str(" AND model = ?"); }
        if start_date.is_some() { sql.push_str(" AND start_time >= ?"); }
        if end_date.is_some() { sql.push_str(" AND start_time <= ?"); }

        sql.push_str(" ORDER BY start_time DESC LIMIT ?");

        let mut query = sqlx::query_as(&sql);
        if let Some(k) = api_key { query = query.bind(k); }
        if let Some(m) = model { query = query.bind(m); }
        if let Some(sd) = start_date { query = query.bind(sd); }
        if let Some(ed) = end_date { query = query.bind(ed); }
        query = query.bind(limit_val);

        query.fetch_all(self).await.map_err(DbError::from)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SpendLogStore implementation for MySqlPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl SpendLogStore for MySqlPool {
    async fn insert_spend_log(&self, log: &SpendLog) -> Result<()> {
        sqlx::query(
            "INSERT INTO spend_logs (request_id, call_type, api_key, spend, total_tokens, \
             prompt_tokens, completion_tokens, start_time, end_time, \
             request_duration_ms, completion_start_time, model, model_id, model_group, \
             custom_llm_provider, api_base, user, metadata, \
             cache_hit, cache_key, request_tags, team_id, organization_id, \
             end_user, requester_ip_address, messages, response, \
             session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request) \
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&log.request_id)
        .bind(&log.call_type)
        .bind(&log.api_key)
        .bind(log.spend)
        .bind(log.total_tokens)
        .bind(log.prompt_tokens)
        .bind(log.completion_tokens)
        .bind(log.start_time)
        .bind(log.end_time)
        .bind(log.request_duration_ms)
        .bind(log.completion_start_time)
        .bind(&log.model)
        .bind(&log.model_id)
        .bind(&log.model_group)
        .bind(&log.custom_llm_provider)
        .bind(&log.api_base)
        .bind(&log.user)
        .bind(&log.metadata)
        .bind(&log.cache_hit)
        .bind(&log.cache_key)
        .bind(&log.request_tags)
        .bind(&log.team_id)
        .bind(&log.organization_id)
        .bind(&log.end_user)
        .bind(&log.requester_ip_address)
        .bind(&log.messages)
        .bind(&log.response)
        .bind(&log.session_id)
        .bind(&log.status)
        .bind(&log.mcp_namespaced_tool_name)
        .bind(&log.agent_id)
        .bind(&log.proxy_server_request)
        .execute(self)
        .await?;
        Ok(())
    }

    async fn query_spend_logs(
        &self,
        api_key: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        let limit_val = limit.unwrap_or(100);
        let sql = "SELECT request_id, call_type, api_key, spend, total_tokens, \
                   prompt_tokens, completion_tokens, start_time, end_time, \
                   request_duration_ms, completion_start_time, model, model_id, model_group, \
                   custom_llm_provider, api_base, user, metadata, \
                   cache_hit, cache_key, request_tags, team_id, organization_id, \
                   end_user, requester_ip_address, messages, response, \
                   session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request \
                   FROM spend_logs";
        match api_key {
            Some(key) => sqlx::query_as(&format!(
                "{} WHERE api_key = ? ORDER BY start_time DESC LIMIT {}",
                sql, limit_val
            ))
            .bind(key)
            .fetch_all(self)
            .await
            .map_err(DbError::from),
            None => sqlx::query_as(&format!(
                "{} ORDER BY start_time DESC LIMIT {}",
                sql, limit_val
            ))
            .fetch_all(self)
            .await
            .map_err(DbError::from),
        }
    }

    async fn get_spend_by_key(&self, api_key: &str) -> Result<f64> {
        let row: (Option<f64>,) =
            sqlx::query_as("SELECT SUM(spend) FROM spend_logs WHERE api_key = ?")
                .bind(api_key)
                .fetch_one(self)
                .await?;
        Ok(row.0.unwrap_or(0.0))
    }

    async fn get_spend_by_user(&self, user_id: &str) -> Result<f64> {
        let row: (Option<f64>,) =
            sqlx::query_as("SELECT SUM(spend) FROM spend_logs WHERE user = ?")
                .bind(user_id)
                .fetch_one(self)
                .await?;
        Ok(row.0.unwrap_or(0.0))
    }

    async fn get_spend_by_tag(&self, tag: &str) -> Result<f64> {
        let pattern = format!("%{}%", tag);
        let row: (Option<f64>,) =
            sqlx::query_as("SELECT SUM(spend) FROM spend_logs WHERE request_tags LIKE ?")
                .bind(&pattern)
                .fetch_one(self)
                .await?;
        Ok(row.0.unwrap_or(0.0))
    }

    async fn get_global_spend(&self) -> Result<f64> {
        let row: (Option<f64>,) = sqlx::query_as("SELECT SUM(spend) FROM spend_logs")
            .fetch_one(self)
            .await?;
        Ok(row.0.unwrap_or(0.0))
    }

    async fn aggregate_spend_by_model(&self, api_key: Option<&str>) -> Result<Vec<SpendModelAgg>> {
        // MySQL aggregate — same pattern as SqlitePool but using ? for bind
        let (sql, bind_key): (&str, Option<&str>) = match api_key {
            Some(_) => ("SELECT model, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE api_key = ? GROUP BY model ORDER BY total_tokens DESC", api_key),
            None => ("SELECT model, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs GROUP BY model ORDER BY total_tokens DESC", None),
        };
        let mut q = sqlx::query_as(sql);
        if let Some(key) = bind_key { q = q.bind(key); }
        q.fetch_all(self).await.map_err(DbError::from)
    }

    async fn aggregate_spend_by_provider(&self) -> Result<Vec<SpendProviderAgg>> {
        // MySQL: use JSON_EXTRACT for litellm_params
        let rows: Vec<(String, i64, f64, i64)> = sqlx::query_as(
            r#"SELECT COALESCE(JSON_UNQUOTE(JSON_EXTRACT(pm.litellm_params, '$.model')), sl.model) as provider,
               COALESCE(SUM(sl.total_tokens), 0) as total_tokens,
               COALESCE(SUM(sl.spend), 0) as total_spend,
               COUNT(sl.request_id) as requests
               FROM spend_logs sl
               LEFT JOIN proxy_models pm ON sl.model = pm.model_name
               GROUP BY provider
               ORDER BY total_tokens DESC"#
        )
        .fetch_all(self)
        .await
        .map_err(DbError::from)?;
        Ok(rows.into_iter().map(|(provider, total_tokens, total_spend, requests)| {
            SpendProviderAgg { provider, total_tokens, total_spend, requests }
        }).collect())
    }

    async fn query_spend_logs_filtered(
        &self, api_key: Option<&str>, model: Option<&str>, _provider: Option<&str>,
        start_date: Option<&str>, end_date: Option<&str>, limit: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        // Fallback: use basic query and do in-memory filter
        let result = self.query_spend_logs(api_key, Some(limit.unwrap_or(100))).await?;
        let filtered: Vec<SpendLog> = result.into_iter().filter(|log| {
            if let Some(m) = model { if log.model != m { return false; } }
            if let Some(sd) = start_date {
                if let Ok(d) = chrono::NaiveDate::parse_from_str(sd, "%Y-%m-%d") {
                    let dt = d.and_hms_opt(0, 0, 0).map(|t| chrono::DateTime::<chrono::Utc>::from_utc(t, chrono::Utc));
                    if let Some(dt) = dt { if log.start_time < dt { return false; } }
                }
            }
            if let Some(ed) = end_date {
                if let Ok(d) = chrono::NaiveDate::parse_from_str(ed, "%Y-%m-%d") {
                    let dt = d.and_hms_opt(23, 59, 59).map(|t| chrono::DateTime::<chrono::Utc>::from_utc(t, chrono::Utc));
                    if let Some(dt) = dt { if log.start_time > dt { return false; } }
                }
            }
            true
        }).collect();
        Ok(filtered)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SpendLogStore implementation for PgPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl SpendLogStore for PgPool {
    async fn insert_spend_log(&self, log: &SpendLog) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO spend_logs (request_id, call_type, api_key, spend, total_tokens,
            prompt_tokens, completion_tokens, start_time, end_time,
            request_duration_ms, completion_start_time, model, model_id, model_group,
            custom_llm_provider, api_base, "user", metadata,
            cache_hit, cache_key, request_tags, team_id, organization_id,
            end_user, requester_ip_address, messages, response,
            session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32)"#
        )
            .bind(&log.request_id).bind(&log.call_type).bind(&log.api_key)
            .bind(log.spend).bind(log.total_tokens).bind(log.prompt_tokens)
            .bind(log.completion_tokens).bind(log.start_time).bind(log.end_time)
            .bind(log.request_duration_ms).bind(log.completion_start_time)
            .bind(&log.model).bind(&log.model_id).bind(&log.model_group)
            .bind(&log.custom_llm_provider).bind(&log.api_base).bind(&log.user)
            .bind(&log.metadata).bind(&log.cache_hit).bind(&log.cache_key)
            .bind(&log.request_tags).bind(&log.team_id).bind(&log.organization_id)
            .bind(&log.end_user).bind(&log.requester_ip_address)
            .bind(&log.messages).bind(&log.response).bind(&log.session_id)
            .bind(&log.status).bind(&log.mcp_namespaced_tool_name).bind(&log.agent_id)
            .bind(&log.proxy_server_request)
            .execute(self).await?;
        Ok(())
    }

    async fn query_spend_logs(
        &self,
        api_key: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        let limit_val = limit.unwrap_or(100);
        let sql = r#"SELECT request_id, call_type, api_key, spend, total_tokens,
            prompt_tokens, completion_tokens, start_time, end_time,
            request_duration_ms, completion_start_time, model, model_id, model_group,
            custom_llm_provider, api_base, "user", metadata,
            cache_hit, cache_key, request_tags, team_id, organization_id,
            end_user, requester_ip_address, messages, response,
            session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request
            FROM spend_logs"#;
        match api_key {
            Some(key) => sqlx::query_as(&format!(
                "{} WHERE api_key = $1 ORDER BY start_time DESC LIMIT {}",
                sql, limit_val
            ))
            .bind(key)
            .fetch_all(self)
            .await
            .map_err(DbError::from),
            None => sqlx::query_as(&format!(
                "{} ORDER BY start_time DESC LIMIT {}",
                sql, limit_val
            ))
            .fetch_all(self)
            .await
            .map_err(DbError::from),
        }
    }

    async fn get_spend_by_key(&self, api_key: &str) -> Result<f64> {
        let row: (Option<f64>,) =
            sqlx::query_as("SELECT SUM(spend) FROM spend_logs WHERE api_key = $1")
                .bind(api_key)
                .fetch_one(self)
                .await?;
        Ok(row.0.unwrap_or(0.0))
    }

    async fn get_spend_by_user(&self, user_id: &str) -> Result<f64> {
        let row: (Option<f64>,) =
            sqlx::query_as(r#"SELECT SUM(spend) FROM spend_logs WHERE "user" = $1"#)
                .bind(user_id)
                .fetch_one(self)
                .await?;
        Ok(row.0.unwrap_or(0.0))
    }

    async fn get_spend_by_tag(&self, tag: &str) -> Result<f64> {
        let pattern = format!("%{}%", tag);
        let row: (Option<f64>,) =
            sqlx::query_as("SELECT SUM(spend) FROM spend_logs WHERE request_tags::text LIKE $1")
                .bind(&pattern)
                .fetch_one(self)
                .await?;
        Ok(row.0.unwrap_or(0.0))
    }

    async fn get_global_spend(&self) -> Result<f64> {
        let row: (Option<f64>,) = sqlx::query_as("SELECT SUM(spend) FROM spend_logs")
            .fetch_one(self)
            .await?;
        Ok(row.0.unwrap_or(0.0))
    }

    async fn aggregate_spend_by_model(&self, api_key: Option<&str>) -> Result<Vec<SpendModelAgg>> {
        let (sql, bind_key): (&str, Option<&str>) = match api_key {
            Some(_) => ("SELECT model, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE api_key = $1 GROUP BY model ORDER BY total_tokens DESC", api_key),
            None => ("SELECT model, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs GROUP BY model ORDER BY total_tokens DESC", None),
        };
        let mut q = sqlx::query_as(sql);
        if let Some(key) = bind_key { q = q.bind(key); }
        q.fetch_all(self).await.map_err(DbError::from)
    }

    async fn aggregate_spend_by_provider(&self) -> Result<Vec<SpendProviderAgg>> {
        let rows: Vec<(String, i64, f64, i64)> = sqlx::query_as(
            r#"SELECT COALESCE(pm.litellm_params->>'model', sl.model) as provider,
               COALESCE(SUM(sl.total_tokens), 0) as total_tokens,
               COALESCE(SUM(sl.spend), 0) as total_spend,
               COUNT(sl.request_id) as requests
               FROM spend_logs sl
               LEFT JOIN proxy_models pm ON sl.model = pm.model_name
               GROUP BY provider
               ORDER BY total_tokens DESC"#
        )
        .fetch_all(self)
        .await
        .map_err(DbError::from)?;
        Ok(rows.into_iter().map(|(provider, total_tokens, total_spend, requests)| {
            SpendProviderAgg { provider, total_tokens, total_spend, requests }
        }).collect())
    }

    async fn query_spend_logs_filtered(
        &self, api_key: Option<&str>, model: Option<&str>, _provider: Option<&str>,
        start_date: Option<&str>, end_date: Option<&str>, limit: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        let result = self.query_spend_logs(api_key, Some(limit.unwrap_or(100))).await?;
        let filtered: Vec<SpendLog> = result.into_iter().filter(|log| {
            if let Some(m) = model { if log.model != m { return false; } }
            if let Some(sd) = start_date {
                if let Ok(d) = chrono::NaiveDate::parse_from_str(sd, "%Y-%m-%d") {
                    let dt = d.and_hms_opt(0, 0, 0).map(|t| chrono::DateTime::<chrono::Utc>::from_utc(t, chrono::Utc));
                    if let Some(dt) = dt { if log.start_time < dt { return false; } }
                }
            }
            if let Some(ed) = end_date {
                if let Ok(d) = chrono::NaiveDate::parse_from_str(ed, "%Y-%m-%d") {
                    let dt = d.and_hms_opt(23, 59, 59).map(|t| chrono::DateTime::<chrono::Utc>::from_utc(t, chrono::Utc));
                    if let Some(dt) = dt { if log.start_time > dt { return false; } }
                }
            }
            true
        }).collect();
        Ok(filtered)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Database enum spend log dispatch
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl Database {
    pub async fn insert_spend_log(&self, log: &SpendLog) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.insert_spend_log(log).await,
            Database::Mysql(pool) => pool.insert_spend_log(log).await,
            Database::Postgres(pool) => pool.insert_spend_log(log).await,
        }
    }

    pub async fn query_spend_logs(
        &self,
        api_key: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        match self {
            Database::Sqlite(pool) => pool.query_spend_logs(api_key, limit).await,
            Database::Mysql(pool) => pool.query_spend_logs(api_key, limit).await,
            Database::Postgres(pool) => pool.query_spend_logs(api_key, limit).await,
        }
    }

    pub async fn get_spend_by_key(&self, api_key: &str) -> Result<f64> {
        match self {
            Database::Sqlite(pool) => pool.get_spend_by_key(api_key).await,
            Database::Mysql(pool) => pool.get_spend_by_key(api_key).await,
            Database::Postgres(pool) => pool.get_spend_by_key(api_key).await,
        }
    }

    pub async fn get_spend_by_user(&self, user_id: &str) -> Result<f64> {
        match self {
            Database::Sqlite(pool) => pool.get_spend_by_user(user_id).await,
            Database::Mysql(pool) => pool.get_spend_by_user(user_id).await,
            Database::Postgres(pool) => pool.get_spend_by_user(user_id).await,
        }
    }

    pub async fn get_spend_by_tag(&self, tag: &str) -> Result<f64> {
        match self {
            Database::Sqlite(pool) => pool.get_spend_by_tag(tag).await,
            Database::Mysql(pool) => pool.get_spend_by_tag(tag).await,
            Database::Postgres(pool) => pool.get_spend_by_tag(tag).await,
        }
    }

    pub async fn get_global_spend(&self) -> Result<f64> {
        match self {
            Database::Sqlite(pool) => pool.get_global_spend().await,
            Database::Mysql(pool) => pool.get_global_spend().await,
            Database::Postgres(pool) => pool.get_global_spend().await,
        }
    }

    pub async fn aggregate_spend_by_model(
        &self,
        api_key: Option<&str>,
    ) -> Result<Vec<SpendModelAgg>> {
        match self {
            Database::Sqlite(pool) => pool.aggregate_spend_by_model(api_key).await,
            Database::Mysql(pool) => pool.aggregate_spend_by_model(api_key).await,
            Database::Postgres(pool) => pool.aggregate_spend_by_model(api_key).await,
        }
    }

    pub async fn aggregate_spend_by_provider(&self) -> Result<Vec<SpendProviderAgg>> {
        match self {
            Database::Sqlite(pool) => pool.aggregate_spend_by_provider().await,
            Database::Mysql(pool) => pool.aggregate_spend_by_provider().await,
            Database::Postgres(pool) => pool.aggregate_spend_by_provider().await,
        }
    }

    pub async fn query_spend_logs_filtered(
        &self,
        api_key: Option<&str>,
        model: Option<&str>,
        provider: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        match self {
            Database::Sqlite(pool) => {
                pool.query_spend_logs_filtered(api_key, model, provider, start_date, end_date, limit)
                    .await
            }
            Database::Mysql(pool) => {
                pool.query_spend_logs_filtered(api_key, model, provider, start_date, end_date, limit)
                    .await
            }
            Database::Postgres(pool) => {
                pool.query_spend_logs_filtered(api_key, model, provider, start_date, end_date, limit)
                    .await
            }
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// ProxyModelStore trait — proxy_models CRUD
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
pub trait ProxyModelStore {
    async fn insert_model(&self, m: &ProxyModel) -> Result<()>;
    async fn get_model_by_id(&self, model_id: &str) -> Result<Option<ProxyModel>>;
    async fn list_models(&self) -> Result<Vec<ProxyModel>>;
    async fn update_model(&self, m: &ProxyModel) -> Result<()>;
    async fn delete_model(&self, model_id: &str) -> Result<()>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// ProxyModelStore implementation for SqlitePool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const INSERT_MODEL_SQLITE: &str = r#"
INSERT INTO proxy_models (model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by)
VALUES (?, ?, ?, ?, ?, ?, ?, ?)
"#;

const GET_MODEL_SQLITE: &str = r#"
SELECT model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by
FROM proxy_models WHERE model_id = ?
"#;

const LIST_MODELS_SQLITE: &str = r#"
SELECT model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by
FROM proxy_models ORDER BY model_name
"#;

const UPDATE_MODEL_SQLITE: &str = r#"
UPDATE proxy_models SET model_name = ?, litellm_params = ?, model_info = ?, updated_at = ?, updated_by = ?
WHERE model_id = ?
"#;

#[async_trait]
impl ProxyModelStore for SqlitePool {
    async fn insert_model(&self, m: &ProxyModel) -> Result<()> {
        sqlx::query(INSERT_MODEL_SQLITE)
            .bind(&m.model_id)
            .bind(&m.model_name)
            .bind(&m.litellm_params)
            .bind(&m.model_info)
            .bind(&m.created_at)
            .bind(&m.created_by)
            .bind(&m.updated_at)
            .bind(&m.updated_by)
            .execute(self).await?;
        Ok(())
    }

    async fn get_model_by_id(&self, model_id: &str) -> Result<Option<ProxyModel>> {
        sqlx::query_as(GET_MODEL_SQLITE)
            .bind(model_id)
            .fetch_optional(self).await
            .map_err(DbError::from)
    }

    async fn list_models(&self) -> Result<Vec<ProxyModel>> {
        sqlx::query_as(LIST_MODELS_SQLITE)
            .fetch_all(self).await
            .map_err(DbError::from)
    }

    async fn update_model(&self, m: &ProxyModel) -> Result<()> {
        sqlx::query(UPDATE_MODEL_SQLITE)
            .bind(&m.model_name)
            .bind(&m.litellm_params)
            .bind(&m.model_info)
            .bind(&m.updated_at)
            .bind(&m.updated_by)
            .bind(&m.model_id)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_model(&self, model_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM proxy_models WHERE model_id = ?")
            .bind(model_id)
            .execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// ProxyModelStore implementation for MySqlPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl ProxyModelStore for MySqlPool {
    async fn insert_model(&self, m: &ProxyModel) -> Result<()> {
        sqlx::query(
            "INSERT INTO proxy_models (model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&m.model_id).bind(&m.model_name).bind(&m.litellm_params).bind(&m.model_info)
        .bind(&m.created_at).bind(&m.created_by).bind(&m.updated_at).bind(&m.updated_by)
        .execute(self).await?;
        Ok(())
    }

    async fn get_model_by_id(&self, model_id: &str) -> Result<Option<ProxyModel>> {
        sqlx::query_as("SELECT model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by FROM proxy_models WHERE model_id = ?")
            .bind(model_id).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_models(&self) -> Result<Vec<ProxyModel>> {
        sqlx::query_as("SELECT model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by FROM proxy_models ORDER BY model_name")
            .fetch_all(self).await.map_err(DbError::from)
    }

    async fn update_model(&self, m: &ProxyModel) -> Result<()> {
        sqlx::query("UPDATE proxy_models SET model_name = ?, litellm_params = ?, model_info = ?, updated_at = ?, updated_by = ? WHERE model_id = ?")
            .bind(&m.model_name).bind(&m.litellm_params).bind(&m.model_info)
            .bind(&m.updated_at).bind(&m.updated_by).bind(&m.model_id)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_model(&self, model_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM proxy_models WHERE model_id = ?")
            .bind(model_id).execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// ProxyModelStore implementation for PgPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl ProxyModelStore for PgPool {
    async fn insert_model(&self, m: &ProxyModel) -> Result<()> {
        sqlx::query(
            "INSERT INTO proxy_models (model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)"
        )
        .bind(&m.model_id).bind(&m.model_name).bind(&m.litellm_params).bind(&m.model_info)
        .bind(&m.created_at).bind(&m.created_by).bind(&m.updated_at).bind(&m.updated_by)
        .execute(self).await?;
        Ok(())
    }

    async fn get_model_by_id(&self, model_id: &str) -> Result<Option<ProxyModel>> {
        sqlx::query_as("SELECT model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by FROM proxy_models WHERE model_id = $1")
            .bind(model_id).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_models(&self) -> Result<Vec<ProxyModel>> {
        sqlx::query_as("SELECT model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by FROM proxy_models ORDER BY model_name")
            .fetch_all(self).await.map_err(DbError::from)
    }

    async fn update_model(&self, m: &ProxyModel) -> Result<()> {
        sqlx::query("UPDATE proxy_models SET model_name = $1, litellm_params = $2, model_info = $3, updated_at = $4, updated_by = $5 WHERE model_id = $6")
            .bind(&m.model_name).bind(&m.litellm_params).bind(&m.model_info)
            .bind(&m.updated_at).bind(&m.updated_by).bind(&m.model_id)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_model(&self, model_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM proxy_models WHERE model_id = $1")
            .bind(model_id).execute(self).await?;
        Ok(())
    }
}

impl Database {
    pub async fn insert_model(&self, m: &ProxyModel) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.insert_model(m).await,
            Database::Mysql(pool) => pool.insert_model(m).await,
            Database::Postgres(pool) => pool.insert_model(m).await,
        }
    }

    pub async fn get_model_by_id(&self, model_id: &str) -> Result<Option<ProxyModel>> {
        match self {
            Database::Sqlite(pool) => pool.get_model_by_id(model_id).await,
            Database::Mysql(pool) => pool.get_model_by_id(model_id).await,
            Database::Postgres(pool) => pool.get_model_by_id(model_id).await,
        }
    }

    pub async fn list_models(&self) -> Result<Vec<ProxyModel>> {
        match self {
            Database::Sqlite(pool) => pool.list_models().await,
            Database::Mysql(pool) => pool.list_models().await,
            Database::Postgres(pool) => pool.list_models().await,
        }
    }

    pub async fn update_model(&self, m: &ProxyModel) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.update_model(m).await,
            Database::Mysql(pool) => pool.update_model(m).await,
            Database::Postgres(pool) => pool.update_model(m).await,
        }
    }

    pub async fn delete_model(&self, model_id: &str) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.delete_model(model_id).await,
            Database::Mysql(pool) => pool.delete_model(model_id).await,
            Database::Postgres(pool) => pool.delete_model(model_id).await,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// CredentialsStore trait — credential CRUD across all DB backends
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Trait for credential operations across all DB backends.
#[async_trait]
pub trait CredentialsStore {
    async fn insert_credential(&self, c: &Credential) -> Result<()>;
    async fn get_credential_by_name(&self, name: &str) -> Result<Option<Credential>>;
    async fn list_credentials(&self) -> Result<Vec<Credential>>;
    async fn update_credential(&self, c: &Credential) -> Result<()>;
    async fn delete_credential(&self, name: &str) -> Result<()>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// CredentialsStore implementation for SqlitePool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const INSERT_CREDENTIAL_SQLITE: &str = r#"
INSERT INTO credentials (credential_id, credential_name, credential_values, credential_info, created_at, created_by, updated_at, updated_by)
VALUES (?, ?, ?, ?, ?, ?, ?, ?)
"#;

const GET_CREDENTIAL_SQLITE: &str = r#"
SELECT credential_id, credential_name, credential_values, credential_info, created_at, created_by, updated_at, updated_by
FROM credentials WHERE credential_name = ?
"#;

const LIST_CREDENTIALS_SQLITE: &str = r#"
SELECT credential_id, credential_name, credential_values, credential_info, created_at, created_by, updated_at, updated_by
FROM credentials ORDER BY credential_name
"#;

const UPDATE_CREDENTIAL_SQLITE: &str = r#"
UPDATE credentials SET credential_values = ?, credential_info = ?, updated_at = ?, updated_by = ?
WHERE credential_name = ?
"#;

#[async_trait]
impl CredentialsStore for SqlitePool {
    async fn insert_credential(&self, c: &Credential) -> Result<()> {
        sqlx::query(INSERT_CREDENTIAL_SQLITE)
            .bind(&c.credential_id)
            .bind(&c.credential_name)
            .bind(&c.credential_values)
            .bind(&c.credential_info)
            .bind(&c.created_at)
            .bind(&c.created_by)
            .bind(&c.updated_at)
            .bind(&c.updated_by)
            .execute(self)
            .await?;
        Ok(())
    }

    async fn get_credential_by_name(&self, name: &str) -> Result<Option<Credential>> {
        sqlx::query_as(GET_CREDENTIAL_SQLITE)
            .bind(name)
            .fetch_optional(self)
            .await
            .map_err(DbError::from)
    }

    async fn list_credentials(&self) -> Result<Vec<Credential>> {
        sqlx::query_as(LIST_CREDENTIALS_SQLITE)
            .fetch_all(self)
            .await
            .map_err(DbError::from)
    }

    async fn update_credential(&self, c: &Credential) -> Result<()> {
        sqlx::query(UPDATE_CREDENTIAL_SQLITE)
            .bind(&c.credential_values)
            .bind(&c.credential_info)
            .bind(&c.updated_at)
            .bind(&c.updated_by)
            .bind(&c.credential_name)
            .execute(self)
            .await?;
        Ok(())
    }

    async fn delete_credential(&self, name: &str) -> Result<()> {
        sqlx::query("DELETE FROM credentials WHERE credential_name = ?")
            .bind(name)
            .execute(self)
            .await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// CredentialsStore implementation for MySqlPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl CredentialsStore for MySqlPool {
    async fn insert_credential(&self, c: &Credential) -> Result<()> {
        sqlx::query(
            "INSERT INTO credentials (credential_id, credential_name, credential_values, credential_info, created_at, created_by, updated_at, updated_by) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&c.credential_id).bind(&c.credential_name).bind(&c.credential_values).bind(&c.credential_info)
        .bind(&c.created_at).bind(&c.created_by).bind(&c.updated_at).bind(&c.updated_by)
        .execute(self).await?;
        Ok(())
    }

    async fn get_credential_by_name(&self, name: &str) -> Result<Option<Credential>> {
        sqlx::query_as("SELECT credential_id, credential_name, credential_values, credential_info, created_at, created_by, updated_at, updated_by FROM credentials WHERE credential_name = ?")
            .bind(name).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_credentials(&self) -> Result<Vec<Credential>> {
        sqlx::query_as("SELECT credential_id, credential_name, credential_values, credential_info, created_at, created_by, updated_at, updated_by FROM credentials ORDER BY credential_name")
            .fetch_all(self).await.map_err(DbError::from)
    }

    async fn update_credential(&self, c: &Credential) -> Result<()> {
        sqlx::query("UPDATE credentials SET credential_values = ?, credential_info = ?, updated_at = ?, updated_by = ? WHERE credential_name = ?")
            .bind(&c.credential_values).bind(&c.credential_info).bind(&c.updated_at).bind(&c.updated_by).bind(&c.credential_name)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_credential(&self, name: &str) -> Result<()> {
        sqlx::query("DELETE FROM credentials WHERE credential_name = ?")
            .bind(name).execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// CredentialsStore implementation for PgPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl CredentialsStore for PgPool {
    async fn insert_credential(&self, c: &Credential) -> Result<()> {
        sqlx::query(
            "INSERT INTO credentials (credential_id, credential_name, credential_values, credential_info, created_at, created_by, updated_at, updated_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)"
        )
        .bind(&c.credential_id).bind(&c.credential_name).bind(&c.credential_values).bind(&c.credential_info)
        .bind(&c.created_at).bind(&c.created_by).bind(&c.updated_at).bind(&c.updated_by)
        .execute(self).await?;
        Ok(())
    }

    async fn get_credential_by_name(&self, name: &str) -> Result<Option<Credential>> {
        sqlx::query_as("SELECT credential_id, credential_name, credential_values, credential_info, created_at, created_by, updated_at, updated_by FROM credentials WHERE credential_name = $1")
            .bind(name).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_credentials(&self) -> Result<Vec<Credential>> {
        sqlx::query_as("SELECT credential_id, credential_name, credential_values, credential_info, created_at, created_by, updated_at, updated_by FROM credentials ORDER BY credential_name")
            .fetch_all(self).await.map_err(DbError::from)
    }

    async fn update_credential(&self, c: &Credential) -> Result<()> {
        sqlx::query("UPDATE credentials SET credential_values = $1, credential_info = $2, updated_at = $3, updated_by = $4 WHERE credential_name = $5")
            .bind(&c.credential_values).bind(&c.credential_info).bind(&c.updated_at).bind(&c.updated_by).bind(&c.credential_name)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_credential(&self, name: &str) -> Result<()> {
        sqlx::query("DELETE FROM credentials WHERE credential_name = $1")
            .bind(name).execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Database enum credential dispatch
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl Database {
    pub async fn insert_credential(&self, c: &Credential) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.insert_credential(c).await,
            Database::Mysql(pool) => pool.insert_credential(c).await,
            Database::Postgres(pool) => pool.insert_credential(c).await,
        }
    }

    pub async fn get_credential_by_name(&self, name: &str) -> Result<Option<Credential>> {
        match self {
            Database::Sqlite(pool) => pool.get_credential_by_name(name).await,
            Database::Mysql(pool) => pool.get_credential_by_name(name).await,
            Database::Postgres(pool) => pool.get_credential_by_name(name).await,
        }
    }

    pub async fn list_credentials(&self) -> Result<Vec<Credential>> {
        match self {
            Database::Sqlite(pool) => pool.list_credentials().await,
            Database::Mysql(pool) => pool.list_credentials().await,
            Database::Postgres(pool) => pool.list_credentials().await,
        }
    }

    pub async fn update_credential(&self, c: &Credential) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.update_credential(c).await,
            Database::Mysql(pool) => pool.update_credential(c).await,
            Database::Postgres(pool) => pool.update_credential(c).await,
        }
    }

    pub async fn delete_credential(&self, name: &str) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.delete_credential(name).await,
            Database::Mysql(pool) => pool.delete_credential(name).await,
            Database::Postgres(pool) => pool.delete_credential(name).await,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Unit tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::hash_token;

    /// All 14 tables defined in the migrations (aigw names)
    const ALL_TABLES: &[&str] = &[
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
        "proxy_models",
        "config",
        "credentials",
    ];

    fn make_test_key(token_hash: &str, key_alias: &str) -> VirtualKey {
        VirtualKey {
            token: token_hash.to_string(),
            key_name: Some(key_alias.to_string()),
            key_alias: Some(key_alias.to_string()),
            soft_budget_cooldown: false,
            spend: 0.0,
            expires: None,
            models: serde_json::json!([]),
            aliases: serde_json::json!({}),
            config: serde_json::json!({}),
            router_settings: None,
            user_id: Some("user-1".to_string()),
            team_id: Some("team-1".to_string()),
            agent_id: None,
            project_id: None,
            permissions: serde_json::json!({}),
            max_parallel_requests: None,
            metadata: serde_json::json!({}),
            blocked: None,
            tpm_limit: Some(1000),
            rpm_limit: Some(100),
            max_budget: Some(100.0),
            budget_duration: None,
            budget_reset_at: None,
            allowed_cache_controls: serde_json::json!([]),
            allowed_routes: serde_json::json!([]),
            policies: serde_json::json!([]),
            access_group_ids: serde_json::json!([]),
            model_spend: serde_json::json!({}),
            model_max_budget: serde_json::json!({}),
            budget_id: None,
            organization_id: None,
            object_permission_id: None,
            created_at: Some(Utc::now()),
            created_by: None,
            updated_at: Some(Utc::now()),
            updated_by: None,
            last_active: None,
            rotation_count: None,
            auto_rotate: None,
            rotation_interval: None,
            last_rotation_at: None,
            key_rotation_at: None,
            budget_limits: None,
        }
    }

    #[tokio::test]
    async fn test_database_init_sqlite() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        match db {
            Database::Sqlite(pool) => {
                let row: (String,) = sqlx::query_as(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name='virtual_keys'",
                )
                .fetch_one(&pool)
                .await
                .expect("virtual_keys table exists");
                assert_eq!(row.0, "virtual_keys");
            }
            _ => panic!("expected SQLite"),
        }
    }

    #[tokio::test]
    async fn test_all_14_tables_exist_after_migration() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        match db {
            Database::Sqlite(pool) => {
                for table_name in ALL_TABLES {
                    let row: (String,) = sqlx::query_as(
                        "SELECT name FROM sqlite_master WHERE type='table' AND name = ?",
                    )
                    .bind(table_name)
                    .fetch_one(&pool)
                    .await
                    .unwrap_or_else(|_| {
                        panic!("table '{}' should exist after migration", table_name)
                    });
                    assert_eq!(&row.0, table_name);
                }
            }
            _ => panic!("expected SQLite"),
        }
    }

    #[tokio::test]
    async fn test_migration_idempotency() {
        let _db = Database::init("sqlite::memory:").await.expect("first init");
        let result = Database::init("sqlite::memory:").await;
        assert!(result.is_ok(), "second init should succeed");
    }

    #[tokio::test]
    async fn test_invalid_url() {
        let result = Database::init("unknown://localhost/db").await;
        assert!(result.is_err());
        match result {
            Err(DbError::InvalidUrl(url)) => {
                assert_eq!(url, "unknown://localhost/db");
            }
            _ => panic!("expected InvalidUrl error"),
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // KeyStore CRUD tests (SQLite in-memory)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[tokio::test]
    async fn test_insert_and_get_key() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw_token = "sk-test-insert-key-12345";
        let hash = hash_token(raw_token);
        let key = make_test_key(&hash, "test-key");

        db.insert_key(&key).await.expect("insert should succeed");

        let retrieved = db
            .get_key_by_token(&hash)
            .await
            .expect("get should succeed");
        assert!(retrieved.is_some());
        let k = retrieved.unwrap();
        assert_eq!(k.key_alias.as_deref(), Some("test-key"));
        assert_eq!(k.tpm_limit, Some(1000));
        assert_eq!(k.spend, 0.0);
    }

    #[tokio::test]
    async fn test_get_nonexistent_key() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let result = db
            .get_key_by_token("nonexistent-hash")
            .await
            .expect("get should succeed");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_blocked_key_returns_none() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw_token = "sk-blocked-key";
        let hash = hash_token(raw_token);
        let mut key = make_test_key(&hash, "blocked-key");
        key.blocked = Some(true);

        db.insert_key(&key).await.expect("insert should succeed");
        let result = db
            .get_key_by_token(&hash)
            .await
            .expect("get should succeed");
        assert!(result.is_none(), "blocked key should return None");
    }

    #[tokio::test]
    async fn test_get_expired_key_returns_none() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw_token = "sk-expired-key";
        let hash = hash_token(raw_token);
        let mut key = make_test_key(&hash, "expired-key");
        // Set expiry in the past
        key.expires = Some(Utc::now() - chrono::Duration::hours(1));

        db.insert_key(&key).await.expect("insert should succeed");
        let result = db
            .get_key_by_token(&hash)
            .await
            .expect("get should succeed");
        assert!(result.is_none(), "expired key should return None");
    }

    #[tokio::test]
    async fn test_list_keys_all() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let k1 = make_test_key(&hash_token("token-1"), "key-1");
        let k2 = make_test_key(&hash_token("token-2"), "key-2");
        db.insert_key(&k1).await.expect("insert k1");
        db.insert_key(&k2).await.expect("insert k2");

        let all = db.list_keys(None, None).await.expect("list should succeed");
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_list_keys_by_team() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let mut k1 = make_test_key(&hash_token("t1-team-a"), "k1");
        k1.team_id = Some("team-a".to_string());
        k1.user_id = Some("user-1".to_string());
        let mut k2 = make_test_key(&hash_token("t2-team-b"), "k2");
        k2.team_id = Some("team-b".to_string());
        k2.user_id = Some("user-2".to_string());
        db.insert_key(&k1).await.expect("insert k1");
        db.insert_key(&k2).await.expect("insert k2");

        let team_a = db
            .list_keys(Some("team-a"), None)
            .await
            .expect("list team-a");
        assert_eq!(team_a.len(), 1);
        assert_eq!(team_a[0].key_alias.as_deref(), Some("k1"));
    }

    #[tokio::test]
    async fn test_list_keys_by_user() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let mut k1 = make_test_key(&hash_token("u1-key"), "u1");
        k1.team_id = Some("team-a".to_string());
        k1.user_id = Some("user-1".to_string());
        let mut k2 = make_test_key(&hash_token("u2-key"), "u2");
        k2.team_id = Some("team-b".to_string());
        k2.user_id = Some("user-2".to_string());
        db.insert_key(&k1).await.expect("insert k1");
        db.insert_key(&k2).await.expect("insert k2");

        let user2 = db
            .list_keys(None, Some("user-2"))
            .await
            .expect("list user-2");
        assert_eq!(user2.len(), 1);
        assert_eq!(user2[0].key_alias.as_deref(), Some("u2"));
    }

    #[tokio::test]
    async fn test_update_key() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let hash = hash_token("sk-update-test");
        let key = make_test_key(&hash, "update-me");
        db.insert_key(&key).await.expect("insert");

        // Create updated key
        let mut updated = key.clone();
        updated.key_alias = Some("updated-alias".to_string());
        updated.spend = 42.0;
        updated.tpm_limit = Some(5000);
        updated.blocked = None;

        db.update_key(&hash, &updated)
            .await
            .expect("update should succeed");

        // Re-fetch and verify
        let retrieved = db.get_key_by_token(&hash).await.expect("get after update");
        assert!(retrieved.is_some());
        let k = retrieved.unwrap();
        assert_eq!(k.key_alias.as_deref(), Some("updated-alias"));
        assert_eq!(k.spend, 42.0);
        assert_eq!(k.tpm_limit, Some(5000));
        assert_eq!(k.blocked, None);
    }

    #[tokio::test]
    async fn test_delete_key() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let hash = hash_token("sk-delete-test");
        let key = make_test_key(&hash, "delete-me");
        db.insert_key(&key).await.expect("insert");

        // Verify it exists
        assert!(db.get_key_by_token(&hash).await.unwrap().is_some());

        // Delete
        db.delete_key(&hash).await.expect("delete should succeed");

        // Verify it's gone from virtual_keys
        assert!(db.get_key_by_token(&hash).await.unwrap().is_none());

        // Verify it's in deleted_keys (SQLite)
        match &db {
            Database::Sqlite(pool) => {
                let count: (i64,) =
                    sqlx::query_as("SELECT COUNT(*) FROM deleted_keys WHERE token = ?")
                        .bind(&hash)
                        .fetch_one(pool)
                        .await
                        .expect("count deleted");
                assert_eq!(count.0, 1, "key should be in deleted_keys");
            }
            _ => {}
        }
    }

    #[tokio::test]
    async fn test_full_crud_cycle() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-crud-cycle-test";
        let hash = hash_token(raw);

        // Insert
        let key = make_test_key(&hash, "crud-key");
        db.insert_key(&key).await.expect("insert");
        assert!(db.get_key_by_token(&hash).await.unwrap().is_some());

        // Update
        let mut updated = key.clone();
        updated.key_alias = Some("crud-updated".to_string());
        db.update_key(&hash, &updated).await.expect("update");
        let fetched = db.get_key_by_token(&hash).await.unwrap().unwrap();
        assert_eq!(fetched.key_alias.as_deref(), Some("crud-updated"));

        // List (should include this key)
        let all = db.list_keys(None, None).await.unwrap();
        assert!(!all.is_empty());

        // Delete
        db.delete_key(&hash).await.expect("delete");
        assert!(db.get_key_by_token(&hash).await.unwrap().is_none());
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // SpendLog CRUD tests (SQLite in-memory)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    fn make_test_spend_log(
        api_key: &str,
        user: &str,
        spend: f64,
        request_tags: Option<serde_json::Value>,
    ) -> SpendLog {
        let now = Utc::now();
        SpendLog {
            request_id: Uuid::new_v4().to_string(),
            call_type: "completion".to_string(),
            api_key: api_key.to_string(),
            spend,
            total_tokens: 100,
            prompt_tokens: 50,
            completion_tokens: 50,
            start_time: now,
            end_time: now,
            request_duration_ms: Some(500),
            completion_start_time: None,
            model: "gpt-4".to_string(),
            model_id: None,
            model_group: None,
            custom_llm_provider: Some("openai".to_string()),
            api_base: None,
            user: Some(user.to_string()),
            metadata: None,
            cache_hit: None,
            cache_key: None,
            request_tags,
            team_id: None,
            organization_id: None,
            end_user: None,
            requester_ip_address: None,
            messages: None,
            response: None,
            session_id: None,
            status: Some("success".to_string()),
            mcp_namespaced_tool_name: None,
            agent_id: None,
            proxy_server_request: None,
        }
    }

    #[tokio::test]
    async fn test_insert_and_query_spend_log() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let key_hash = hash_token("sk-spend-test");
        let log = make_test_spend_log(&key_hash, "user-1", 0.5, None);

        db.insert_spend_log(&log).await.expect("insert spend log");

        // Query all
        let logs = db.query_spend_logs(None, None).await.expect("query all");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].api_key, key_hash);
        assert_eq!(logs[0].spend, 0.5);
    }

    #[tokio::test]
    async fn test_query_spend_logs_by_key() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let key_a = hash_token("key-a");
        let key_b = hash_token("key-b");

        let log1 = make_test_spend_log(&key_a, "user-1", 1.0, None);
        let log2 = make_test_spend_log(&key_b, "user-2", 2.0, None);

        db.insert_spend_log(&log1).await.expect("insert 1");
        db.insert_spend_log(&log2).await.expect("insert 2");

        let filtered = db
            .query_spend_logs(Some(&key_a), None)
            .await
            .expect("query by key");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].spend, 1.0);
    }

    #[tokio::test]
    async fn test_query_spend_logs_limit() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let key = hash_token("limit-test");

        for i in 0..5 {
            let log = make_test_spend_log(&key, "user-1", i as f64, None);
            db.insert_spend_log(&log).await.expect("insert");
        }

        let limited = db
            .query_spend_logs(None, Some(2))
            .await
            .expect("query limited");
        assert_eq!(limited.len(), 2);
    }

    #[tokio::test]
    async fn test_get_spend_by_key() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let key_a = hash_token("key-spend-a");
        let key_b = hash_token("key-spend-b");

        db.insert_spend_log(&make_test_spend_log(&key_a, "u1", 10.0, None))
            .await
            .expect("insert");
        db.insert_spend_log(&make_test_spend_log(&key_a, "u1", 20.0, None))
            .await
            .expect("insert");
        db.insert_spend_log(&make_test_spend_log(&key_b, "u2", 5.0, None))
            .await
            .expect("insert");

        let spend_a = db.get_spend_by_key(&key_a).await.expect("get spend a");
        assert_eq!(spend_a, 30.0);

        let spend_b = db.get_spend_by_key(&key_b).await.expect("get spend b");
        assert_eq!(spend_b, 5.0);
    }

    #[tokio::test]
    async fn test_get_spend_by_user() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        db.insert_spend_log(&make_test_spend_log("key-1", "alice", 15.0, None))
            .await
            .expect("insert");
        db.insert_spend_log(&make_test_spend_log("key-2", "alice", 25.0, None))
            .await
            .expect("insert");
        db.insert_spend_log(&make_test_spend_log("key-3", "bob", 10.0, None))
            .await
            .expect("insert");

        assert_eq!(db.get_spend_by_user("alice").await.unwrap(), 40.0);
        assert_eq!(db.get_spend_by_user("bob").await.unwrap(), 10.0);
        assert_eq!(db.get_spend_by_user("nobody").await.unwrap(), 0.0);
    }

    #[tokio::test]
    async fn test_get_spend_by_tag() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        let tags_prod = serde_json::json!(["production", "high-priority"]);
        let tags_staging = serde_json::json!(["staging", "low-priority"]);

        db.insert_spend_log(&make_test_spend_log("k1", "u1", 100.0, Some(tags_prod)))
            .await
            .expect("insert");
        db.insert_spend_log(&make_test_spend_log("k2", "u1", 50.0, Some(tags_staging)))
            .await
            .expect("insert");

        assert_eq!(db.get_spend_by_tag("production").await.unwrap(), 100.0);
        assert_eq!(db.get_spend_by_tag("priority").await.unwrap(), 150.0);
        assert_eq!(db.get_spend_by_tag("nonexistent").await.unwrap(), 0.0);
    }

    #[tokio::test]
    async fn test_get_global_spend() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        assert_eq!(db.get_global_spend().await.unwrap(), 0.0);

        db.insert_spend_log(&make_test_spend_log("k1", "u1", 10.0, None))
            .await
            .expect("insert");
        db.insert_spend_log(&make_test_spend_log("k2", "u2", 20.0, None))
            .await
            .expect("insert");
        db.insert_spend_log(&make_test_spend_log("k3", "u3", 30.0, None))
            .await
            .expect("insert");

        assert_eq!(db.get_global_spend().await.unwrap(), 60.0);
    }

    #[tokio::test]
    async fn test_spend_log_full_cycle() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let key = hash_token("spend-cycle-key");

        // Insert 3 logs
        for i in 1..=3 {
            let log = make_test_spend_log(&key, "cycle-user", i as f64 * 10.0, None);
            db.insert_spend_log(&log).await.expect("insert");
        }

        // Query all
        let all = db.query_spend_logs(None, None).await.unwrap();
        assert_eq!(all.len(), 3);

        // Query by key
        let by_key = db.query_spend_logs(Some(&key), None).await.unwrap();
        assert_eq!(by_key.len(), 3);

        // Spend aggregates
        assert_eq!(db.get_spend_by_key(&key).await.unwrap(), 60.0);
        assert_eq!(db.get_spend_by_user("cycle-user").await.unwrap(), 60.0);
        assert_eq!(db.get_global_spend().await.unwrap(), 60.0);
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // CredentialsStore CRUD tests (SQLite in-memory)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    fn make_test_credential(name: &str) -> Credential {
        let now = Utc::now().to_rfc3339();
        Credential {
            credential_id: Uuid::new_v4().to_string(),
            credential_name: name.to_string(),
            credential_values: serde_json::json!({"api_key": "test-key-123"}),
            credential_info: serde_json::json!({"provider": "openai"}),
            created_at: now.clone(),
            created_by: None,
            updated_at: now,
            updated_by: None,
        }
    }

    #[tokio::test]
    async fn test_insert_and_get_credential() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let cred = make_test_credential("openai-key");
        db.insert_credential(&cred).await.expect("insert");

        let retrieved = db.get_credential_by_name("openai-key").await.expect("get");
        assert!(retrieved.is_some());
        let c = retrieved.unwrap();
        assert_eq!(c.credential_name, "openai-key");
        assert_eq!(c.credential_values["api_key"], "test-key-123");
    }

    #[tokio::test]
    async fn test_get_nonexistent_credential() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let result = db.get_credential_by_name("no-such-cred").await.expect("get");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_credentials() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        db.insert_credential(&make_test_credential("cred-a")).await.expect("insert");
        db.insert_credential(&make_test_credential("cred-b")).await.expect("insert");
        db.insert_credential(&make_test_credential("cred-c")).await.expect("insert");

        let all = db.list_credentials().await.expect("list");
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn test_update_credential() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let cred = make_test_credential("update-me");
        db.insert_credential(&cred).await.expect("insert");

        let mut updated = cred.clone();
        updated.credential_values = serde_json::json!({"api_key": "updated-key"});
        updated.credential_info = serde_json::json!({"provider": "updated-provider"});
        updated.updated_at = Utc::now().to_rfc3339();

        db.update_credential(&updated).await.expect("update");

        let fetched = db.get_credential_by_name("update-me").await.expect("get").unwrap();
        assert_eq!(fetched.credential_values["api_key"], "updated-key");
        assert_eq!(fetched.credential_info["provider"], "updated-provider");
    }

    #[tokio::test]
    async fn test_delete_credential() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let cred = make_test_credential("delete-me");
        db.insert_credential(&cred).await.expect("insert");

        assert!(db.get_credential_by_name("delete-me").await.unwrap().is_some());

        db.delete_credential("delete-me").await.expect("delete");

        assert!(db.get_credential_by_name("delete-me").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_credential_full_crud_cycle() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        // Insert
        let cred = make_test_credential("full-cycle");
        db.insert_credential(&cred).await.expect("insert");
        assert!(db.get_credential_by_name("full-cycle").await.unwrap().is_some());

        // Update
        let mut updated = cred.clone();
        updated.credential_values = serde_json::json!({"api_key": "cycled-key"});
        updated.updated_at = Utc::now().to_rfc3339();
        db.update_credential(&updated).await.expect("update");
        let fetched = db.get_credential_by_name("full-cycle").await.unwrap().unwrap();
        assert_eq!(fetched.credential_values["api_key"], "cycled-key");

        // List
        let all = db.list_credentials().await.unwrap();
        assert_eq!(all.len(), 1);

        // Delete
        db.delete_credential("full-cycle").await.expect("delete");
        assert!(db.get_credential_by_name("full-cycle").await.unwrap().is_none());
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // ProxyModel CRUD tests (SQLite in-memory)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    fn make_test_model(name: &str) -> ProxyModel {
        let now = Utc::now().to_rfc3339();
        ProxyModel {
            model_id: Uuid::new_v4().to_string(),
            model_name: name.to_string(),
            litellm_params: serde_json::json!({"model": "gpt-4", "api_base": "https://api.openai.com"}),
            model_info: serde_json::json!({"id": "gpt-4"}),
            created_at: now.clone(),
            created_by: None,
            updated_at: now,
            updated_by: None,
        }
    }

    #[tokio::test]
    async fn test_insert_and_get_model() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let m = make_test_model("test-model");
        db.insert_model(&m).await.expect("insert");

        let retrieved = db.get_model_by_id(&m.model_id).await.expect("get");
        assert!(retrieved.is_some());
        let r = retrieved.unwrap();
        assert_eq!(r.model_name, "test-model");
        assert_eq!(r.litellm_params["model"], "gpt-4");
    }

    #[tokio::test]
    async fn test_list_models() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        db.insert_model(&make_test_model("model-a")).await.expect("insert");
        db.insert_model(&make_test_model("model-b")).await.expect("insert");

        let all = db.list_models().await.expect("list");
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_update_model() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let m = make_test_model("update-model");
        db.insert_model(&m).await.expect("insert");

        let mut updated = m.clone();
        updated.model_name = "updated-model-name".to_string();
        updated.litellm_params = serde_json::json!({"model": "gpt-4-turbo"});
        updated.updated_at = Utc::now().to_rfc3339();
        db.update_model(&updated).await.expect("update");

        let fetched = db.get_model_by_id(&m.model_id).await.expect("get").unwrap();
        assert_eq!(fetched.model_name, "updated-model-name");
        assert_eq!(fetched.litellm_params["model"], "gpt-4-turbo");
    }

    #[tokio::test]
    async fn test_delete_model() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let m = make_test_model("delete-model");
        db.insert_model(&m).await.expect("insert");

        assert!(db.get_model_by_id(&m.model_id).await.unwrap().is_some());

        db.delete_model(&m.model_id).await.expect("delete");

        assert!(db.get_model_by_id(&m.model_id).await.unwrap().is_none());
    }
}
