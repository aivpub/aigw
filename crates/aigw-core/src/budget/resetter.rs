//! BudgetResetter — AsyncTask that resets entity spend counters at their budget period boundaries.
//!
//! Scans virtual_keys, teams, users, and organizations (via budgets) for entities
//! whose budget_reset_at has passed or is unset (first-time reset). Resets spend to
//! 0 and computes the next reset timestamp using `compute_next_reset_at`.
//!
//! Supports manual trigger via `{"entity_type": "key"|"user"|"team"|"org", "entity_ids": [...]}`.

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

use crate::async_task::{AsyncTask, NewStep, StepOutput, StepRecord};
use crate::budget::duration::compute_next_reset_at;
use crate::db::{Database, DbError};

/// How often the BudgetResetter periodic scan ticks. Single source of truth —
/// reused by `AsyncTask::tick_interval()` and the stats endpoint's `next_tick_at`.
pub const BUDGET_RESET_TICK_INTERVAL: Duration = Duration::from_secs(60);

/// BudgetResetter — resets spend counters for entities whose budget period has elapsed.
pub struct BudgetResetter;

impl BudgetResetter {
    pub fn new() -> Self {
        Self
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Internal entity types and helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// The kind of entity being reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityType {
    Key,
    Team,
    User,
    Organization,
}

impl EntityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityType::Key => "key",
            EntityType::Team => "team",
            EntityType::User => "user",
            EntityType::Organization => "org",
        }
    }

    fn table_name(&self) -> &'static str {
        match self {
            EntityType::Key => "virtual_keys",
            EntityType::Team => "teams",
            EntityType::User => "users",
            EntityType::Organization => "organizations",
        }
    }

    pub fn pk_column(&self) -> &'static str {
        match self {
            EntityType::Key => "token",
            EntityType::Team => "team_id",
            EntityType::User => "user_id",
            EntityType::Organization => "organization_id",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "key" => Some(EntityType::Key),
            "team" => Some(EntityType::Team),
            "user" => Some(EntityType::User),
            "org" => Some(EntityType::Organization),
            _ => None,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Scanned row (from SELECT queries)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Represents an entity row that needs a budget reset.
#[derive(Debug)]
pub struct ResetCandidate {
    pub entity_type: EntityType,
    pub entity_id: String,
    pub budget_duration: String,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Scanning logic
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Return the database-specific current-time function for use in SQL WHERE clauses.
///
/// SQLite uses `datetime('now')`; MySQL and PostgreSQL use `NOW()`.
fn now_func(db: &Database) -> &'static str {
    match db {
        Database::Sqlite(_) => "datetime('now')",
        Database::Mysql(_) => "NOW()",
        Database::Postgres(_) => "NOW()",
    }
}

/// Scan a single entity table (virtual_keys, teams, users) for rows needing reset.
///
/// The WHERE clause finds rows where:
///   1. budget_reset_at has passed (already set, but expired).
///   2. budget_reset_at is NULL but budget_duration IS NOT NULL (first-time reset).
async fn scan_entity_table(
    db: &Database,
    entity_type: EntityType,
) -> Result<Vec<ResetCandidate>, DbError> {
    let table = entity_type.table_name();
    let pk = entity_type.pk_column();
    let now_f = now_func(db);
    let sql = format!(
        "SELECT {pk}, budget_duration FROM {table}
         WHERE budget_duration IS NOT NULL
           AND budget_duration != ''
           AND (budget_reset_at IS NULL OR budget_reset_at < {now_f})"
    );

    let rows: Vec<(String, String)> = match db {
        Database::Sqlite(pool) => {
            sqlx::query_as::<_, (String, String)>(&sql)
                .fetch_all(pool)
                .await?
        }
        Database::Mysql(pool) => {
            sqlx::query_as::<_, (String, String)>(&sql)
                .fetch_all(pool)
                .await?
        }
        Database::Postgres(pool) => {
            sqlx::query_as::<_, (String, String)>(&sql)
                .fetch_all(pool)
                .await?
        }
    };

    Ok(rows
        .into_iter()
        .map(|(entity_id, budget_duration)| ResetCandidate {
            entity_type,
            entity_id,
            budget_duration,
        })
        .collect())
}

/// Scan organizations via a JOIN with budgets.
///
/// organizations.budget_id -> budgets.budget_id, where budget_duration IS NOT NULL
/// and budget_reset_at has passed or is unset.
async fn scan_organizations(db: &Database) -> Result<Vec<ResetCandidate>, DbError> {
    let now_f = now_func(db);
    let sql = format!(
        r#"SELECT o.organization_id, b.budget_duration
        FROM organizations o
        JOIN budgets b ON o.budget_id = b.budget_id
        WHERE b.budget_duration IS NOT NULL
          AND b.budget_duration != ''
          AND (b.budget_reset_at IS NULL OR b.budget_reset_at < {now_f})"#
    );

    let rows: Vec<(String, String)> = match db {
        Database::Sqlite(pool) => {
            sqlx::query_as::<_, (String, String)>(&sql)
                .fetch_all(pool)
                .await?
        }
        Database::Mysql(pool) => {
            sqlx::query_as::<_, (String, String)>(&sql)
                .fetch_all(pool)
                .await?
        }
        Database::Postgres(pool) => {
            sqlx::query_as::<_, (String, String)>(&sql)
                .fetch_all(pool)
                .await?
        }
    };

    Ok(rows
        .into_iter()
        .map(|(entity_id, budget_duration)| ResetCandidate {
            entity_type: EntityType::Organization,
            entity_id,
            budget_duration,
        })
        .collect())
}

/// Scan all entity tables and return the combined list of reset candidates.
pub async fn scan_all(db: &Database) -> Result<Vec<ResetCandidate>, DbError> {
    let mut candidates = Vec::new();

    for et in [EntityType::Key, EntityType::Team, EntityType::User] {
        let mut rows = scan_entity_table(db, et).await?;
        candidates.append(&mut rows);
    }

    let mut org_rows = scan_organizations(db).await?;
    candidates.append(&mut org_rows);

    Ok(candidates)
}

/// Scan only entities of a specific type and return reset candidates.
pub async fn scan_by_type(
    db: &Database,
    entity_type: EntityType,
) -> Result<Vec<ResetCandidate>, DbError> {
    match entity_type {
        EntityType::Organization => scan_organizations(db).await,
        et => scan_entity_table(db, et).await,
    }
}

/// Convert reset candidates into NewStep entries for the async job engine.
pub fn candidates_to_steps(candidates: Vec<ResetCandidate>) -> Vec<NewStep> {
    candidates
        .into_iter()
        .map(|c| NewStep {
            key: format!("{}:{}", c.entity_type.as_str(), c.entity_id),
            payload: json!({
                "entity_type": c.entity_type.as_str(),
                "entity_id": c.entity_id,
                "budget_duration": c.budget_duration,
            }),
        })
        .collect()
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stats / preview (for the admin budget-reset UI)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// One entity that would be reset on the next tick / trigger — display-only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewEntity {
    pub entity_type: String,
    pub entity_id: String,
    /// Human-readable name/alias: COALESCE(key_alias, key_name), team_alias,
    /// user_alias, organization_alias.
    pub alias: String,
    pub spend: f64,
    /// Parsed from the TEXT-typed max_budget column (null when unset/unparseable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_budget: Option<f64>,
    pub budget_duration: String,
    /// Previous budget_reset_at (null for first-time resets), RFC3339-normalized.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_reset_at: Option<String>,
}

/// Per-entity-type ready/total counts plus a preview list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetResetStats {
    pub counts: std::collections::BTreeMap<String, EntityCount>,
    pub ready_total: i64,
    pub preview: Vec<PreviewEntity>,
}

/// `ready` = expired and due for reset; `total` = all rows of that entity type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityCount {
    pub ready: i64,
    pub total: i64,
}

/// Column names for the display SELECT per entity type (alias column).
fn alias_column(et: EntityType) -> &'static str {
    match et {
        EntityType::Key => "COALESCE(key_alias, key_name)",
        EntityType::Team => "team_alias",
        EntityType::User => "user_alias",
        EntityType::Organization => "organization_alias",
    }
}

/// Count entities of `et` whose budget period has expired (ready to reset).
///
/// Mirrors the `scan_entity_table` predicate — including the `!= ''` guard on
/// `budget_duration` — but uses `COUNT(*)` so the 30s admin-poll is cheap.
pub async fn count_expired_resets(db: &Database, et: EntityType) -> Result<i64, DbError> {
    let now_f = now_func(db);
    let sql = match et {
        EntityType::Organization => format!(
            r#"SELECT COUNT(*) FROM organizations o
               JOIN budgets b ON o.budget_id = b.budget_id
               WHERE b.budget_duration IS NOT NULL
                 AND b.budget_duration != ''
                 AND (b.budget_reset_at IS NULL OR b.budget_reset_at < {now_f})"#
        ),
        _ => format!(
            "SELECT COUNT(*) FROM {} WHERE budget_duration IS NOT NULL AND budget_duration != '' \
             AND (budget_reset_at IS NULL OR budget_reset_at < {now_f})",
            et.table_name()
        ),
    };

    let row: (i64,) = match db {
        Database::Sqlite(pool) => sqlx::query_as(&sql).fetch_one(pool).await?,
        Database::Mysql(pool) => sqlx::query_as(&sql).fetch_one(pool).await?,
        Database::Postgres(pool) => sqlx::query_as(&sql).fetch_one(pool).await?,
    };
    Ok(row.0)
}

/// Build the dialect-specific display SQL for `preview_expired`.
///
/// Extracted from `preview_expired` so the per-dialect LIMIT placeholder can be
/// unit-tested without a live connection (lazy pool — SQL is never executed).
fn preview_sql(db: &Database, et: EntityType) -> String {
    let now_f = now_func(db);
    let alias = alias_column(et);
    let limit_ph = match db {
        Database::Postgres(_) => "$1",
        _ => "?",
    };
    match et {
        EntityType::Organization => format!(
            r#"SELECT o.organization_id, {alias}, o.spend, b.max_budget,
                      b.budget_duration, {reset_at}
               FROM organizations o
               JOIN budgets b ON o.budget_id = b.budget_id
               WHERE b.budget_duration IS NOT NULL
                 AND b.budget_duration != ''
                 AND (b.budget_reset_at IS NULL OR b.budget_reset_at < {now_f})
               ORDER BY {alias} LIMIT {limit_ph}"#,
            alias = alias,
            reset_at = reset_at_expr(db, "b.budget_reset_at"),
            limit_ph = limit_ph,
        ),
        _ => format!(
            "SELECT {pk}, {alias}, spend, max_budget, budget_duration, {reset_at} \
             FROM {table} \
             WHERE budget_duration IS NOT NULL AND budget_duration != '' \
               AND (budget_reset_at IS NULL OR budget_reset_at < {now_f}) \
             ORDER BY {alias} LIMIT {limit_ph}",
            pk = et.pk_column(),
            alias = alias,
            reset_at = reset_at_expr(db, "budget_reset_at"),
            table = et.table_name(),
            limit_ph = limit_ph,
        ),
    }
}

/// Select up to `limit` entities of `et` that are due for reset, with display columns.
///
/// `budget_reset_at` is selected as TEXT per dialect (mirroring `execute_reset`):
/// sqlite plain, mysql `DATE_FORMAT`, pg `::text`. The server normalizes to RFC3339
/// for display (mixed formats: backfill writes `%Y-%m-%d %H:%M:%S`, reset writes RFC3339).
///
/// The LIMIT placeholder is dialect-specific: sqlite/mysql use `?`, pg uses `$1`
/// (`?` is not valid PostgreSQL — sqlx's `?`-style bound params are only usable with
/// the `any` driver). Using the wrong placeholder surfaces as
/// `syntax error at end of input` on PG, which is exactly what broke
/// `GET /admin/budget-reset/stats` in the real-PG BDD run.
pub async fn preview_expired(
    db: &Database,
    et: EntityType,
    limit: i64,
) -> Result<Vec<PreviewEntity>, DbError> {
    let sql = preview_sql(db, et);

    // (entity_id, alias, spend, max_budget TEXT, budget_duration, budget_reset_at TEXT)
    type PreviewRow = (String, Option<String>, f64, Option<String>, String, Option<String>);
    let rows: Vec<PreviewRow> = match db {
        Database::Sqlite(pool) => sqlx::query_as(&sql).bind(limit).fetch_all(pool).await?,
        Database::Mysql(pool) => sqlx::query_as(&sql).bind(limit).fetch_all(pool).await?,
        Database::Postgres(pool) => sqlx::query_as(&sql).bind(limit).fetch_all(pool).await?,
    };

    Ok(rows
        .into_iter()
        .map(|(entity_id, alias, spend, max_budget, budget_duration, budget_reset_at)| {
            let alias = alias.unwrap_or_else(|| entity_id.clone());
            PreviewEntity {
                entity_type: et.as_str().to_string(),
                entity_id,
                alias,
                spend,
                max_budget: max_budget.and_then(|s| s.parse::<f64>().ok()),
                budget_duration,
                budget_reset_at,
            }
        })
        .collect())
}

/// Build the dialect-specific expression selecting a timestamp column as text.
fn reset_at_expr(db: &Database, col: &str) -> String {
    match db {
        Database::Sqlite(_) => col.to_string(),
        Database::Mysql(_) => format!("DATE_FORMAT({col}, '%Y-%m-%d %H:%i:%s')"),
        Database::Postgres(_) => format!("{col}::text"),
    }
}

/// Orchestrate the per-entity ready/total counts and previews for the stats endpoint.
///
/// Reuses the existing `count_*` helpers on `Database` for the total columns, and
/// `count_expired_resets`/`preview_expired` for the ready/preview data.
pub async fn budget_reset_stats(db: &Database, preview_limit: i64) -> Result<BudgetResetStats, DbError> {
    let keys_ready = count_expired_resets(db, EntityType::Key).await?;
    let teams_ready = count_expired_resets(db, EntityType::Team).await?;
    let users_ready = count_expired_resets(db, EntityType::User).await?;
    let orgs_ready = count_expired_resets(db, EntityType::Organization).await?;

    let keys_total = db.count_keys(None, None).await?;
    let teams_total = db.count_teams_store(None).await?;
    let users_total = db.count_users().await?;
    let orgs_total = db.count_organizations_store().await?;

    let mut preview = Vec::new();
    for et in [
        EntityType::Key,
        EntityType::Team,
        EntityType::User,
        EntityType::Organization,
    ] {
        preview.extend(preview_expired(db, et, preview_limit).await?);
    }

    let mut counts = std::collections::BTreeMap::new();
    counts.insert("key".to_string(), EntityCount { ready: keys_ready, total: keys_total });
    counts.insert("team".to_string(), EntityCount { ready: teams_ready, total: teams_total });
    counts.insert("user".to_string(), EntityCount { ready: users_ready, total: users_total });
    counts.insert("org".to_string(), EntityCount { ready: orgs_ready, total: orgs_total });

    let ready_total = keys_ready + teams_ready + users_ready + orgs_ready;

    Ok(BudgetResetStats {
        counts,
        ready_total,
        preview,
    })
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Reset execution
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Execute the actual spend reset for a single entity.
///
/// Steps:
///  1. Query the current budget_reset_at (before overwriting it).
///  2. Compute next_reset_at from budget_duration using compute_next_reset_at().
///  3. For organizations: UPDATE budgets (via joined budget_id) and reset spend on
///     organizations.spend.
///  4. For other entities: UPDATE <table> SET spend = 0, budget_reset_at = ?.
async fn execute_reset(
    db: &Database,
    entity_type: EntityType,
    entity_id: &str,
    budget_duration: &str,
) -> Result<StepOutput, DbError> {
    let now = Utc::now();

    // Query current budget_reset_at before overwriting
    let previous_reset_at: Option<String> = match entity_type {
        EntityType::Organization => {
            let sql = r#"
                SELECT b.budget_reset_at
                FROM organizations o
                JOIN budgets b ON o.budget_id = b.budget_id
                WHERE o.organization_id = ?
            "#;
            match db {
                Database::Sqlite(pool) => sqlx::query_scalar(sql)
                    .bind(entity_id)
                    .fetch_optional(pool)
                    .await?
                    .flatten(),
                Database::Mysql(pool) => sqlx::query_scalar(
                    r#"SELECT DATE_FORMAT(b.budget_reset_at, '%Y-%m-%d %H:%i:%s')
                        FROM organizations o
                        JOIN budgets b ON o.budget_id = b.budget_id
                        WHERE o.organization_id = ?"#,
                )
                .bind(entity_id)
                .fetch_optional(pool)
                .await?
                .flatten(),
                Database::Postgres(pool) => sqlx::query_scalar(
                    r#"SELECT b.budget_reset_at::text
                        FROM organizations o
                        JOIN budgets b ON o.budget_id = b.budget_id
                        WHERE o.organization_id = $1"#,
                )
                .bind(entity_id)
                .fetch_optional(pool)
                .await?
                .flatten(),
            }
        }
        _ => {
            let sql = format!(
                "SELECT budget_reset_at FROM {} WHERE {} = ?",
                entity_type.table_name(),
                entity_type.pk_column()
            );
            match db {
                Database::Sqlite(pool) => sqlx::query_scalar(&sql)
                    .bind(entity_id)
                    .fetch_optional(pool)
                    .await?
                    .flatten(),
                Database::Mysql(pool) => {
                    let mysql_sql = format!(
                        "SELECT DATE_FORMAT(budget_reset_at, '%Y-%m-%d %H:%i:%s') FROM {} WHERE {} = ?",
                        entity_type.table_name(),
                        entity_type.pk_column()
                    );
                    sqlx::query_scalar(&mysql_sql)
                        .bind(entity_id)
                        .fetch_optional(pool)
                        .await?
                        .flatten()
                }
                Database::Postgres(pool) => {
                    let pg_sql = format!(
                        "SELECT budget_reset_at::text FROM {} WHERE {} = $1",
                        entity_type.table_name(),
                        entity_type.pk_column()
                    );
                    sqlx::query_scalar(&pg_sql)
                        .bind(entity_id)
                        .fetch_optional(pool)
                        .await?
                        .flatten()
                }
            }
        }
    };

    // Compute next reset time
    // Defensive: treat empty string as if no duration were set (shouldn't reach here
    // after the scan SQL fix, but protects against race conditions or stale jobs).
    let trimmed = budget_duration.trim();
    if trimmed.is_empty() {
        return Err(DbError::Other(format!(
            "skip reset: entity '{entity_id}' ({}) has empty budget_duration (set it or clear budget_reset_at)",
            entity_type.as_str()
        )));
    }
    let next_reset_at =
        compute_next_reset_at(trimmed, now, None, None).ok_or_else(|| {
            DbError::Other(format!(
                "unable to compute next_reset_at for '{trimmed}'"
            ))
        })?;
    let next_reset_at_str = next_reset_at.to_rfc3339();

    // Execute the UPDATE(s)
    match entity_type {
        EntityType::Organization => {
            // Update budgets.budget_reset_at (subquery on organization_id)
            let sql_budget = r#"
                UPDATE budgets SET budget_reset_at = ?
                WHERE budget_id = (SELECT budget_id FROM organizations WHERE organization_id = ?)
            "#;
            let sql_spend = "UPDATE organizations SET spend = 0 WHERE organization_id = ?";

            match db {
                Database::Sqlite(pool) => {
                    sqlx::query(sql_budget)
                        .bind(&next_reset_at_str)
                        .bind(entity_id)
                        .execute(pool)
                        .await?;
                    sqlx::query(sql_spend).bind(entity_id).execute(pool).await?;
                }
                Database::Mysql(pool) => {
                    sqlx::query(sql_budget)
                        .bind(&next_reset_at_str)
                        .bind(entity_id)
                        .execute(pool)
                        .await?;
                    sqlx::query(sql_spend).bind(entity_id).execute(pool).await?;
                }
                Database::Postgres(pool) => {
                    let pg_sql_budget = r#"
                        UPDATE budgets SET budget_reset_at = $1::timestamptz
                        WHERE budget_id = (SELECT budget_id FROM organizations WHERE organization_id = $2)
                    "#;
                    sqlx::query(pg_sql_budget)
                        .bind(&next_reset_at_str)
                        .bind(entity_id)
                        .execute(pool)
                        .await?;
                    sqlx::query("UPDATE organizations SET spend = 0 WHERE organization_id = $1")
                        .bind(entity_id)
                        .execute(pool)
                        .await?;
                }
            }
        }
        _ => {
            let sql = format!(
                "UPDATE {} SET spend = 0, budget_reset_at = ? WHERE {} = ?",
                entity_type.table_name(),
                entity_type.pk_column()
            );

            match db {
                Database::Sqlite(pool) => {
                    sqlx::query(&sql)
                        .bind(&next_reset_at_str)
                        .bind(entity_id)
                        .execute(pool)
                        .await?;
                }
                Database::Mysql(pool) => {
                    sqlx::query(&sql)
                        .bind(&next_reset_at_str)
                        .bind(entity_id)
                        .execute(pool)
                        .await?;
                }
                Database::Postgres(pool) => {
                    let pg_sql = format!(
                        "UPDATE {} SET spend = 0, budget_reset_at = $1::timestamptz WHERE {} = $2",
                        entity_type.table_name(),
                        entity_type.pk_column()
                    );
                    sqlx::query(&pg_sql)
                        .bind(&next_reset_at_str)
                        .bind(entity_id)
                        .execute(pool)
                        .await?;
                }
            }
        }
    }

    Ok(StepOutput {
        result: json!({
            "entity_type": entity_type.as_str(),
            "entity_id": entity_id,
            "previous_reset_at": previous_reset_at,
            "new_reset_at": next_reset_at_str,
            "reset_at_utc": now.to_rfc3339(),
        }),
    })
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// AsyncTask implementation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[async_trait]
impl AsyncTask for BudgetResetter {
    fn step_type(&self) -> &'static str {
        "budget_reset"
    }

    fn tick_interval(&self) -> Duration {
        BUDGET_RESET_TICK_INTERVAL
    }

    fn concurrency(&self) -> usize {
        1
    }

    /// Periodic scan: find all entities whose budget_reset_at has passed.
    async fn tick(&self, db: &Database) -> crate::db::Result<Option<Vec<NewStep>>> {
        let candidates = scan_all(db).await?;
        if candidates.is_empty() {
            return Ok(None);
        }
        Ok(Some(candidates_to_steps(candidates)))
    }

    /// Execute a single budget reset for one entity.
    async fn execute(&self, db: &Database, step: &StepRecord) -> crate::db::Result<StepOutput> {
        let entity_type_str = step.payload["entity_type"]
            .as_str()
            .ok_or_else(|| DbError::Other("payload missing entity_type".into()))?;

        let entity_type = EntityType::from_str(entity_type_str)
            .ok_or_else(|| DbError::Other(format!("unknown entity_type '{entity_type_str}'")))?;

        let entity_id = step.payload["entity_id"]
            .as_str()
            .ok_or_else(|| DbError::Other("payload missing entity_id".into()))?;

        let budget_duration = step.payload["budget_duration"]
            .as_str()
            .ok_or_else(|| DbError::Other("payload missing budget_duration".into()))?;

        execute_reset(db, entity_type, entity_id, budget_duration).await
    }

    /// Manual trigger: `{"entity_type": "key"|"user"|"team"|"org", "entity_ids": [...]}`
    ///
    /// Because `steps_from_payload` doesn't have DB access, the caller must provide
    /// explicit entity_ids. Scanning all tables is handled by periodic tick, or the
    /// caller can list ids explicitly.
    async fn steps_from_payload(
        &self,
        payload: &serde_json::Value,
    ) -> crate::db::Result<Vec<NewStep>> {
        let entity_type_str = payload
            .get("entity_type")
            .and_then(|v| v.as_str())
            .unwrap_or("all");

        let entity_ids = payload
            .get("entity_ids")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                DbError::Other(
                    "entity_ids array required for manual trigger (scan uses periodic tick)".into(),
                )
            })?;

        if entity_type_str == "all" {
            return Err(DbError::Other(
                "manual trigger with entity_type='all' requires explicit entity_ids".into(),
            ));
        }

        let entity_type = EntityType::from_str(entity_type_str)
            .ok_or_else(|| DbError::Other(format!("unknown entity_type '{entity_type_str}'")))?;

        let budget_duration = payload
            .get("budget_duration")
            .and_then(|v| v.as_str())
            .unwrap_or("monthly");

        let steps: Vec<NewStep> = entity_ids
            .iter()
            .map(|v| {
                let id = v.as_str().unwrap_or_default().to_string();
                NewStep {
                    key: format!("{}:{}", entity_type.as_str(), id),
                    payload: json!({
                        "entity_type": entity_type.as_str(),
                        "entity_id": id,
                        "budget_duration": budget_duration,
                    }),
                }
            })
            .collect();

        Ok(steps)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Unit tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_type_from_str() {
        assert_eq!(EntityType::from_str("key"), Some(EntityType::Key));
        assert_eq!(EntityType::from_str("team"), Some(EntityType::Team));
        assert_eq!(EntityType::from_str("user"), Some(EntityType::User));
        assert_eq!(EntityType::from_str("org"), Some(EntityType::Organization));
        assert_eq!(EntityType::from_str("unknown"), None);
        assert_eq!(EntityType::from_str(""), None);
    }

    #[test]
    fn test_entity_type_table_names() {
        assert_eq!(EntityType::Key.table_name(), "virtual_keys");
        assert_eq!(EntityType::Team.table_name(), "teams");
        assert_eq!(EntityType::User.table_name(), "users");
        assert_eq!(EntityType::Organization.table_name(), "organizations");
    }

    #[test]
    fn test_entity_type_pk_columns() {
        assert_eq!(EntityType::Key.pk_column(), "token");
        assert_eq!(EntityType::Team.pk_column(), "team_id");
        assert_eq!(EntityType::User.pk_column(), "user_id");
        assert_eq!(EntityType::Organization.pk_column(), "organization_id");
    }

    #[test]
    fn test_candidates_to_steps() {
        let candidates = vec![
            ResetCandidate {
                entity_type: EntityType::Key,
                entity_id: "hash123".into(),
                budget_duration: "1mo".into(),
            },
            ResetCandidate {
                entity_type: EntityType::Team,
                entity_id: "team-1".into(),
                budget_duration: "7d".into(),
            },
        ];

        let steps = candidates_to_steps(candidates);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].key, "key:hash123");
        assert_eq!(steps[0].payload["entity_type"], "key");
        assert_eq!(steps[0].payload["entity_id"], "hash123");
        assert_eq!(steps[0].payload["budget_duration"], "1mo");
        assert_eq!(steps[1].key, "team:team-1");
        assert_eq!(steps[1].payload["entity_type"], "team");
    }

    #[test]
    fn test_resetter_metadata() {
        let r = BudgetResetter::new();
        assert_eq!(r.step_type(), "budget_reset");
        assert_eq!(r.tick_interval(), Duration::from_secs(60));
        assert_eq!(r.concurrency(), 1);
    }

    // ── steps_from_payload unit tests ──────────────────────────────

    #[tokio::test]
    async fn test_steps_from_payload_with_entity_ids() {
        let resetter = BudgetResetter::new();
        let payload = json!({
            "entity_type": "key",
            "entity_ids": ["hash1", "hash2"],
            "budget_duration": "1mo"
        });

        let steps = resetter.steps_from_payload(&payload).await.unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].key, "key:hash1");
        assert_eq!(steps[0].payload["entity_type"], "key");
        assert_eq!(steps[0].payload["entity_id"], "hash1");
        assert_eq!(steps[0].payload["budget_duration"], "1mo");
        assert_eq!(steps[1].key, "key:hash2");
        assert_eq!(steps[1].payload["entity_type"], "key");
        assert_eq!(steps[1].payload["entity_id"], "hash2");
    }

    #[tokio::test]
    async fn test_steps_from_payload_missing_ids_fails() {
        let resetter = BudgetResetter::new();
        let payload = json!({"entity_type": "key"});
        let result = resetter.steps_from_payload(&payload).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_steps_from_payload_all_requires_ids() {
        let resetter = BudgetResetter::new();
        let payload = json!({"entity_type": "all", "entity_ids": []});
        let result = resetter.steps_from_payload(&payload).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_steps_from_payload_default_budget_duration() {
        let resetter = BudgetResetter::new();
        let payload = json!({
            "entity_type": "team",
            "entity_ids": ["team-a"]
        });

        let steps = resetter.steps_from_payload(&payload).await.unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].payload["budget_duration"], "monthly");
    }

    // ── Integration tests with in-memory SQLite ────────────────────

    /// Helper: get the SQLite pool ref from a Database enum.
    fn sqlite_pool(db: &Database) -> &sqlx::SqlitePool {
        match db {
            Database::Sqlite(pool) => pool,
            _ => panic!("expected SQLite database"),
        }
    }

    #[tokio::test]
    async fn test_scan_entity_table_empty_db() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let candidates = scan_entity_table(&db, EntityType::Key).await.unwrap();
        assert!(candidates.is_empty());
    }

    #[tokio::test]
    async fn test_reset_virtual_key_with_expired_budget_reset_at() {
        use crate::crypto::hash_token;

        let db = Database::init("sqlite::memory:").await.unwrap();
        let hash = hash_token("sk-test-reset-1");
        let past = (Utc::now() - chrono::Duration::hours(2))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        sqlx::query(
            r#"INSERT INTO virtual_keys (token, key_name, spend, budget_duration, budget_reset_at, models, aliases, config, permissions, metadata, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, soft_budget_cooldown)
               VALUES (?, 'test-key', 100.0, '1mo', ?, '[]', '{}', '{}', '{}', '{}', '[]', '[]', '[]', '[]', '{}', '{}', 'false')"#,
        )
        .bind(&hash)
        .bind(&past)
        .execute(sqlite_pool(&db))
        .await
        .unwrap();

        let candidates = scan_entity_table(&db, EntityType::Key).await.unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].entity_id, hash);

        let result = execute_reset(&db, EntityType::Key, &hash, "1mo")
            .await
            .unwrap();
        let r = &result.result;
        assert_eq!(r["entity_type"], "key");
        assert!(r["new_reset_at"].as_str().unwrap() > past.as_str());

        let spend: (f64,) = sqlx::query_as("SELECT spend FROM virtual_keys WHERE token = ?")
            .bind(&hash)
            .fetch_one(sqlite_pool(&db))
            .await
            .unwrap();
        assert_eq!(spend.0, 0.0);
    }

    #[tokio::test]
    async fn test_first_time_reset_null_budget_reset_at() {
        use crate::crypto::hash_token;

        let db = Database::init("sqlite::memory:").await.unwrap();
        let hash = hash_token("sk-first-time");

        sqlx::query(
            r#"INSERT INTO virtual_keys (token, key_name, spend, budget_duration, budget_reset_at, models, aliases, config, permissions, metadata, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, soft_budget_cooldown)
               VALUES (?, 'first-time', 50.0, 'daily', NULL, '[]', '{}', '{}', '{}', '{}', '[]', '[]', '[]', '[]', '{}', '{}', 'false')"#,
        )
        .bind(&hash)
        .execute(sqlite_pool(&db))
        .await
        .unwrap();

        let candidates = scan_entity_table(&db, EntityType::Key).await.unwrap();
        assert_eq!(candidates.len(), 1);

        let result = execute_reset(&db, EntityType::Key, &hash, "daily")
            .await
            .unwrap();
        assert_eq!(result.result["entity_type"], "key");
        assert!(result.result["new_reset_at"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_no_reset_when_budget_reset_at_is_future() {
        use crate::crypto::hash_token;

        let db = Database::init("sqlite::memory:").await.unwrap();
        let hash = hash_token("sk-fresh");
        let future = (Utc::now() + chrono::Duration::hours(24))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        sqlx::query(
            r#"INSERT INTO virtual_keys (token, key_name, spend, budget_duration, budget_reset_at, models, aliases, config, permissions, metadata, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, soft_budget_cooldown)
               VALUES (?, 'fresh', 50.0, '1mo', ?, '[]', '{}', '{}', '{}', '{}', '[]', '[]', '[]', '[]', '{}', '{}', 'false')"#,
        )
        .bind(&hash)
        .bind(&future)
        .execute(sqlite_pool(&db))
        .await
        .unwrap();

        let candidates = scan_entity_table(&db, EntityType::Key).await.unwrap();
        assert!(candidates.is_empty());
    }

    // ── now_func unit tests ──────────────────────────────────

    #[tokio::test]
    async fn test_now_func_sqlite() {
        let db = Database::Sqlite(sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap());
        assert_eq!(now_func(&db), "datetime('now')");
    }

    #[tokio::test]
    async fn test_now_func_mysql_uses_now() {
        // MySQL lazy pool won't connect — now_func only inspects the variant
        let pool = sqlx::MySqlPool::connect_lazy("mysql://localhost").unwrap();
        let db = Database::Mysql(pool);
        assert_eq!(now_func(&db), "NOW()");
    }

    #[tokio::test]
    async fn test_now_func_postgres_uses_now() {
        // Postgres lazy pool won't connect — now_func only inspects the variant
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost").unwrap();
        let db = Database::Postgres(pool);
        assert_eq!(now_func(&db), "NOW()");
    }

    // ── scan_entity_table edge case tests ────────────────────

    #[tokio::test]
    async fn test_scan_entity_table_mixed_states() {
        use crate::crypto::hash_token;
        use chrono::Duration;

        let db = Database::init("sqlite::memory:").await.unwrap();

        let past = (Utc::now() - Duration::hours(2))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let future = (Utc::now() + Duration::hours(24))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        let pool = sqlite_pool(&db);

        let expired = hash_token("sk-expired");
        let null_rst = hash_token("sk-null");
        let fresh = hash_token("sk-fresh");
        let no_dur = hash_token("sk-no-dur");

        sqlx::query(
            r#"INSERT INTO virtual_keys (token, key_name, spend, budget_duration, budget_reset_at, models, aliases, config, permissions, metadata, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, soft_budget_cooldown)
               VALUES (?, 'test', 0, '1mo', ?, '[]', '{}', '{}', '{}', '{}', '[]', '[]', '[]', '[]', '{}', '{}', 'false')"#,
        )
        .bind(&expired).bind(&past)
        .execute(pool).await.unwrap();

        sqlx::query(
            r#"INSERT INTO virtual_keys (token, key_name, spend, budget_duration, budget_reset_at, models, aliases, config, permissions, metadata, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, soft_budget_cooldown)
               VALUES (?, 'test', 0, 'daily', NULL, '[]', '{}', '{}', '{}', '{}', '[]', '[]', '[]', '[]', '{}', '{}', 'false')"#,
        )
        .bind(&null_rst)
        .execute(pool).await.unwrap();

        sqlx::query(
            r#"INSERT INTO virtual_keys (token, key_name, spend, budget_duration, budget_reset_at, models, aliases, config, permissions, metadata, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, soft_budget_cooldown)
               VALUES (?, 'test', 0, '1mo', ?, '[]', '{}', '{}', '{}', '{}', '[]', '[]', '[]', '[]', '{}', '{}', 'false')"#,
        )
        .bind(&fresh).bind(&future)
        .execute(pool).await.unwrap();

        sqlx::query(
            r#"INSERT INTO virtual_keys (token, key_name, spend, budget_duration, budget_reset_at, models, aliases, config, permissions, metadata, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, soft_budget_cooldown)
               VALUES (?, 'test', 0, NULL, NULL, '[]', '{}', '{}', '{}', '{}', '[]', '[]', '[]', '[]', '{}', '{}', 'false')"#,
        )
        .bind(&no_dur)
        .execute(pool).await.unwrap();

        let candidates = scan_entity_table(&db, EntityType::Key).await.unwrap();
        assert_eq!(
            candidates.len(),
            2,
            "should find only expired and null-reset-at keys"
        );

        let ids: Vec<&str> = candidates.iter().map(|c| c.entity_id.as_str()).collect();
        assert!(
            ids.contains(&expired.as_str()),
            "should contain expired key"
        );
        assert!(
            ids.contains(&null_rst.as_str()),
            "should contain null-reset-at key"
        );
    }

    #[tokio::test]
    async fn test_scan_entity_table_skips_budget_duration_null() {
        use crate::crypto::hash_token;

        let db = Database::init("sqlite::memory:").await.unwrap();
        let hash = hash_token("sk-no-bud-dur");
        let past = (Utc::now() - chrono::Duration::hours(2))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        sqlx::query(
            r#"INSERT INTO virtual_keys (token, key_name, spend, budget_duration, budget_reset_at, models, aliases, config, permissions, metadata, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, soft_budget_cooldown)
               VALUES (?, 'no-dur', 0, NULL, ?, '[]', '{}', '{}', '{}', '{}', '[]', '[]', '[]', '[]', '{}', '{}', 'false')"#,
        )
        .bind(&hash)
        .bind(&past)
        .execute(sqlite_pool(&db))
        .await
        .unwrap();

        let candidates = scan_entity_table(&db, EntityType::Key).await.unwrap();
        assert!(
            candidates.is_empty(),
            "key without budget_duration should not be scanned"
        );
    }

    #[tokio::test]
    async fn test_scan_all_combines_all_types() {
        use crate::crypto::hash_token;

        let db = Database::init("sqlite::memory:").await.unwrap();
        let pool = sqlite_pool(&db);
        let past = (Utc::now() - chrono::Duration::hours(1))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        // Insert key
        let kh = hash_token("sk-scan-all");
        sqlx::query(
            r#"INSERT INTO virtual_keys (token, key_name, spend, budget_duration, budget_reset_at, models, aliases, config, permissions, metadata, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, soft_budget_cooldown)
               VALUES (?, 'scan-all', 0, 'daily', ?, '[]', '{}', '{}', '{}', '{}', '[]', '[]', '[]', '[]', '{}', '{}', 'false')"#,
        )
        .bind(&kh).bind(&past).execute(pool).await.unwrap();

        // Insert team
        sqlx::query(
            "INSERT INTO teams (team_id, spend, budget_duration, budget_reset_at, models, admins, members, members_with_roles, metadata, access_group_ids, policies, team_member_permissions, model_spend, model_max_budget, default_team_member_models, created_at, updated_at) VALUES ('t1', 0, '7d', ?, '[]','{}','{}','{}','{}','[]','[]','{}','{}','{}','[]', datetime('now'), datetime('now'))",
        )
        .bind(&past).execute(pool).await.unwrap();

        // Insert user
        sqlx::query(
            "INSERT INTO users (user_id, spend, budget_duration, budget_reset_at, models, metadata, allowed_cache_controls, policies, model_spend, model_max_budget, teams, user_email, created_at, updated_at) VALUES ('u1', 0, 'daily', ?, '[]','{}','[]','[]','{}','{}','{}','u1@test.com', datetime('now'), datetime('now'))",
        )
        .bind(&past).execute(pool).await.unwrap();

        // Insert org + budget (with expired reset)
        sqlx::query(
            "INSERT INTO budgets (budget_id, max_budget, budget_duration, budget_reset_at, model_max_budget, allowed_models, created_at, created_by, updated_at, updated_by) VALUES ('b1', '100', '1mo', ?, '{}', '[]', datetime('now'), 'test', datetime('now'), 'test')",
        )
        .bind(&past).execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO organizations (organization_id, organization_alias, budget_id, spend, models, metadata, model_spend, created_at, created_by, updated_at, updated_by) VALUES ('o1', 'org1', 'b1', 0, '[]','{}','{}', datetime('now'), 'test', datetime('now'), 'test')",
        )
        .execute(pool).await.unwrap();

        let candidates = scan_all(&db).await.unwrap();
        assert_eq!(
            candidates.len(),
            4,
            "should find 4 expired entities (key, team, user, org)"
        );

        let types: Vec<&str> = candidates.iter().map(|c| c.entity_type.as_str()).collect();
        assert!(types.contains(&"key"));
        assert!(types.contains(&"team"));
        assert!(types.contains(&"user"));
        assert!(types.contains(&"org"));
    }

    // ── Budget-reset stats / preview tests ─────────────────────────

    #[tokio::test]
    async fn test_count_expired_resets_empty_db() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        assert_eq!(count_expired_resets(&db, EntityType::Key).await.unwrap(), 0);
        assert_eq!(count_expired_resets(&db, EntityType::Team).await.unwrap(), 0);
        assert_eq!(count_expired_resets(&db, EntityType::User).await.unwrap(), 0);
        assert_eq!(
            count_expired_resets(&db, EntityType::Organization).await.unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn test_count_expired_resets_mixed_states() {
        use crate::crypto::hash_token;

        let db = Database::init("sqlite::memory:").await.unwrap();
        let pool = sqlite_pool(&db);
        let past = (Utc::now() - chrono::Duration::hours(2))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let future = (Utc::now() + chrono::Duration::hours(24))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        let expired = hash_token("sk-count-expired");
        let fresh = hash_token("sk-count-fresh");
        let no_dur = hash_token("sk-count-no-dur");
        let empty_dur = hash_token("sk-count-empty-dur");

        for (token, dur, rst) in [
            (&expired, "1mo", &past),
            (&fresh, "1mo", &future),
            (&no_dur, "", &past),
            (&empty_dur, "", &past),
        ] {
            let dur_opt: Option<&str> = if dur.is_empty() { None } else { Some(dur) };
            let rst_opt: Option<&str> = if rst.is_empty() { None } else { Some(rst) };
            sqlx::query(
                r#"INSERT INTO virtual_keys (token, key_name, spend, budget_duration, budget_reset_at, models, aliases, config, permissions, metadata, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, soft_budget_cooldown)
                   VALUES (?, 't', 0, ?, ?, '[]', '{}', '{}', '{}', '{}', '[]', '[]', '[]', '[]', '{}', '{}', 'false')"#,
            )
            .bind(token)
            .bind(dur_opt)
            .bind(rst_opt)
            .execute(pool)
            .await
            .unwrap();
        }

        // Only the expired row (past reset_at + valid duration) counts. The empty-string
        // duration row must be excluded (the `!= ''` guard the db.rs find_expired_* lacks).
        assert_eq!(count_expired_resets(&db, EntityType::Key).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_preview_expired_returns_alias_not_hash() {
        use crate::crypto::hash_token;

        let db = Database::init("sqlite::memory:").await.unwrap();
        let pool = sqlite_pool(&db);
        let past = (Utc::now() - chrono::Duration::hours(1))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let kh = hash_token("sk-preview");
        let expired = hash_token("sk-preview-2");

        sqlx::query(
            r#"INSERT INTO virtual_keys (token, key_name, key_alias, spend, budget_duration, budget_reset_at, max_budget, models, aliases, config, permissions, metadata, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, soft_budget_cooldown)
               VALUES (?, 'prod-key-name', 'prod-key-alias', 12.4, '1mo', ?, '50', '[]', '{}', '{}', '{}', '{}', '[]', '[]', '[]', '[]', '{}', '{}', 'false')"#,
        )
        .bind(&kh).bind(&past).execute(pool).await.unwrap();
        sqlx::query(
            r#"INSERT INTO virtual_keys (token, key_name, spend, budget_duration, budget_reset_at, models, aliases, config, permissions, metadata, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, soft_budget_cooldown)
               VALUES (?, NULL, 3.0, '7d', NULL, '[]', '{}', '{}', '{}', '{}', '[]', '[]', '[]', '[]', '{}', '{}', 'false')"#,
        )
        .bind(&expired).execute(pool).await.unwrap();

        let preview = preview_expired(&db, EntityType::Key, 10).await.unwrap();
        assert_eq!(preview.len(), 2);

        let by_alias: std::collections::HashMap<_, _> = preview
            .into_iter()
            .map(|p| (p.alias.clone(), p))
            .collect();
        let first = by_alias.get("prod-key-alias").expect("alias-preferred key");
        assert_eq!(first.entity_id, kh);
        assert_eq!(first.max_budget, Some(50.0));
        assert_eq!(first.budget_duration, "1mo");
        assert!(first.budget_reset_at.is_some());
        assert_eq!(first.spend, 12.4);

        // A key with no alias falls back to its hashed token as the display name.
        assert!(by_alias.get(&expired).is_some());
    }

    #[tokio::test]
    async fn test_budget_reset_stats_combines_all_types() {
        use crate::crypto::hash_token;

        let db = Database::init("sqlite::memory:").await.unwrap();
        let pool = sqlite_pool(&db);
        let past = (Utc::now() - chrono::Duration::hours(1))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        // 2 expired keys (one alias), 1 fresh key (future reset_at)
        let kh1 = hash_token("sk-stats-1");
        let kh2 = hash_token("sk-stats-2");
        let kh3 = hash_token("sk-stats-3");
        let future = (Utc::now() + chrono::Duration::hours(24))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        for (token, alias, rst_opt) in [
            (&kh1, "key-one", Some(past.as_str())),
            (&kh2, "key-two", Some(past.as_str())),
            (&kh3, "key-fresh", Some(future.as_str())),
        ] {
            sqlx::query(
                r#"INSERT INTO virtual_keys (token, key_name, spend, budget_duration, budget_reset_at, models, aliases, config, permissions, metadata, allowed_cache_controls, allowed_routes, policies, access_group_ids, model_spend, model_max_budget, soft_budget_cooldown)
                   VALUES (?, ?, 0, 'daily', ?, '[]', '{}', '{}', '{}', '{}', '[]', '[]', '[]', '[]', '{}', '{}', 'false')"#,
            )
            .bind(token)
            .bind(alias)
            .bind(rst_opt)
            .execute(pool)
            .await
            .unwrap();
        }

        // team + user expired
        sqlx::query(
            "INSERT INTO teams (team_id, team_alias, spend, budget_duration, budget_reset_at, models, admins, members, members_with_roles, metadata, access_group_ids, policies, team_member_permissions, model_spend, model_max_budget, default_team_member_models, created_at, updated_at) VALUES ('t1', 'team-a', 0, '7d', ?, '[]','{}','{}','{}','{}','[]','[]','{}','{}','{}','[]', datetime('now'), datetime('now'))",
        )
        .bind(&past).execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO users (user_id, user_alias, spend, budget_duration, budget_reset_at, models, metadata, allowed_cache_controls, policies, model_spend, model_max_budget, teams, user_email, created_at, updated_at) VALUES ('u1', 'user-a', 0, 'daily', ?, '[]','{}','[]','[]','{}','{}','{}','u1@test.com', datetime('now'), datetime('now'))",
        )
        .bind(&past).execute(pool).await.unwrap();

        // org + budget expired
        sqlx::query(
            "INSERT INTO budgets (budget_id, max_budget, budget_duration, budget_reset_at, model_max_budget, allowed_models, created_at, created_by, updated_at, updated_by) VALUES ('b1', '100', '1mo', ?, '{}', '[]', datetime('now'), 'test', datetime('now'), 'test')",
        )
        .bind(&past).execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO organizations (organization_id, organization_alias, budget_id, spend, models, metadata, model_spend, created_at, created_by, updated_at, updated_by) VALUES ('o1', 'org-a', 'b1', 0, '[]','{}','{}', datetime('now'), 'test', datetime('now'), 'test')",
        )
        .execute(pool).await.unwrap();

        let stats = budget_reset_stats(&db, 10).await.unwrap();
        assert_eq!(stats.counts["key"].ready, 2);
        assert_eq!(stats.counts["key"].total, 3);
        assert_eq!(stats.counts["team"].ready, 1);
        assert_eq!(stats.counts["user"].ready, 1);
        assert_eq!(stats.counts["org"].ready, 1);
        assert_eq!(stats.ready_total, 5);
        assert_eq!(stats.preview.len(), 5);

        let types: std::collections::HashSet<&str> = stats
            .preview
            .iter()
            .map(|p| p.entity_type.as_str())
            .collect();
        assert!(types.contains("key"));
        assert!(types.contains("team"));
        assert!(types.contains("user"));
        assert!(types.contains("org"));
    }

    // ── preview_sql dialect LIMIT placeholder tests ──────────────────

    /// The PG preview SQL must use `$1` for LIMIT, never `?` — `?` is not valid
    /// PostgreSQL and sqlx only rewrites it under the `any` driver. A stray `?`
    /// surfaces as `syntax error at end of input` on the live PG pool, which is
    /// exactly the 500 seen on `GET /admin/budget-reset/stats` in real-PG BDD.
    #[tokio::test]
    async fn test_preview_sql_uses_pg_limit_placeholder() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost").unwrap();
        let db = Database::Postgres(pool);

        let key_sql = preview_sql(&db, EntityType::Key);
        assert!(
            key_sql.contains("LIMIT $1"),
            "pg key preview must use $1, got: {key_sql}"
        );
        assert!(!key_sql.contains("?"), "pg key preview must not use ?, got: {key_sql}");

        let org_sql = preview_sql(&db, EntityType::Organization);
        assert!(
            org_sql.contains("LIMIT $1"),
            "pg org preview must use $1, got: {org_sql}"
        );
        assert!(!org_sql.contains("?"), "pg org preview must not use ?, got: {org_sql}");
    }

    #[tokio::test]
    async fn test_preview_sql_uses_qmark_limit_placeholder_for_sqlite_mysql() {
        for db in [
            Database::Sqlite(sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap()),
            Database::Mysql(sqlx::MySqlPool::connect_lazy("mysql://localhost").unwrap()),
        ] {
            for et in [EntityType::Key, EntityType::Organization] {
                let sql = preview_sql(&db, et);
                assert!(
                    sql.contains("LIMIT ?"),
                    "{} preview must use '?', got: {sql}",
                    match db {
                        Database::Sqlite(_) => "sqlite",
                        _ => "mysql",
                    }
                );
            }
        }
    }
}
