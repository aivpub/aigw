//! aigw-server — AI Gateway HTTP server binary
//!
//! Provides:
//! - `/v1/chat/completions` — OpenAI-compatible chat endpoint (SSE streaming)
//! - `/v1/models` — model list
//! - `/key/*` — virtual key management
//! - `/spend/*`, `/global/spend/*` — usage tracking
//! - `/health`, `/health/readiness`, `/health/liveliness` — health checks
//! - `/docs` — OpenAPI documentation (Stage 4)

mod frontend;
mod openapi;
mod routes;

use aigw_core::config::AigwConfig;
use aigw_core::daily_spend_queue::DailySpendQueue;
use aigw_core::db::Database;
use aigw_core::provider::ProviderRegistry;
use aigw_core::rate_limiter::RateLimiter;
use aigw_core::resolver::ModelResolver;
use aigw_core::router::{Router as AigwRouter, RouterConfig, RouterState};
use axum::http::HeaderName;
use axum::{middleware, routing::get, Router};
use clap::Parser;
use routes::keys::{self, AppState, SharedState};
use routes::{chat, cors_layer, credentials, docs, health, login, models, org, spend, team, user, v1_messages};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::request_id::{MakeRequestId, RequestId, SetRequestIdLayer};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;
use tracing::Span;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

/// AI Gateway — litellm-compatible LLM proxy in Rust
#[derive(Parser, Debug)]
#[command(name = "aigw", version, about)]
struct Cli {
    /// Path to config file (litellm-compatible YAML)
    #[arg(short, long, default_value = "config.yaml")]
    config: String,

    /// Override database URL
    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,

    /// Override master key
    #[arg(long, env = "MASTER_KEY")]
    master_key: Option<String>,

    /// Deployment mode: saas | onprem
    #[arg(long, env = "DEPLOYMENT_MODE", default_value = "onprem")]
    deployment_mode: String,

    /// Bind address
    #[arg(long, default_value = "0.0.0.0:4000")]
    bind: String,
}

/// Custom UUID v7 request ID generator.
///
/// Produces lexicographically sortable request IDs for log correlation.
#[derive(Clone, Default)]
struct UuidV7RequestId;

impl MakeRequestId for UuidV7RequestId {
    fn make_request_id<B>(&mut self, _request: &axum::http::Request<B>) -> Option<RequestId> {
        let id = Uuid::now_v7().to_string();
        Span::current().record("request_id", &id);
        Some(RequestId::new(id.parse().ok()?))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .init();

    let cli = Cli::parse();

    // Read config.yaml for general_settings (CLI/ENV overrides take precedence)
    let config: Option<AigwConfig> = match std::fs::read_to_string(&cli.config) {
        Ok(content) => match serde_yaml::from_str(&content) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                tracing::warn!("Failed to parse {}: {}, using defaults", cli.config, e);
                None
            }
        },
        Err(_) => {
            tracing::info!("{} not found, using CLI args / env vars only", cli.config);
            None
        }
    };

    tracing::info!(
        "aigw starting (mode: {}, bind: {})",
        cli.deployment_mode,
        cli.bind
    );

    // Determine master key (CLI/ENV > config.yaml > default)
    let master_key = cli
        .master_key
        .or_else(|| {
            config
                .as_ref()
                .and_then(|c| c.general_settings.as_ref())
                .and_then(|gs| gs.master_key.clone())
                .filter(|k| !k.is_empty())
        })
        .unwrap_or_else(|| "sk-master-change-me".to_string());

    // Determine database URL (CLI/ENV > config.yaml > default)
    let database_url = cli
        .database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .or_else(|| {
            config
                .as_ref()
                .and_then(|c| c.general_settings.as_ref())
                .and_then(|gs| gs.database_url.clone())
                .filter(|u| !u.is_empty())
        })
        .unwrap_or_else(|| "sqlite:aigw.db".to_string());

    // Initialize database
    let db = Database::init(&database_url).await?;
    tracing::info!("Database initialized: {}", database_url);

    // Initialize provider registry (load from env vars or config)
    let provider_registry = ProviderRegistry::default_with_env();
    tracing::info!(
        "Provider registry: {} providers, {} model routes",
        provider_registry.providers.len(),
        provider_registry.model_routing.len(),
    );

    // Initialize router state
    let router_state: RouterState = Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    // Initialize rate limiter
    let rate_limiter = Arc::new(RateLimiter::new());

    // Determine aigw master key for runtime decryption (CREDENTIALS/encrypted fields)
    // Priority: AIGW_MASTER_KEY env var → config table (general_settings.master_key)
    let aigw_master_key = match std::env::var("AIGW_MASTER_KEY").ok() {
        Some(key) if !key.is_empty() => Some(key),
        _ => match db.get_master_key_from_db().await {
            Ok(Some(key)) => {
                tracing::info!("AIGW_MASTER_KEY loaded from config table");
                Some(key)
            }
            Ok(None) => {
                tracing::warn!(
                    "AIGW_MASTER_KEY not configured (env or config table) — encrypted fields will not be decrypted"
                );
                None
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to query config table for master_key: {} — encrypted fields will not be decrypted",
                    e
                );
                None
            }
        },
    };

    // Build shared state
    let db_arc = Arc::new(db);
    let daily_spend_queue = Arc::new(DailySpendQueue::new(Arc::clone(&db_arc)));

    // Build ModelResolver for unified upstream resolution
    let resolver = ModelResolver::new(
        (*db_arc).clone(),
        aigw_master_key.clone(),
        cli.deployment_mode.clone(),
    );

    let state: SharedState = Arc::new(AppState {
        db: (*db_arc).clone(),
        master_key: Some(master_key.clone()),
        aigw_master_key,
        provider_registry,
        router: AigwRouter::from_config(&RouterConfig::default()),
        router_state,
        rate_limiter,
        deployment_mode: cli.deployment_mode.clone(),
        started_at: std::time::Instant::now(),
        daily_spend_queue: Some(daily_spend_queue),
        resolver,
    });

    // Build router
    let app = Router::new()
        .route("/docs", get(docs::docs_ui))
        .route("/openapi.json", get(openapi::openapi_json))
        .route("/health", get(health::health))
        .route("/health/readiness", get(health::readiness))
        .route("/health/liveliness", get(health::liveliness))
        .route("/health/metrics", get(health::health_metrics))
        .route("/system/info", get(health::system_info))
        // Frontend admin console (embedded SPA)
        .route("/dash", get(frontend::serve_frontend))
        .route("/dash/{*rest}", get(frontend::serve_frontend))
        // OpenAI-compatible endpoints
        .route(
            "/v1/chat/completions",
            axum::routing::post(chat::chat_completions),
        )
        .route("/v1/models", get(chat::models_list))
        // Key management routes
        .route("/key/generate", axum::routing::post(keys::generate_key))
        .route("/key/info", get(keys::key_info))
        .route("/key/list", get(keys::key_list))
        .route("/key/update", axum::routing::put(keys::key_update))
        .route("/key/delete", axum::routing::delete(keys::key_delete))
        .route("/key/regenerate", axum::routing::post(keys::key_regenerate))
        // Model management routes
        .route("/model/new", axum::routing::post(models::model_new))
        .route("/model/info", get(models::model_info))
        .route("/model/list", get(models::model_list))
        .route("/model/update", axum::routing::put(models::model_update))
        .route("/model/delete", axum::routing::delete(models::model_delete))
        // Credential management routes
        .route(
            "/credential/new",
            axum::routing::post(credentials::credential_new),
        )
        .route("/credential/info", get(credentials::credential_info))
        .route("/credential/list", get(credentials::credential_list))
        .route(
            "/credential/update",
            axum::routing::put(credentials::credential_update),
        )
        .route(
            "/credential/delete",
            axum::routing::delete(credentials::credential_delete),
        )
        // Organization management routes
        .route("/org/new", axum::routing::post(org::org_new))
        .route("/org/info", get(org::org_info))
        .route("/org/list", get(org::org_list))
        .route("/org/update", axum::routing::put(org::org_update))
        .route("/org/delete", axum::routing::delete(org::org_delete))
        // Team management routes
        .route("/team/new", axum::routing::post(team::team_new))
        .route("/team/info", get(team::team_info))
        .route("/team/list", get(team::team_list))
        .route("/team/update", axum::routing::put(team::team_update))
        .route("/team/delete", axum::routing::delete(team::team_delete))
        // User management routes
        .route("/user/new", axum::routing::post(user::user_new))
        .route("/user/info", get(user::user_info))
        .route("/user/list", get(user::user_list))
        .route("/user/update", axum::routing::put(user::user_update))
        .route("/user/delete", axum::routing::delete(user::user_delete))
        // Spend/usage tracking routes
        .route("/spend/logs", get(spend::spend_logs))
        .route("/spend/keys", get(spend::spend_keys))
        .route("/spend/users", get(spend::spend_users))
        .route("/spend/tags", get(spend::spend_tags))
        .route("/spend/models", get(spend::spend_models))
        .route("/spend/providers", get(spend::spend_providers))
        .route("/global/spend", get(spend::global_spend))
        .route("/global/spend/logs", get(spend::global_spend_logs))
        .route("/global/spend/keys", get(spend::global_spend_keys))
        .route("/global/spend/models", get(spend::global_spend_models))
        .route("/global/spend/providers", get(spend::global_spend_providers))
        .route("/global/spend/activity", get(spend::global_spend_activity))
        // Login/Logout endpoints (litellm-compatible /v2/login/*)
        .route("/v2/login", axum::routing::post(login::login))
        .route("/v2/logout", axum::routing::post(login::logout_with_cleanup))
        .route("/v2/login/check", get(login::login_check))
        // Claude-compatible endpoint
        .route("/v1/messages", axum::routing::post(v1_messages::messages_handler))
        .with_state(state)
        // UUID v7 request ID layer — propagates request_id into tracing span
        .layer(SetRequestIdLayer::new(
            HeaderName::from_static("x-request-id"),
            UuidV7RequestId,
        ))
        // HTTP request tracing — JSON logs with request_id, method, path, latency
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(
                    DefaultMakeSpan::new()
                        .include_headers(false)
                        .level(Level::INFO),
                )
                .on_response(
                    DefaultOnResponse::new()
                        .include_headers(false)
                        .level(Level::INFO)
                        .latency_unit(tower_http::LatencyUnit::Millis),
                ),
        )
        // CORS middleware — allows browser-based frontend to call API
        .layer(middleware::from_fn(cors_layer::add_cors_headers));

    // Bind and serve
    let addr: SocketAddr = cli.bind.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Server listening on {}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Graceful shutdown on SIGTERM / Ctrl+C
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for Ctrl+C");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received, shutting down gracefully");
}
