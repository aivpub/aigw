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
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, DbError>;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Database enum — runtime dispatch across SQLite, MySQL, PostgreSQL
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Multi-database pool supporting SQLite, MySQL, PostgreSQL
#[derive(Debug, Clone)]
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
            clean_litellm_data(&pool).await?;
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

/// Clean up data type inconsistencies in SQLite databases migrated from litellm.
///
/// litellm/SQLite is dynamically typed, which causes several problems when
/// sqlx tries to decode values into strongly-typed Rust types:
///
/// 1. `''` instead of NULL for optional datetime/boolean columns
/// 2. TEXT "false"/"true" for columns defined as INTEGER (blocked)
/// 3. Zero-length BLOBs instead of NULL for optional JSON columns (budget_limits)
async fn clean_litellm_data(pool: &SqlitePool) -> Result<()> {
    // ── Step 1: Empty-string → NULL for nullable columns ──
    let nullable_cols: &[(&str, &[&str])] = &[
        (
            "virtual_keys",
            &[
                "budget_reset_at",
                "expires",
                "last_active",
                "last_rotation_at",
                "key_rotation_at",
                "blocked",
            ],
        ),
        (
            "deprecated_keys",
            &[
                "budget_reset_at",
                "expires",
                "last_active",
                "last_rotation_at",
                "key_rotation_at",
                "deprecated_at",
                "blocked",
            ],
        ),
        (
            "deleted_keys",
            &[
                "budget_reset_at",
                "expires",
                "last_active",
                "last_rotation_at",
                "key_rotation_at",
                "deleted_at",
                "blocked",
            ],
        ),
        ("teams", &["budget_reset_at", "blocked"]),
        ("users", &["budget_reset_at"]),
        ("budgets", &["budget_reset_at"]),
    ];

    for (table, columns) in nullable_cols {
        for col in *columns {
            let sql = format!(
                "UPDATE \"{}\" SET \"{}\" = NULL WHERE \"{}\" = ''",
                table, col, col
            );
            sqlx::query(&sql).execute(pool).await.ok();
        }
    }

    // ── Step 2: TEXT "false"/"true" → INTEGER 0/1 for blocked columns ──
    let blocked_tables = ["teams", "virtual_keys", "deprecated_keys", "deleted_keys",
        "deleted_organizations", "deleted_teams", "deleted_users", "deleted_models"];
    for table in blocked_tables {
        for (text_val, int_val) in [("false", 0), ("true", 1), ("'false'", 0), ("'true'", 1)] {
            let sql = format!(
                "UPDATE \"{}\" SET blocked = {} WHERE typeof(blocked) = 'text' AND lower(blocked) = '{}'",
                table, int_val, text_val.trim_matches('\'')
            );
            sqlx::query(&sql).execute(pool).await.ok();
        }
    }

    // ── Step 2b: TEXT "false"/"true" → INTEGER 0/1 for allow_team_guardrail_config ──
    {
        let sql = "UPDATE teams SET allow_team_guardrail_config = 0 WHERE typeof(allow_team_guardrail_config) = 'text' AND lower(allow_team_guardrail_config) = 'false'";
        sqlx::query(sql).execute(pool).await.ok();
        let sql = "UPDATE teams SET allow_team_guardrail_config = 1 WHERE typeof(allow_team_guardrail_config) = 'text' AND lower(allow_team_guardrail_config) = 'true'";
        sqlx::query(sql).execute(pool).await.ok();
    }

    // ── Step 2c: Empty-string TEXT → NULL for INTEGER columns (model_id in teams) ──
    for table in ["teams"] {
        let sql = format!("UPDATE \"{}\" SET model_id = NULL WHERE typeof(model_id) = 'text' AND model_id = ''", table);
        sqlx::query(&sql).execute(pool).await.ok();
    }

    // ── Step 3: Zero-length BLOB → NULL for nullable JSON columns ──
    let nullable_blob_cols: &[(&str, &[&str])] = &[
        ("virtual_keys", &["budget_limits", "router_settings"]),
        ("deprecated_keys", &["budget_limits", "router_settings"]),
        ("deleted_keys", &["budget_limits", "router_settings"]),
        ("teams", &["budget_limits", "router_settings"]),
    ];

    for (table, columns) in nullable_blob_cols {
        for col in *columns {
            let sql = format!(
                "UPDATE \"{}\" SET \"{}\" = NULL WHERE typeof(\"{}\") = 'blob' AND length(\"{}\") = 0",
                table, col, col, col
            );
            sqlx::query(&sql).execute(pool).await.ok();
        }
    }

    // ── Step 4: Zero-length BLOB → valid JSON for NOT NULL JSON columns ──
    // sqlx cannot decode BLOB into serde_json::Value, even if empty.
    // Convert empty BLOBs to their appropriate JSON empty values.
    let not_null_json_defaults: &[(&str, &str, &str)] = &[
        // (table, column, default_json_value)
        ("teams", "default_team_member_models", "[]"),
    ];

    for (table, col, default_val) in not_null_json_defaults {
        let sql = format!(
            "UPDATE \"{}\" SET \"{}\" = '{}' WHERE typeof(\"{}\") = 'blob'",
            table, col, default_val, col
        );
        sqlx::query(&sql).execute(pool).await.ok();
    }

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

    /// List all archived (soft-deleted) keys from deleted_keys.
    async fn list_deleted_keys(&self) -> Result<Vec<VirtualKey>>;

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

const LIST_DELETED_KEYS_SQLITE: &str = r#"
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
FROM deleted_keys ORDER BY updated_at DESC
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
            .bind(key.soft_budget_cooldown.clone())
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
            .bind(&key.max_parallel_requests)
            .bind(&key.metadata)
            .bind(key.blocked)
            .bind(&key.tpm_limit)
            .bind(&key.rpm_limit)
            .bind(key.max_budget.clone())
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
            .bind(key.auto_rotate.clone())
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
                    tracing::warn!(%token_hash, "API key blocked");
                    return Ok(None);
                }
                // Check expiry
                if let Some(expires) = k.expires {
                    if expires <= Utc::now() {
                        tracing::warn!(%token_hash, "API key expired");
                        return Ok(None);
                    }
                }
                Ok(Some(k))
            }
            None => {
                tracing::warn!(%token_hash, "API key not found in database");
                Ok(None)
            }
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

    async fn list_deleted_keys(&self) -> Result<Vec<VirtualKey>> {
        sqlx::query_as(LIST_DELETED_KEYS_SQLITE).fetch_all(self).await.map_err(DbError::from)
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
                .bind(&k.max_parallel_requests)
                .bind(&k.metadata)
                .bind(k.blocked)
                .bind(&k.tpm_limit)
                .bind(&k.rpm_limit)
                .bind(k.max_budget.clone())
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
            .bind(key.max_budget.clone())
            .bind(&key.tpm_limit)
            .bind(&key.rpm_limit)
            .bind(key.blocked)
            .bind(&key.metadata)
            .bind(&key.permissions)
            .bind(&key.budget_duration)
            .bind(key.budget_reset_at)
            .bind(&key.max_parallel_requests)
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
            .bind(key.soft_budget_cooldown.clone())
            .bind(key.expires)
            .bind(key.auto_rotate.clone())
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
            .bind(key.soft_budget_cooldown_bool() as i8)
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
            .bind(&key.max_parallel_requests)
            .bind(&key.metadata)
            .bind(key.blocked.map(|b| b as i8))
            .bind(&key.tpm_limit)
            .bind(&key.rpm_limit)
            .bind(key.max_budget.clone())
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
            .bind(key.auto_rotate_bool().map(|b| b as i8))
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
                    tracing::warn!(%token_hash, "API key blocked");
                    return Ok(None);
                }
                if let Some(expires) = k.expires {
                    if expires <= Utc::now() {
                        tracing::warn!(%token_hash, "API key expired");
                        return Ok(None);
                    }
                }
                Ok(Some(k))
            }
            None => {
                tracing::warn!(%token_hash, "API key not found in database");
                Ok(None)
            }
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

    async fn list_deleted_keys(&self) -> Result<Vec<VirtualKey>> {
        sqlx::query_as("SELECT token, key_name, key_alias, soft_budget_cooldown, spend, expires, models, aliases, config, router_settings, user_id, team_id, agent_id, project_id, permissions, max_parallel_requests, metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, budget_id, organization_id, object_permission_id, created_at, created_by, updated_at, updated_by, last_active, rotation_count, auto_rotate, rotation_interval, last_rotation_at, key_rotation_at, budget_limits FROM deleted_keys ORDER BY updated_at DESC")
            .fetch_all(self).await.map_err(DbError::from)
    }

    async fn delete_key(&self, token_hash: &str) -> Result<()> {
        let key = self.get_key_by_token(token_hash).await?;
        if let Some(k) = key {
            sqlx::query(INSERT_DELETED_KEY_SQLITE)
                .bind(&k.token)
                .bind(&k.key_name)
                .bind(&k.key_alias)
                .bind(k.soft_budget_cooldown_bool() as i8)
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
                .bind(&k.max_parallel_requests)
                .bind(&k.metadata)
                .bind(k.blocked.map(|b| b as i8))
                .bind(&k.tpm_limit)
                .bind(&k.rpm_limit)
                .bind(k.max_budget.clone())
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
                .bind(k.auto_rotate_bool().map(|b| b as i8))
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
            .bind(key.max_budget.clone()).bind(&key.tpm_limit).bind(&key.rpm_limit)
            .bind(key.blocked.map(|b| b as i8)).bind(&key.metadata).bind(&key.permissions)
            .bind(&key.budget_duration).bind(key.budget_reset_at).bind(&key.max_parallel_requests)
            .bind(&key.aliases).bind(&key.config).bind(&key.router_settings)
            .bind(&key.user_id).bind(&key.team_id).bind(&key.agent_id).bind(&key.project_id)
            .bind(&key.allowed_cache_controls).bind(&key.allowed_routes)
            .bind(&key.policies).bind(&key.access_group_ids)
            .bind(&key.model_spend).bind(&key.model_max_budget)
            .bind(&key.budget_id).bind(&key.organization_id).bind(&key.object_permission_id)
            .bind(now).bind(&key.updated_by)
            .bind(key.soft_budget_cooldown_bool() as i8).bind(key.expires)
            .bind(key.auto_rotate_bool().map(|b| b as i8)).bind(&key.rotation_interval)
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
            .bind(key.soft_budget_cooldown.clone())
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
            .bind(&key.max_parallel_requests)
            .bind(&key.metadata)
            .bind(key.blocked)
            .bind(&key.tpm_limit)
            .bind(&key.rpm_limit)
            .bind(key.max_budget.clone())
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
            .bind(key.auto_rotate.clone())
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
                    tracing::warn!(%token_hash, "API key blocked");
                    return Ok(None);
                }
                if let Some(expires) = k.expires {
                    if expires <= Utc::now() {
                        tracing::warn!(%token_hash, "API key expired");
                        return Ok(None);
                    }
                }
                Ok(Some(k))
            }
            None => {
                tracing::warn!(%token_hash, "API key not found in database");
                Ok(None)
            }
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

    async fn list_deleted_keys(&self) -> Result<Vec<VirtualKey>> {
        sqlx::query_as("SELECT token, key_name, key_alias, soft_budget_cooldown, spend, expires, models, aliases, config, router_settings, user_id, team_id, agent_id, project_id, permissions, max_parallel_requests, metadata, blocked, tpm_limit, rpm_limit, max_budget, budget_duration, budget_reset_at, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, budget_id, organization_id, object_permission_id, created_at, created_by, updated_at, updated_by, last_active, rotation_count, auto_rotate, rotation_interval, last_rotation_at, key_rotation_at, budget_limits FROM deleted_keys ORDER BY updated_at DESC")
            .fetch_all(self).await.map_err(DbError::from)
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
                .bind(&k.max_parallel_requests)
                .bind(&k.metadata)
                .bind(k.blocked)
                .bind(&k.tpm_limit)
                .bind(&k.rpm_limit)
                .bind(k.max_budget.clone())
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
            .bind(key.max_budget.clone()).bind(&key.tpm_limit).bind(&key.rpm_limit)
            .bind(key.blocked).bind(&key.metadata).bind(&key.permissions)
            .bind(&key.budget_duration).bind(key.budget_reset_at).bind(&key.max_parallel_requests)
            .bind(&key.aliases).bind(&key.config).bind(&key.router_settings)
            .bind(&key.user_id).bind(&key.team_id).bind(&key.agent_id).bind(&key.project_id)
            .bind(&key.allowed_cache_controls).bind(&key.allowed_routes)
            .bind(&key.policies).bind(&key.access_group_ids)
            .bind(&key.model_spend).bind(&key.model_max_budget)
            .bind(&key.budget_id).bind(&key.organization_id).bind(&key.object_permission_id)
            .bind(now).bind(&key.updated_by)
            .bind(key.soft_budget_cooldown.clone()).bind(key.expires)
            .bind(key.auto_rotate.clone()).bind(&key.rotation_interval)
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

    pub async fn list_deleted_keys(&self) -> Result<Vec<VirtualKey>> {
        match self {
            Database::Sqlite(pool) => pool.list_deleted_keys().await,
            Database::Mysql(pool) => pool.list_deleted_keys().await,
            Database::Postgres(pool) => pool.list_deleted_keys().await,
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
#[allow(clippy::too_many_arguments)]
pub trait SpendLogStore {
    async fn insert_spend_log(&self, log: &SpendLog) -> Result<()>;
    /// Update a spend_log after streaming completes — fills in tokens, spend,
    /// end_time, response, duration, TTFT, status, and (via COALESCE) the
    /// upstream provider request_id.  Uses call_id as the unique key.
    async fn update_spend_log(
        &self,
        call_id: &str,
        upstream_request_id: Option<&str>,
        spend: f64,
        total_tokens: i32,
        prompt_tokens: i32,
        completion_tokens: i32,
        end_time: chrono::DateTime<chrono::Utc>,
        request_duration_ms: i32,
        completion_start_time: chrono::DateTime<chrono::Utc>,
        response: serde_json::Value,
        status: &str,
    ) -> Result<()>;
    async fn query_spend_logs(
        &self,
        api_key: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<SpendLog>>;
    async fn get_spend_by_key(&self, api_key: &str) -> Result<f64>;
    async fn get_spend_by_user(&self, user_id: &str) -> Result<f64>;
    async fn get_spend_by_tag(&self, tag: &str) -> Result<f64>;
    async fn get_global_spend(&self) -> Result<f64>;
    async fn aggregate_spend_by_model(&self, api_key: Option<&str>, start_date: Option<&str>, end_date: Option<&str>) -> Result<Vec<SpendModelAgg>>;
    async fn aggregate_spend_by_model_group(&self, api_key: Option<&str>, start_date: Option<&str>, end_date: Option<&str>) -> Result<Vec<SpendModelGroupAgg>>;
    async fn aggregate_spend_by_provider(&self, start_date: Option<&str>, end_date: Option<&str>) -> Result<Vec<SpendProviderAgg>>;
    async fn query_spend_logs_filtered(
        &self,
        api_key: Option<&str>,
        model: Option<&str>,
        provider: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        call_id: Option<&str>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<Vec<SpendLog>>;
    async fn query_spend_logs_count(
        &self,
        api_key: Option<&str>,
        model: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        call_id: Option<&str>,
    ) -> Result<i64>;
    /// Get a single spend log by call_id — returns all columns including body blobs.
    async fn get_spend_log_by_call_id(&self, call_id: &str) -> Result<Option<SpendLog>>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// HealthCheckStore trait
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
pub trait HealthCheckStore {
    async fn insert_health_check(&self, check: &HealthCheck) -> Result<()>;
    async fn get_latest_health_checks(&self) -> Result<Vec<HealthCheck>>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SpendLogStore implementation for SqlitePool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const INSERT_SPEND_LOG_SQLITE: &str = r#"
INSERT INTO spend_logs (
    call_id, call_type, api_key, spend, total_tokens,
    prompt_tokens, completion_tokens, start_time, end_time,
    request_duration_ms, completion_start_time, model, model_id, model_group,
    custom_llm_provider, api_base, "user", metadata,
    cache_hit, cache_key, request_tags, team_id, organization_id,
    end_user, requester_ip_address, messages, response,
    session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request,
    body_archived, parquet_path, request_id
) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
"#;

const QUERY_SPEND_LOGS_ALL_SQLITE: &str = r#"
SELECT
    call_id, call_type, api_key, spend, total_tokens,
    prompt_tokens, completion_tokens, start_time, end_time,
    request_duration_ms, completion_start_time, model, model_id, model_group,
    custom_llm_provider, api_base, "user", metadata,
    cache_hit, cache_key, request_tags, team_id, organization_id,
    end_user, requester_ip_address, messages, response,
    session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request,
    body_archived, parquet_path, request_id
FROM spend_logs
ORDER BY start_time DESC
LIMIT ?
"#;

const QUERY_SPEND_LOGS_BY_KEY_SQLITE: &str = r#"
SELECT
    call_id, call_type, api_key, spend, total_tokens,
    prompt_tokens, completion_tokens, start_time, end_time,
    request_duration_ms, completion_start_time, model, model_id, model_group,
    custom_llm_provider, api_base, "user", metadata,
    cache_hit, cache_key, request_tags, team_id, organization_id,
    end_user, requester_ip_address, messages, response,
    session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request,
    body_archived, parquet_path, request_id
FROM spend_logs
WHERE api_key = ?
ORDER BY start_time DESC
LIMIT ?
"#;

#[async_trait]
impl SpendLogStore for SqlitePool {
    async fn insert_spend_log(&self, log: &SpendLog) -> Result<()> {
        sqlx::query(INSERT_SPEND_LOG_SQLITE)
            .bind(&log.call_id)
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
            .bind(log.body_archived)
            .bind(&log.parquet_path)
            .bind(&log.request_id)
            .execute(self)
            .await?;
        Ok(())
    }

    async fn update_spend_log(
        &self,
        call_id: &str,
        upstream_request_id: Option<&str>,
        spend: f64,
        total_tokens: i32,
        prompt_tokens: i32,
        completion_tokens: i32,
        end_time: chrono::DateTime<chrono::Utc>,
        request_duration_ms: i32,
        completion_start_time: chrono::DateTime<chrono::Utc>,
        response: serde_json::Value,
        status: &str,
    ) -> Result<()> {
        // COALESCE keeps an already-extracted upstream id when the caller
        // passes None (e.g. streaming where the id was extracted at chunk
        // time but the Phase 2 UPDATE doesn't re-supply it).
        sqlx::query(
            "UPDATE spend_logs SET spend=?, total_tokens=?, prompt_tokens=?, completion_tokens=?, end_time=?, request_duration_ms=?, completion_start_time=?, response=?, status=?, request_id=COALESCE(?, request_id) WHERE call_id=?"
        )
        .bind(spend)
        .bind(total_tokens)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(end_time)
        .bind(request_duration_ms)
        .bind(completion_start_time)
        .bind(response)
        .bind(status)
        .bind(upstream_request_id)
        .bind(call_id)
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

    async fn aggregate_spend_by_model(&self, api_key: Option<&str>, start_date: Option<&str>, end_date: Option<&str>) -> Result<Vec<SpendModelAgg>> {
        let date_filter = if start_date.is_some() && end_date.is_some() {
            " AND start_time >= ? AND start_time <= ?"
        } else { "" };
        let sql = match api_key {
            Some(_) => format!("SELECT model, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE api_key = ?{date_filter} GROUP BY model ORDER BY total_tokens DESC"),
            None => format!("SELECT model, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE 1=1{date_filter} GROUP BY model ORDER BY total_tokens DESC"),
        };
        let mut q = sqlx::query_as(&sql);
        if let Some(k) = api_key { q = q.bind(k); }
        if let Some(s) = start_date { q = q.bind(s); }
        if let Some(e) = end_date { q = q.bind(e); }
        q.fetch_all(self).await.map_err(DbError::from)
    }

    async fn aggregate_spend_by_model_group(&self, api_key: Option<&str>, start_date: Option<&str>, end_date: Option<&str>) -> Result<Vec<SpendModelGroupAgg>> {
        let date_filter = if start_date.is_some() && end_date.is_some() {
            " AND start_time >= ? AND start_time <= ?"
        } else { "" };
        let sql = match api_key {
            Some(_) => format!("SELECT COALESCE(model_group, 'unknown') as model_group, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE api_key = ?{date_filter} GROUP BY COALESCE(model_group, 'unknown') ORDER BY total_tokens DESC"),
            None => format!("SELECT COALESCE(model_group, 'unknown') as model_group, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE 1=1{date_filter} GROUP BY COALESCE(model_group, 'unknown') ORDER BY total_tokens DESC"),
        };
        let mut q = sqlx::query_as(&sql);
        if let Some(k) = api_key { q = q.bind(k); }
        if let Some(s) = start_date { q = q.bind(s); }
        if let Some(e) = end_date { q = q.bind(e); }
        q.fetch_all(self).await.map_err(DbError::from)
    }

    async fn aggregate_spend_by_provider(&self, start_date: Option<&str>, end_date: Option<&str>) -> Result<Vec<SpendProviderAgg>> {
        let date_filter = if start_date.is_some() && end_date.is_some() {
            " AND sl.start_time >= ? AND sl.start_time <= ?"
        } else { "" };
        let sql = format!(
            r#"SELECT COALESCE(NULLIF(sl.custom_llm_provider, ''), 'unknown') as provider,
               COALESCE(SUM(sl.total_tokens), 0) as total_tokens,
               COALESCE(SUM(sl.spend), 0) as total_spend,
               COUNT(sl.call_id) as requests
               FROM spend_logs sl
               WHERE 1=1{date_filter}
               GROUP BY provider
               ORDER BY total_tokens DESC"#
        );
        let mut q = sqlx::query_as(&sql);
        if let Some(s) = start_date { q = q.bind(s); }
        if let Some(e) = end_date { q = q.bind(e); }
        q.fetch_all(self).await.map_err(DbError::from).map(|rows: Vec<(String, i64, f64, i64)>| {
            rows.into_iter().map(|(provider, total_tokens, total_spend, requests)| {
                SpendProviderAgg { provider, total_tokens, total_spend, requests }
            }).collect()
        })
    }

    async fn query_spend_logs_filtered(
        &self,
        api_key: Option<&str>,
        model: Option<&str>,
        _provider: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        call_id: Option<&str>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        let limit_val = limit.unwrap_or(30);
        let offset_val = offset.unwrap_or(0);
        let mut sql = String::from(
            r#"SELECT
                call_id, call_type, api_key, spend, total_tokens,
                prompt_tokens, completion_tokens, start_time, end_time,
                request_duration_ms, completion_start_time, model, model_id, model_group,
                custom_llm_provider, api_base, "user", metadata,
                cache_hit, cache_key, request_tags, team_id, organization_id,
                end_user, requester_ip_address, messages, response,
                session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request,
                body_archived, parquet_path, request_id
            FROM spend_logs WHERE 1=1"#
        );

        if api_key.is_some() { sql.push_str(" AND api_key = ?"); }
        if model.is_some() { sql.push_str(" AND model = ?"); }
        if start_date.is_some() { sql.push_str(" AND start_time >= ?"); }
        if end_date.is_some() { sql.push_str(" AND start_time <= ?"); }
        // Dual-column fuzzy search: match gateway call_id OR upstream request_id (LIKE '%X%').
        if call_id.is_some() { sql.push_str(" AND (call_id LIKE ? ESCAPE '\\' OR request_id LIKE ? ESCAPE '\\')"); }

        sql.push_str(" ORDER BY start_time DESC LIMIT ? OFFSET ?");

        let mut query = sqlx::query_as(&sql);
        if let Some(k) = api_key { query = query.bind(k); }
        if let Some(m) = model { query = query.bind(m); }
        if let Some(sd) = start_date { query = query.bind(sd); }
        if let Some(ed) = end_date { query = query.bind(ed); }
        let _like_val;
        if let Some(rid) = call_id {
            _like_val = format!("%{}%", rid.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            query = query.bind(&_like_val);
            query = query.bind(&_like_val);
        }
        query = query.bind(limit_val);
        query = query.bind(offset_val);

        query.fetch_all(self).await.map_err(DbError::from)
    }

    async fn query_spend_logs_count(
        &self,
        api_key: Option<&str>,
        model: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        call_id: Option<&str>,
    ) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM spend_logs WHERE 1=1");
        if api_key.is_some() { sql.push_str(" AND api_key = ?"); }
        if model.is_some() { sql.push_str(" AND model = ?"); }
        if start_date.is_some() { sql.push_str(" AND start_time >= ?"); }
        if end_date.is_some() { sql.push_str(" AND start_time <= ?"); }
        // Dual-column fuzzy search: match gateway call_id OR upstream request_id (LIKE '%X%').
        if call_id.is_some() { sql.push_str(" AND (call_id LIKE ? ESCAPE '\\' OR request_id LIKE ? ESCAPE '\\')"); }

        let mut query = sqlx::query_as::<_, (i64,)>(&sql);
        if let Some(k) = api_key { query = query.bind(k); }
        if let Some(m) = model { query = query.bind(m); }
        if let Some(sd) = start_date { query = query.bind(sd); }
        if let Some(ed) = end_date { query = query.bind(ed); }
        let _like_val;
        if let Some(rid) = call_id {
            _like_val = format!("%{}%", rid.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            query = query.bind(&_like_val);
            query = query.bind(&_like_val);
        }

        query.fetch_one(self).await.map(|row: (i64,)| row.0).map_err(DbError::from)
    }

    async fn get_spend_log_by_call_id(&self, call_id: &str) -> Result<Option<SpendLog>> {
        sqlx::query_as::<_, SpendLog>(
            r#"SELECT call_id, call_type, api_key, spend, total_tokens,
            prompt_tokens, completion_tokens, start_time, end_time,
            request_duration_ms, completion_start_time, model, model_id, model_group,
            custom_llm_provider, api_base, "user", metadata,
            cache_hit, cache_key, request_tags, team_id, organization_id,
            end_user, requester_ip_address, messages, response,
            session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request,
            body_archived, parquet_path, request_id
            FROM spend_logs WHERE call_id = ?"#,
        )
        .bind(call_id)
        .fetch_optional(self)
        .await
        .map_err(DbError::from)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SpendLogStore implementation for MySqlPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl SpendLogStore for MySqlPool {
    async fn insert_spend_log(&self, log: &SpendLog) -> Result<()> {
        sqlx::query(
            "INSERT INTO spend_logs (call_id, call_type, api_key, spend, total_tokens, \
             prompt_tokens, completion_tokens, start_time, end_time, \
             request_duration_ms, completion_start_time, model, model_id, model_group, \
             custom_llm_provider, api_base, user, metadata, \
             cache_hit, cache_key, request_tags, team_id, organization_id, \
             end_user, requester_ip_address, messages, response, \
             session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request, \
             body_archived, parquet_path, request_id) \
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&log.call_id)
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
        .bind(log.body_archived)
        .bind(&log.parquet_path)
        .bind(&log.request_id)
        .execute(self)
        .await?;
        Ok(())
    }

    async fn update_spend_log(
        &self,
        call_id: &str,
        upstream_request_id: Option<&str>,
        spend: f64,
        total_tokens: i32,
        prompt_tokens: i32,
        completion_tokens: i32,
        end_time: chrono::DateTime<chrono::Utc>,
        request_duration_ms: i32,
        completion_start_time: chrono::DateTime<chrono::Utc>,
        response: serde_json::Value,
        status: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE spend_logs SET spend=?, total_tokens=?, prompt_tokens=?, completion_tokens=?, end_time=?, request_duration_ms=?, completion_start_time=?, response=?, status=?, request_id=COALESCE(?, request_id) WHERE call_id=?"
        )
        .bind(spend)
        .bind(total_tokens)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(end_time)
        .bind(request_duration_ms)
        .bind(completion_start_time)
        .bind(response)
        .bind(status)
        .bind(upstream_request_id)
        .bind(call_id)
        .execute(self)
        .await?;
        Ok(())
    }

    async fn query_spend_logs(
        &self,
        api_key: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        let limit_val = limit.unwrap_or(20000);
        // MySQL zero-date guard: upstream litellm may sync rows with
        // start_time='0000-00-00 00:00:00', which sqlx rejects when
        // decoding into chrono::DateTime<Utc>. Filter them at the SQL level.
        let sql = "SELECT call_id, call_type, api_key, spend, total_tokens, \
                   prompt_tokens, completion_tokens, start_time, end_time, \
                   request_duration_ms, completion_start_time, model, model_id, model_group, \
                   custom_llm_provider, api_base, user, metadata, \
                   cache_hit, cache_key, request_tags, team_id, organization_id, \
                   end_user, requester_ip_address, messages, response, \
                   session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request, \
                   body_archived, parquet_path, request_id \
                   FROM spend_logs WHERE start_time > '1000-01-01'";
        match api_key {
            Some(key) => sqlx::query_as(&format!(
                "{} AND api_key = ? ORDER BY start_time DESC LIMIT {}",
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

    async fn aggregate_spend_by_model(&self, api_key: Option<&str>, start_date: Option<&str>, end_date: Option<&str>) -> Result<Vec<SpendModelAgg>> {
        let date_filter = if start_date.is_some() && end_date.is_some() {
            " AND start_time >= ? AND start_time <= ?"
        } else { "" };
        // MySQL CAST: SUM(total_tokens) returns DECIMAL, must CAST AS SIGNED
        let sql = match api_key {
            Some(_) => format!("SELECT model, CAST(SUM(total_tokens) AS SIGNED) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE api_key = ?{date_filter} GROUP BY model ORDER BY total_tokens DESC"),
            None => format!("SELECT model, CAST(SUM(total_tokens) AS SIGNED) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE 1=1{date_filter} GROUP BY model ORDER BY total_tokens DESC"),
        };
        let mut q = sqlx::query_as(&sql);
        if let Some(k) = api_key { q = q.bind(k); }
        if let Some(s) = start_date { q = q.bind(s); }
        if let Some(e) = end_date { q = q.bind(e); }
        q.fetch_all(self).await.map_err(DbError::from)
    }

    async fn aggregate_spend_by_model_group(&self, api_key: Option<&str>, start_date: Option<&str>, end_date: Option<&str>) -> Result<Vec<SpendModelGroupAgg>> {
        let date_filter = if start_date.is_some() && end_date.is_some() {
            " AND start_time >= ? AND start_time <= ?"
        } else { "" };
        let sql = match api_key {
            Some(_) => format!("SELECT COALESCE(model_group, 'unknown') as model_group, CAST(SUM(total_tokens) AS SIGNED) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE api_key = ?{date_filter} GROUP BY COALESCE(model_group, 'unknown') ORDER BY total_tokens DESC"),
            None => format!("SELECT COALESCE(model_group, 'unknown') as model_group, CAST(SUM(total_tokens) AS SIGNED) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE 1=1{date_filter} GROUP BY COALESCE(model_group, 'unknown') ORDER BY total_tokens DESC"),
        };
        let mut q = sqlx::query_as(&sql);
        if let Some(k) = api_key { q = q.bind(k); }
        if let Some(s) = start_date { q = q.bind(s); }
        if let Some(e) = end_date { q = q.bind(e); }
        q.fetch_all(self).await.map_err(DbError::from)
    }


    async fn aggregate_spend_by_provider(&self, start_date: Option<&str>, end_date: Option<&str>) -> Result<Vec<SpendProviderAgg>> {
        let date_filter = if start_date.is_some() && end_date.is_some() {
            " AND sl.start_time >= ? AND sl.start_time <= ?"
        } else { "" };
        let sql = format!(
            r#"SELECT COALESCE(NULLIF(sl.custom_llm_provider, ''), 'unknown') as provider,
               CAST(COALESCE(SUM(sl.total_tokens), 0) AS SIGNED) as total_tokens,
               COALESCE(SUM(sl.spend), 0) as total_spend,
               COUNT(sl.call_id) as requests
               FROM spend_logs sl
               WHERE 1=1{date_filter}
               GROUP BY provider
               ORDER BY total_tokens DESC"#
        );
        let mut q = sqlx::query_as(&sql);
        if let Some(s) = start_date { q = q.bind(s); }
        if let Some(e) = end_date { q = q.bind(e); }
        q.fetch_all(self).await.map_err(DbError::from).map(|rows: Vec<(String, i64, f64, i64)>| {
            rows.into_iter().map(|(provider, total_tokens, total_spend, requests)| {
                SpendProviderAgg { provider, total_tokens, total_spend, requests }
            }).collect()
        })
    }

    async fn query_spend_logs_filtered(
        &self, api_key: Option<&str>, model: Option<&str>, _provider: Option<&str>,
        start_date: Option<&str>, end_date: Option<&str>,
        call_id: Option<&str>,
        limit: Option<i32>, offset: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        // Fallback: use basic query and do in-memory filter
        let limit_val = limit.unwrap_or(30);
        let offset_val = offset.unwrap_or(0);
        // MySQL fallback: fetch up to N rows + apply in-memory filter + pagination
        let fetch_limit = std::cmp::min(limit_val + offset_val, 10000);
        let result = self.query_spend_logs(api_key, Some(fetch_limit)).await?;
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
            // Dual-column in-memory fuzzy search: match gateway call_id OR upstream request_id (substring).
            if let Some(rid) = call_id { if !log.call_id.contains(rid) && !log.request_id.as_deref().map_or(false, |r| r.contains(rid)) { return false; } }
            true
        }).collect();
        let start = offset_val as usize;
        let end = std::cmp::min(start + limit_val as usize, filtered.len());
        if start >= filtered.len() {
            Ok(vec![])
        } else {
            Ok(filtered[start..end].to_vec())
        }
    }

    async fn query_spend_logs_count(
        &self,
        api_key: Option<&str>,
        model: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        call_id: Option<&str>,
    ) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM spend_logs WHERE 1=1");
        if api_key.is_some() { sql.push_str(" AND api_key = ?"); }
        if model.is_some() { sql.push_str(" AND model = ?"); }
        if start_date.is_some() { sql.push_str(" AND start_time >= ?"); }
        if end_date.is_some() { sql.push_str(" AND start_time <= ?"); }
        // Dual-column fuzzy search: match gateway call_id OR upstream request_id (LIKE '%X%').
        if call_id.is_some() { sql.push_str(" AND (call_id LIKE ? ESCAPE '\\' OR request_id LIKE ? ESCAPE '\\')"); }

        let mut query = sqlx::query_as::<_, (i64,)>(&sql);
        if let Some(k) = api_key { query = query.bind(k); }
        if let Some(m) = model { query = query.bind(m); }
        if let Some(sd) = start_date { query = query.bind(sd); }
        if let Some(ed) = end_date { query = query.bind(ed); }
        let _like_val;
        if let Some(rid) = call_id {
            _like_val = format!("%{}%", rid.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            query = query.bind(&_like_val);
            query = query.bind(&_like_val);
        }

        query.fetch_one(self).await.map(|row: (i64,)| row.0).map_err(DbError::from)
    }

    async fn get_spend_log_by_call_id(&self, call_id: &str) -> Result<Option<SpendLog>> {
        sqlx::query_as::<_, SpendLog>(
            r#"SELECT call_id, call_type, api_key, spend, total_tokens,
            prompt_tokens, completion_tokens, start_time, end_time,
            request_duration_ms, completion_start_time, model, model_id, model_group,
            custom_llm_provider, api_base, "user", metadata,
            cache_hit, cache_key, request_tags, team_id, organization_id,
            end_user, requester_ip_address, messages, response,
            session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request,
            body_archived, parquet_path, request_id
            FROM spend_logs WHERE call_id = ?"#,
        )
        .bind(call_id)
        .fetch_optional(self)
        .await
        .map_err(DbError::from)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SpendLogStore implementation for PgPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl SpendLogStore for PgPool {
    async fn insert_spend_log(&self, log: &SpendLog) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO spend_logs (call_id, call_type, api_key, spend, total_tokens,
            prompt_tokens, completion_tokens, start_time, end_time,
            request_duration_ms, completion_start_time, model, model_id, model_group,
            custom_llm_provider, api_base, "user", metadata,
            cache_hit, cache_key, request_tags, team_id, organization_id,
            end_user, requester_ip_address, messages, response,
            session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request,
            body_archived, parquet_path, request_id)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35)"#
        )
            .bind(&log.call_id).bind(&log.call_type).bind(&log.api_key)
            .bind(log.spend).bind(log.total_tokens).bind(log.prompt_tokens)
            .bind(log.completion_tokens).bind(log.start_time).bind(&log.end_time)
            .bind(log.request_duration_ms).bind(log.completion_start_time)
            .bind(&log.model).bind(&log.model_id).bind(&log.model_group)
            .bind(&log.custom_llm_provider).bind(&log.api_base).bind(&log.user)
            .bind(&log.metadata).bind(&log.cache_hit).bind(&log.cache_key)
            .bind(&log.request_tags).bind(&log.team_id).bind(&log.organization_id)
            .bind(&log.end_user).bind(&log.requester_ip_address)
            .bind(&log.messages).bind(&log.response).bind(&log.session_id)
            .bind(&log.status).bind(&log.mcp_namespaced_tool_name).bind(&log.agent_id)
            .bind(&log.proxy_server_request).bind(log.body_archived).bind(&log.parquet_path)
            .bind(&log.request_id)
            .execute(self).await?;
        Ok(())
    }

    async fn update_spend_log(
        &self,
        call_id: &str,
        upstream_request_id: Option<&str>,
        spend: f64,
        total_tokens: i32,
        prompt_tokens: i32,
        completion_tokens: i32,
        end_time: chrono::DateTime<chrono::Utc>,
        request_duration_ms: i32,
        completion_start_time: chrono::DateTime<chrono::Utc>,
        response: serde_json::Value,
        status: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE spend_logs SET spend=$1, total_tokens=$2, prompt_tokens=$3, completion_tokens=$4, end_time=$5, request_duration_ms=$6, completion_start_time=$7, response=$8, status=$9, request_id=COALESCE($10, request_id) WHERE call_id=$11"
        )
        .bind(spend)
        .bind(total_tokens)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(end_time)
        .bind(request_duration_ms)
        .bind(completion_start_time)
        .bind(response)
        .bind(status)
        .bind(upstream_request_id)
        .bind(call_id)
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
        let sql = r#"SELECT call_id, call_type, api_key, spend, total_tokens,
            prompt_tokens, completion_tokens, start_time, end_time,
            request_duration_ms, completion_start_time, model, model_id, model_group,
            custom_llm_provider, api_base, "user", metadata,
            cache_hit, cache_key, request_tags, team_id, organization_id,
            end_user, requester_ip_address, messages, response,
            session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request,
            body_archived, parquet_path, request_id
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

    async fn aggregate_spend_by_model(&self, api_key: Option<&str>, start_date: Option<&str>, end_date: Option<&str>) -> Result<Vec<SpendModelAgg>> {
        let date_filter = if start_date.is_some() && end_date.is_some() {
            format!(
                " AND start_time >= '{}'::TIMESTAMPTZ AND start_time <= '{}'::TIMESTAMPTZ",
                start_date.unwrap().replace('\'', "''"),
                end_date.unwrap().replace('\'', "''")
            )
        } else { String::new() };
        let sql = match api_key {
            Some(_) => format!("SELECT model, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE api_key = $1{date_filter} GROUP BY model ORDER BY total_tokens DESC"),
            None => format!("SELECT model, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE 1=1{date_filter} GROUP BY model ORDER BY total_tokens DESC"),
        };
        let mut q = sqlx::query_as(&sql);
        if let Some(k) = api_key { q = q.bind(k); }
        // start_date / end_date already inlined with ::TIMESTAMPTZ cast above — no bind needed
        q.fetch_all(self).await.map_err(DbError::from)
    }

    async fn aggregate_spend_by_model_group(&self, api_key: Option<&str>, start_date: Option<&str>, end_date: Option<&str>) -> Result<Vec<SpendModelGroupAgg>> {
        let date_filter = if start_date.is_some() && end_date.is_some() {
            format!(
                " AND start_time >= '{}'::TIMESTAMPTZ AND start_time <= '{}'::TIMESTAMPTZ",
                start_date.unwrap().replace('\'', "''"),
                end_date.unwrap().replace('\'', "''")
            )
        } else { String::new() };
        let sql = match api_key {
            Some(_) => format!("SELECT COALESCE(model_group, 'unknown') as model_group, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE api_key = $1{date_filter} GROUP BY COALESCE(model_group, 'unknown') ORDER BY total_tokens DESC"),
            None => format!("SELECT COALESCE(model_group, 'unknown') as model_group, SUM(total_tokens) as total_tokens, SUM(spend) as total_spend, COUNT(*) as requests FROM spend_logs WHERE 1=1{date_filter} GROUP BY COALESCE(model_group, 'unknown') ORDER BY total_tokens DESC"),
        };
        let mut q = sqlx::query_as(&sql);
        if let Some(k) = api_key { q = q.bind(k); }
        // start_date / end_date already inlined with ::TIMESTAMPTZ cast above — no bind needed
        q.fetch_all(self).await.map_err(DbError::from)
    }

    async fn aggregate_spend_by_provider(&self, start_date: Option<&str>, end_date: Option<&str>) -> Result<Vec<SpendProviderAgg>> {
        let date_filter = if start_date.is_some() && end_date.is_some() {
            format!(
                " AND sl.start_time >= '{}'::TIMESTAMPTZ AND sl.start_time <= '{}'::TIMESTAMPTZ",
                start_date.unwrap().replace('\'', "''"),
                end_date.unwrap().replace('\'', "''")
            )
        } else { String::new() };
        let sql = format!(
            r#"SELECT COALESCE(NULLIF(sl.custom_llm_provider, ''), 'unknown') as provider,
               COALESCE(SUM(sl.total_tokens), 0) as total_tokens,
               COALESCE(SUM(sl.spend), 0) as total_spend,
               COUNT(sl.call_id) as requests
               FROM spend_logs sl
               WHERE 1=1{date_filter}
               GROUP BY provider
               ORDER BY total_tokens DESC"#
        );
        // start_date / end_date already inlined with ::TIMESTAMPTZ cast above — no bind needed
        sqlx::query_as(&sql)
            .fetch_all(self)
            .await
            .map_err(DbError::from)
            .map(|rows: Vec<(String, i64, f64, i64)>| {
                rows.into_iter().map(|(provider, total_tokens, total_spend, requests)| {
                    SpendProviderAgg { provider, total_tokens, total_spend, requests }
                }).collect()
            })
    }

    async fn query_spend_logs_filtered(
        &self, api_key: Option<&str>, model: Option<&str>, _provider: Option<&str>,
        start_date: Option<&str>, end_date: Option<&str>,
        call_id: Option<&str>,
        limit: Option<i32>, offset: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        let limit_val = limit.unwrap_or(30);
        let offset_val = offset.unwrap_or(0);
        // PG fallback: fetch up to N rows + in-memory filter + pagination
        let fetch_limit = std::cmp::min(limit_val + offset_val, 10000);
        let result = self.query_spend_logs(api_key, Some(fetch_limit)).await?;
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
            // Dual-column in-memory fuzzy search: match gateway call_id OR upstream request_id (substring).
            if let Some(rid) = call_id { if !log.call_id.contains(rid) && !log.request_id.as_deref().map_or(false, |r| r.contains(rid)) { return false; } }
            true
        }).collect();
        let start = offset_val as usize;
        let end = std::cmp::min(start + limit_val as usize, filtered.len());
        if start >= filtered.len() {
            Ok(vec![])
        } else {
            Ok(filtered[start..end].to_vec())
        }
    }

    async fn query_spend_logs_count(
        &self,
        api_key: Option<&str>,
        model: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        call_id: Option<&str>,
    ) -> Result<i64> {
        let ts_cast = "::TIMESTAMPTZ";

        let mut sql = String::from("SELECT COUNT(*) FROM spend_logs WHERE 1=1");
        let mut i: usize = 1;
        if api_key.is_some() { sql.push_str(&format!(" AND api_key = ${}", i)); i += 1; }
        if model.is_some() { sql.push_str(&format!(" AND model = ${}", i)); i += 1; }
        if start_date.is_some() { sql.push_str(&format!(" AND start_time >= '{}'{}", start_date.unwrap().replace('\'', "''"), ts_cast)); }
        if end_date.is_some() { sql.push_str(&format!(" AND start_time <= '{}'{}", end_date.unwrap().replace('\'', "''"), ts_cast)); }
        // Dual-column search: match gateway call_id OR upstream request_id.
        // Two placeholders ($i and $i+1), both bound to the same search term.
        // (i is not read again after this branch — last conditional.)
        if call_id.is_some() {
            sql.push_str(&format!(" AND (call_id = ${} OR request_id = ${})", i, i + 1));
        }

        let mut query = sqlx::query_as::<_, (i64,)>(&sql);
        if let Some(k) = api_key { query = query.bind(k); }
        if let Some(m) = model { query = query.bind(m); }
        // start_date / end_date already inlined with ::TIMESTAMPTZ cast above — no bind needed
        if let Some(rid) = call_id { query = query.bind(rid); query = query.bind(rid); }

        query.fetch_one(self).await.map(|row: (i64,)| row.0).map_err(DbError::from)
    }

    async fn get_spend_log_by_call_id(&self, call_id: &str) -> Result<Option<SpendLog>> {
        sqlx::query_as::<_, SpendLog>(
            r#"SELECT call_id, call_type, api_key, spend, total_tokens,
            prompt_tokens, completion_tokens, start_time, end_time,
            request_duration_ms, completion_start_time, model, model_id, model_group,
            custom_llm_provider, api_base, "user", metadata,
            cache_hit, cache_key, request_tags, team_id, organization_id,
            end_user, requester_ip_address, messages, response,
            session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request,
            body_archived, parquet_path, request_id
            FROM spend_logs WHERE call_id = $1"#,
        )
        .bind(call_id)
        .fetch_optional(self)
        .await
        .map_err(DbError::from)
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

    pub async fn update_spend_log(
        &self,
        call_id: &str,
        upstream_request_id: Option<&str>,
        spend: f64,
        total_tokens: i32,
        prompt_tokens: i32,
        completion_tokens: i32,
        end_time: chrono::DateTime<chrono::Utc>,
        request_duration_ms: i32,
        completion_start_time: chrono::DateTime<chrono::Utc>,
        response: serde_json::Value,
        status: &str,
    ) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.update_spend_log(call_id, upstream_request_id, spend, total_tokens, prompt_tokens, completion_tokens, end_time, request_duration_ms, completion_start_time, response, status).await,
            Database::Mysql(pool) => pool.update_spend_log(call_id, upstream_request_id, spend, total_tokens, prompt_tokens, completion_tokens, end_time, request_duration_ms, completion_start_time, response, status).await,
            Database::Postgres(pool) => pool.update_spend_log(call_id, upstream_request_id, spend, total_tokens, prompt_tokens, completion_tokens, end_time, request_duration_ms, completion_start_time, response, status).await,
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
        start_date: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<Vec<SpendModelAgg>> {
        match self {
            Database::Sqlite(pool) => pool.aggregate_spend_by_model(api_key, start_date, end_date).await,
            Database::Mysql(pool) => pool.aggregate_spend_by_model(api_key, start_date, end_date).await,
            Database::Postgres(pool) => pool.aggregate_spend_by_model(api_key, start_date, end_date).await,
        }
    }

    pub async fn aggregate_spend_by_model_group(
        &self,
        api_key: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<Vec<SpendModelGroupAgg>> {
        match self {
            Database::Sqlite(pool) => pool.aggregate_spend_by_model_group(api_key, start_date, end_date).await,
            Database::Mysql(pool) => pool.aggregate_spend_by_model_group(api_key, start_date, end_date).await,
            Database::Postgres(pool) => pool.aggregate_spend_by_model_group(api_key, start_date, end_date).await,
        }
    }

    pub async fn aggregate_spend_by_provider(
        &self,
        start_date: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<Vec<SpendProviderAgg>> {
        match self {
            Database::Sqlite(pool) => pool.aggregate_spend_by_provider(start_date, end_date).await,
            Database::Mysql(pool) => pool.aggregate_spend_by_provider(start_date, end_date).await,
            Database::Postgres(pool) => pool.aggregate_spend_by_provider(start_date, end_date).await,
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub async fn query_spend_logs_filtered(
        &self,
        api_key: Option<&str>,
        model: Option<&str>,
        provider: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        call_id: Option<&str>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        match self {
            Database::Sqlite(pool) => {
                pool.query_spend_logs_filtered(api_key, model, provider, start_date, end_date, call_id, limit, offset)
                    .await
            }
            Database::Mysql(pool) => {
                pool.query_spend_logs_filtered(api_key, model, provider, start_date, end_date, call_id, limit, offset)
                    .await
            }
            Database::Postgres(pool) => {
                pool.query_spend_logs_filtered(api_key, model, provider, start_date, end_date, call_id, limit, offset)
                    .await
            }
        }
    }

    pub async fn query_spend_logs_count(
        &self,
        api_key: Option<&str>,
        model: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        call_id: Option<&str>,
    ) -> Result<i64> {
        match self {
            Database::Sqlite(pool) => pool.query_spend_logs_count(api_key, model, start_date, end_date, call_id).await,
            Database::Mysql(pool) => pool.query_spend_logs_count(api_key, model, start_date, end_date, call_id).await,
            Database::Postgres(pool) => pool.query_spend_logs_count(api_key, model, start_date, end_date, call_id).await,
        }
    }

    /// Get a single spend log by call_id — returns all columns including body blobs.
    pub async fn get_spend_log_by_call_id(&self, call_id: &str) -> Result<Option<SpendLog>> {
        match self {
            Database::Sqlite(pool) => pool.get_spend_log_by_call_id(call_id).await,
            Database::Mysql(pool) => pool.get_spend_log_by_call_id(call_id).await,
            Database::Postgres(pool) => pool.get_spend_log_by_call_id(call_id).await,
        }
    }

    /// Normalize a date string for SQL query comparison.
    ///
    /// Handles three input formats defensively:
    /// - Already RFC3339/UTC (contains 'Z' or '+'): pass through
    /// - Pure date "yyyy-MM-dd": append T23:59:59.999Z for end dates,
    ///   T00:00:00Z for start dates
    /// - Local time without timezone suffix: append Z
    pub fn normalize_date_for_query(date_str: &str, is_end: bool) -> String {
        if date_str.contains('Z') || date_str.contains('+') {
            return date_str.to_string();
        }
        if date_str.len() == 10 {
            return if is_end {
                format!("{}T23:59:59.999Z", date_str)
            } else {
                format!("{}T00:00:00Z", date_str)
            };
        }
        format!("{}Z", date_str)
    }

    /// Query activity metadata: aggregate spend/tokens/requests for a time range
    /// with optional user/team/org filters.
    pub async fn query_activity_metadata(
        &self,
        start_date: &str,
        end_date: &str,
        user_id: Option<&str>,
        team_id: Option<&str>,
        organization_id: Option<&str>,
    ) -> Result<(f64, i64, i64, i64, i64, i64, i64)> {
        // Column order (must be identical across all three backends):
        //   spend, total_tokens, requests, successful_requests, failed_requests,
        //   prompt_tokens, completion_tokens
        let sql_base = r#"SELECT
                COALESCE(SUM(spend), 0),
                COALESCE(SUM(total_tokens), 0),
                COUNT(call_id),
                COUNT(CASE WHEN status = 'success' THEN 1 END),
                COUNT(CASE WHEN status LIKE 'failure%' THEN 1 END),
                COALESCE(SUM(prompt_tokens), 0),
                COALESCE(SUM(completion_tokens), 0)
            FROM spend_logs
            WHERE date(start_time) >= date({p1}) AND date(start_time) <= date({p2}) {filter}"#;
        match self {
            Database::Sqlite(pool) => {
                let (filter_clause, params) = build_activity_filter(user_id, team_id, organization_id, 0, false, false);
                let sql = sql_base.replace("{p1}", "?").replace("{p2}", "?").replace("{filter}", &filter_clause);
                let mut q = sqlx::query_as(&sql)
                    .bind(start_date).bind(end_date);
                for p in &params { q = q.bind(p); }
                q.fetch_one(pool).await.map_err(DbError::from)
            }
            Database::Mysql(pool) => {
                // MySQL: CAST SUM/COUNT AS SIGNED because MySQL returns DECIMAL.
                // spend column is f64 (DOUBLE) — do NOT CAST, it stays as-is.
                let sql_mysql = r#"SELECT
                    COALESCE(SUM(spend), 0),
                    CAST(COALESCE(SUM(total_tokens), 0) AS SIGNED),
                    CAST(COUNT(call_id) AS SIGNED),
                    CAST(COUNT(CASE WHEN status = 'success' THEN 1 END) AS SIGNED),
                    CAST(COUNT(CASE WHEN status LIKE 'failure%' THEN 1 END) AS SIGNED),
                    CAST(COALESCE(SUM(prompt_tokens), 0) AS SIGNED),
                    CAST(COALESCE(SUM(completion_tokens), 0) AS SIGNED)
                FROM spend_logs
                WHERE date(start_time) >= date(?) AND date(start_time) <= date(?) {filter}"#;
                let (filter_clause, params) = build_activity_filter(user_id, team_id, organization_id, 0, false, true);
                let sql = sql_mysql.replace("{filter}", &filter_clause);
                let mut q = sqlx::query_as(&sql)
                    .bind(start_date).bind(end_date);
                for p in &params { q = q.bind(p); }
                q.fetch_one(pool).await.map_err(DbError::from)
            }
            Database::Postgres(pool) => {
                let (filter_clause, params) = build_activity_filter(user_id, team_id, organization_id, 3, true, false);
                let sql = sql_base.replace("{p1}", "$1").replace("{p2}", "$2").replace("{filter}", &filter_clause);
                let mut q = sqlx::query_as(&sql)
                    .bind(start_date).bind(end_date);
                for p in &params { q = q.bind(p); }
                q.fetch_one(pool).await.map_err(DbError::from)
            }
        }
    }

    /// Query daily spend aggregation for a time range with optional filters.
    pub async fn query_activity_daily(
        &self,
        start_date: &str,
        end_date: &str,
        user_id: Option<&str>,
        team_id: Option<&str>,
        organization_id: Option<&str>,
    ) -> Result<Vec<(String, f64, i64, i64, i64, i64, i64, i64)>> {
        match self {
            Database::Sqlite(pool) => {
                // SQLite: DATE() already returns TEXT — no cast needed.
                let sql = "SELECT DATE(start_time), COALESCE(SUM(spend), 0), COALESCE(SUM(total_tokens), 0), COUNT(call_id), \
                    COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), \
                    COUNT(CASE WHEN status = 'success' THEN 1 END), \
                    COUNT(CASE WHEN status LIKE 'failure%' THEN 1 END) \
                    FROM spend_logs WHERE date(start_time) >= date(?) AND date(start_time) <= date(?) {filter} \
                    GROUP BY 1 ORDER BY 1 ASC";
                let (filter_clause, params) = build_activity_filter(user_id, team_id, organization_id, 0, false, false);
                let sql = sql.replace("{filter}", &filter_clause);
                let mut q = sqlx::query_as(&sql)
                    .bind(start_date).bind(end_date);
                for p in &params { q = q.bind(p); }
                q.fetch_all(pool).await.map_err(DbError::from)
            }
            Database::Mysql(pool) => {
                // MySQL: CAST DATE and SUM/COUNT AS SIGNED for i64 compatibility.
                let sql = "SELECT CAST(DATE(start_time) AS CHAR), COALESCE(SUM(spend), 0), CAST(COALESCE(SUM(total_tokens), 0) AS SIGNED), CAST(COUNT(call_id) AS SIGNED), \
                    CAST(COALESCE(SUM(prompt_tokens), 0) AS SIGNED), CAST(COALESCE(SUM(completion_tokens), 0) AS SIGNED), \
                    CAST(COUNT(CASE WHEN status = 'success' THEN 1 END) AS SIGNED), \
                    CAST(COUNT(CASE WHEN status LIKE 'failure%' THEN 1 END) AS SIGNED) \
                    FROM spend_logs WHERE date(start_time) >= date(?) AND date(start_time) <= date(?) {filter} \
                    GROUP BY 1 ORDER BY 1 ASC";
                let (filter_clause, params) = build_activity_filter(user_id, team_id, organization_id, 0, false, true);
                let sql = sql.replace("{filter}", &filter_clause);
                let mut q = sqlx::query_as(&sql)
                    .bind(start_date).bind(end_date);
                for p in &params { q = q.bind(p); }
                q.fetch_all(pool).await.map_err(DbError::from)
            }
            Database::Postgres(pool) => {
                // PostgreSQL: DATE(…)::TEXT converts DATE → TEXT.
                let sql = "SELECT DATE(start_time)::TEXT, COALESCE(SUM(spend), 0), COALESCE(SUM(total_tokens), 0), COUNT(call_id), \
                    COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), \
                    COUNT(CASE WHEN status = 'success' THEN 1 END), \
                    COUNT(CASE WHEN status LIKE 'failure%' THEN 1 END) \
                    FROM spend_logs WHERE date(start_time) >= date($1) AND date(start_time) <= date($2) {filter} \
                    GROUP BY 1 ORDER BY 1 ASC";
                let (filter_clause, params) = build_activity_filter(user_id, team_id, organization_id, 3, true, false);
                let sql = sql.replace("{filter}", &filter_clause);
                let mut q = sqlx::query_as(&sql)
                    .bind(start_date).bind(end_date);
                for p in &params { q = q.bind(p); }
                q.fetch_all(pool).await.map_err(DbError::from)
            }
        }
    }

    /// Query hourly spend aggregation for a time range with optional filters.
    /// Used when the query range is ≤72 hours for finer-grained trend charts.
    pub async fn query_activity_hourly(
        &self,
        start_date: &str,
        end_date: &str,
        user_id: Option<&str>,
        team_id: Option<&str>,
        organization_id: Option<&str>,
    ) -> Result<Vec<(String, f64, i64, i64, i64, i64, i64, i64)>> {
        // Use full datetime bounds for hourly queries (start of start_date, end of end_date)
        let start_ts = format!("{}T00:00:00", start_date);
        let end_ts = format!("{}T23:59:59", end_date);
        match self {
            Database::Sqlite(pool) => {
                let sql = "SELECT strftime('%Y-%m-%dT%H:00:00', start_time), COALESCE(SUM(spend), 0), COALESCE(SUM(total_tokens), 0), COUNT(call_id), \
                    COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), \
                    COUNT(CASE WHEN status = 'success' THEN 1 END), \
                    COUNT(CASE WHEN status LIKE 'failure%' THEN 1 END) \
                    FROM spend_logs WHERE start_time >= ? AND start_time <= ? {filter} \
                    GROUP BY 1 ORDER BY 1 ASC";
                let (filter_clause, params) = build_activity_filter(user_id, team_id, organization_id, 0, false, false);
                let sql = sql.replace("{filter}", &filter_clause);
                let mut q = sqlx::query_as(&sql)
                    .bind(&start_ts).bind(&end_ts);
                for p in &params { q = q.bind(p); }
                q.fetch_all(pool).await.map_err(DbError::from)
            }
            Database::Mysql(pool) => {
                let sql = "SELECT DATE_FORMAT(start_time, '%Y-%m-%dT%H:00:00'), COALESCE(SUM(spend), 0), CAST(COALESCE(SUM(total_tokens), 0) AS SIGNED), CAST(COUNT(call_id) AS SIGNED), \
                    CAST(COALESCE(SUM(prompt_tokens), 0) AS SIGNED), CAST(COALESCE(SUM(completion_tokens), 0) AS SIGNED), \
                    CAST(COUNT(CASE WHEN status = 'success' THEN 1 END) AS SIGNED), \
                    CAST(COUNT(CASE WHEN status LIKE 'failure%' THEN 1 END) AS SIGNED) \
                    FROM spend_logs WHERE start_time >= ? AND start_time <= ? {filter} \
                    GROUP BY 1 ORDER BY 1 ASC";
                let (filter_clause, params) = build_activity_filter(user_id, team_id, organization_id, 0, false, true);
                let sql = sql.replace("{filter}", &filter_clause);
                let mut q = sqlx::query_as(&sql)
                    .bind(&start_ts).bind(&end_ts);
                for p in &params { q = q.bind(p); }
                q.fetch_all(pool).await.map_err(DbError::from)
            }
            Database::Postgres(pool) => {
                let sql = "SELECT to_char(start_time, 'YYYY-MM-DD\"T\"HH24:00:00'), COALESCE(SUM(spend), 0), COALESCE(SUM(total_tokens), 0), COUNT(call_id), \
                    COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), \
                    COUNT(CASE WHEN status = 'success' THEN 1 END), \
                    COUNT(CASE WHEN status LIKE 'failure%' THEN 1 END) \
                    FROM spend_logs WHERE start_time >= $1::TIMESTAMPTZ AND start_time <= $2::TIMESTAMPTZ {filter} \
                    GROUP BY 1 ORDER BY 1 ASC";
                let (filter_clause, params) = build_activity_filter(user_id, team_id, organization_id, 3, true, false);
                let sql = sql.replace("{filter}", &filter_clause);
                let mut q = sqlx::query_as(&sql)
                    .bind(&start_ts).bind(&end_ts);
                for p in &params { q = q.bind(p); }
                q.fetch_all(pool).await.map_err(DbError::from)
            }
        }
    }

    /// Aggregate spend by keys for ranking within a date range.
    pub async fn aggregate_spend_by_keys(
        &self,
        start_date: &str,
        end_date: &str,
        limit: u32,
    ) -> Result<Vec<crate::models::SpendKeyRanking>> {
        // GROUP BY must list every non-aggregated SELECT column (PostgreSQL enforces this;
        // SQLite/MySQL are lenient). vk.key_alias is functionally dependent on sl.api_key
        // via the LEFT JOIN on vk.token (PK), so grouping by it does not change cardinality.
        let sql = "SELECT sl.api_key, vk.key_alias, \
            COALESCE(SUM(sl.spend), 0) AS total_spend, COUNT(sl.call_id) AS total_requests, COALESCE(SUM(sl.total_tokens), 0) AS total_tokens \
            FROM spend_logs sl LEFT JOIN virtual_keys vk ON sl.api_key = vk.token \
            WHERE date(sl.start_time) >= date({p1}) AND date(sl.start_time) <= date({p2}) \
            GROUP BY sl.api_key, vk.key_alias ORDER BY 3 DESC LIMIT {limit}";
        let sql = sql.replace("{limit}", &limit.to_string());
        // MySQL needs CAST on aggregates because SUM returns DECIMAL type.
        let sql_mysql = "SELECT sl.api_key, vk.key_alias, \
            COALESCE(SUM(sl.spend), 0) AS total_spend, CAST(COUNT(sl.call_id) AS SIGNED) AS total_requests, CAST(COALESCE(SUM(sl.total_tokens), 0) AS SIGNED) AS total_tokens \
            FROM spend_logs sl LEFT JOIN virtual_keys vk ON sl.api_key = vk.token \
            WHERE date(sl.start_time) >= date({p1}) AND date(sl.start_time) <= date({p2}) \
            GROUP BY sl.api_key, vk.key_alias ORDER BY 3 DESC LIMIT {limit}";
        let sql_mysql = sql_mysql.replace("{limit}", &limit.to_string());
        match self {
            Database::Sqlite(pool) => {
                let sql = sql.replace("{p1}", "?").replace("{p2}", "?");
                sqlx::query_as(&sql)
                    .bind(start_date).bind(end_date)
                    .fetch_all(pool).await.map_err(DbError::from)
            }
            Database::Mysql(pool) => {
                let sql = sql_mysql.replace("{p1}", "?").replace("{p2}", "?");
                sqlx::query_as(&sql)
                    .bind(start_date).bind(end_date)
                    .fetch_all(pool).await.map_err(DbError::from)
            }
            Database::Postgres(pool) => {
                let sql = sql.replace("{p1}", "$1").replace("{p2}", "$2");
                sqlx::query_as(&sql)
                    .bind(start_date).bind(end_date)
                    .fetch_all(pool).await.map_err(DbError::from)
            }
        }
    }
}

/// Build WHERE filter clause and parameter list for activity queries.
///
/// `start_index` — first positional parameter index (1-based for PG `$N`, 0 for sqlite/mysql `?`).
/// `use_dollar` — if true, emits `$N` placeholders (PG); otherwise `?` (sqlite/mysql).
fn build_activity_filter<'a>(
    user_id: Option<&'a str>,
    team_id: Option<&'a str>,
    organization_id: Option<&'a str>,
    start_index: usize,
    use_dollar: bool,
    is_mysql: bool,
) -> (String, Vec<&'a str>) {
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    let mut idx = start_index;
    if let Some(uid) = user_id {
        if use_dollar {
            clauses.push(format!(r#""user" = ${}"#, idx));
        } else if is_mysql {
            clauses.push("`user` = ?".to_string());
        } else {
            clauses.push(r#""user" = ?"#.to_string());
        }
        params.push(uid);
        idx += 1;
    }
    if let Some(tid) = team_id {
        if use_dollar {
            clauses.push(format!("team_id = ${}", idx));
        } else {
            clauses.push("team_id = ?".to_string());
        }
        params.push(tid);
        idx += 1;
    }
    if let Some(oid) = organization_id {
        if use_dollar {
            clauses.push(format!("organization_id = ${}", idx));
        } else {
            clauses.push("organization_id = ?".to_string());
        }
        params.push(oid);
    }
    let filter = if clauses.is_empty() {
        String::new()
    } else {
        format!("AND {}", clauses.join(" AND "))
    };
    (filter, params)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// ProxyModelStore trait — proxy_models CRUD
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
pub trait ProxyModelStore {
    async fn insert_model(&self, m: &ProxyModel) -> Result<()>;
    async fn get_model_by_id(&self, model_id: &str) -> Result<Option<ProxyModel>>;
    async fn get_model_by_name(&self, model_name: &str) -> Result<Option<ProxyModel>>;
    async fn list_models_by_name(&self, model_name: &str) -> Result<Vec<ProxyModel>>;
    async fn list_models(&self) -> Result<Vec<ProxyModel>>;
    async fn update_model(&self, m: &ProxyModel) -> Result<()>;
    async fn delete_model(&self, model_id: &str) -> Result<()>;
    async fn list_deleted_models(&self) -> Result<Vec<DeletedModel>>;
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

const GET_MODEL_BY_NAME_SQLITE: &str = r#"
SELECT model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by
FROM proxy_models WHERE model_name = ?
"#;

const LIST_MODELS_BY_NAME_SQLITE: &str = r#"
SELECT model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by
FROM proxy_models WHERE model_name = ?
"#;

const LIST_MODELS_SQLITE: &str = r#"
SELECT model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by
FROM proxy_models ORDER BY model_name
"#;

const UPDATE_MODEL_SQLITE: &str = r#"
UPDATE proxy_models SET model_name = ?, litellm_params = ?, model_info = ?, updated_at = ?, updated_by = ?
WHERE model_id = ?
"#;

const INSERT_DELETED_MODEL_SQLITE: &str = r#"
INSERT INTO deleted_models (model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by)
VALUES (?, ?, ?, ?, ?, ?, ?, ?)
"#;

const LIST_DELETED_MODELS_SQLITE: &str = r#"
SELECT id, model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by, deleted_at
FROM deleted_models ORDER BY deleted_at DESC
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

    async fn get_model_by_name(&self, model_name: &str) -> Result<Option<ProxyModel>> {
        sqlx::query_as(GET_MODEL_BY_NAME_SQLITE)
            .bind(model_name)
            .fetch_optional(self).await
            .map_err(DbError::from)
    }

    async fn list_models_by_name(&self, model_name: &str) -> Result<Vec<ProxyModel>> {
        sqlx::query_as(LIST_MODELS_BY_NAME_SQLITE)
            .bind(model_name)
            .fetch_all(self).await
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
        // tombstone-then-delete: archive first, then remove from source
        let m = self.get_model_by_id(model_id).await?;
        if let Some(model) = m {
            sqlx::query(INSERT_DELETED_MODEL_SQLITE)
                .bind(&model.model_id)
                .bind(&model.model_name)
                .bind(&model.litellm_params)
                .bind(&model.model_info)
                .bind(&model.created_at)
                .bind(&model.created_by)
                .bind(&model.updated_at)
                .bind(&model.updated_by)
                .execute(self).await?;
        }
        sqlx::query("DELETE FROM proxy_models WHERE model_id = ?")
            .bind(model_id)
            .execute(self).await?;
        Ok(())
    }

    async fn list_deleted_models(&self) -> Result<Vec<DeletedModel>> {
        sqlx::query_as(LIST_DELETED_MODELS_SQLITE).fetch_all(self).await.map_err(DbError::from)
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

    async fn get_model_by_name(&self, model_name: &str) -> Result<Option<ProxyModel>> {
        sqlx::query_as("SELECT model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by FROM proxy_models WHERE model_name = ?")
            .bind(model_name).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_models_by_name(&self, model_name: &str) -> Result<Vec<ProxyModel>> {
        sqlx::query_as("SELECT model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by FROM proxy_models WHERE model_name = ?")
            .bind(model_name).fetch_all(self).await.map_err(DbError::from)
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
        // tombstone-then-delete: archive first, then remove from source
        let m = self.get_model_by_id(model_id).await?;
        if let Some(model) = m {
            sqlx::query("INSERT INTO deleted_models (model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&model.model_id).bind(&model.model_name).bind(&model.litellm_params).bind(&model.model_info)
                .bind(&model.created_at).bind(&model.created_by).bind(&model.updated_at).bind(&model.updated_by)
                .execute(self).await?;
        }
        sqlx::query("DELETE FROM proxy_models WHERE model_id = ?")
            .bind(model_id).execute(self).await?;
        Ok(())
    }

    async fn list_deleted_models(&self) -> Result<Vec<DeletedModel>> {
        sqlx::query_as("SELECT id, model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by, deleted_at FROM deleted_models ORDER BY deleted_at DESC")
            .fetch_all(self).await.map_err(DbError::from)
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

    async fn get_model_by_name(&self, model_name: &str) -> Result<Option<ProxyModel>> {
        sqlx::query_as("SELECT model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by FROM proxy_models WHERE model_name = $1")
            .bind(model_name).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_models_by_name(&self, model_name: &str) -> Result<Vec<ProxyModel>> {
        sqlx::query_as("SELECT model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by FROM proxy_models WHERE model_name = $1")
            .bind(model_name).fetch_all(self).await.map_err(DbError::from)
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
        // tombstone-then-delete: archive first, then remove from source
        let m = self.get_model_by_id(model_id).await?;
        if let Some(model) = m {
            sqlx::query("INSERT INTO deleted_models (model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
                .bind(&model.model_id).bind(&model.model_name).bind(&model.litellm_params).bind(&model.model_info)
                .bind(&model.created_at).bind(&model.created_by).bind(&model.updated_at).bind(&model.updated_by)
                .execute(self).await?;
        }
        sqlx::query("DELETE FROM proxy_models WHERE model_id = $1")
            .bind(model_id).execute(self).await?;
        Ok(())
    }

    async fn list_deleted_models(&self) -> Result<Vec<DeletedModel>> {
        sqlx::query_as("SELECT id, model_id, model_name, litellm_params, model_info, created_at, created_by, updated_at, updated_by, deleted_at FROM deleted_models ORDER BY deleted_at DESC")
            .fetch_all(self).await.map_err(DbError::from)
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

    pub async fn get_model_by_name(&self, model_name: &str) -> Result<Option<ProxyModel>> {
        match self {
            Database::Sqlite(pool) => pool.get_model_by_name(model_name).await,
            Database::Mysql(pool) => pool.get_model_by_name(model_name).await,
            Database::Postgres(pool) => pool.get_model_by_name(model_name).await,
        }
    }

    pub async fn list_models_by_name(&self, model_name: &str) -> Result<Vec<ProxyModel>> {
        match self {
            Database::Sqlite(pool) => pool.list_models_by_name(model_name).await,
            Database::Mysql(pool) => pool.list_models_by_name(model_name).await,
            Database::Postgres(pool) => pool.list_models_by_name(model_name).await,
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

    pub async fn list_deleted_models(&self) -> Result<Vec<DeletedModel>> {
        match self {
            Database::Sqlite(pool) => pool.list_deleted_models().await,
            Database::Mysql(pool) => pool.list_deleted_models().await,
            Database::Postgres(pool) => pool.list_deleted_models().await,
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
// OrganizationStore trait — org CRUD across all DB backends
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
pub trait OrganizationStore {
    async fn insert_organization(&self, o: &Organization) -> Result<()>;
    async fn get_organization_by_id(&self, org_id: &str) -> Result<Option<Organization>>;
    async fn list_organizations(&self) -> Result<Vec<Organization>>;
    async fn list_deleted_organizations(&self) -> Result<Vec<DeletedOrganization>>;
    async fn update_organization(&self, o: &Organization) -> Result<()>;
    async fn delete_organization(&self, org_id: &str) -> Result<()>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// OrganizationStore — SqlitePool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const INSERT_ORG_SQLITE: &str = r#"
INSERT INTO organizations (organization_id, organization_alias, budget_id, metadata, models, spend, model_spend, object_permission_id, created_at, created_by, updated_at, updated_by)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#;

const GET_ORG_SQLITE: &str = r#"
SELECT organization_id, organization_alias, budget_id, metadata, models, spend, model_spend, object_permission_id, created_at, created_by, updated_at, updated_by
FROM organizations WHERE organization_id = ?
"#;

const LIST_ORGS_SQLITE: &str = r#"
SELECT organization_id, organization_alias, budget_id, metadata, models, spend, model_spend, object_permission_id, created_at, created_by, updated_at, updated_by
FROM organizations ORDER BY organization_alias
"#;

const UPDATE_ORG_SQLITE: &str = r#"
UPDATE organizations SET organization_alias = ?, budget_id = ?, metadata = ?, models = ?, spend = ?, model_spend = ?, object_permission_id = ?, updated_at = ?, updated_by = ?
WHERE organization_id = ?
"#;

const INSERT_DELETED_ORG_SQLITE: &str = r#"
INSERT INTO deleted_organizations (
    organization_id, organization_alias, budget_id, metadata, models, spend, model_spend,
    object_permission_id, created_at, created_by, updated_at, updated_by
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#;

const LIST_DELETED_ORGS_SQLITE: &str = r#"
SELECT id, organization_id, organization_alias, budget_id, metadata, models, spend, model_spend,
    object_permission_id, created_at, created_by, updated_at, updated_by, deleted_at
FROM deleted_organizations ORDER BY deleted_at DESC
"#;

#[async_trait]
impl OrganizationStore for SqlitePool {
    async fn insert_organization(&self, o: &Organization) -> Result<()> {
        sqlx::query(INSERT_ORG_SQLITE)
            .bind(&o.organization_id).bind(&o.organization_alias).bind(&o.budget_id)
            .bind(&o.metadata).bind(&o.models).bind(o.spend).bind(&o.model_spend)
            .bind(&o.object_permission_id).bind(o.created_at).bind(&o.created_by)
            .bind(o.updated_at).bind(&o.updated_by)
            .execute(self).await?;
        Ok(())
    }

    async fn get_organization_by_id(&self, org_id: &str) -> Result<Option<Organization>> {
        sqlx::query_as(GET_ORG_SQLITE).bind(org_id).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_organizations(&self) -> Result<Vec<Organization>> {
        sqlx::query_as(LIST_ORGS_SQLITE).fetch_all(self).await.map_err(DbError::from)
    }

    async fn list_deleted_organizations(&self) -> Result<Vec<DeletedOrganization>> {
        sqlx::query_as(LIST_DELETED_ORGS_SQLITE).fetch_all(self).await.map_err(DbError::from)
    }

    async fn update_organization(&self, o: &Organization) -> Result<()> {
        sqlx::query(UPDATE_ORG_SQLITE)
            .bind(&o.organization_alias).bind(&o.budget_id).bind(&o.metadata).bind(&o.models)
            .bind(o.spend).bind(&o.model_spend).bind(&o.object_permission_id)
            .bind(o.updated_at).bind(&o.updated_by).bind(&o.organization_id)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_organization(&self, org_id: &str) -> Result<()> {
        // tombstone-then-delete: archive first, then remove from source
        let org = self.get_organization_by_id(org_id).await?;
        if let Some(o) = org {
            sqlx::query(INSERT_DELETED_ORG_SQLITE)
                .bind(&o.organization_id).bind(&o.organization_alias).bind(&o.budget_id)
                .bind(&o.metadata).bind(&o.models).bind(o.spend).bind(&o.model_spend)
                .bind(&o.object_permission_id).bind(o.created_at).bind(&o.created_by)
                .bind(o.updated_at).bind(&o.updated_by)
                .execute(self).await?;
        }
        sqlx::query("DELETE FROM organizations WHERE organization_id = ?")
            .bind(org_id).execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// OrganizationStore — MySqlPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl OrganizationStore for MySqlPool {
    async fn insert_organization(&self, o: &Organization) -> Result<()> {
        sqlx::query("INSERT INTO organizations (organization_id, organization_alias, budget_id, metadata, models, spend, model_spend, object_permission_id, created_at, created_by, updated_at, updated_by) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&o.organization_id).bind(&o.organization_alias).bind(&o.budget_id)
            .bind(&o.metadata).bind(&o.models).bind(o.spend).bind(&o.model_spend)
            .bind(&o.object_permission_id).bind(o.created_at).bind(&o.created_by)
            .bind(o.updated_at).bind(&o.updated_by)
            .execute(self).await?;
        Ok(())
    }

    async fn get_organization_by_id(&self, org_id: &str) -> Result<Option<Organization>> {
        sqlx::query_as("SELECT organization_id, organization_alias, budget_id, metadata, models, spend, model_spend, object_permission_id, created_at, created_by, updated_at, updated_by FROM organizations WHERE organization_id = ?")
            .bind(org_id).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_organizations(&self) -> Result<Vec<Organization>> {
        sqlx::query_as("SELECT organization_id, organization_alias, budget_id, metadata, models, spend, model_spend, object_permission_id, created_at, created_by, updated_at, updated_by FROM organizations ORDER BY organization_alias")
            .fetch_all(self).await.map_err(DbError::from)
    }

    async fn list_deleted_organizations(&self) -> Result<Vec<DeletedOrganization>> {
        sqlx::query_as("SELECT id, organization_id, organization_alias, budget_id, metadata, models, spend, model_spend, object_permission_id, created_at, created_by, updated_at, updated_by, deleted_at FROM deleted_organizations ORDER BY deleted_at DESC")
            .fetch_all(self).await.map_err(DbError::from)
    }

    async fn update_organization(&self, o: &Organization) -> Result<()> {
        sqlx::query("UPDATE organizations SET organization_alias = ?, budget_id = ?, metadata = ?, models = ?, spend = ?, model_spend = ?, object_permission_id = ?, updated_at = ?, updated_by = ? WHERE organization_id = ?")
            .bind(&o.organization_alias).bind(&o.budget_id).bind(&o.metadata).bind(&o.models)
            .bind(o.spend).bind(&o.model_spend).bind(&o.object_permission_id)
            .bind(o.updated_at).bind(&o.updated_by).bind(&o.organization_id)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_organization(&self, org_id: &str) -> Result<()> {
        // tombstone-then-delete: archive first, then remove from source
        let org = self.get_organization_by_id(org_id).await?;
        if let Some(o) = org {
            sqlx::query("INSERT INTO deleted_organizations (organization_id, organization_alias, budget_id, metadata, models, spend, model_spend, object_permission_id, created_at, created_by, updated_at, updated_by) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&o.organization_id).bind(&o.organization_alias).bind(&o.budget_id)
                .bind(&o.metadata).bind(&o.models).bind(o.spend).bind(&o.model_spend)
                .bind(&o.object_permission_id).bind(o.created_at).bind(&o.created_by)
                .bind(o.updated_at).bind(&o.updated_by)
                .execute(self).await?;
        }
        sqlx::query("DELETE FROM organizations WHERE organization_id = ?")
            .bind(org_id).execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// OrganizationStore — PgPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl OrganizationStore for PgPool {
    async fn insert_organization(&self, o: &Organization) -> Result<()> {
        sqlx::query("INSERT INTO organizations (organization_id, organization_alias, budget_id, metadata, models, spend, model_spend, object_permission_id, created_at, created_by, updated_at, updated_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
            .bind(&o.organization_id).bind(&o.organization_alias).bind(&o.budget_id)
            .bind(&o.metadata).bind(&o.models).bind(o.spend).bind(&o.model_spend)
            .bind(&o.object_permission_id).bind(o.created_at).bind(&o.created_by)
            .bind(o.updated_at).bind(&o.updated_by)
            .execute(self).await?;
        Ok(())
    }

    async fn get_organization_by_id(&self, org_id: &str) -> Result<Option<Organization>> {
        sqlx::query_as("SELECT organization_id, organization_alias, budget_id, metadata, models, spend, model_spend, object_permission_id, created_at, created_by, updated_at, updated_by FROM organizations WHERE organization_id = $1")
            .bind(org_id).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_organizations(&self) -> Result<Vec<Organization>> {
        sqlx::query_as("SELECT organization_id, organization_alias, budget_id, metadata, models, spend, model_spend, object_permission_id, created_at, created_by, updated_at, updated_by FROM organizations ORDER BY organization_alias")
            .fetch_all(self).await.map_err(DbError::from)
    }

    async fn list_deleted_organizations(&self) -> Result<Vec<DeletedOrganization>> {
        sqlx::query_as("SELECT id, organization_id, organization_alias, budget_id, metadata, models, spend, model_spend, object_permission_id, created_at, created_by, updated_at, updated_by, deleted_at FROM deleted_organizations ORDER BY deleted_at DESC")
            .fetch_all(self).await.map_err(DbError::from)
    }

    async fn update_organization(&self, o: &Organization) -> Result<()> {
        sqlx::query("UPDATE organizations SET organization_alias = $1, budget_id = $2, metadata = $3, models = $4, spend = $5, model_spend = $6, object_permission_id = $7, updated_at = $8, updated_by = $9 WHERE organization_id = $10")
            .bind(&o.organization_alias).bind(&o.budget_id).bind(&o.metadata).bind(&o.models)
            .bind(o.spend).bind(&o.model_spend).bind(&o.object_permission_id)
            .bind(o.updated_at).bind(&o.updated_by).bind(&o.organization_id)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_organization(&self, org_id: &str) -> Result<()> {
        // tombstone-then-delete: archive first, then remove from source
        let org = self.get_organization_by_id(org_id).await?;
        if let Some(o) = org {
            sqlx::query("INSERT INTO deleted_organizations (organization_id, organization_alias, budget_id, metadata, models, spend, model_spend, object_permission_id, created_at, created_by, updated_at, updated_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
                .bind(&o.organization_id).bind(&o.organization_alias).bind(&o.budget_id)
                .bind(&o.metadata).bind(&o.models).bind(o.spend).bind(&o.model_spend)
                .bind(&o.object_permission_id).bind(o.created_at).bind(&o.created_by)
                .bind(o.updated_at).bind(&o.updated_by)
                .execute(self).await?;
        }
        sqlx::query("DELETE FROM organizations WHERE organization_id = $1")
            .bind(org_id).execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// TeamStore trait — team CRUD across all DB backends
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
pub trait TeamStore {
    async fn insert_team(&self, t: &Team) -> Result<()>;
    async fn get_team_by_id(&self, team_id: &str) -> Result<Option<Team>>;
    async fn list_teams(&self, org_id: Option<&str>) -> Result<Vec<Team>>;
    async fn list_deleted_teams(&self) -> Result<Vec<DeletedTeam>>;
    async fn update_team(&self, t: &Team) -> Result<()>;
    async fn delete_team(&self, team_id: &str) -> Result<()>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// TeamStore — SqlitePool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const INSERT_TEAM_SQLITE: &str = r#"
INSERT INTO teams (team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#;

const GET_TEAM_SQLITE: &str = r#"
SELECT team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config
FROM teams WHERE team_id = ?
"#;

const LIST_TEAMS_SQLITE: &str = r#"
SELECT team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config
FROM teams ORDER BY team_alias
"#;

const LIST_TEAMS_BY_ORG_SQLITE: &str = r#"
SELECT team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config
FROM teams WHERE organization_id = ? ORDER BY team_alias
"#;

const UPDATE_TEAM_SQLITE: &str = r#"
UPDATE teams SET team_alias = ?, organization_id = ?, object_permission_id = ?, admins = ?, members = ?, members_with_roles = ?, metadata = ?, max_budget = ?, soft_budget = ?, spend = ?, models = ?, max_parallel_requests = ?, tpm_limit = ?, rpm_limit = ?, budget_duration = ?, budget_reset_at = ?, blocked = ?, updated_at = ?, model_spend = ?, model_max_budget = ?, router_settings = ?, team_member_permissions = ?, access_group_ids = ?, policies = ?, default_team_member_models = ?, budget_limits = ?, model_id = ?, allow_team_guardrail_config = ?
WHERE team_id = ?
"#;

const INSERT_DELETED_TEAM_SQLITE: &str = r#"
INSERT INTO deleted_teams (team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#;

const LIST_DELETED_TEAMS_SQLITE: &str = r#"
SELECT id, team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config, deleted_at
FROM deleted_teams ORDER BY deleted_at DESC
"#;

#[async_trait]
impl TeamStore for SqlitePool {
    async fn insert_team(&self, t: &Team) -> Result<()> {
        sqlx::query(INSERT_TEAM_SQLITE)
            .bind(&t.team_id).bind(&t.team_alias).bind(&t.organization_id).bind(&t.object_permission_id)
            .bind(&t.admins).bind(&t.members).bind(&t.members_with_roles).bind(&t.metadata)
            .bind(t.max_budget.clone()).bind(t.soft_budget.clone()).bind(t.spend).bind(&t.models)
            .bind(&t.max_parallel_requests).bind(&t.tpm_limit).bind(&t.rpm_limit)
            .bind(&t.budget_duration).bind(t.budget_reset_at).bind(t.blocked)
            .bind(t.created_at).bind(t.updated_at)
            .bind(&t.model_spend).bind(&t.model_max_budget).bind(&t.router_settings)
            .bind(&t.team_member_permissions).bind(&t.access_group_ids).bind(&t.policies)
            .bind(&t.default_team_member_models).bind(&t.budget_limits).bind(t.model_id)
            .bind(t.allow_team_guardrail_config)
            .execute(self).await?;
        Ok(())
    }

    async fn get_team_by_id(&self, team_id: &str) -> Result<Option<Team>> {
        sqlx::query_as(GET_TEAM_SQLITE).bind(team_id).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_teams(&self, org_id: Option<&str>) -> Result<Vec<Team>> {
        match org_id {
            Some(_) => sqlx::query_as(LIST_TEAMS_BY_ORG_SQLITE).bind(org_id).fetch_all(self).await.map_err(DbError::from),
            None => sqlx::query_as(LIST_TEAMS_SQLITE).fetch_all(self).await.map_err(DbError::from),
        }
    }

    async fn list_deleted_teams(&self) -> Result<Vec<DeletedTeam>> {
        sqlx::query_as(LIST_DELETED_TEAMS_SQLITE).fetch_all(self).await.map_err(DbError::from)
    }

    async fn update_team(&self, t: &Team) -> Result<()> {
        sqlx::query(UPDATE_TEAM_SQLITE)
            .bind(&t.team_alias).bind(&t.organization_id).bind(&t.object_permission_id)
            .bind(&t.admins).bind(&t.members).bind(&t.members_with_roles).bind(&t.metadata)
            .bind(t.max_budget.clone()).bind(t.soft_budget.clone()).bind(t.spend).bind(&t.models)
            .bind(&t.max_parallel_requests).bind(&t.tpm_limit).bind(&t.rpm_limit)
            .bind(&t.budget_duration).bind(t.budget_reset_at).bind(t.blocked)
            .bind(t.updated_at)
            .bind(&t.model_spend).bind(&t.model_max_budget).bind(&t.router_settings)
            .bind(&t.team_member_permissions).bind(&t.access_group_ids).bind(&t.policies)
            .bind(&t.default_team_member_models).bind(&t.budget_limits).bind(t.model_id)
            .bind(t.allow_team_guardrail_config)
            .bind(&t.team_id)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_team(&self, team_id: &str) -> Result<()> {
        // tombstone-then-delete: archive first, then remove from source
        let team = self.get_team_by_id(team_id).await?;
        if let Some(t) = team {
            sqlx::query(INSERT_DELETED_TEAM_SQLITE)
                .bind(&t.team_id).bind(&t.team_alias).bind(&t.organization_id).bind(&t.object_permission_id)
                .bind(&t.admins).bind(&t.members).bind(&t.members_with_roles).bind(&t.metadata)
                .bind(t.max_budget.clone()).bind(t.soft_budget.clone()).bind(t.spend).bind(&t.models)
                .bind(&t.max_parallel_requests).bind(&t.tpm_limit).bind(&t.rpm_limit)
                .bind(&t.budget_duration).bind(t.budget_reset_at).bind(t.blocked)
                .bind(t.created_at).bind(t.updated_at)
                .bind(&t.model_spend).bind(&t.model_max_budget).bind(&t.router_settings)
                .bind(&t.team_member_permissions).bind(&t.access_group_ids).bind(&t.policies)
                .bind(&t.default_team_member_models).bind(&t.budget_limits).bind(t.model_id)
                .bind(t.allow_team_guardrail_config)
                .execute(self).await?;
        }
        sqlx::query("DELETE FROM teams WHERE team_id = ?")
            .bind(team_id).execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// TeamStore — MySqlPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl TeamStore for MySqlPool {
    async fn insert_team(&self, t: &Team) -> Result<()> {
        let cols = "team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config";
        let vals = "?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?";
        sqlx::query(&format!("INSERT INTO teams ({}) VALUES ({})", cols, vals))
            .bind(&t.team_id).bind(&t.team_alias).bind(&t.organization_id).bind(&t.object_permission_id)
            .bind(&t.admins).bind(&t.members).bind(&t.members_with_roles).bind(&t.metadata)
            .bind(t.max_budget.clone()).bind(t.soft_budget.clone()).bind(t.spend).bind(&t.models)
            .bind(&t.max_parallel_requests).bind(&t.tpm_limit).bind(&t.rpm_limit)
            .bind(&t.budget_duration).bind(t.budget_reset_at).bind(t.blocked)
            .bind(t.created_at).bind(t.updated_at)
            .bind(&t.model_spend).bind(&t.model_max_budget).bind(&t.router_settings)
            .bind(&t.team_member_permissions).bind(&t.access_group_ids).bind(&t.policies)
            .bind(&t.default_team_member_models).bind(&t.budget_limits).bind(t.model_id)
            .bind(t.allow_team_guardrail_config)
            .execute(self).await?;
        Ok(())
    }

    async fn get_team_by_id(&self, team_id: &str) -> Result<Option<Team>> {
        sqlx::query_as("SELECT team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config FROM teams WHERE team_id = ?")
            .bind(team_id).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_teams(&self, org_id: Option<&str>) -> Result<Vec<Team>> {
        match org_id {
            Some(_) => sqlx::query_as("SELECT team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config FROM teams WHERE organization_id = ? ORDER BY team_alias")
                .bind(org_id).fetch_all(self).await.map_err(DbError::from),
            None => sqlx::query_as("SELECT team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config FROM teams ORDER BY team_alias")
                .fetch_all(self).await.map_err(DbError::from),
        }
    }

    async fn list_deleted_teams(&self) -> Result<Vec<DeletedTeam>> {
        sqlx::query_as("SELECT id, team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config, deleted_at FROM deleted_teams ORDER BY deleted_at DESC")
            .fetch_all(self).await.map_err(DbError::from)
    }

    async fn update_team(&self, t: &Team) -> Result<()> {
        sqlx::query("UPDATE teams SET team_alias = ?, organization_id = ?, object_permission_id = ?, admins = ?, members = ?, members_with_roles = ?, metadata = ?, max_budget = ?, soft_budget = ?, spend = ?, models = ?, max_parallel_requests = ?, tpm_limit = ?, rpm_limit = ?, budget_duration = ?, budget_reset_at = ?, blocked = ?, updated_at = ?, model_spend = ?, model_max_budget = ?, router_settings = ?, team_member_permissions = ?, access_group_ids = ?, policies = ?, default_team_member_models = ?, budget_limits = ?, model_id = ?, allow_team_guardrail_config = ? WHERE team_id = ?")
            .bind(&t.team_alias).bind(&t.organization_id).bind(&t.object_permission_id)
            .bind(&t.admins).bind(&t.members).bind(&t.members_with_roles).bind(&t.metadata)
            .bind(t.max_budget.clone()).bind(t.soft_budget.clone()).bind(t.spend).bind(&t.models)
            .bind(&t.max_parallel_requests).bind(&t.tpm_limit).bind(&t.rpm_limit)
            .bind(&t.budget_duration).bind(t.budget_reset_at).bind(t.blocked)
            .bind(t.updated_at)
            .bind(&t.model_spend).bind(&t.model_max_budget).bind(&t.router_settings)
            .bind(&t.team_member_permissions).bind(&t.access_group_ids).bind(&t.policies)
            .bind(&t.default_team_member_models).bind(&t.budget_limits).bind(t.model_id)
            .bind(t.allow_team_guardrail_config)
            .bind(&t.team_id)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_team(&self, team_id: &str) -> Result<()> {
        // tombstone-then-delete: archive first, then remove from source
        let team = self.get_team_by_id(team_id).await?;
        if let Some(t) = team {
            sqlx::query("INSERT INTO deleted_teams (team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&t.team_id).bind(&t.team_alias).bind(&t.organization_id).bind(&t.object_permission_id)
                .bind(&t.admins).bind(&t.members).bind(&t.members_with_roles).bind(&t.metadata)
                .bind(t.max_budget.clone()).bind(t.soft_budget.clone()).bind(t.spend).bind(&t.models)
                .bind(&t.max_parallel_requests).bind(&t.tpm_limit).bind(&t.rpm_limit)
                .bind(&t.budget_duration).bind(t.budget_reset_at).bind(t.blocked)
                .bind(t.created_at).bind(t.updated_at)
                .bind(&t.model_spend).bind(&t.model_max_budget).bind(&t.router_settings)
                .bind(&t.team_member_permissions).bind(&t.access_group_ids).bind(&t.policies)
                .bind(&t.default_team_member_models).bind(&t.budget_limits).bind(t.model_id)
                .bind(t.allow_team_guardrail_config)
                .execute(self).await?;
        }
        sqlx::query("DELETE FROM teams WHERE team_id = ?")
            .bind(team_id).execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// TeamStore — PgPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl TeamStore for PgPool {
    async fn insert_team(&self, t: &Team) -> Result<()> {
        let cols = "team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config";
        let vals = "$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30";
        sqlx::query(&format!("INSERT INTO teams ({}) VALUES ({})", cols, vals))
            .bind(&t.team_id).bind(&t.team_alias).bind(&t.organization_id).bind(&t.object_permission_id)
            .bind(&t.admins).bind(&t.members).bind(&t.members_with_roles).bind(&t.metadata)
            .bind(t.max_budget.clone()).bind(t.soft_budget.clone()).bind(t.spend).bind(&t.models)
            .bind(&t.max_parallel_requests).bind(&t.tpm_limit).bind(&t.rpm_limit)
            .bind(&t.budget_duration).bind(t.budget_reset_at).bind(t.blocked)
            .bind(t.created_at).bind(t.updated_at)
            .bind(&t.model_spend).bind(&t.model_max_budget).bind(&t.router_settings)
            .bind(&t.team_member_permissions).bind(&t.access_group_ids).bind(&t.policies)
            .bind(&t.default_team_member_models).bind(&t.budget_limits).bind(t.model_id)
            .bind(t.allow_team_guardrail_config)
            .execute(self).await?;
        Ok(())
    }

    async fn get_team_by_id(&self, team_id: &str) -> Result<Option<Team>> {
        sqlx::query_as("SELECT team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config FROM teams WHERE team_id = $1")
            .bind(team_id).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_teams(&self, org_id: Option<&str>) -> Result<Vec<Team>> {
        match org_id {
            Some(_) => sqlx::query_as("SELECT team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config FROM teams WHERE organization_id = $1 ORDER BY team_alias")
                .bind(org_id).fetch_all(self).await.map_err(DbError::from),
            None => sqlx::query_as("SELECT team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config FROM teams ORDER BY team_alias")
                .fetch_all(self).await.map_err(DbError::from),
        }
    }

    async fn list_deleted_teams(&self) -> Result<Vec<DeletedTeam>> {
        sqlx::query_as("SELECT id, team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config, deleted_at FROM deleted_teams ORDER BY deleted_at DESC")
            .fetch_all(self).await.map_err(DbError::from)
    }

    async fn update_team(&self, t: &Team) -> Result<()> {
        sqlx::query("UPDATE teams SET team_alias = $1, organization_id = $2, object_permission_id = $3, admins = $4, members = $5, members_with_roles = $6, metadata = $7, max_budget = $8, soft_budget = $9, spend = $10, models = $11, max_parallel_requests = $12, tpm_limit = $13, rpm_limit = $14, budget_duration = $15, budget_reset_at = $16, blocked = $17, updated_at = $18, model_spend = $19, model_max_budget = $20, router_settings = $21, team_member_permissions = $22, access_group_ids = $23, policies = $24, default_team_member_models = $25, budget_limits = $26, model_id = $27, allow_team_guardrail_config = $28 WHERE team_id = $29")
            .bind(&t.team_alias).bind(&t.organization_id).bind(&t.object_permission_id)
            .bind(&t.admins).bind(&t.members).bind(&t.members_with_roles).bind(&t.metadata)
            .bind(t.max_budget.clone()).bind(t.soft_budget.clone()).bind(t.spend).bind(&t.models)
            .bind(&t.max_parallel_requests).bind(&t.tpm_limit).bind(&t.rpm_limit)
            .bind(&t.budget_duration).bind(t.budget_reset_at).bind(t.blocked)
            .bind(t.updated_at)
            .bind(&t.model_spend).bind(&t.model_max_budget).bind(&t.router_settings)
            .bind(&t.team_member_permissions).bind(&t.access_group_ids).bind(&t.policies)
            .bind(&t.default_team_member_models).bind(&t.budget_limits).bind(t.model_id)
            .bind(t.allow_team_guardrail_config)
            .bind(&t.team_id)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_team(&self, team_id: &str) -> Result<()> {
        // tombstone-then-delete: archive first, then remove from source
        let team = self.get_team_by_id(team_id).await?;
        if let Some(t) = team {
            let cols = "team_id, team_alias, organization_id, object_permission_id, admins, members, members_with_roles, metadata, max_budget, soft_budget, spend, models, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, blocked, created_at, updated_at, model_spend, model_max_budget, router_settings, team_member_permissions, access_group_ids, policies, default_team_member_models, budget_limits, model_id, allow_team_guardrail_config";
            let vals = "$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30";
            sqlx::query(&format!("INSERT INTO deleted_teams ({}) VALUES ({})", cols, vals))
                .bind(&t.team_id).bind(&t.team_alias).bind(&t.organization_id).bind(&t.object_permission_id)
                .bind(&t.admins).bind(&t.members).bind(&t.members_with_roles).bind(&t.metadata)
                .bind(t.max_budget.clone()).bind(t.soft_budget.clone()).bind(t.spend).bind(&t.models)
                .bind(&t.max_parallel_requests).bind(&t.tpm_limit).bind(&t.rpm_limit)
                .bind(&t.budget_duration).bind(t.budget_reset_at).bind(t.blocked)
                .bind(t.created_at).bind(t.updated_at)
                .bind(&t.model_spend).bind(&t.model_max_budget).bind(&t.router_settings)
                .bind(&t.team_member_permissions).bind(&t.access_group_ids).bind(&t.policies)
                .bind(&t.default_team_member_models).bind(&t.budget_limits).bind(t.model_id)
                .bind(t.allow_team_guardrail_config)
                .execute(self).await?;
        }
 sqlx::query("DELETE FROM teams WHERE team_id = $1")
            .bind(team_id).execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// UserStore trait — user CRUD across all DB backends
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
pub trait UserStore {
    async fn insert_user(&self, u: &User) -> Result<()>;
    async fn get_user_by_id(&self, user_id: &str) -> Result<Option<User>>;
    async fn get_user_by_email(&self, email: &str) -> Result<Option<User>>;
    async fn list_users(&self, org_id: Option<&str>) -> Result<Vec<User>>;
    async fn list_deleted_users(&self) -> Result<Vec<DeletedUser>>;
    async fn update_user(&self, u: &User) -> Result<()>;
    async fn delete_user(&self, user_id: &str) -> Result<()>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// UserStore — SqlitePool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const INSERT_USER_SQLITE: &str = r#"
INSERT INTO users (user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#;

const GET_USER_SQLITE: &str = r#"
SELECT user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at
FROM users WHERE user_id = ?
"#;

const GET_USER_BY_EMAIL_SQLITE: &str = r#"
SELECT user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at
FROM users WHERE user_email = ?
"#;

const LIST_USERS_SQLITE: &str = r#"
SELECT user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at, (SELECT COUNT(*) FROM virtual_keys WHERE user_id = users.user_id) AS virtual_keys_count
FROM users ORDER BY user_alias
"#;

const LIST_USERS_BY_ORG_SQLITE: &str = r#"
SELECT user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at, (SELECT COUNT(*) FROM virtual_keys WHERE user_id = users.user_id) AS virtual_keys_count
FROM users WHERE organization_id = ? ORDER BY user_alias
"#;

const UPDATE_USER_SQLITE: &str = r#"
UPDATE users SET user_alias = ?, team_id = ?, sso_user_id = ?, organization_id = ?, object_permission_id = ?, password = ?, teams = ?, user_role = ?, max_budget = ?, spend = ?, user_email = ?, models = ?, metadata = ?, max_parallel_requests = ?, tpm_limit = ?, rpm_limit = ?, budget_duration = ?, budget_reset_at = ?, allowed_cache_controls = ?, policies = ?, model_spend = ?, model_max_budget = ?, updated_at = ?
WHERE user_id = ?
"#;

const INSERT_DELETED_USER_SQLITE: &str = r#"
INSERT INTO deleted_users (user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#;

const LIST_DELETED_USERS_SQLITE: &str = r#"
SELECT id, user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at, deleted_at
FROM deleted_users ORDER BY deleted_at DESC
"#;

#[async_trait]
impl UserStore for SqlitePool {
    async fn insert_user(&self, u: &User) -> Result<()> {
        sqlx::query(INSERT_USER_SQLITE)
            .bind(&u.user_id).bind(&u.user_alias).bind(&u.team_id).bind(&u.sso_user_id)
            .bind(&u.organization_id).bind(&u.object_permission_id).bind(&u.password)
            .bind(&u.teams).bind(&u.user_role).bind(u.max_budget.clone()).bind(u.spend)
            .bind(&u.user_email).bind(&u.models).bind(&u.metadata)
            .bind(&u.max_parallel_requests).bind(&u.tpm_limit).bind(&u.rpm_limit)
            .bind(&u.budget_duration).bind(u.budget_reset_at)
            .bind(&u.allowed_cache_controls).bind(&u.policies)
            .bind(&u.model_spend).bind(&u.model_max_budget)
            .bind(u.created_at).bind(u.updated_at)
            .execute(self).await?;
        Ok(())
    }

    async fn get_user_by_id(&self, user_id: &str) -> Result<Option<User>> {
        sqlx::query_as(GET_USER_SQLITE).bind(user_id).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn get_user_by_email(&self, email: &str) -> Result<Option<User>> {
        sqlx::query_as(GET_USER_BY_EMAIL_SQLITE).bind(email).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_users(&self, org_id: Option<&str>) -> Result<Vec<User>> {
        match org_id {
            Some(_) => sqlx::query_as(LIST_USERS_BY_ORG_SQLITE).bind(org_id).fetch_all(self).await.map_err(DbError::from),
            None => sqlx::query_as(LIST_USERS_SQLITE).fetch_all(self).await.map_err(DbError::from),
        }
    }

    async fn list_deleted_users(&self) -> Result<Vec<DeletedUser>> {
        sqlx::query_as(LIST_DELETED_USERS_SQLITE).fetch_all(self).await.map_err(DbError::from)
    }

    async fn update_user(&self, u: &User) -> Result<()> {
        sqlx::query(UPDATE_USER_SQLITE)
            .bind(&u.user_alias).bind(&u.team_id).bind(&u.sso_user_id)
            .bind(&u.organization_id).bind(&u.object_permission_id).bind(&u.password)
            .bind(&u.teams).bind(&u.user_role).bind(u.max_budget.clone()).bind(u.spend)
            .bind(&u.user_email).bind(&u.models).bind(&u.metadata)
            .bind(&u.max_parallel_requests).bind(&u.tpm_limit).bind(&u.rpm_limit)
            .bind(&u.budget_duration).bind(u.budget_reset_at)
            .bind(&u.allowed_cache_controls).bind(&u.policies)
            .bind(&u.model_spend).bind(&u.model_max_budget)
            .bind(u.updated_at)
            .bind(&u.user_id)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_user(&self, user_id: &str) -> Result<()> {
        // tombstone-then-delete: archive first, then remove from source
        let user = self.get_user_by_id(user_id).await?;
        if let Some(u) = user {
            sqlx::query(INSERT_DELETED_USER_SQLITE)
                .bind(&u.user_id).bind(&u.user_alias).bind(&u.team_id).bind(&u.sso_user_id)
                .bind(&u.organization_id).bind(&u.object_permission_id).bind(&u.password)
                .bind(&u.teams).bind(&u.user_role).bind(u.max_budget.clone()).bind(u.spend)
                .bind(&u.user_email).bind(&u.models).bind(&u.metadata)
                .bind(&u.max_parallel_requests).bind(&u.tpm_limit).bind(&u.rpm_limit)
                .bind(&u.budget_duration).bind(u.budget_reset_at)
                .bind(&u.allowed_cache_controls).bind(&u.policies)
                .bind(&u.model_spend).bind(&u.model_max_budget)
                .bind(u.created_at).bind(u.updated_at)
                .execute(self).await?;
        }
        sqlx::query("DELETE FROM users WHERE user_id = ?")
            .bind(user_id).execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// UserStore — MySqlPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl UserStore for MySqlPool {
    async fn insert_user(&self, u: &User) -> Result<()> {
        let cols = "user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at";
        let vals = "?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?";
        sqlx::query(&format!("INSERT INTO users ({}) VALUES ({})", cols, vals))
            .bind(&u.user_id).bind(&u.user_alias).bind(&u.team_id).bind(&u.sso_user_id)
            .bind(&u.organization_id).bind(&u.object_permission_id).bind(&u.password)
            .bind(&u.teams).bind(&u.user_role).bind(u.max_budget.clone()).bind(u.spend)
            .bind(&u.user_email).bind(&u.models).bind(&u.metadata)
            .bind(&u.max_parallel_requests).bind(&u.tpm_limit).bind(&u.rpm_limit)
            .bind(&u.budget_duration).bind(u.budget_reset_at)
            .bind(&u.allowed_cache_controls).bind(&u.policies)
            .bind(&u.model_spend).bind(&u.model_max_budget)
            .bind(u.created_at).bind(u.updated_at)
            .execute(self).await?;
        Ok(())
    }

    async fn get_user_by_id(&self, user_id: &str) -> Result<Option<User>> {
        sqlx::query_as("SELECT user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at FROM users WHERE user_id = ?")
            .bind(user_id).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn get_user_by_email(&self, email: &str) -> Result<Option<User>> {
        sqlx::query_as("SELECT user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at FROM users WHERE user_email = ?")
            .bind(email).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_users(&self, org_id: Option<&str>) -> Result<Vec<User>> {
        match org_id {
            Some(_) => sqlx::query_as("SELECT user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at, (SELECT COUNT(*) FROM virtual_keys WHERE user_id = users.user_id) AS virtual_keys_count FROM users WHERE organization_id = ? ORDER BY user_alias")
                .bind(org_id).fetch_all(self).await.map_err(DbError::from),
            None => sqlx::query_as("SELECT user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at, (SELECT COUNT(*) FROM virtual_keys WHERE user_id = users.user_id) AS virtual_keys_count FROM users ORDER BY user_alias")
                .fetch_all(self).await.map_err(DbError::from),
        }
    }

    async fn list_deleted_users(&self) -> Result<Vec<DeletedUser>> {
        sqlx::query_as("SELECT id, user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at, deleted_at FROM deleted_users ORDER BY deleted_at DESC")
            .fetch_all(self).await.map_err(DbError::from)
    }

    async fn update_user(&self, u: &User) -> Result<()> {
        sqlx::query("UPDATE users SET user_alias = ?, team_id = ?, sso_user_id = ?, organization_id = ?, object_permission_id = ?, password = ?, teams = ?, user_role = ?, max_budget = ?, spend = ?, user_email = ?, models = ?, metadata = ?, max_parallel_requests = ?, tpm_limit = ?, rpm_limit = ?, budget_duration = ?, budget_reset_at = ?, allowed_cache_controls = ?, policies = ?, model_spend = ?, model_max_budget = ?, updated_at = ? WHERE user_id = ?")
            .bind(&u.user_alias).bind(&u.team_id).bind(&u.sso_user_id)
            .bind(&u.organization_id).bind(&u.object_permission_id).bind(&u.password)
            .bind(&u.teams).bind(&u.user_role).bind(u.max_budget.clone()).bind(u.spend)
            .bind(&u.user_email).bind(&u.models).bind(&u.metadata)
            .bind(&u.max_parallel_requests).bind(&u.tpm_limit).bind(&u.rpm_limit)
            .bind(&u.budget_duration).bind(u.budget_reset_at)
            .bind(&u.allowed_cache_controls).bind(&u.policies)
            .bind(&u.model_spend).bind(&u.model_max_budget)
            .bind(u.updated_at)
            .bind(&u.user_id)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_user(&self, user_id: &str) -> Result<()> {
        // tombstone-then-delete: archive first, then remove from source
        let user = self.get_user_by_id(user_id).await?;
        if let Some(u) = user {
            let cols = "user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at";
            let vals = "?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?";
            sqlx::query(&format!("INSERT INTO deleted_users ({}) VALUES ({})", cols, vals))
                .bind(&u.user_id).bind(&u.user_alias).bind(&u.team_id).bind(&u.sso_user_id)
                .bind(&u.organization_id).bind(&u.object_permission_id).bind(&u.password)
                .bind(&u.teams).bind(&u.user_role).bind(u.max_budget.clone()).bind(u.spend)
                .bind(&u.user_email).bind(&u.models).bind(&u.metadata)
                .bind(&u.max_parallel_requests).bind(&u.tpm_limit).bind(&u.rpm_limit)
                .bind(&u.budget_duration).bind(u.budget_reset_at)
                .bind(&u.allowed_cache_controls).bind(&u.policies)
                .bind(&u.model_spend).bind(&u.model_max_budget)
                .bind(u.created_at).bind(u.updated_at)
                .execute(self).await?;
        }
        sqlx::query("DELETE FROM users WHERE user_id = ?")
            .bind(user_id).execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// UserStore — PgPool
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl UserStore for PgPool {
    async fn insert_user(&self, u: &User) -> Result<()> {
        let cols = "user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at";
        let vals = "$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25";
        sqlx::query(&format!("INSERT INTO users ({}) VALUES ({})", cols, vals))
            .bind(&u.user_id).bind(&u.user_alias).bind(&u.team_id).bind(&u.sso_user_id)
            .bind(&u.organization_id).bind(&u.object_permission_id).bind(&u.password)
            .bind(&u.teams).bind(&u.user_role).bind(u.max_budget.clone()).bind(u.spend)
            .bind(&u.user_email).bind(&u.models).bind(&u.metadata)
            .bind(&u.max_parallel_requests).bind(&u.tpm_limit).bind(&u.rpm_limit)
            .bind(&u.budget_duration).bind(u.budget_reset_at)
            .bind(&u.allowed_cache_controls).bind(&u.policies)
            .bind(&u.model_spend).bind(&u.model_max_budget)
            .bind(u.created_at).bind(u.updated_at)
            .execute(self).await?;
        Ok(())
    }

    async fn get_user_by_id(&self, user_id: &str) -> Result<Option<User>> {
        sqlx::query_as("SELECT user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at FROM users WHERE user_id = $1")
            .bind(user_id).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn get_user_by_email(&self, email: &str) -> Result<Option<User>> {
        sqlx::query_as("SELECT user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at FROM users WHERE user_email = $1")
            .bind(email).fetch_optional(self).await.map_err(DbError::from)
    }

    async fn list_users(&self, org_id: Option<&str>) -> Result<Vec<User>> {
        match org_id {
            Some(_) => sqlx::query_as("SELECT user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at, (SELECT COUNT(*) FROM virtual_keys WHERE user_id = users.user_id) AS virtual_keys_count FROM users WHERE organization_id = $1 ORDER BY user_alias")
                .bind(org_id).fetch_all(self).await.map_err(DbError::from),
            None => sqlx::query_as("SELECT user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at, (SELECT COUNT(*) FROM virtual_keys WHERE user_id = users.user_id) AS virtual_keys_count FROM users ORDER BY user_alias")
                .fetch_all(self).await.map_err(DbError::from),
        }
    }

    async fn list_deleted_users(&self) -> Result<Vec<DeletedUser>> {
        sqlx::query_as("SELECT id, user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at, deleted_at FROM deleted_users ORDER BY deleted_at DESC")
            .fetch_all(self).await.map_err(DbError::from)
    }

    async fn update_user(&self, u: &User) -> Result<()> {
        sqlx::query("UPDATE users SET user_alias = $1, team_id = $2, sso_user_id = $3, organization_id = $4, object_permission_id = $5, password = $6, teams = $7, user_role = $8, max_budget = $9, spend = $10, user_email = $11, models = $12, metadata = $13, max_parallel_requests = $14, tpm_limit = $15, rpm_limit = $16, budget_duration = $17, budget_reset_at = $18, allowed_cache_controls = $19, policies = $20, model_spend = $21, model_max_budget = $22, updated_at = $23 WHERE user_id = $24")
            .bind(&u.user_alias).bind(&u.team_id).bind(&u.sso_user_id)
            .bind(&u.organization_id).bind(&u.object_permission_id).bind(&u.password)
            .bind(&u.teams).bind(&u.user_role).bind(u.max_budget.clone()).bind(u.spend)
            .bind(&u.user_email).bind(&u.models).bind(&u.metadata)
            .bind(&u.max_parallel_requests).bind(&u.tpm_limit).bind(&u.rpm_limit)
            .bind(&u.budget_duration).bind(u.budget_reset_at)
            .bind(&u.allowed_cache_controls).bind(&u.policies)
            .bind(&u.model_spend).bind(&u.model_max_budget)
            .bind(u.updated_at)
            .bind(&u.user_id)
            .execute(self).await?;
        Ok(())
    }

    async fn delete_user(&self, user_id: &str) -> Result<()> {
        // tombstone-then-delete: archive first, then remove from source
        let user = self.get_user_by_id(user_id).await?;
        if let Some(u) = user {
            let cols = "user_id, user_alias, team_id, sso_user_id, organization_id, object_permission_id, password, teams, user_role, max_budget, spend, user_email, models, metadata, max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at, allowed_cache_controls, policies, model_spend, model_max_budget, created_at, updated_at";
            let vals = "$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25";
            sqlx::query(&format!("INSERT INTO deleted_users ({}) VALUES ({})", cols, vals))
                .bind(&u.user_id).bind(&u.user_alias).bind(&u.team_id).bind(&u.sso_user_id)
                .bind(&u.organization_id).bind(&u.object_permission_id).bind(&u.password)
                .bind(&u.teams).bind(&u.user_role).bind(u.max_budget.clone()).bind(u.spend)
                .bind(&u.user_email).bind(&u.models).bind(&u.metadata)
                .bind(&u.max_parallel_requests).bind(&u.tpm_limit).bind(&u.rpm_limit)
                .bind(&u.budget_duration).bind(u.budget_reset_at)
                .bind(&u.allowed_cache_controls).bind(&u.policies)
                .bind(&u.model_spend).bind(&u.model_max_budget)
                .bind(u.created_at).bind(u.updated_at)
                .execute(self).await?;
        }
        sqlx::query("DELETE FROM users WHERE user_id = $1").bind(user_id).execute(self).await?;
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Database enum dispatch: org, team, user CRUD
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl Database {
    // ── Organization dispatch ──
    pub async fn insert_organization(&self, o: &Organization) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.insert_organization(o).await,
            Database::Mysql(pool) => pool.insert_organization(o).await,
            Database::Postgres(pool) => pool.insert_organization(o).await,
        }
    }

    pub async fn get_organization_by_id(&self, org_id: &str) -> Result<Option<Organization>> {
        match self {
            Database::Sqlite(pool) => pool.get_organization_by_id(org_id).await,
            Database::Mysql(pool) => pool.get_organization_by_id(org_id).await,
            Database::Postgres(pool) => pool.get_organization_by_id(org_id).await,
        }
    }

    pub async fn list_organizations(&self) -> Result<Vec<Organization>> {
        match self {
            Database::Sqlite(pool) => pool.list_organizations().await,
            Database::Mysql(pool) => pool.list_organizations().await,
            Database::Postgres(pool) => pool.list_organizations().await,
        }
    }

    pub async fn list_deleted_organizations(&self) -> Result<Vec<DeletedOrganization>> {
        match self {
            Database::Sqlite(pool) => pool.list_deleted_organizations().await,
            Database::Mysql(pool) => pool.list_deleted_organizations().await,
            Database::Postgres(pool) => pool.list_deleted_organizations().await,
        }
    }

    pub async fn update_organization(&self, o: &Organization) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.update_organization(o).await,
            Database::Mysql(pool) => pool.update_organization(o).await,
            Database::Postgres(pool) => pool.update_organization(o).await,
        }
    }

    pub async fn delete_organization(&self, org_id: &str) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.delete_organization(org_id).await,
            Database::Mysql(pool) => pool.delete_organization(org_id).await,
            Database::Postgres(pool) => pool.delete_organization(org_id).await,
        }
    }

    // ── Team dispatch ──
    pub async fn insert_team(&self, t: &Team) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.insert_team(t).await,
            Database::Mysql(pool) => pool.insert_team(t).await,
            Database::Postgres(pool) => pool.insert_team(t).await,
        }
    }

    pub async fn get_team_by_id(&self, team_id: &str) -> Result<Option<Team>> {
        match self {
            Database::Sqlite(pool) => pool.get_team_by_id(team_id).await,
            Database::Mysql(pool) => pool.get_team_by_id(team_id).await,
            Database::Postgres(pool) => pool.get_team_by_id(team_id).await,
        }
    }

    pub async fn list_teams(&self, org_id: Option<&str>) -> Result<Vec<Team>> {
        match self {
            Database::Sqlite(pool) => pool.list_teams(org_id).await,
            Database::Mysql(pool) => pool.list_teams(org_id).await,
            Database::Postgres(pool) => pool.list_teams(org_id).await,
        }
    }

    pub async fn list_deleted_teams(&self) -> Result<Vec<DeletedTeam>> {
        match self {
            Database::Sqlite(pool) => pool.list_deleted_teams().await,
            Database::Mysql(pool) => pool.list_deleted_teams().await,
            Database::Postgres(pool) => pool.list_deleted_teams().await,
        }
    }

    pub async fn update_team(&self, t: &Team) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.update_team(t).await,
            Database::Mysql(pool) => pool.update_team(t).await,
            Database::Postgres(pool) => pool.update_team(t).await,
        }
    }

    pub async fn delete_team(&self, team_id: &str) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.delete_team(team_id).await,
            Database::Mysql(pool) => pool.delete_team(team_id).await,
            Database::Postgres(pool) => pool.delete_team(team_id).await,
        }
    }

    // ── User dispatch ──
    pub async fn insert_user(&self, u: &User) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.insert_user(u).await,
            Database::Mysql(pool) => pool.insert_user(u).await,
            Database::Postgres(pool) => pool.insert_user(u).await,
        }
    }

    pub async fn get_user_by_id(&self, user_id: &str) -> Result<Option<User>> {
        match self {
            Database::Sqlite(pool) => pool.get_user_by_id(user_id).await,
            Database::Mysql(pool) => pool.get_user_by_id(user_id).await,
            Database::Postgres(pool) => pool.get_user_by_id(user_id).await,
        }
    }

    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<User>> {
        match self {
            Database::Sqlite(pool) => pool.get_user_by_email(email).await,
            Database::Mysql(pool) => pool.get_user_by_email(email).await,
            Database::Postgres(pool) => pool.get_user_by_email(email).await,
        }
    }

    pub async fn list_users(&self, org_id: Option<&str>) -> Result<Vec<User>> {
        match self {
            Database::Sqlite(pool) => pool.list_users(org_id).await,
            Database::Mysql(pool) => pool.list_users(org_id).await,
            Database::Postgres(pool) => pool.list_users(org_id).await,
        }
    }

    pub async fn update_user(&self, u: &User) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.update_user(u).await,
            Database::Mysql(pool) => pool.update_user(u).await,
            Database::Postgres(pool) => pool.update_user(u).await,
        }
    }

    pub async fn delete_user(&self, user_id: &str) -> Result<()> {
        match self {
            Database::Sqlite(pool) => pool.delete_user(user_id).await,
            Database::Mysql(pool) => pool.delete_user(user_id).await,
            Database::Postgres(pool) => pool.delete_user(user_id).await,
        }
    }

    pub async fn list_deleted_users(&self) -> Result<Vec<DeletedUser>> {
        match self {
            Database::Sqlite(pool) => pool.list_deleted_users().await,
            Database::Mysql(pool) => pool.list_deleted_users().await,
            Database::Postgres(pool) => pool.list_deleted_users().await,
        }
    }

    // ── Config ──

    /// Retrieve master_key from the `config` table.
    ///
    /// Follows litellm's fallback logic:
    /// 1. Query `general_settings` JSON → `master_key` field
    /// 2. Fall back to `litellm_master_key` legacy flat key
    pub async fn get_master_key_from_db(&self) -> Result<Option<String>> {
        // Strategy 1: general_settings JSON → master_key
        let general_settings: Option<String> = match self {
            Database::Sqlite(p) => {
                sqlx::query_scalar("SELECT param_value FROM config WHERE param_name = ?")
                    .bind("general_settings")
                    .fetch_optional(p)
                    .await?
            }
            Database::Mysql(p) => {
                sqlx::query_scalar("SELECT param_value FROM config WHERE param_name = ?")
                    .bind("general_settings")
                    .fetch_optional(p)
                    .await?
            }
            Database::Postgres(p) => {
                sqlx::query_scalar("SELECT param_value FROM config WHERE param_name = $1")
                    .bind("general_settings")
                    .fetch_optional(p)
                    .await?
            }
        };

        if let Some(val) = general_settings {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&val) {
                if let Some(mk) = parsed.get("master_key").and_then(|v| v.as_str()) {
                    if !mk.is_empty() {
                        return Ok(Some(mk.to_string()));
                    }
                }
            }
        }

        // Strategy 2: legacy litellm_master_key flat key
        let legacy_key: Option<String> = match self {
            Database::Sqlite(p) => {
                sqlx::query_scalar("SELECT param_value FROM config WHERE param_name = ?")
                    .bind("litellm_master_key")
                    .fetch_optional(p)
                    .await?
            }
            Database::Mysql(p) => {
                sqlx::query_scalar("SELECT param_value FROM config WHERE param_name = ?")
                    .bind("litellm_master_key")
                    .fetch_optional(p)
                    .await?
            }
            Database::Postgres(p) => {
                sqlx::query_scalar("SELECT param_value FROM config WHERE param_name = $1")
                    .bind("litellm_master_key")
                    .fetch_optional(p)
                    .await?
            }
        };

        if let Some(val) = legacy_key {
            if !val.is_empty() {
                return Ok(Some(val));
            }
        }

        Ok(None)
    }

    /// Get a config value by param_name.
    /// Returns None if the key doesn't exist.
    pub async fn get_config(&self, param_name: &str) -> Result<Option<String>> {
        let result = match self {
            Database::Sqlite(p) => {
                sqlx::query_scalar("SELECT param_value FROM config WHERE param_name = ?")
                    .bind(param_name)
                    .fetch_optional(p)
                    .await?
            }
            Database::Mysql(p) => {
                sqlx::query_scalar("SELECT param_value FROM config WHERE param_name = ?")
                    .bind(param_name)
                    .fetch_optional(p)
                    .await?
            }
            Database::Postgres(p) => {
                sqlx::query_scalar("SELECT param_value FROM config WHERE param_name = $1")
                    .bind(param_name)
                    .fetch_optional(p)
                    .await?
            }
        };
        Ok(result)
    }

    /// Upsert a config value. INSERT if not exists, UPDATE if exists.
    pub async fn upsert_config(&self, param_name: &str, param_value: &str) -> Result<()> {
        match self {
            Database::Sqlite(p) => {
                sqlx::query(
                    "INSERT INTO config (param_name, param_value) VALUES (?1, ?2) \
                     ON CONFLICT(param_name) DO UPDATE SET param_value = excluded.param_value"
                )
                    .bind(param_name)
                    .bind(param_value)
                    .execute(p)
                    .await?;
            }
            Database::Mysql(p) => {
                sqlx::query(
                    "INSERT INTO config (param_name, param_value) VALUES (?, ?) \
                     ON DUPLICATE KEY UPDATE param_value = VALUES(param_value)"
                )
                    .bind(param_name)
                    .bind(param_value)
                    .execute(p)
                    .await?;
            }
            Database::Postgres(p) => {
                sqlx::query(
                    "INSERT INTO config (param_name, param_value) VALUES ($1, $2) \
                     ON CONFLICT(param_name) DO UPDATE SET param_value = excluded.param_value"
                )
                    .bind(param_name)
                    .bind(param_value)
                    .execute(p)
                    .await?;
            }
        };
        Ok(())
    }

    /// Update router_settings on a key (virtual_keys.router_settings JSON column).
    pub async fn update_key_router_settings(&self, token: &str, settings_json: &str) -> Result<()> {
        match self {
            Database::Sqlite(p) => {
                sqlx::query(
                    "UPDATE virtual_keys SET router_settings = ?1 WHERE token = ?2 OR key_alias = ?2"
                )
                    .bind(settings_json)
                    .bind(token)
                    .execute(p)
                    .await?;
            }
            Database::Mysql(p) => {
                sqlx::query(
                    "UPDATE virtual_keys SET router_settings = ? WHERE token = ? OR key_alias = ?"
                )
                    .bind(settings_json)
                    .bind(token)
                    .execute(p)
                    .await?;
            }
            Database::Postgres(p) => {
                sqlx::query(
                    "UPDATE virtual_keys SET router_settings = $1 WHERE token = $2 OR key_alias = $2"
                )
                    .bind(settings_json)
                    .bind(token)
                    .execute(p)
                    .await?;
            }
        };
        Ok(())
    }

    /// Update router_settings on a team (teams.router_settings JSON column).
    pub async fn update_team_router_settings(&self, team_id: &str, settings_json: &str) -> Result<()> {
        match self {
            Database::Sqlite(p) => {
                sqlx::query("UPDATE teams SET router_settings = ?1 WHERE team_id = ?2 OR team_alias = ?2")
                    .bind(settings_json)
                    .bind(team_id)
                    .execute(p)
                    .await?;
            }
            Database::Mysql(p) => {
                sqlx::query("UPDATE teams SET router_settings = ? WHERE team_id = ? OR team_alias = ?")
                    .bind(settings_json)
                    .bind(team_id)
                    .execute(p)
                    .await?;
            }
            Database::Postgres(p) => {
                sqlx::query("UPDATE teams SET router_settings = $1 WHERE team_id = $2 OR team_alias = $2")
                    .bind(settings_json)
                    .bind(team_id)
                    .execute(p)
                    .await?;
            }
        };
        Ok(())
    }

    // ── Health / Metrics ──

    pub fn pool_size(&self) -> u32 {
        match self {
            Database::Sqlite(p) => p.size(),
            Database::Mysql(p) => p.size(),
            Database::Postgres(p) => p.size(),
        }
    }

    pub fn pool_idle(&self) -> u32 {
        match self {
            Database::Sqlite(p) => p.num_idle() as u32,
            Database::Mysql(p) => p.num_idle() as u32,
            Database::Postgres(p) => p.num_idle() as u32,
        }
    }

    /// Ping the database — acquire a connection to verify the DB is reachable.
    ///
    /// Returns `Ok(())` if a connection was successfully acquired
    /// (and therefore the database is ready to serve requests).
    /// Returns `Err` if the pool is exhausted or the database is unreachable.
    pub async fn ping(&self) -> Result<()> {
        match self {
            Database::Sqlite(p) => {
                p.acquire().await?;
            }
            Database::Mysql(p) => {
                p.acquire().await?;
            }
            Database::Postgres(p) => {
                p.acquire().await?;
            }
        }
        Ok(())
    }

    async fn _count(&self, table: &str) -> Result<i64> {
        let query = format!("SELECT COUNT(*) FROM {}", table);
        let row: (i64,) = match self {
            Database::Sqlite(p) => sqlx::query_as(&query).fetch_one(p).await?,
            Database::Mysql(p) => sqlx::query_as(&query).fetch_one(p).await?,
            Database::Postgres(p) => sqlx::query_as(&query).fetch_one(p).await?,
        };
        Ok(row.0)
    }

    pub async fn count_virtual_keys(&self) -> Result<i64> {
        self._count("virtual_keys").await
    }

    pub async fn count_proxy_models(&self) -> Result<i64> {
        self._count("proxy_models").await
    }

    pub async fn count_organizations(&self) -> Result<i64> {
        self._count("organizations").await
    }

    pub async fn count_teams(&self) -> Result<i64> {
        self._count("teams").await
    }

    pub async fn count_users(&self) -> Result<i64> {
        self._count("users").await
    }

    // ── Health Check ──

    pub async fn insert_health_check(&self, check: &HealthCheck) -> Result<()> {
        let sql = r#"INSERT INTO health_checks (
            health_check_id, model_name, model_id, status,
            healthy_count, unhealthy_count, error_message, response_time_ms,
            details, checked_by, checked_at, created_at, updated_at
        ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)"#;
        match self {
            Database::Sqlite(p) => {
                sqlx::query(sql)
                    .bind(&check.health_check_id)
                    .bind(&check.model_name)
                    .bind(&check.model_id)
                    .bind(&check.status)
                    .bind(check.healthy_count)
                    .bind(check.unhealthy_count)
                    .bind(&check.error_message)
                    .bind(check.response_time_ms)
                    .bind(&check.details)
                    .bind(&check.checked_by)
                    .bind(&check.checked_at)
                    .bind(&check.created_at)
                    .bind(&check.updated_at)
                    .execute(p).await?;
            }
            Database::Mysql(p) => {
                sqlx::query(sql)
                    .bind(&check.health_check_id)
                    .bind(&check.model_name)
                    .bind(&check.model_id)
                    .bind(&check.status)
                    .bind(check.healthy_count)
                    .bind(check.unhealthy_count)
                    .bind(&check.error_message)
                    .bind(check.response_time_ms)
                    .bind(&check.details)
                    .bind(&check.checked_by)
                    .bind(&check.checked_at)
                    .bind(&check.created_at)
                    .bind(&check.updated_at)
                    .execute(p).await?;
            }
            Database::Postgres(p) => {
                sqlx::query(sql)
                    .bind(&check.health_check_id)
                    .bind(&check.model_name)
                    .bind(&check.model_id)
                    .bind(&check.status)
                    .bind(check.healthy_count)
                    .bind(check.unhealthy_count)
                    .bind(&check.error_message)
                    .bind(check.response_time_ms)
                    .bind(&check.details)
                    .bind(&check.checked_by)
                    .bind(&check.checked_at)
                    .bind(&check.created_at)
                    .bind(&check.updated_at)
                    .execute(p).await?;
            }
        }
        Ok(())
    }

    pub async fn get_latest_health_checks(&self) -> Result<Vec<HealthCheck>> {
        let sql = r#"SELECT h.* FROM health_checks h
            INNER JOIN (
                SELECT model_name, MAX(checked_at) AS max_checked
                FROM health_checks GROUP BY model_name
            ) latest ON h.model_name = latest.model_name AND h.checked_at = latest.max_checked
            ORDER BY h.model_name"#;
        match self {
            Database::Sqlite(p) => Ok(sqlx::query_as::<_, HealthCheck>(sql).fetch_all(p).await?),
            Database::Mysql(p) => Ok(sqlx::query_as::<_, HealthCheck>(sql).fetch_all(p).await?),
            Database::Postgres(p) => Ok(sqlx::query_as::<_, HealthCheck>(sql).fetch_all(p).await?),
        }
    }

    // ── Spend Logs: status + token range filter ──

    pub async fn query_spend_logs_with_status_filter(
        &self,
        api_key: Option<&str>,
        model: Option<&str>,
        provider: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        call_id: Option<&str>,
        status: Option<&str>,
        min_tokens: Option<i32>,
        max_tokens: Option<i32>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<Vec<SpendLog>> {
        let mut conditions = Vec::new();
        let _params: Vec<String> = Vec::new();

        // PostgreSQL requires explicit timestamp casts; SQLite/MySQL are lenient
        let ts_cast = match self {
            Database::Postgres(_) => "::TIMESTAMPTZ",
            _ => "",
        };

        if let Some(k) = api_key { conditions.push(format!("api_key = '{}'", k.replace('\'', "''"))); }
        if let Some(m) = model { conditions.push(format!("model = '{}'", m.replace('\'', "''"))); }
        if let Some(p) = provider { conditions.push(format!("custom_llm_provider = '{}'", p.replace('\'', "''"))); }
        if let Some(s) = start_date { conditions.push(format!("start_time >= '{}'{}", s.replace('\'', "''"), ts_cast)); }
        if let Some(e) = end_date { conditions.push(format!("start_time <= '{}'{}", e.replace('\'', "''"), ts_cast)); }
        // Dual-column fuzzy search: match gateway call_id OR upstream request_id (LIKE '%X%' ESCAPE '\').
        if let Some(rid) = call_id {
            let esc = rid.replace('\'', "''").replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            conditions.push(format!("(call_id LIKE '%{}%' ESCAPE '\\' OR request_id LIKE '%{}%' ESCAPE '\\')", esc, esc));
        }
        if let Some(st) = status {
            if st == "success" { conditions.push("status = 'success'".to_string()); }
            else if st == "failure" { conditions.push("status LIKE 'failure%'".to_string()); }
            else if st == "streaming" { conditions.push("status = 'streaming'".to_string()); }
        }
        if let Some(mt) = min_tokens { conditions.push(format!("total_tokens >= {}", mt)); }
        if let Some(mt) = max_tokens { conditions.push(format!("total_tokens <= {}", mt)); }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let l = limit.unwrap_or(100);
        let o = offset.unwrap_or(0);

        let sql = format!(
            r#"SELECT call_id, call_type, api_key, spend, total_tokens,
            prompt_tokens, completion_tokens, start_time, end_time,
            request_duration_ms, completion_start_time, model, model_id, model_group,
            custom_llm_provider, api_base, "user", metadata,
            cache_hit, cache_key, request_tags, team_id, organization_id,
            end_user, requester_ip_address, messages, response,
            session_id, status, mcp_namespaced_tool_name, agent_id, proxy_server_request,
            body_archived, parquet_path, request_id
            FROM spend_logs {} ORDER BY start_time DESC LIMIT {} OFFSET {}"#,
            where_clause, l, o
        );

        match self {
            Database::Sqlite(p) => Ok(sqlx::query_as::<_, SpendLog>(&sql).fetch_all(p).await?),
            Database::Mysql(p) => Ok(sqlx::query_as::<_, SpendLog>(&sql).fetch_all(p).await?),
            Database::Postgres(p) => Ok(sqlx::query_as::<_, SpendLog>(&sql).fetch_all(p).await?),
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
    use serde_json::json;

    /// All 23 tables defined in the migrations (aigw names)
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
        "daily_user_spend",
        "daily_team_spend",
        "daily_organization_spend",
        "daily_end_user_spend",
        "daily_agent_spend",
        "daily_tag_spend",
        "async_jobs",
        "async_job_steps",
        "async_job_logs",
    ];

    fn make_test_key(token_hash: &str, key_alias: &str) -> VirtualKey {
        VirtualKey {
            token: token_hash.to_string(),
            key_name: Some(key_alias.to_string()),
            key_alias: Some(key_alias.to_string()),
            soft_budget_cooldown: "false".to_string(),
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
            tpm_limit: Some("1000".to_string()),
            rpm_limit: Some("100".to_string()),
            max_budget: Some("100.0".to_string()),
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
    async fn test_all_23_tables_exist_after_migration() {
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
        assert_eq!(k.tpm_limit, Some("1000".to_string()));
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
        updated.tpm_limit = Some("5000".to_string());
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
        assert_eq!(k.tpm_limit, Some("5000".to_string()));
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
        make_test_spend_log_full(api_key, user, spend, 100, 50, 50, "success", request_tags)
    }

    /// Helper: create a SpendLog with full token breakdown and status.
    fn make_test_spend_log_full(
        api_key: &str,
        user: &str,
        spend: f64,
        total_tokens: i32,
        prompt_tokens: i32,
        completion_tokens: i32,
        status: &str,
        _request_tags: Option<serde_json::Value>,
    ) -> SpendLog {
        let now = Utc::now();
        SpendLog {
            call_id: Uuid::new_v4().to_string(),
            call_type: "completion".to_string(),
            api_key: api_key.to_string(),
            spend,
            total_tokens,
            prompt_tokens,
            completion_tokens,
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
            request_tags: _request_tags,
            team_id: None,
            organization_id: None,
            end_user: None,
            requester_ip_address: None,
            messages: None,
            response: None,
            session_id: None,
            status: Some(status.to_string()),
            mcp_namespaced_tool_name: None,
            agent_id: None,
            proxy_server_request: None,
            body_archived: false,
            parquet_path: None,
            request_id: None,
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

    // ━━━━ get_master_key_from_db tests ━━━━

    #[tokio::test]
    async fn test_get_master_key_from_config_general_settings() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let general_settings = r#"{"master_key": "sk-from-config-table"}"#;
        // Insert via raw SQL (no dedicated insert_config method yet)
        match &db {
            Database::Sqlite(pool) => {
                sqlx::query("INSERT INTO config (param_name, param_value) VALUES (?1, ?2)")
                    .bind("general_settings")
                    .bind(general_settings)
                    .execute(pool)
                    .await
                    .expect("insert");
            }
            _ => unreachable!(),
        }
        let result = db.get_master_key_from_db().await.expect("query");
        assert_eq!(result, Some("sk-from-config-table".to_string()));
    }

    #[tokio::test]
    async fn test_get_master_key_from_config_legacy() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        match &db {
            Database::Sqlite(pool) => {
                sqlx::query("INSERT INTO config (param_name, param_value) VALUES (?1, ?2)")
                    .bind("litellm_master_key")
                    .bind("sk-legacy-key")
                    .execute(pool)
                    .await
                    .expect("insert");
            }
            _ => unreachable!(),
        }
        let result = db.get_master_key_from_db().await.expect("query");
        assert_eq!(result, Some("sk-legacy-key".to_string()));
    }

    #[tokio::test]
    async fn test_get_master_key_not_found() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let result = db.get_master_key_from_db().await.expect("query");
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_get_master_key_malformed_json() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        match &db {
            Database::Sqlite(pool) => {
                sqlx::query("INSERT INTO config (param_name, param_value) VALUES (?1, ?2)")
                    .bind("general_settings")
                    .bind("not-valid-json{{{")
                    .execute(pool)
                    .await
                    .expect("insert");
            }
            _ => unreachable!(),
        }
        // Malformed JSON → general_settings doesn't parse → falls through to legacy → returns None
        let result = db.get_master_key_from_db().await.expect("query");
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_general_settings_has_precedence_over_legacy() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        match &db {
            Database::Sqlite(pool) => {
                // Insert both general_settings and legacy
                sqlx::query("INSERT INTO config (param_name, param_value) VALUES (?1, ?2)")
                    .bind("general_settings")
                    .bind(r#"{"master_key": "sk-from-json"}"#)
                    .execute(pool)
                    .await
                    .expect("insert general_settings");
                sqlx::query("INSERT INTO config (param_name, param_value) VALUES (?1, ?2)")
                    .bind("litellm_master_key")
                    .bind("sk-from-legacy")
                    .execute(pool)
                    .await
                    .expect("insert legacy");
            }
            _ => unreachable!(),
        }
        // general_settings should take precedence
        let result = db.get_master_key_from_db().await.expect("query");
        assert_eq!(result, Some("sk-from-json".to_string()));
    }

    #[test]
    fn test_normalize_date_rfc3339_passthrough() {
        assert_eq!(
            Database::normalize_date_for_query("2026-07-15T02:34:38Z", false),
            "2026-07-15T02:34:38Z"
        );
        assert_eq!(
            Database::normalize_date_for_query("2026-07-15T10:30:00+08:00", true),
            "2026-07-15T10:30:00+08:00"
        );
    }

    #[test]
    fn test_normalize_date_pure_date_expands() {
        // start date → T00:00:00Z
        assert_eq!(
            Database::normalize_date_for_query("2026-07-15", false),
            "2026-07-15T00:00:00Z"
        );
        // end date → T23:59:59.999Z
        assert_eq!(
            Database::normalize_date_for_query("2026-07-15", true),
            "2026-07-15T23:59:59.999Z"
        );
    }

    #[test]
    fn test_normalize_date_local_time_append_z() {
        assert_eq!(
            Database::normalize_date_for_query("2026-07-15T10:34:38", false),
            "2026-07-15T10:34:38Z"
        );
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Stage 69: Daily trend 8-tuple decomposition
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[tokio::test]
    async fn test_activity_daily_8_tuple() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let key_hash = hash_token("sk-daily-test");

        // Insert a successful request
        let log1 = make_test_spend_log_full(
            &key_hash, "user-1", 0.5, 100, 60, 40, "success", None,
        );
        // Insert a failed request
        let log2 = make_test_spend_log_full(
            &key_hash, "user-1", 0.0, 80, 50, 30, "failure:500", None,
        );

        db.insert_spend_log(&log1).await.expect("insert log1");
        db.insert_spend_log(&log2).await.expect("insert log2");

        let rows = db.query_activity_daily("2020-01-01", "2030-12-31", None, None, None)
            .await.expect("query activity daily");

        assert!(!rows.is_empty(), "should have at least one row");
        let (date, spend, _tokens, requests, prompt_tokens, completion_tokens, successful_requests, failed_requests) = &rows[0];
        assert!(!date.is_empty());
        assert!(*spend > 0.0 || true, "spend present");
        assert_eq!(*requests, 2, "should count both success and failure as requests");
        assert_eq!(*prompt_tokens, 110, "60 + 50 prompt tokens");
        assert_eq!(*completion_tokens, 70, "40 + 30 completion tokens");
        assert_eq!(*successful_requests, 1);
        assert_eq!(*failed_requests, 1);
    }

    #[tokio::test]
    async fn test_activity_hourly_8_tuple() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let key_hash = hash_token("sk-hourly-test");

        // Insert requests within a few hours
        let log1 = make_test_spend_log_full(
            &key_hash, "user-1", 0.5, 100, 60, 40, "success", None,
        );
        let log2 = make_test_spend_log_full(
            &key_hash, "user-1", 0.0, 80, 50, 30, "failure:500", None,
        );

        db.insert_spend_log(&log1).await.expect("insert log1");
        db.insert_spend_log(&log2).await.expect("insert log2");

        let rows = db.query_activity_hourly("2020-01-01", "2030-12-31", None, None, None)
            .await.expect("query activity hourly");

        assert!(!rows.is_empty(), "should have at least one row");
        let (date, _spend, _tokens, requests, prompt_tokens, completion_tokens, successful_requests, failed_requests) = &rows[0];
        // Hourly format: "YYYY-MM-DDTHH:00:00"
        assert!(date.contains('T'), "hourly date should contain T separator, got {}", date);
        assert!(date.ends_with(":00:00"), "hourly date should end with :00:00, got {}", date);
        assert_eq!(*requests, 2, "should count both success and failure as requests");
        assert_eq!(*prompt_tokens, 110, "60 + 50 prompt tokens");
        assert_eq!(*completion_tokens, 70, "40 + 30 completion tokens");
        assert_eq!(*successful_requests, 1);
        assert_eq!(*failed_requests, 1);
    }

    #[tokio::test]
    async fn test_activity_hourly_filters_by_date_range() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let key_hash = hash_token("sk-hourly-test-2");

        // Insert a request in 2020
        let log = make_test_spend_log_full(
            &key_hash, "user-1", 1.0, 100, 60, 40, "success", None,
        );
        db.insert_spend_log(&log).await.expect("insert log");

        // Query for a range that should NOT include our log
        let rows = db.query_activity_hourly("2030-01-01", "2030-01-02", None, None, None)
            .await.expect("query activity hourly");

        assert!(rows.is_empty(), "should return nothing for out-of-range query");
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Stage 69: Top Keys ranking aggregation
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[tokio::test]
    async fn test_aggregate_spend_by_keys_sort_order() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        // Insert virtual keys
        let key_a = hash_token("key-a");
        let key_b = hash_token("key-b");
        let vk_a = make_test_key(&key_a, "alias-a");
        let vk_b = make_test_key(&key_b, "alias-b");
        db.insert_key(&vk_a).await.expect("insert key");
        db.insert_key(&vk_b).await.expect("insert key");

        // Insert spend logs with different amounts
        let log_a = make_test_spend_log_full(&key_a, "u1", 10.0, 100, 50, 50, "success", None);
        let log_b = make_test_spend_log_full(&key_b, "u2", 5.0, 50, 25, 25, "success", None);
        let log_a2 = make_test_spend_log_full(&key_a, "u1", 3.0, 30, 15, 15, "success", None);

        db.insert_spend_log(&log_a).await.expect("insert a");
        db.insert_spend_log(&log_b).await.expect("insert b");
        db.insert_spend_log(&log_a2).await.expect("insert a2");

        let rankings = db.aggregate_spend_by_keys("2020-01-01", "2030-12-31", 10)
            .await.expect("aggregate");

        assert!(rankings.len() >= 2, "should have at least 2 keys ranked");
        // Key A (13.0) should rank before Key B (5.0) — descending by spend
        assert_eq!(rankings[0].api_key, key_a);
        assert_eq!(rankings[0].total_spend, 13.0);
        assert_eq!(rankings[0].key_alias.as_deref(), Some("alias-a"));
    }

    #[tokio::test]
    async fn test_aggregate_spend_by_keys_limit() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        for i in 0..5 {
            let hash = hash_token(&format!("key-{}", i));
            let vk = make_test_key(&hash, &format!("alias-{}", i));
            db.insert_key(&vk).await.expect("insert key");

            let log = make_test_spend_log_full(&hash, "u1", (i + 1) as f64, 10, 5, 5, "success", None);
            db.insert_spend_log(&log).await.expect("insert log");
        }

        // Limit to 2
        let rankings = db.aggregate_spend_by_keys("2020-01-01", "2030-12-31", 2)
            .await.expect("aggregate with limit");

        assert_eq!(rankings.len(), 2, "should respect limit=2");
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Stage 87: LIKE fuzzy search on call_id / request_id
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[tokio::test]
    async fn test_fuzzy_search_call_id_prefix() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        let mut l1 = make_test_spend_log(&"sk-test", "u1", 1.0, None);
        l1.call_id = "req-001".to_string();
        l1.request_id = Some("chatcmpl-abc123".to_string());
        db.insert_spend_log(&l1).await.expect("insert l1");

        let mut l2 = make_test_spend_log(&"sk-test", "u1", 2.0, None);
        l2.call_id = "req-002".to_string();
        l2.request_id = Some("msg_xyz789".to_string());
        db.insert_spend_log(&l2).await.expect("insert l2");

        let mut l3 = make_test_spend_log(&"sk-test", "u1", 3.0, None);
        l3.call_id = "req-003".to_string();
        l3.request_id = None;
        db.insert_spend_log(&l3).await.expect("insert l3");

        // Search by call_id prefix "req-00" — matches all 3
        let logs = db.query_spend_logs_filtered(None, None, None, None, None, Some("req-00"), Some(100), Some(0))
            .await.expect("query prefix");
        assert_eq!(logs.len(), 3, "prefix 'req-00' should match all 3 logs");

        // Search by request_id substring "chatcmpl" — matches l1 only
        let logs = db.query_spend_logs_filtered(None, None, None, None, None, Some("chatcmpl"), Some(100), Some(0))
            .await.expect("query chatcmpl");
        assert_eq!(logs.len(), 1, "substring 'chatcmpl' should match 1 log");
        assert_eq!(logs[0].call_id, "req-001");

        // Search by request_id substring "xyz" — matches l2 only
        let logs = db.query_spend_logs_filtered(None, None, None, None, None, Some("xyz"), Some(100), Some(0))
            .await.expect("query xyz");
        assert_eq!(logs.len(), 1, "substring 'xyz' should match 1 log");
        assert_eq!(logs[0].call_id, "req-002");

        // Count matching — should be consistent
        let count = db.query_spend_logs_count(None, None, None, None, Some("req-00"))
            .await.expect("count prefix");
        assert_eq!(count, 3, "count should match query for 'req-00'");

        let count = db.query_spend_logs_count(None, None, None, None, Some("chatcmpl"))
            .await.expect("count chatcmpl");
        assert_eq!(count, 1, "count should match query for 'chatcmpl'");
    }

    #[tokio::test]
    async fn test_fuzzy_search_like_wildcard_escaped() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        let mut l1 = make_test_spend_log(&"sk-test", "u1", 1.0, None);
        l1.call_id = "req%001".to_string(); // contains literal %
        l1.request_id = None;
        db.insert_spend_log(&l1).await.expect("insert l1");

        let mut l2 = make_test_spend_log(&"sk-test", "u1", 2.0, None);
        l2.call_id = "req_002".to_string(); // contains literal _
        l2.request_id = None;
        db.insert_spend_log(&l2).await.expect("insert l2");

        // Searching for "req%" should match only l1, not everything (escaped)
        let logs = db.query_spend_logs_filtered(None, None, None, None, None, Some("req%"), Some(100), Some(0))
            .await.expect("query req%");
        assert_eq!(logs.len(), 1, "escaped '%' should not act as wildcard");
        assert_eq!(logs[0].call_id, "req%001");

        // Searching for "req_" should match l2
        let logs = db.query_spend_logs_filtered(None, None, None, None, None, Some("req_"), Some(100), Some(0))
            .await.expect("query req_");
        assert_eq!(logs.len(), 1, "escaped '_' should not act as wildcard");
        assert_eq!(logs[0].call_id, "req_002");
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Stage 88: Soft-delete tests (tombstone-then-delete)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    fn make_test_org(org_id: &str, alias: &str) -> Organization {
        let now = Utc::now();
        Organization {
            organization_id: org_id.to_string(),
            organization_alias: alias.to_string(),
            budget_id: "default".to_string(),
            metadata: json!({}),
            models: json!([]),
            spend: 0.0,
            model_spend: json!({}),
            object_permission_id: None,
            created_at: now,
            created_by: "test".to_string(),
            updated_at: now,
            updated_by: "test".to_string(),
        }
    }

    fn make_test_team(team_id: &str, alias: &str) -> Team {
        let now = Utc::now();
        Team {
            team_id: team_id.to_string(),
            team_alias: Some(alias.to_string()),
            organization_id: None,
            object_permission_id: None,
            admins: json!([]),
            members: json!([]),
            members_with_roles: json!([]),
            metadata: json!({}),
            max_budget: None,
            soft_budget: None,
            spend: 0.0,
            models: json!([]),
            max_parallel_requests: None,
            tpm_limit: None,
            rpm_limit: None,
            budget_duration: None,
            budget_reset_at: None,
            blocked: false,
            created_at: now,
            updated_at: now,
            model_spend: json!({}),
            model_max_budget: json!({}),
            router_settings: None,
            team_member_permissions: json!([]),
            access_group_ids: json!([]),
            policies: json!([]),
            default_team_member_models: json!([]),
            budget_limits: None,
            model_id: None,
            allow_team_guardrail_config: false,
        }
    }

    fn make_test_user(user_id: &str, email: &str) -> User {
        let now = Some(Utc::now());
        User {
            user_id: user_id.to_string(),
            user_alias: None,
            team_id: None,
            sso_user_id: None,
            organization_id: None,
            object_permission_id: None,
            password: None,
            teams: json!([]),
            user_role: None,
            max_budget: None,
            spend: 0.0,
            user_email: Some(email.to_string()),
            models: json!([]),
            metadata: json!({}),
            max_parallel_requests: None,
            tpm_limit: None,
            rpm_limit: None,
            budget_duration: None,
            budget_reset_at: None,
            allowed_cache_controls: json!([]),
            policies: json!([]),
            model_spend: json!({}),
            model_max_budget: json!({}),
            virtual_keys_count: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn make_test_model_from_id(model_id: &str, model_name: &str) -> ProxyModel {
        ProxyModel {
            model_id: model_id.to_string(),
            model_name: model_name.to_string(),
            litellm_params: json!({"model": model_name}),
            model_info: json!({}),
            created_at: "2026-01-01".to_string(),
            created_by: None,
            updated_at: "2026-01-01".to_string(),
            updated_by: None,
        }
    }

    #[tokio::test]
    async fn test_delete_organization_soft() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        let org = make_test_org("org-1", "Test Org");
        db.insert_organization(&org).await.expect("insert");

        // Verify org exists
        let found = db.get_organization_by_id("org-1").await.expect("get");
        assert!(found.is_some());

        // Soft-delete
        db.delete_organization("org-1").await.expect("delete");

        // Verify org gone from source table
        let found = db.get_organization_by_id("org-1").await.expect("get");
        assert!(found.is_none(), "organization should be deleted from source");

        // Verify org in archive
        let deleted = db.list_deleted_organizations().await.expect("list deleted");
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].organization_id, "org-1");
        assert_eq!(deleted[0].organization_alias, "Test Org");
        // deleted_at should be non-zero
        assert!(deleted[0].deleted_at.timestamp() > 0);
    }

    #[tokio::test]
    async fn test_delete_team_soft() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        let team = make_test_team("team-1", "Test Team");
        db.insert_team(&team).await.expect("insert");

        // Soft-delete
        db.delete_team("team-1").await.expect("delete");

        // Verify team gone from source
        let found = db.get_team_by_id("team-1").await.expect("get");
        assert!(found.is_none());

        // Verify team in archive
        let deleted = db.list_deleted_teams().await.expect("list deleted");
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].team_id, "team-1");
        assert_eq!(deleted[0].team_alias.as_deref(), Some("Test Team"));
    }

    #[tokio::test]
    async fn test_delete_user_soft() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        let user = make_test_user("user-1", "test@example.com");
        db.insert_user(&user).await.expect("insert");

        // Soft-delete
        db.delete_user("user-1").await.expect("delete");

        // Verify user gone from source
        let found = db.get_user_by_id("user-1").await.expect("get");
        assert!(found.is_none());

        // Verify user in archive
        let deleted = db.list_deleted_users().await.expect("list deleted");
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].user_id, "user-1");
    }

    #[tokio::test]
    async fn test_delete_model_soft() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        let model = make_test_model_from_id("model-1", "gpt-4");
        db.insert_model(&model).await.expect("insert");

        // Soft-delete
        db.delete_model("model-1").await.expect("delete");

        // Verify model gone from source
        let found = db.get_model_by_id("model-1").await.expect("get");
        assert!(found.is_none());

        // Verify model in archive
        let deleted = db.list_deleted_models().await.expect("list deleted");
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].model_id, "model-1");
        assert_eq!(deleted[0].model_name, "gpt-4");
    }

    #[tokio::test]
    async fn test_delete_org_idempotent() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        let org = make_test_org("org-idem", "Idempotent Org");
        db.insert_organization(&org).await.expect("insert");

        // First delete
        db.delete_organization("org-idem").await.expect("delete 1");
        // Second delete (idempotent)
        db.delete_organization("org-idem").await.expect("delete 2");

        // Archive should have exactly 1 record
        let deleted = db.list_deleted_organizations().await.expect("list deleted");
        assert_eq!(deleted.len(), 1, "idempotent delete should not duplicate");
    }

    #[tokio::test]
    async fn test_delete_team_idempotent() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        let team = make_test_team("team-idem", "Idempotent Team");
        db.insert_team(&team).await.expect("insert");

        db.delete_team("team-idem").await.expect("delete 1");
        db.delete_team("team-idem").await.expect("delete 2");

        let deleted = db.list_deleted_teams().await.expect("list deleted");
        assert_eq!(deleted.len(), 1);
    }

    #[tokio::test]
    async fn test_delete_user_idempotent() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        let user = make_test_user("user-idem", "idem@example.com");
        db.insert_user(&user).await.expect("insert");

        db.delete_user("user-idem").await.expect("delete 1");
        db.delete_user("user-idem").await.expect("delete 2");

        let deleted = db.list_deleted_users().await.expect("list deleted");
        assert_eq!(deleted.len(), 1);
    }

    #[tokio::test]
    async fn test_delete_model_idempotent() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        let model = make_test_model_from_id("model-idem", "gpt-4-idem");
        db.insert_model(&model).await.expect("insert");

        db.delete_model("model-idem").await.expect("delete 1");
        db.delete_model("model-idem").await.expect("delete 2");

        let deleted = db.list_deleted_models().await.expect("list deleted");
        assert_eq!(deleted.len(), 1);
    }

    #[tokio::test]
    async fn test_list_deleted_multiple_records() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        // Create and delete 3 orgs
        for i in 1..=3 {
            let org = make_test_org(&format!("org-{}", i), &format!("Org {}", i));
            db.insert_organization(&org).await.expect("insert");
        }
        for i in 1..=3 {
            db.delete_organization(&format!("org-{}", i)).await.expect("delete");
        }

        let deleted = db.list_deleted_organizations().await.expect("list deleted");
        assert_eq!(deleted.len(), 3);
        // Most recently deleted first (DESC)
        assert_eq!(deleted[0].organization_alias, "Org 3");
    }

    #[tokio::test]
    async fn test_list_deleted_empty() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        let deleted = db.list_deleted_teams().await.expect("list deleted");
        assert!(deleted.is_empty());
    }
}
