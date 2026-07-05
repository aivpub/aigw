//! BDD test entry point — cucumber-rust integration test
//!
//! Run with: cargo test --test bdd -p aigw-server

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
    /// Created keys by alias → raw token
    #[world(skip)]
    pub created_keys: std::collections::HashMap<String, String>,
}

impl Default for TestWorld {
    fn default() -> Self {
        Self {
            state: None,
            master_key: std::env::var("AIGW_MASTER_KEY")
                .unwrap_or_else(|_| "sk-master-test".to_string()),
            last_status: None,
            last_body: None,
            created_keys: std::collections::HashMap::new(),
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
            let state: aigw_server::routes::keys::SharedState = Arc::new(
                aigw_server::routes::keys::AppState {
                    db,
                    master_key: Some(mk.clone()),
                    aigw_master_key: None,
                    provider_registry: aigw_core::provider::ProviderRegistry::new(),
                    router_state: aigw_core::router::RouterState::default(),
                    rate_limiter: Arc::new(aigw_core::rate_limiter::RateLimiter::new()),
                    deployment_mode: "test".to_string(),
                },
            );
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
    // Cucumber run() resolves path relative to Cargo manifest dir.
    // Run scenarios sequentially: the mock upstream uses shared state
    // and concurrent scenarios would interfere with each other.
    TestWorld::cucumber()
        .max_concurrent_scenarios(1)
        .run("tests/features")
        .await;
}
