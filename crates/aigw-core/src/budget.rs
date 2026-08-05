//! Budget enforcement module
//!
//! Checks whether an API key has exceeded its max_budget by comparing the
//! key's `spend` field against its `max_budget`. Supports multi-level
//! enforcement: key → user → team → organization. This module works with
//! the `Database` enum to support SQLite, MySQL, and PostgreSQL backends.

pub mod duration;
pub mod resetter;

use crate::db::{Database, DbError};
use crate::middleware::KeyIdentity;
use thiserror::Error;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Error types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Errors that can occur during budget enforcement.
#[derive(Debug, Error)]
pub enum BudgetError {
    /// The entity's spend has exceeded its max_budget.
    #[error("{entity_type} budget exceeded: spent {spent:.4}, limit {limit:.4}")]
    Exceeded {
        /// Which entity type triggered the rejection ("key", "user", "team", "organization")
        entity_type: String,
        /// Total spend recorded for the entity
        spent: f64,
        /// Maximum budget allowed for the entity
        limit: f64,
    },
    /// A database error occurred while querying the entity.
    #[error("Database error: {0}")]
    DbError(#[from] DbError),
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Entity-level check helper
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Check a single entity's spend against its max_budget.
///
/// Uses `>` (strict greater-than), matching the existing key-level behavior:
/// spend exactly at max_budget is allowed (boundary pass).
fn check_entity(entity_type: &str, spend: f64, max_budget: Option<f64>) -> Result<(), BudgetError> {
    let limit = match max_budget {
        Some(mb) if mb.is_finite() && mb > 0.0 => mb,
        _ => return Ok(()),
    };
    if spend > limit {
        return Err(BudgetError::Exceeded {
            entity_type: entity_type.to_string(),
            spent: spend,
            limit,
        });
    }
    Ok(())
}

/// Log a warning if the entity has exceeded its soft_budget threshold
/// but not the hard max_budget. Never rejects — only logs.
fn check_soft_budget(
    entity_type: &str,
    entity_id: Option<&str>,
    spend: f64,
    soft_budget: Option<f64>,
) {
    let sb = match soft_budget {
        Some(v) if v.is_finite() && v > 0.0 => v,
        _ => return, // no soft_budget set → nothing to warn about
    };
    if spend > sb {
        tracing::warn!(
            entity_type = %entity_type,
            entity_id = %entity_id.unwrap_or("unknown"),
            spent = %spend,
            soft_budget = %sb,
            "soft_budget exceeded (request continues)"
        );
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// BudgetEnforcer
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Stateless budget enforcement utility.
///
/// Provides both single-level (key-only) and multi-level
/// (key → user → team → org) checking.
/// Master keys bypass budget checks entirely.
pub struct BudgetEnforcer;

impl BudgetEnforcer {
    /// Check whether the key identified by `key_hash` still has remaining budget.
    ///
    /// Looks up the key in the database and compares its `spend` against its
    /// `max_budget`. If `max_budget` is `None`, the key has unlimited budget.
    ///
    /// # Arguments
    ///
    /// * `db` - The database connection (supports SQLite, MySQL, PostgreSQL)
    /// * `key_hash` - The SHA256 hash of the API token
    ///
    /// # Returns
    ///
    /// * `Ok(())` if budget is not exceeded
    /// * `Err(BudgetError::Exceeded)` if spend > max_budget
    /// * `Err(BudgetError::DbError)` if the database lookup failed
    pub async fn check_budget(db: &Database, key_hash: &str) -> Result<(), BudgetError> {
        let key = db
            .get_key_by_token(key_hash)
            .await
            .map_err(BudgetError::DbError)?;

        let key = match key {
            Some(k) => k,
            None => {
                // Key not found in DB — let auth middleware handle this
                return Ok(());
            }
        };

        check_entity("key", key.spend, key.max_budget_f64())
    }

    /// Multi-level budget enforcement: key → user → team → organization.
    ///
    /// Checks each entity level in sequence. Any level exceeding its
    /// max_budget returns immediately with `BudgetError::Exceeded`.
    /// Missing intermediate entities are silently skipped with a warning
    /// (aligned with litellm behavior — don't break service due to FK gaps).
    ///
    /// # Arguments
    ///
    /// * `db` - The database connection
    /// * `key` - The authenticated key identity (carries user_id/team_id/org_id)
    ///
    /// # Note on TOCTOU
    ///
    /// Spend is updated before budget check (Stage 94 async increment), so
    /// by the time this function reads entity.spend, the current request's
    /// cost is already reflected. Concurrent requests on the same key share
    /// a ~ms race window (both pass check, cumulative spend > max_budget),
    /// which is an accepted distributed-systems trade-off (litellm same).
    pub async fn check_budget_multi(db: &Database, key: &KeyIdentity) -> Result<(), BudgetError> {
        // 1. Key level (always check)
        let k = db
            .get_key_by_token(&key.token_hash)
            .await
            .map_err(BudgetError::DbError)?;

        if let Some(k) = k {
            check_entity("key", k.spend, k.max_budget_f64())?;
            check_soft_budget("key", k.key_alias.as_deref(), k.spend, k.soft_budget_f64());
        } else {
            // Key not found — let auth middleware handle
            return Ok(());
        }

        // 2. User level (if associated)
        if let Some(ref uid) = key.user_id {
            match db.get_user_by_id(uid).await {
                Ok(Some(u)) => check_entity("user", u.spend, u.max_budget_f64())?,
                Ok(None) => {
                    tracing::warn!(
                        user_id = %uid,
                        "user budget check skipped: entity not found"
                    );
                }
                Err(e) => return Err(BudgetError::DbError(e)),
            }
        }

        // 3. Team level (if associated)
        if let Some(ref tid) = key.team_id {
            match db.get_team_by_id(tid).await {
                Ok(Some(t)) => {
                    check_entity("team", t.spend, t.max_budget_f64())?;
                    check_soft_budget("team", Some(&t.team_id), t.spend, t.soft_budget_f64());
                }
                Ok(None) => {
                    tracing::warn!(
                        team_id = %tid,
                        "team budget check skipped: entity not found"
                    );
                }
                Err(e) => return Err(BudgetError::DbError(e)),
            }
        }

        // 4. Organization level (if associated, quota from budgets table)
        if let Some(ref oid) = key.organization_id {
            match db.get_organization_by_id(oid).await {
                Ok(Some(org)) => {
                    if let Ok(Some(budget)) = db.get_budget_by_id(&org.budget_id).await {
                        check_entity("organization", org.spend, budget.max_budget_f64())?;
                        check_soft_budget(
                            "organization",
                            Some(oid),
                            org.spend,
                            budget.soft_budget_f64(),
                        );
                    }
                    // budget lookup failure: silently skip (org has no
                    // effective quota if budget row is missing)
                }
                Ok(None) => {
                    tracing::warn!(
                        org_id = %oid,
                        "org budget check skipped: entity not found"
                    );
                }
                Err(e) => return Err(BudgetError::DbError(e)),
            }
        }

        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Unit tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::hash_token;
    use crate::db::Database;
    use crate::models::{Budget, Organization, Team, User, VirtualKey};
    use chrono::Utc;
    use serde_json::json;

    fn make_test_key(token_hash: &str, spend: f64, max_budget: Option<f64>) -> VirtualKey {
        VirtualKey {
            token: token_hash.to_string(),
            key_name: Some("budget-test-key".to_string()),
            key_alias: Some("budget-test".to_string()),
            soft_budget_cooldown: "false".to_string(),
            spend,
            expires: None,
            models: json!([]),
            aliases: json!({}),
            config: json!({}),
            router_settings: None,
            user_id: Some("budget-user".to_string()),
            team_id: Some("budget-team".to_string()),
            agent_id: None,
            project_id: None,
            permissions: json!({}),
            max_parallel_requests: None,
            metadata: json!({}),
            blocked: None,
            tpm_limit: None,
            rpm_limit: None,
            max_budget: max_budget.map(|v| v.to_string()),
            soft_budget: None,
            budget_duration: None,
            budget_reset_at: None,
            allowed_cache_controls: json!([]),
            allowed_routes: json!([]),
            policies: json!([]),
            access_group_ids: json!([]),
            model_spend: json!({}),
            model_max_budget: json!({}),
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
            user_email: None,
            user_alias: None,
        }
    }

    // ── check_entity tests ──

    #[test]
    fn test_check_entity_ok() {
        assert!(check_entity("key", 50.0, Some(100.0)).is_ok());
    }

    #[test]
    fn test_check_entity_exceeded() {
        let err = check_entity("key", 150.0, Some(100.0)).unwrap_err();
        match err {
            BudgetError::Exceeded {
                entity_type,
                spent,
                limit,
            } => {
                assert_eq!(entity_type, "key");
                assert_eq!(spent, 150.0);
                assert_eq!(limit, 100.0);
            }
            _ => panic!("expected Exceeded"),
        }
    }

    #[test]
    fn test_check_entity_boundary_pass() {
        assert!(check_entity("key", 100.0, Some(100.0)).is_ok());
    }

    #[test]
    fn test_check_entity_unlimited() {
        assert!(check_entity("key", 9999.0, None).is_ok());
    }

    #[test]
    fn test_check_entity_nan_treated_as_unlimited() {
        assert!(check_entity("key", 100.0, Some(f64::NAN)).is_ok());
    }

    #[test]
    fn test_check_entity_inf_treated_as_unlimited() {
        assert!(check_entity("key", 100.0, Some(f64::INFINITY)).is_ok());
    }

    // ── single-level check_budget tests (compatible with entity_type field) ──

    #[tokio::test]
    async fn test_budget_not_exceeded() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-budget-ok";
        let hash = hash_token(raw);

        let key = make_test_key(&hash, 50.0, Some(100.0));
        db.insert_key(&key).await.expect("insert");

        let result = BudgetEnforcer::check_budget(&db, &hash).await;
        assert!(result.is_ok(), "budget should not be exceeded");
    }

    #[tokio::test]
    async fn test_budget_exceeded() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-budget-exceeded";
        let hash = hash_token(raw);

        let key = make_test_key(&hash, 150.0, Some(100.0));
        db.insert_key(&key).await.expect("insert");

        let result = BudgetEnforcer::check_budget(&db, &hash).await;
        assert!(result.is_err(), "budget should be exceeded");

        match result.unwrap_err() {
            BudgetError::Exceeded {
                entity_type: _,
                spent,
                limit,
            } => {
                assert_eq!(spent, 150.0);
                assert_eq!(limit, 100.0);
            }
            _ => panic!("expected BudgetError::Exceeded"),
        }
    }

    #[tokio::test]
    async fn test_budget_exact_boundary() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-budget-boundary";
        let hash = hash_token(raw);

        let key = make_test_key(&hash, 100.0, Some(100.0));
        db.insert_key(&key).await.expect("insert");

        let result = BudgetEnforcer::check_budget(&db, &hash).await;
        assert!(result.is_ok(), "exact budget should not exceed");
    }

    #[tokio::test]
    async fn test_budget_unlimited() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-budget-unlimited";
        let hash = hash_token(raw);

        let key = make_test_key(&hash, 999999.0, None);
        db.insert_key(&key).await.expect("insert");

        let result = BudgetEnforcer::check_budget(&db, &hash).await;
        assert!(result.is_ok(), "unlimited budget should pass");
    }

    #[tokio::test]
    async fn test_budget_zero_limit() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-budget-zero";
        let hash = hash_token(raw);

        let key = make_test_key(&hash, 10.0, Some(0.0));
        db.insert_key(&key).await.expect("insert");

        let result = BudgetEnforcer::check_budget(&db, &hash).await;
        assert!(result.is_ok(), "zero budget should be unlimited");
    }

    #[tokio::test]
    async fn test_budget_nonexistent_key() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        let result = BudgetEnforcer::check_budget(&db, "nonexistent-hash").await;
        assert!(
            result.is_ok(),
            "nonexistent key should pass (let auth handle it)"
        );
    }

    #[tokio::test]
    async fn test_budget_nan_max_budget() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-budget-nan";
        let hash = hash_token(raw);

        let key = make_test_key(&hash, 100.0, Some(f64::NAN));
        db.insert_key(&key).await.expect("insert");

        let result = BudgetEnforcer::check_budget(&db, &hash).await;
        assert!(
            result.is_ok(),
            "NaN max_budget should be treated as unlimited"
        );
    }

    #[tokio::test]
    async fn test_budget_infinity_max_budget() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-budget-inf";
        let hash = hash_token(raw);

        let key = make_test_key(&hash, 100.0, Some(f64::INFINITY));
        db.insert_key(&key).await.expect("insert");

        let result = BudgetEnforcer::check_budget(&db, &hash).await;
        assert!(
            result.is_ok(),
            "INFINITY max_budget should be treated as unlimited"
        );
    }

    // ── multi-level check_budget_multi tests ──

    #[tokio::test]
    async fn test_multi_level_key_exceeded() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-multi-key-exceeded";
        let hash = hash_token(raw);

        let mut key = make_test_key(&hash, 150.0, Some(100.0));
        key.user_id = None;
        key.team_id = None;
        key.organization_id = None;
        db.insert_key(&key).await.expect("insert");

        let identity = KeyIdentity {
            token_hash: hash,
            key_alias: None,
            user_id: None,
            team_id: None,
            organization_id: None,
            is_master_key: false,
            user_role: None,
        };

        let err = BudgetEnforcer::check_budget_multi(&db, &identity)
            .await
            .unwrap_err();
        match err {
            BudgetError::Exceeded {
                entity_type,
                spent,
                limit,
            } => {
                assert_eq!(entity_type, "key");
                assert_eq!(spent, 150.0);
                assert_eq!(limit, 100.0);
            }
            _ => panic!("expected Exceeded"),
        }
    }

    #[tokio::test]
    async fn test_multi_level_all_pass() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-multi-all-pass";
        let hash = hash_token(raw);

        let mut key = make_test_key(&hash, 50.0, Some(100.0));
        key.user_id = Some("u1".to_string());
        key.team_id = Some("t1".to_string());
        db.insert_key(&key).await.expect("insert");

        let user = User {
            user_id: "u1".to_string(),
            spend: 30.0,
            max_budget: Some("200.0".to_string()),
            user_alias: None,
            team_id: Some("t1".to_string()),
            sso_user_id: None,
            organization_id: None,
            object_permission_id: None,
            password: None,
            teams: json!([]),
            user_role: None,
            user_email: None,
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
            created_at: None,
            updated_at: None,
        };
        db.insert_user(&user).await.expect("insert user");

        let team = Team {
            team_id: "t1".to_string(),
            team_alias: Some("t1".to_string()),
            spend: 40.0,
            max_budget: Some("500.0".to_string()),
            organization_id: None,
            object_permission_id: None,
            admins: json!([]),
            members: json!([]),
            members_with_roles: json!({}),
            metadata: json!({}),
            models: json!([]),
            max_parallel_requests: None,
            tpm_limit: None,
            rpm_limit: None,
            budget_duration: None,
            budget_reset_at: None,
            blocked: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            soft_budget: None,
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
        };
        db.insert_team(&team).await.expect("insert team");

        let identity = KeyIdentity {
            token_hash: hash,
            key_alias: None,
            user_id: Some("u1".to_string()),
            team_id: Some("t1".to_string()),
            organization_id: None,
            is_master_key: false,
            user_role: None,
        };

        let result = BudgetEnforcer::check_budget_multi(&db, &identity).await;
        assert!(result.is_ok(), "all levels should pass");
    }

    #[tokio::test]
    async fn test_multi_level_user_exceeded_key_ok() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-multi-user-exceeded";
        let hash = hash_token(raw);

        let mut key = make_test_key(&hash, 1.0, Some(100.0));
        key.user_id = Some("u2".to_string());
        key.team_id = None;
        key.organization_id = None;
        db.insert_key(&key).await.expect("insert");

        let user = User {
            user_id: "u2".to_string(),
            spend: 150.0,
            max_budget: Some("10.0".to_string()),
            user_alias: None,
            team_id: None,
            sso_user_id: None,
            organization_id: None,
            object_permission_id: None,
            password: None,
            teams: json!([]),
            user_role: None,
            user_email: None,
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
            created_at: None,
            updated_at: None,
        };
        db.insert_user(&user).await.expect("insert user");

        let identity = KeyIdentity {
            token_hash: hash,
            key_alias: None,
            user_id: Some("u2".to_string()),
            team_id: None,
            organization_id: None,
            is_master_key: false,
            user_role: None,
        };

        let err = BudgetEnforcer::check_budget_multi(&db, &identity)
            .await
            .unwrap_err();
        match err {
            BudgetError::Exceeded { entity_type, .. } => {
                assert_eq!(entity_type, "user");
            }
            _ => panic!("expected user-level Exceeded"),
        }
    }

    #[tokio::test]
    async fn test_multi_level_missing_entity_silently_skips() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-multi-missing-entity";
        let hash = hash_token(raw);

        let mut key = make_test_key(&hash, 5.0, Some(100.0));
        key.user_id = Some("ghost-user".to_string());
        key.team_id = None;
        key.organization_id = None;
        db.insert_key(&key).await.expect("insert");

        let identity = KeyIdentity {
            token_hash: hash,
            key_alias: None,
            user_id: Some("ghost-user".to_string()),
            team_id: None,
            organization_id: None,
            is_master_key: false,
            user_role: None,
        };

        let result = BudgetEnforcer::check_budget_multi(&db, &identity).await;
        assert!(result.is_ok(), "missing entity should be silently skipped");
    }

    // ── team-level exceeded test ──

    #[tokio::test]
    async fn test_multi_level_team_exceeded() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-multi-team-exceeded";
        let hash = hash_token(raw);

        // Key has plenty of budget; team is the one that exceeds
        let mut key = make_test_key(&hash, 1.0, Some(100.0));
        key.user_id = None;
        key.team_id = Some("t-over".to_string());
        key.organization_id = None;
        db.insert_key(&key).await.expect("insert");

        let team = Team {
            team_id: "t-over".to_string(),
            team_alias: Some("t-over".to_string()),
            spend: 150.0,
            max_budget: Some("10.0".to_string()),
            organization_id: None,
            object_permission_id: None,
            admins: json!([]),
            members: json!([]),
            members_with_roles: json!({}),
            metadata: json!({}),
            models: json!([]),
            max_parallel_requests: None,
            tpm_limit: None,
            rpm_limit: None,
            budget_duration: None,
            budget_reset_at: None,
            blocked: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            soft_budget: None,
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
        };
        db.insert_team(&team).await.expect("insert team");

        let identity = KeyIdentity {
            token_hash: hash,
            key_alias: None,
            user_id: None,
            team_id: Some("t-over".to_string()),
            organization_id: None,
            is_master_key: false,
            user_role: None,
        };

        let err = BudgetEnforcer::check_budget_multi(&db, &identity)
            .await
            .unwrap_err();
        match err {
            BudgetError::Exceeded {
                entity_type,
                spent,
                limit,
            } => {
                assert_eq!(entity_type, "team");
                assert_eq!(spent, 150.0);
                assert_eq!(limit, 10.0);
            }
            _ => panic!("expected team-level Exceeded"),
        }
    }

    // ── org-level exceeded test (JOIN budgets table) ──

    #[tokio::test]
    async fn test_multi_level_org_exceeded() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-multi-org-exceeded";
        let hash = hash_token(raw);

        // Key + team have plenty of budget; org's budget row is the limit
        let mut key = make_test_key(&hash, 1.0, Some(100.0));
        key.user_id = None;
        key.team_id = Some("t-org-ok".to_string());
        key.organization_id = Some("o-over".to_string());
        db.insert_key(&key).await.expect("insert");

        let team = Team {
            team_id: "t-org-ok".to_string(),
            team_alias: Some("t-org-ok".to_string()),
            spend: 5.0,
            max_budget: Some("500.0".to_string()),
            organization_id: Some("o-over".to_string()),
            object_permission_id: None,
            admins: json!([]),
            members: json!([]),
            members_with_roles: json!({}),
            metadata: json!({}),
            models: json!([]),
            max_parallel_requests: None,
            tpm_limit: None,
            rpm_limit: None,
            budget_duration: None,
            budget_reset_at: None,
            blocked: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            soft_budget: None,
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
        };
        db.insert_team(&team).await.expect("insert team");

        let org = Organization {
            organization_id: "o-over".to_string(),
            organization_alias: "o-over".to_string(),
            budget_id: "b-over".to_string(),
            metadata: json!({}),
            models: json!([]),
            spend: 20.5,
            model_spend: json!({}),
            object_permission_id: None,
            created_at: Utc::now(),
            created_by: "test".to_string(),
            updated_at: Utc::now(),
            updated_by: "test".to_string(),
        };
        db.insert_organization(&org).await.expect("insert org");

        let budget = Budget {
            budget_id: "b-over".to_string(),
            max_budget: Some("20.0".to_string()),
            soft_budget: None,
            max_parallel_requests: None,
            tpm_limit: None,
            rpm_limit: None,
            model_max_budget: json!({}),
            budget_duration: None,
            budget_reset_at: None,
            allowed_models: json!([]),
            created_at: Utc::now(),
            created_by: "test".to_string(),
            updated_at: Utc::now(),
            updated_by: "test".to_string(),
        };
        db.insert_budget(&budget).await.expect("insert budget");

        let identity = KeyIdentity {
            token_hash: hash,
            key_alias: None,
            user_id: None,
            team_id: Some("t-org-ok".to_string()),
            organization_id: Some("o-over".to_string()),
            is_master_key: false,
            user_role: None,
        };

        let err = BudgetEnforcer::check_budget_multi(&db, &identity)
            .await
            .unwrap_err();
        match err {
            BudgetError::Exceeded { entity_type, .. } => {
                assert_eq!(entity_type, "organization");
            }
            _ => panic!("expected org-level Exceeded via budgets JOIN"),
        }
    }

    // ── boundary test: spend == max_budget at all levels passes ──

    #[tokio::test]
    async fn test_multi_level_boundary_pass() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-multi-boundary";
        let hash = hash_token(raw);

        // All entities at exact max_budget
        let mut key = make_test_key(&hash, 100.0, Some(100.0));
        key.user_id = Some("u-boundary".to_string());
        key.team_id = Some("t-boundary".to_string());
        key.organization_id = None;
        db.insert_key(&key).await.expect("insert");

        let user = User {
            user_id: "u-boundary".to_string(),
            spend: 200.0,
            max_budget: Some("200.0".to_string()),
            user_alias: None,
            team_id: Some("t-boundary".to_string()),
            sso_user_id: None,
            organization_id: None,
            object_permission_id: None,
            password: None,
            teams: json!([]),
            user_role: None,
            user_email: None,
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
            created_at: None,
            updated_at: None,
        };
        db.insert_user(&user).await.expect("insert user");

        let team = Team {
            team_id: "t-boundary".to_string(),
            team_alias: Some("t-boundary".to_string()),
            spend: 500.0,
            max_budget: Some("500.0".to_string()),
            organization_id: None,
            object_permission_id: None,
            admins: json!([]),
            members: json!([]),
            members_with_roles: json!({}),
            metadata: json!({}),
            models: json!([]),
            max_parallel_requests: None,
            tpm_limit: None,
            rpm_limit: None,
            budget_duration: None,
            budget_reset_at: None,
            blocked: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            soft_budget: None,
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
        };
        db.insert_team(&team).await.expect("insert team");

        let identity = KeyIdentity {
            token_hash: hash,
            key_alias: None,
            user_id: Some("u-boundary".to_string()),
            team_id: Some("t-boundary".to_string()),
            organization_id: None,
            is_master_key: false,
            user_role: None,
        };

        let result = BudgetEnforcer::check_budget_multi(&db, &identity).await;
        assert!(result.is_ok(), "boundary (spend == max_budget) should pass");
    }

    // ── soft_budget warn tests: spend exceeds soft_budget but not max_budget ──

    #[tokio::test]
    async fn test_soft_budget_warn_key_passes() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-softbudget-key";
        let hash = hash_token(raw);

        // Key: spend=80, soft_budget=50, max_budget=100 → should pass with warn
        let mut key = make_test_key(&hash, 80.0, Some(100.0));
        key.soft_budget = Some("50".to_string());
        key.key_alias = Some("softbudget-key".to_string());
        key.user_id = None;
        key.team_id = None;
        key.organization_id = None;
        db.insert_key(&key).await.expect("insert");

        let identity = KeyIdentity {
            token_hash: hash,
            key_alias: Some("softbudget-key".to_string()),
            user_id: None,
            team_id: None,
            organization_id: None,
            is_master_key: false,
            user_role: None,
        };

        let result = BudgetEnforcer::check_budget_multi(&db, &identity).await;
        assert!(
            result.is_ok(),
            "soft_budget exceeded but hard budget still within limit → should pass"
        );
    }

    #[tokio::test]
    async fn test_soft_budget_warn_team_passes() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-softbudget-team";
        let hash = hash_token(raw);

        // Key: ok, Team: spend=80, soft_budget=50, max_budget=100 → passes with warn
        let mut key = make_test_key(&hash, 1.0, Some(100.0));
        key.user_id = None;
        key.team_id = Some("t-soft".to_string());
        key.organization_id = None;
        db.insert_key(&key).await.expect("insert");

        let team = Team {
            team_id: "t-soft".to_string(),
            team_alias: Some("t-soft".to_string()),
            spend: 80.0,
            max_budget: Some("100.0".to_string()),
            soft_budget: Some("50".to_string()),
            organization_id: None,
            object_permission_id: None,
            admins: json!([]),
            members: json!([]),
            members_with_roles: json!({}),
            metadata: json!({}),
            models: json!([]),
            max_parallel_requests: None,
            tpm_limit: None,
            rpm_limit: None,
            budget_duration: None,
            budget_reset_at: None,
            blocked: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
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
        };
        db.insert_team(&team).await.expect("insert team");

        let identity = KeyIdentity {
            token_hash: hash,
            key_alias: None,
            user_id: None,
            team_id: Some("t-soft".to_string()),
            organization_id: None,
            is_master_key: false,
            user_role: None,
        };

        let result = BudgetEnforcer::check_budget_multi(&db, &identity).await;
        assert!(
            result.is_ok(),
            "team soft_budget exceeded but hard budget still within limit → should pass"
        );
    }
}
