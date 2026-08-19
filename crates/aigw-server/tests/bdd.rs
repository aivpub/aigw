//! BDD test entry point — cucumber-rust integration test
//!
//! Run with: cargo test --test bdd -p aigw-server

use aigw_server::routes::keys::DEFAULT_KEY_TOKEN_LEN;
use cucumber::World as _;
use std::sync::Arc;

mod bdd_steps;
mod bdd_support;

/// TestWorld — shared state across Given/When/Then steps within a Scenario.
#[derive(Debug, Clone, cucumber::World)]
pub struct TestWorld {
    /// The shared app state (DB, master_key, etc.)
    #[world(skip)]
    pub state: Option<aigw_server::routes::keys::SharedState>,
    /// Master key for admin auth
    pub master_key: String,
    /// Last HTTP response status
    #[world(skip)]
    pub last_status: Option<u16>,
    /// Last HTTP response body as JSON
    #[world(skip)]
    pub last_body: Option<serde_json::Value>,
    /// Last HTTP response headers (for x-call-id assertions, TD-006)
    #[world(skip)]
    pub last_headers: Option<axum::http::HeaderMap>,
    /// Created keys by alias → raw token
    #[world(skip)]
    pub created_keys: std::collections::HashMap<String, String>,
    /// Created users by user_id → (email, password)
    #[world(skip)]
    pub created_users: std::collections::HashMap<String, (String, String)>,
}

impl Default for TestWorld {
    fn default() -> Self {
        Self {
            state: None,
            master_key: std::env::var("AIGW_MASTER_KEY")
                .unwrap_or_else(|_| "sk-master-test".to_string()),
            last_status: None,
            last_body: None,
            last_headers: None,
            created_keys: std::collections::HashMap::new(),
            created_users: std::collections::HashMap::new(),
        }
    }
}

impl TestWorld {
    pub async fn ensure_state(&mut self) -> aigw_server::routes::keys::SharedState {
        if self.state.is_none() {
            let db = aigw_core::db::Database::init("sqlite::memory:")
                .await
                .expect("db init");
            let mk = "sk-master-test".to_string();
            let state: aigw_server::routes::keys::SharedState =
                Arc::new(aigw_server::routes::keys::AppState {
                    resolver: aigw_core::resolver::ModelResolver::new(db.clone(), None, "onprem"),
                    router: aigw_core::router::Router::default(),
                    db,
                    master_key: Some(mk.clone()),
                    aigw_master_key: None,
                    key_generate_length: DEFAULT_KEY_TOKEN_LEN,
                    disable_custom_api_keys: false,
                    provider_registry: aigw_core::provider::ProviderRegistry::new(),
                    router_state: aigw_core::router::RouterState::default(),
                    rate_limiter: Arc::new(aigw_core::rate_limiter::RateLimiter::new()),
                    deployment_mode: "test".to_string(),
                    started_at: std::time::Instant::now(),
                    daily_spend_queue: None,
                    otel_active: false,
                    body_archiver: None,
                    token_provider: std::sync::Arc::new(
                        aigw_core::claude_token::TokenProvider::new(),
                    ),
                    metrics: None,
                });
            self.master_key = mk;
            self.state = Some(state.clone());
            state
        } else {
            self.state.as_ref().unwrap().clone()
        }
    }
}

#[tokio::main]
async fn main() {
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // DB + server lifecycle (before all scenarios — once per test run)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    let manager = bdd_support::test_db::TestDatabaseManager::from_env();
    let db_info = if let Some(ref mgr) = manager {
        let info = mgr.create_db().await.expect("create test db");
        // Expose the auto-created DB URL to migration sync steps
        // (they read AIGW_TEST_DB_URL to know where to sync data)
        std::env::set_var("AIGW_TEST_DB_URL", &info.database_url);
        Some(info)
    } else {
        None
    };

    let server = if let (Some(info), true) = (
        &db_info,
        std::env::var("AIGW_TEST_START_SERVER").as_deref() == Ok("1"),
    ) {
        let s = bdd_support::server::ServerGuard::start(&info.database_url, "sk-master-test")
            .await
            .expect("start server");
        std::env::set_var("AIGW_BASE_URL", &s.base_url);
        std::env::set_var("AIGW_MASTER_KEY", "sk-master-test");
        Some(s)
    } else {
        None
    };

    // Cucumber run() resolves path relative to Cargo manifest dir.
    // Run scenarios sequentially: the mock upstream uses shared state
    // and concurrent scenarios would interfere with each other.
    //
    // tag filter: when AIGW_REAL_API=1, only run @real_api scenarios.
    // Mock scenarios (63 of 78) are skipped — they would waste time
    // creating sqlite::memory: DBs and running migrations for nothing,
    // since their steps become no-ops when AIGW_REAL_API is set.
    let real_api_mode = std::env::var("AIGW_REAL_API").as_deref() == Ok("1");
    TestWorld::cucumber()
        .max_concurrent_scenarios(1)
        .before(bdd_steps::real_api_steps::before_scenario_hook)
        .after(bdd_steps::real_api_steps::after_scenario_hook)
        .filter_run("tests/features", move |feature, _rule, scenario| {
            // Never run scenarios tagged @skip — they document future work
            // (e.g. Stage 83 ColChunkCache / read-path scenarios awaiting
            // implementation) and would fail on undefined steps today.
            if scenario.tags.iter().any(|t| t == "skip") {
                return false;
            }
            if real_api_mode {
                feature
                    .tags
                    .iter()
                    .chain(scenario.tags.iter())
                    .any(|t| t == "real_api")
            } else {
                // In mock mode, skip features that require a real upstream DB
                // (@needs_upstream_db) — their steps no-op with skip_pass but
                // assert real rejection semantics (e.g. multi_level_budget 429)
                // that a sqlite::memory: DB cannot reproduce.
                let tags: Vec<&str> = feature
                    .tags
                    .iter()
                    .chain(scenario.tags.iter())
                    .map(|t| t.as_str())
                    .collect();
                !tags.contains(&"needs_upstream_db")
            }
        })
        .await;

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Cleanup (after all scenarios)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    if let Some(s) = server {
        s.stop().await;
    }
    if let (Some(mgr), Some(ref info)) = (manager, db_info) {
        if std::env::var("AIGW_TEST_KEEP_DB").as_deref() == Ok("1") {
            eprintln!(
                "==> AIGW_TEST_KEEP_DB=1: keeping test DB: {}",
                info.database_url
            );
        } else {
            mgr.drop_db(info).await.ok();
        }
    }
}
