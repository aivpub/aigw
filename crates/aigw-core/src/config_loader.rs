//! Static config-file model support — the bridge between `config.yaml` and the
//! running server.
//!
//! litellm's core deployment paradigm is "mount a `config.yaml` and it takes
//! effect at boot" (`--config`). aigw previously parsed the whole `AigwConfig`
//! but wired almost nothing:
//!
//! - `model_list` was parsed then discarded (models came only from the DB via
//!   `/model/new` / `aigw-migrate`).
//! - `router_settings` was parsed but the router always booted with
//!   [`RouterConfig::default`].
//! - `environment_variables` was never read at all.
//!
//! This module closes that gap with three boot-time primitives, each pure and
//! unit-testable:
//!
//! - [`seed_models_from_config`] — idempotently insert `config.model_list`
//!   entries into `proxy_models`. Existing rows (created via the admin API) are
//!   left untouched, so the DB remains the source of truth for anything that
//!   was not declared in the file.
//! - [`apply_environment_variables`] — fill missing env vars from
//!   `config.environment_variables` (never override already-set values, same
//!   semantics as `dotenvy`).
//! - [`build_router_config`] — map the parsed `router_settings` block onto a
//!   [`RouterConfig`] for [`Router::from_config`].
//!
//! The `litellm_settings` block (`drop_params` / `request_timeout` /
//! `set_verbose`) has no corresponding implementation in aigw today and is
//! deliberately left unwired — documented, not dead-wired.

use crate::config::{ModelEntry, RouterSettings};
use crate::db::Database;
use crate::models::ProxyModel;
use crate::router::RouterConfig;
use serde_json::json;
use std::collections::HashMap;

/// Result of seeding models from `config.yaml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeedStats {
    /// Number of models newly inserted into `proxy_models`.
    pub inserted: usize,
    /// Number of `model_list` entries skipped because a row with the same
    /// `model_name` already exists in the DB.
    pub skipped: usize,
}

/// Idempotently seed `model_list` from `config.yaml` into `proxy_models`.
///
/// For each entry, the model is inserted only when no row with the same
/// `model_name` already exists — the DB (admin API / `aigw-migrate`) stays the
/// source of truth for anything already present, and re-running at every boot
/// is a no-op.
pub async fn seed_models_from_config(
    db: &Database,
    model_list: &[ModelEntry],
) -> Result<SeedStats, crate::db::DbError> {
    let mut stats = SeedStats {
        inserted: 0,
        skipped: 0,
    };
    if model_list.is_empty() {
        return Ok(stats);
    }
    for entry in model_list {
        // Skip if a row with this model_name already exists (DB-first).
        if let Ok(Some(_)) = db.get_model_by_name(&entry.model_name).await {
            stats.skipped += 1;
            continue;
        }
        let now = chrono::Utc::now().to_rfc3339();
        let model = ProxyModel {
            model_id: uuid::Uuid::new_v4().to_string(),
            model_name: entry.model_name.clone(),
            // `config.yaml` may reference `${OPENAI_API_KEY}` in api_key —
            // resolved by `apply_environment_variables` before this runs.
            litellm_params: serde_json::to_value(&entry.litellm_params)
                .unwrap_or_else(|_| json!({})),
            model_info: json!({}),
            created_at: now.clone(),
            created_by: Some("config".to_string()),
            updated_at: now,
            updated_by: Some("config".to_string()),
        };
        match db.insert_model(&model).await {
            Ok(()) => stats.inserted += 1,
            // Race: another boot already inserted it between our check and the
            // insert. Treat as skipped, not an error.
            Err(_) => stats.skipped += 1,
        }
    }
    Ok(stats)
}

/// Fill missing environment variables from `config.environment_variables`.
///
/// Only vars that are currently unset are injected (`std::env::set_var`),
/// matching `dotenvy` semantics: shell / real env always wins over the file.
/// Returns the list of vars actually set, for logging.
pub fn apply_environment_variables(env_vars: &serde_json::Value) -> Vec<String> {
    let mut set = Vec::new();
    let Some(map) = env_vars.as_object() else {
        return set;
    };
    for (k, v) in map {
        if let Some(val) = v.as_str() {
            // `std::env::var` returns Err when unset (or non-unicode); only
            // inject then. `var_os` is the reliable "is it set" probe.
            if std::env::var_os(k).is_none() {
                std::env::set_var(k, val);
                set.push(k.clone());
            }
        }
    }
    set
}

/// Map the parsed `config.yaml` `router_settings` block onto a `RouterConfig`.
///
/// The two types use different field shapes (see `config.rs` `RouterSettings`
/// vs `router.rs` `RouterConfig`):
/// - `RouterSettings` stores `cooldown_time`; `RouterConfig` also uses
///   `cooldown_time` (a `cooldown_secs` alias is not part of the serde schema
///   and would be ignored).
/// - `RouterSettings.num_retries` / `allowed_fails` map 1:1.
/// - `routing_strategy` is passed through; `Router::from_config` falls back to
///   `SimpleShuffle` for unknown strings.
///
/// When `router_settings` is `None`, returns [`RouterConfig::default`].
pub fn build_router_config(router_settings: &Option<RouterSettings>) -> RouterConfig {
    let Some(settings) = router_settings else {
        return RouterConfig::default();
    };
    RouterConfig {
        routing_strategy: settings
            .routing_strategy
            .clone()
            .unwrap_or_else(|| "simple-shuffle".to_string()),
        num_retries: settings.num_retries.max(0) as u32,
        allowed_fails: settings.allowed_fails.max(0) as u32,
        cooldown_time: settings.cooldown_time.max(0.0),
        model_group_alias: HashMap::new(),
    }
}

/// Build the JSON value persisted to the `config` table as the initial
/// `router_settings` row (what `GET /router/settings` returns before any PUT).
///
/// Reuses [`RouterSettings`] serialization so the seed row matches the file.
pub fn router_settings_seed_json(router_settings: &Option<RouterSettings>) -> serde_json::Value {
    match router_settings {
        Some(s) => serde_json::to_value(s).unwrap_or_else(|_| json!({})),
        None => json!({}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::router::RouterStrategy;
    use std::str::FromStr;

    async fn test_db() -> Database {
        // in-memory sqlite: runs migrations (012 proxy_models etc.)
        // via Database::init.
        Database::init("sqlite::memory:")
            .await
            .expect("init in-memory database")
    }

    fn model_entry(name: &str) -> ModelEntry {
        ModelEntry {
            model_name: name.to_string(),
            litellm_params: crate::config::ModelParams {
                model: format!("openai/{}", name),
                api_base: Some("https://api.openai.com/v1".to_string()),
                api_key: None,
                rpm: Some(1000),
                tpm: Some(100000),
                max_parallel_requests: None,
                input_cost_per_token: None,
                output_cost_per_token: None,
                tpm_limit: None,
                rpm_limit: None,
            },
        }
    }

    async fn count_models(db: &Database) -> i64 {
        db.count_models().await.expect("count")
    }

    // ── seed_models_from_config ──────────────────────────────────────────

    #[tokio::test]
    async fn seed_empty_list_is_noop() {
        let db = test_db().await;
        let stats = seed_models_from_config(&db, &[]).await.expect("seed");
        assert_eq!(
            stats,
            SeedStats {
                inserted: 0,
                skipped: 0
            }
        );
        assert_eq!(count_models(&db).await, 0);
    }

    #[tokio::test]
    async fn seed_inserts_new_models() {
        let db = test_db().await;
        let list = vec![model_entry("seed-gpt-4"), model_entry("seed-claude")];
        let stats = seed_models_from_config(&db, &list).await.expect("seed");
        assert_eq!(
            stats,
            SeedStats {
                inserted: 2,
                skipped: 0
            }
        );
        assert_eq!(count_models(&db).await, 2);
        let by_name = db.get_model_by_name("seed-gpt-4").await.expect("get");
        assert!(by_name.is_some());
        let model = by_name.unwrap();
        assert_eq!(model.model_name, "seed-gpt-4");
        assert_eq!(model.created_by.as_deref(), Some("config"));
    }

    #[tokio::test]
    async fn seed_is_idempotent_on_rerun() {
        let db = test_db().await;
        let list = vec![model_entry("seed-idem")];
        let first = seed_models_from_config(&db, &list).await.expect("seed");
        assert_eq!(
            first,
            SeedStats {
                inserted: 1,
                skipped: 0
            }
        );
        let second = seed_models_from_config(&db, &list).await.expect("seed");
        assert_eq!(
            second,
            SeedStats {
                inserted: 0,
                skipped: 1
            }
        );
        assert_eq!(count_models(&db).await, 1);
    }

    #[tokio::test]
    async fn seed_skips_existing_db_model() {
        let db = test_db().await;
        // Pre-insert a model via the DB path (simulating admin API created it).
        let now = chrono::Utc::now().to_rfc3339();
        let existing = ProxyModel {
            model_id: uuid::Uuid::new_v4().to_string(),
            model_name: "seed-exists".to_string(),
            litellm_params: json!({"model": "openai/seed-exists"}),
            model_info: json!({}),
            created_at: now.clone(),
            created_by: Some("api".to_string()),
            updated_at: now,
            updated_by: Some("api".to_string()),
        };
        db.insert_model(&existing).await.expect("pre-insert");

        let list = vec![model_entry("seed-exists"), model_entry("seed-new")];
        let stats = seed_models_from_config(&db, &list).await.expect("seed");
        assert_eq!(
            stats,
            SeedStats {
                inserted: 1,
                skipped: 1
            }
        );
        assert_eq!(count_models(&db).await, 2);
        // DB row untouched (still api-created, not config-created).
        let row = db
            .get_model_by_name("seed-exists")
            .await
            .expect("get")
            .unwrap();
        assert_eq!(row.created_by.as_deref(), Some("api"));
    }

    // ── apply_environment_variables ──────────────────────────────────────

    #[test]
    fn apply_env_fills_missing_only() {
        // Remove any pre-existing value to make the test hermetic.
        unsafe { std::env::remove_var("AIGW_CFGLOADER_TEST_A") };
        unsafe { std::env::remove_var("AIGW_CFGLOADER_TEST_B") };
        // Pre-set B via env so it must NOT be overwritten.
        unsafe { std::env::set_var("AIGW_CFGLOADER_TEST_B", "from-shell") };

        let env_vars = json!({
            "AIGW_CFGLOADER_TEST_A": "from-config",
            "AIGW_CFGLOADER_TEST_B": "from-config",
            "AIGW_CFGLOADER_TEST_NONSTRING": 123,
        });
        let set = apply_environment_variables(&env_vars);
        assert!(set.contains(&"AIGW_CFGLOADER_TEST_A".to_string()));
        assert!(!set.contains(&"AIGW_CFGLOADER_TEST_B".to_string()));
        assert!(!set.contains(&"AIGW_CFGLOADER_TEST_NONSTRING".to_string()));

        assert_eq!(
            std::env::var("AIGW_CFGLOADER_TEST_A").expect("set"),
            "from-config"
        );
        // B preserved from shell.
        assert_eq!(
            std::env::var("AIGW_CFGLOADER_TEST_B").expect("set"),
            "from-shell"
        );

        // Cleanup.
        unsafe { std::env::remove_var("AIGW_CFGLOADER_TEST_A") };
        unsafe { std::env::remove_var("AIGW_CFGLOADER_TEST_B") };
    }

    #[test]
    fn apply_env_non_object_is_noop() {
        let set = apply_environment_variables(&json!(null));
        assert!(set.is_empty());
    }

    // ── build_router_config ──────────────────────────────────────────────

    #[test]
    fn build_router_config_none_is_default() {
        let cfg = build_router_config(&None);
        assert_eq!(cfg.routing_strategy, "simple-shuffle");
        assert_eq!(cfg.allowed_fails, 3);
        assert_eq!(cfg.cooldown_time, 5.0);
        assert_eq!(cfg.num_retries, 0);
    }

    #[test]
    fn build_router_config_maps_fields() {
        let settings = RouterSettings {
            routing_strategy: Some("usage-based-routing-v2".to_string()),
            allowed_fails: 7,
            num_retries: 2,
            cooldown_time: 30.0,
            fallbacks: None,
        };
        let cfg = build_router_config(&Some(settings));
        assert_eq!(cfg.routing_strategy, "usage-based-routing-v2");
        assert_eq!(cfg.allowed_fails, 7);
        assert_eq!(cfg.num_retries, 2);
        assert_eq!(cfg.cooldown_time, 30.0);
        // Strategy string parses to the routing variant.
        let strat = RouterStrategy::from_str(&cfg.routing_strategy).expect("parse");
        assert_eq!(strat, RouterStrategy::UsageBasedRoutingV2);
    }

    #[test]
    fn build_router_config_clamps_negatives() {
        let settings = RouterSettings {
            routing_strategy: Some("unknown-strategy".to_string()),
            allowed_fails: -3,
            num_retries: -1,
            cooldown_time: -5.0,
            fallbacks: None,
        };
        let cfg = build_router_config(&Some(settings));
        assert_eq!(cfg.allowed_fails, 0);
        assert_eq!(cfg.num_retries, 0);
        assert_eq!(cfg.cooldown_time, 0.0);
        // Unknown strategy falls back to SimpleShuffle at from_config time.
        let strat = RouterStrategy::from_str(&cfg.routing_strategy).expect("parse");
        assert_eq!(strat, RouterStrategy::SimpleShuffle);
    }

    #[test]
    fn router_settings_seed_json_preserves_shape() {
        let settings = Some(RouterSettings {
            routing_strategy: Some("latency-based-routing".to_string()),
            allowed_fails: 4,
            num_retries: 1,
            cooldown_time: 15.0,
            fallbacks: None,
        });
        let v = router_settings_seed_json(&settings);
        assert_eq!(v["routing_strategy"], "latency-based-routing");
        assert_eq!(v["allowed_fails"], 4);
        assert_eq!(v["cooldown_time"], 15.0);
        // None -> empty object (GET /router/settings returns {} before any PUT).
        assert_eq!(router_settings_seed_json(&None), json!({}));
    }
}
