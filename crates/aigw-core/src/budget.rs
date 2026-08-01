//! Budget enforcement module
//!
//! Checks whether an API key has exceeded its max_budget by comparing the
//! key's `spend` field against its `max_budget`. This module works with
//! the `Database` enum to support SQLite, MySQL, and PostgreSQL backends.

pub mod duration;
pub mod resetter;

use crate::db::{Database, DbError};
use thiserror::Error;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Error types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Errors that can occur during budget enforcement.
#[derive(Debug, Error)]
pub enum BudgetError {
    /// The key's spend has exceeded its max_budget.
    #[error("Budget exceeded: spent {spent:.4}, limit {limit:.4}")]
    Exceeded {
        /// Total spend recorded for the key
        spent: f64,
        /// Maximum budget allowed for the key
        limit: f64,
    },
    /// A database error occurred while querying the key.
    #[error("Database error: {0}")]
    DbError(#[from] DbError),
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// BudgetEnforcer
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Stateless budget enforcement utility.
///
/// Checks key spend against max_budget using the `Database` abstraction.
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

        // If no max_budget is set, the key has unlimited budget
        let max_budget = match key.max_budget_f64() {
            Some(mb) if mb.is_finite() && mb > 0.0 => mb,
            _ => return Ok(()),
        };

        if key.spend > max_budget {
            return Err(BudgetError::Exceeded {
                spent: key.spend,
                limit: max_budget,
            });
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
    use crate::models::VirtualKey;
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

    #[tokio::test]
    async fn test_budget_not_exceeded() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-budget-ok";
        let hash = hash_token(raw);

        // Spend 50.0, max_budget 100.0 — should be OK
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

        // Spend 150.0, max_budget 100.0 — should fail
        let key = make_test_key(&hash, 150.0, Some(100.0));
        db.insert_key(&key).await.expect("insert");

        let result = BudgetEnforcer::check_budget(&db, &hash).await;
        assert!(result.is_err(), "budget should be exceeded");

        match result.unwrap_err() {
            BudgetError::Exceeded { spent, limit } => {
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

        // Spend exactly at max_budget — should still be OK (not exceeded)
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

        // No max_budget set — unlimited
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

        // max_budget = 0.0 — treated as unlimited
        let key = make_test_key(&hash, 10.0, Some(0.0));
        db.insert_key(&key).await.expect("insert");

        let result = BudgetEnforcer::check_budget(&db, &hash).await;
        assert!(result.is_ok(), "zero budget should be unlimited");
    }

    #[tokio::test]
    async fn test_budget_nonexistent_key() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        // Key doesn't exist in DB
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

        // max_budget = NaN — treated as unlimited
        let key = make_test_key(&hash, 100.0, Some(f64::NAN));
        db.insert_key(&key).await.expect("insert");

        let result = BudgetEnforcer::check_budget(&db, &hash).await;
        assert!(result.is_ok(), "NaN max_budget should be treated as unlimited");
    }

    #[tokio::test]
    async fn test_budget_infinity_max_budget() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let raw = "sk-budget-inf";
        let hash = hash_token(raw);

        // max_budget = INFINITY, spend = 100.0 — inf treated as unlimited
        let key = make_test_key(&hash, 100.0, Some(f64::INFINITY));
        db.insert_key(&key).await.expect("insert");

        let result = BudgetEnforcer::check_budget(&db, &hash).await;
        assert!(result.is_ok(), "INFINITY max_budget should be treated as unlimited");
    }
}
