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
//! This module closes that gap with four boot-time primitives, each pure and
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
//! - [`build_websearch_registry`] — turn the parsed `web_search` block into a
//!   runtime [`WebSearchRegistry`] (Phase 53): validate, decrypt per-instance
//!   keys, build the shared HTTP client and the instance pools. Takes an
//!   already-parsed struct rather than a file path, so Stage 138's DB-backed
//!   config source reuses it unchanged.
//!
//! The `litellm_settings` block (`drop_params` / `request_timeout` /
//! `set_verbose`) has no corresponding implementation in aigw today and is
//! deliberately left unwired — documented, not dead-wired.

use crate::config::{ModelEntry, RouterSettings};
use crate::db::Database;
use crate::models::ProxyModel;
use crate::router::RouterConfig;
use crate::websearch::{
    config::{WebSearchConfig, WebSearchInstanceConfig},
    CooldownPolicy, Guardrails, SearchProvider, WebSearchRegistry,
};
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
            enabled: true,
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

/// Build the runtime web search layer from the parsed `web_search` block.
///
/// Returns `Ok(None)` when the block is absent or `enabled: false` — the layer
/// then costs nothing, matching `CacheConfig`'s master-switch semantics.
///
/// Takes an **already-parsed struct and never touches the filesystem**, which is
/// what makes Stage 138's DB-backed config source a pure addition: build the same
/// `WebSearchConfig` from DB rows and all validation, key decryption, instance
/// pooling and client construction below are reused verbatim.
///
/// `Err` fails startup. Invalid values (unknown `default_provider`, unsupported
/// `kind`, empty `instances`, conflicting domain lists) are rejected here rather
/// than at request time.
pub fn build_websearch_registry(
    cfg: &Option<WebSearchConfig>,
    master_key: &str,
) -> Result<Option<WebSearchRegistry>, String> {
    let Some(cfg) = cfg else { return Ok(None) };
    if !cfg.enabled {
        return Ok(None);
    }
    cfg.validate()?;

    let policy = CooldownPolicy {
        allowed_fails: cfg.allowed_fails,
        cooldown_secs: cfg.cooldown_secs,
    };
    let proxy = (!cfg.proxy_url.trim().is_empty()).then_some(cfg.proxy_url.as_str());

    let mut providers: Vec<std::sync::Arc<dyn SearchProvider>> =
        Vec::with_capacity(cfg.providers.len());
    for p in &cfg.providers {
        // Each instance's api_key may be plaintext or a `v2:gcm:` ciphertext.
        // `decrypt_litellm_value` dispatches on the prefix and safely refuses
        // non-ciphertext, so plaintext passes through untouched.
        let instances = p
            .instances
            .iter()
            .map(|i| {
                let api_key = i.api_key.as_ref().and_then(|k| {
                    if k.trim().is_empty() {
                        None
                    } else {
                        Some(
                            crate::crypto::decrypt_litellm_value(k, master_key)
                                .unwrap_or_else(|_| k.clone()),
                        )
                    }
                });
                WebSearchInstanceConfig {
                    api_key,
                    ..i.clone()
                }
            })
            .collect();

        let timeout_ms = p.effective_timeout_ms(cfg.timeout_ms);
        // One client per provider, built once and shared: search sits before the
        // first upstream byte, so connection reuse shows up directly in TTFT.
        let client = crate::websearch::client::build_search_client(
            proxy,
            std::time::Duration::from_millis(timeout_ms),
            cfg.retries,
        )?;
        providers.push(crate::websearch::build_provider(
            p, instances, policy, client, timeout_ms,
        )?);
    }

    let guardrails = Guardrails {
        max_results: cfg.max_results.max(1),
        snippet_max_chars: cfg.snippet_max_chars,
        allowed_domains: cfg.allowed_domains.clone(),
        blocked_domains: cfg.blocked_domains.clone(),
    };

    Ok(Some(WebSearchRegistry::new(
        providers,
        cfg.attempt_order(),
        guardrails,
    )))
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
            enabled: true,
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

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // build_websearch_registry (Phase 53, Stage 134)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    const WEB_SEARCH_YAML: &str = r#"
enabled: true
default_provider: searxng
failover_order: [searxng]
max_results: 3
snippet_max_chars: 500
blocked_domains: [reddit.com]
providers:
  - name: searxng
    kind: searxng
    cost_per_query: 0.0002
    timeout_ms: 8000
    instances:
      - base_url: http://searxng-a.internal:8080
        weight: 2
      - base_url: http://searxng-b.internal:8080
        weight: 1
"#;

    fn parse_web_search(yaml: &str) -> WebSearchConfig {
        serde_yaml::from_str(yaml).expect("parse web_search block")
    }

    #[test]
    fn test_websearch_config_absent_disables_layer() {
        // The whole block missing from config.yaml must cost nothing at all —
        // same master-switch semantics as CacheConfig.
        let reg = build_websearch_registry(&None, "mk").expect("absent is not an error");
        assert!(reg.is_none());
    }

    #[test]
    fn test_websearch_disabled_flag_disables_layer() {
        let mut cfg = parse_web_search(WEB_SEARCH_YAML);
        cfg.enabled = false;
        assert!(build_websearch_registry(&Some(cfg), "mk")
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_build_websearch_registry_wires_providers_and_guardrails() {
        let cfg = parse_web_search(WEB_SEARCH_YAML);
        let reg = build_websearch_registry(&Some(cfg), "mk")
            .expect("valid config must build")
            .expect("enabled config yields a registry");

        assert_eq!(reg.provider_names(), vec!["searxng"]);
        assert_eq!(
            reg.cost_per_query(None),
            Some(0.0002),
            "cost_per_query must have a live reader so it cannot rot like litellm_settings"
        );
        let g = reg.guardrails();
        assert_eq!(g.max_results, 3);
        assert_eq!(g.snippet_max_chars, 500);
        assert_eq!(g.blocked_domains, vec!["reddit.com"]);
    }

    #[test]
    fn test_build_websearch_registry_rejects_invalid_config_at_load() {
        // Startup must fail rather than surfacing the problem per-request.
        let mut cfg = parse_web_search(WEB_SEARCH_YAML);
        cfg.default_provider = "ghost".to_string();
        assert!(build_websearch_registry(&Some(cfg), "mk").is_err());

        let mut cfg = parse_web_search(WEB_SEARCH_YAML);
        cfg.providers[0].kind = "tavily".to_string();
        let err = build_websearch_registry(&Some(cfg), "mk").unwrap_err();
        assert!(err.contains("tavily") && err.contains("searxng"), "{err}");

        let mut cfg = parse_web_search(WEB_SEARCH_YAML);
        cfg.allowed_domains = vec!["a.com".into()];
        assert!(build_websearch_registry(&Some(cfg), "mk").is_err());
    }

    #[test]
    fn test_build_websearch_registry_rejects_bad_proxy_url() {
        let mut cfg = parse_web_search(WEB_SEARCH_YAML);
        cfg.proxy_url = "not a proxy".to_string();
        let err = build_websearch_registry(&Some(cfg), "mk")
            .expect_err("a malformed proxy must fail at boot, not per request");
        assert!(err.contains("proxy"), "{err}");
    }

    #[test]
    fn test_build_websearch_registry_decrypts_instance_api_keys() {
        let master_key = "sk-master-loader-test";
        let cipher = crate::crypto::encrypt_litellm_value_gcm("sk-paid-key", master_key).unwrap();
        let yaml = format!(
            r#"
enabled: true
default_provider: searxng
providers:
  - name: searxng
    kind: searxng
    instances:
      - base_url: http://a.internal:8080
        api_key: "{cipher}"
      - base_url: http://b.internal:8080
        api_key: plain-key
      - base_url: http://c.internal:8080
"#
        );
        let cfg = parse_web_search(&yaml);
        // The registry hides its instances, so assert the loader's transform on
        // the same path the loader uses.
        let decrypted: Vec<Option<String>> = cfg.providers[0]
            .instances
            .iter()
            .map(|i| {
                i.api_key.as_ref().and_then(|k| {
                    if k.trim().is_empty() {
                        None
                    } else {
                        Some(
                            crate::crypto::decrypt_litellm_value(k, master_key)
                                .unwrap_or_else(|_| k.clone()),
                        )
                    }
                })
            })
            .collect();
        assert_eq!(
            decrypted[0].as_deref(),
            Some("sk-paid-key"),
            "v2:gcm: decrypted"
        );
        assert_eq!(
            decrypted[1].as_deref(),
            Some("plain-key"),
            "plaintext preserved"
        );
        assert_eq!(decrypted[2], None, "absent key stays absent");

        assert!(build_websearch_registry(&Some(cfg), master_key)
            .unwrap()
            .is_some());
    }
}
