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

// ── Build-time version info (injected by build.rs) ──
/// Full version string: `0.1.0 (abc1234)` or `0.1.0 (abc1234-dirty)`
pub const VERSION_INFO: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("GIT_COMMIT_HASH"),
    env!("GIT_DIRTY"),
    ")"
);
pub const BUILD_DATE: &str = env!("BUILD_DATE");
pub const GIT_COMMIT_HASH: &str = env!("GIT_COMMIT_HASH");
pub const GIT_DESCRIBE: &str = env!("GIT_DESCRIBE");

/// Long version string for `--version`
pub const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("GIT_COMMIT_HASH"),
    env!("GIT_DIRTY"),
    ")\nbuild date: ",
    env!("BUILD_DATE"),
    "\ngit describe: ",
    env!("GIT_DESCRIBE"),
);

use aigw_core::body_archive::BodyArchiver;
use aigw_core::config::{AigwConfig, CompressionConfig};
use aigw_core::daily_spend_queue::DailySpendQueue;
use aigw_core::db::Database;
use aigw_core::engine::{Engine, EngineConfig};
use aigw_core::provider::ProviderRegistry;
use aigw_core::rate_limiter::RateLimiter;
use aigw_core::resolver::ModelResolver;
use aigw_core::router::{Router as AigwRouter, RouterConfig, RouterState};
use axum::extract::DefaultBodyLimit;
use axum::http::HeaderName;
use axum::{middleware, routing::get, Router};
use clap::Parser;
use routes::keys::{self, AppState, SharedState};
use routes::{chat, cors_layer, credentials, docs, health, jobs, login, models, org, router_settings, spend, team, user, v1_messages};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::request_id::{MakeRequestId, RequestId, SetRequestIdLayer};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tower_http::compression::CompressionLayer;
use tracing::Level;
use tracing::Span;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use uuid::Uuid;

/// AI Gateway — litellm-compatible LLM proxy in Rust
#[derive(Parser, Debug)]
#[command(
    name = "aigw",
    version = VERSION_INFO,
    long_version = LONG_VERSION,
    about
)]
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
        Some(RequestId::new(id.parse().ok()?))
    }
}

/// Custom `MakeSpan` that reads `RequestId` from request extensions (injected by
/// the outer `SetRequestIdLayer`) and records it as a span field so every
/// `tracing` event emitted within the request scope carries `request_id` in JSON logs.
#[derive(Clone, Default)]
struct RequestIdMakeSpan;

impl<B> tower_http::trace::MakeSpan<B> for RequestIdMakeSpan {
    fn make_span(&mut self, request: &axum::http::Request<B>) -> Span {
        let request_id = request
            .extensions()
            .get::<RequestId>()
            .and_then(|id| id.header_value().to_str().ok())
            .unwrap_or("unknown");
        tracing::span!(
            Level::INFO,
            "request",
            // v6.1 §10 option A: span field renamed call_id to match the DB PK
            // semantics (the HTTP-layer `request_id` variable holds aigw's UUID v7,
            // which is the value stored as spend_logs.call_id). Variable name stays.
            call_id = %request_id,
            method = %request.method(),
            uri = %request.uri(),
        )
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env from cwd (falls back to parent dirs) so operators can supply
    // AIGW_MASTER_KEY / AIGW_DATABASE_URL / etc. via a dotenv file instead of
    // exporting them in the shell.  Silent on absence — .env is optional and
    // real deployments typically inject via env vars or systemd unit files.
    // MUST run before tracing init so subsequent std::env::var reads see it.
    let _ = dotenvy::dotenv();

    // Initialize tracing with optional OTEL layer
    let cli = Cli::parse();

    // Read config.yaml to know if OTEL is enabled BEFORE initing subscriber
    let config: Option<AigwConfig> = match std::fs::read_to_string(&cli.config) {
        Ok(content) => match serde_yaml::from_str(&content) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                eprintln!("Failed to parse {}: {} (will use defaults)", cli.config, e);
                None
            }
        },
        Err(_) => None,
    };

    // Extract OTEL config before initing subscriber
    let otel_config = config
        .as_ref()
        .and_then(|c| c.general_settings.as_ref())
        .and_then(|gs| gs.otel.clone())
        .unwrap_or_default();

    // Initialize OTEL tracer BEFORE subscriber so global::set_tracer_provider runs first
    let otel_tracer = aigw_core::otel_tracing::OtelTracer::init(&otel_config);
    let otel_active = otel_tracer.as_ref().map(|t| t.is_active()).unwrap_or(false);

    // Build subscriber layers
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer().json();
    let otel_layer = aigw_core::otel_tracing::build_otel_layer();

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(otel_layer)
        .init();

    tracing::info!(
        "aigw {} starting (mode: {}, bind: {}, otel: {})",
        VERSION_INFO,
        cli.deployment_mode,
        cli.bind,
        if otel_active { "enabled" } else { "disabled" }
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

    // Initialize Prometheus metrics (global default registry)
    let buckets_override = config
        .as_ref()
        .and_then(|c| c.general_settings.as_ref())
        .and_then(|gs| gs.metrics_buckets.as_ref());
    let metrics = Arc::new(aigw_core::metrics::MetricsRecorder::init("aigw", buckets_override)
        .expect("Failed to initialize prometheus metrics"));
    tracing::info!("Prometheus metrics initialized (namespace: aigw)");

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

    // Build body_archiver from parsed config (or default if not configured)
    let body_archive_config = config
        .as_ref()
        .and_then(|c| c.body_archive.clone())
        .unwrap_or_default();
    let body_archiver = Arc::new(BodyArchiver::new(body_archive_config));
    let body_archiver_arc: Option<Arc<BodyArchiver>> = Some(body_archiver.clone());

    let state: SharedState = Arc::new(AppState {
        db: (*db_arc).clone(),
        master_key: Some(master_key.clone()),
        aigw_master_key,
        metrics: Some(metrics),
        provider_registry,
        router: AigwRouter::from_config(&RouterConfig::default()),
        router_state,
        rate_limiter,
        deployment_mode: cli.deployment_mode.clone(),
        started_at: std::time::Instant::now(),
        daily_spend_queue: Some(daily_spend_queue),
        resolver,
        otel_active,
        body_archiver: body_archiver_arc.clone(),
    });

    // Start async job engine with body_archive worker
    {
        let mut engine = Engine::new(Arc::clone(&db_arc), EngineConfig::default());
        engine.register(body_archiver);
        tokio::spawn(async move { engine.run().await });
        tracing::info!("Async job engine started");
    }

    // Build compression config from general_settings
    let compression_cfg = config
        .as_ref()
        .and_then(|c| c.general_settings.as_ref())
        .and_then(|gs| gs.compression.as_ref())
        .cloned()
        .unwrap_or_default();

    // Resolve request body limit (bytes) from general_settings.request_body_limit_mb.
    // Unset → default 32 MiB (DEFAULT_REQUEST_BODY_LIMIT_MB). Some(0) → axum built-in 2 MiB.
    let request_body_limit_mb = config
        .as_ref()
        .and_then(|c| c.general_settings.as_ref())
        .and_then(|gs| gs.request_body_limit_mb);
    let body_limit_bytes =
        aigw_core::config::resolve_body_limit_bytes(request_body_limit_mb);
    match body_limit_bytes {
        Some(bytes) => tracing::info!(
            "Request body limit: {} MiB ({} bytes)",
            bytes / 1024 / 1024,
            bytes
        ),
        None => tracing::info!("Request body limit: axum built-in default (2 MiB)"),
    }

    // Build router
    let app = Router::new()
        .route("/docs", get(docs::docs_ui))
        .route("/openapi.json", get(openapi::openapi_json))
        .route("/health", get(health::health))
        .route("/health/readiness", get(health::readiness))
        .route("/health/liveliness", get(health::liveliness))
        .route("/health/metrics", get(health::health_metrics))
        .route("/health/latest", get(health::health_latest))
        .route("/system/info", get(health::system_info))
        // Prometheus metrics
        .route("/metrics", get(health::prometheus_metrics))
        // Model health checks
        .route("/model/health-check", axum::routing::post(health::model_health_check))
        .route("/model/health-check/all", axum::routing::post(health::model_health_check_all))
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
        .route("/key/deleted", get(keys::key_deleted_list))
        .route("/key/regenerate", axum::routing::post(keys::key_regenerate))
        // Model management routes
        .route("/model/new", axum::routing::post(models::model_new))
        .route("/model/info", get(models::model_info))
        .route("/model/list", get(models::model_list))
        .route("/model/update", axum::routing::put(models::model_update))
        .route("/model/delete", axum::routing::delete(models::model_delete))
        .route("/model/deleted", get(models::model_deleted_list))
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
        .route("/org/deleted", get(org::org_deleted_list))
        // Team management routes
        .route("/team/new", axum::routing::post(team::team_new))
        .route("/team/info", get(team::team_info))
        .route("/team/list", get(team::team_list))
        .route("/team/update", axum::routing::put(team::team_update))
        .route("/team/delete", axum::routing::delete(team::team_delete))
        .route("/team/deleted", get(team::team_deleted_list))
        // User management routes
        .route("/user/new", axum::routing::post(user::user_new))
        .route("/user/info", get(user::user_info))
        .route("/user/list", get(user::user_list))
        .route("/user/update", axum::routing::put(user::user_update))
        .route("/user/delete", axum::routing::delete(user::user_delete))
        .route("/user/deleted", get(user::user_deleted_list))
        // Router settings endpoints (Phase 23)
        .route("/router/settings", get(router_settings::get_global).put(router_settings::put_global))
        .route("/key/{token}/router/settings", axum::routing::patch(router_settings::patch_key))
        .route("/team/{id}/router/settings", axum::routing::patch(router_settings::patch_team))
        // Spend/usage tracking routes
        .route("/spend/logs", get(spend::spend_logs))
        .route("/spend/keys", get(spend::spend_keys))
        .route("/spend/users", get(spend::spend_users))
        .route("/spend/tags", get(spend::spend_tags))
        .route("/spend/models", get(spend::spend_models))
        .route("/spend/providers", get(spend::spend_providers))
        .route("/global/spend", get(spend::global_spend))
        .route("/global/spend/logs/{call_id}", get(spend::global_spend_log_detail))
        .route("/global/spend/logs", get(spend::global_spend_logs))
        .route("/global/spend/keys", get(spend::global_spend_keys))
        .route("/global/spend/models", get(spend::global_spend_models))
        .route("/global/spend/providers", get(spend::global_spend_providers))
        .route("/spend/model-groups", get(spend::spend_model_groups))
        .route("/global/spend/model-groups", get(spend::global_spend_model_groups))
        .route("/global/spend/activity", get(spend::global_spend_activity))
        .route("/global/spend/keys/rankings", get(spend::global_spend_keys_rankings))
        // Admin job management endpoints (Phase 30)
        .route("/admin/jobs/trigger", axum::routing::post(jobs::trigger_job))
        .route("/admin/jobs/stats", get(jobs::job_stats_handler))
        .route("/admin/jobs/{job_id}/logs", get(jobs::job_logs_handler))
        .route("/admin/jobs/{job_id}", get(jobs::job_detail_handler))
        .route("/admin/jobs", get(jobs::list_jobs_handler))
        .route("/admin/archive/stats", get(jobs::archive_stats_handler))
        // Login/Logout endpoints (litellm-compatible /v2/login/*)
        .route("/v2/login", axum::routing::post(login::login))
        .route("/v2/logout", axum::routing::post(login::logout_with_cleanup))
        .route("/v2/login/check", get(login::login_check))
        // Claude-compatible endpoint
        .route("/v1/messages", axum::routing::post(v1_messages::messages_handler))
        .with_state(state)
        // HTTP request tracing — JSON logs with request_id, method, path, latency.
        // Must be INSIDE SetRequestIdLayer so RequestIdMakeSpan can read RequestId from extensions.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(RequestIdMakeSpan::default())
                .on_response(
                    DefaultOnResponse::new()
                        .include_headers(false)
                        .level(Level::INFO)
                        .latency_unit(tower_http::LatencyUnit::Millis),
                ),
        )
        // UUID v7 request ID layer — generates RequestId and injects into extensions.
        // Must be OUTSIDE TraceLayer so it runs BEFORE make_span_with reads the extension.
        .layer(SetRequestIdLayer::new(
            HeaderName::from_static("x-request-id"),
            UuidV7RequestId,
        ))
        // CORS middleware — allows browser-based frontend to call API
        .layer(middleware::from_fn(cors_layer::add_cors_headers))
        // Response compression — gzip, deflate, brotli (configurable via general_settings.compression)
        .layer(build_compression_layer(&compression_cfg))
        // Request body size limit — configurable via general_settings.request_body_limit_mb (default 32 MiB).
        // body_limit_bytes=Some(n) → n MiB; None → axum built-in 2 MiB (opt-out, request_body_limit_mb=0).
        .layer(match body_limit_bytes {
            Some(bytes) => DefaultBodyLimit::max(bytes),
            None => DefaultBodyLimit::disable(),
        });

    // Bind and serve
    let addr: SocketAddr = cli.bind.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Server listening on {}", addr);

    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
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

/// Build a `CompressionLayer` from configuration.
fn build_compression_layer(cfg: &CompressionConfig) -> CompressionLayer {
    use tower_http::compression::CompressionLevel;

    if !cfg.enabled {
        return CompressionLayer::new()
            .gzip(false)
            .deflate(false)
            .br(false);
    }

    let level = match cfg.level {
        0..=3 => CompressionLevel::Fastest,
        7..=9 => CompressionLevel::Best,
        4..=6 => CompressionLevel::Default,
        _ => CompressionLevel::Default,
    };

    let has = |name: &str| cfg.algorithms.iter().any(|a| a.eq_ignore_ascii_case(name));

    CompressionLayer::new()
        .quality(level)
        .gzip(has("gzip"))
        .deflate(has("deflate"))
        .br(has("brotli"))
}

// ── Unit tests for compression ──

#[cfg(test)]
mod tests {
    use super::*;
    use aigw_core::config::CompressionConfig;
    use axum::{body::Body, Router};
    use tower::ServiceExt;
    /// Helper: build a minimal Router with the compression layer and a fixed JSON response.
    fn test_app(cfg: &CompressionConfig) -> Router {
        async fn handler() -> axum::http::Response<Body> {
            let body = serde_json::json!({"long_text": "I am Kofj".repeat(200)}).to_string();
            axum::http::Response::builder()
                .header("Content-Type", "application/json")
                .body(Body::from(body))
                .unwrap()
        }

        Router::new()
            .route("/test", axum::routing::get(handler))
            .layer(build_compression_layer(cfg))
    }

    #[tokio::test]
    async fn compression_gzip_enabled() {
        let app = test_app(&CompressionConfig::default());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/test")
                    .header("Accept-Encoding", "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        let has_encoding = response
            .headers()
            .get("Content-Encoding")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());

        // With a large enough JSON body, the response should be gzip-compressed.
        assert_eq!(
            has_encoding.as_deref(),
            Some("gzip"),
            "Expected Content-Encoding: gzip, got: {has_encoding:?}"
        );
    }

    #[tokio::test]
    async fn compression_brotli_negotiation() {
        let app = test_app(&CompressionConfig::default());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/test")
                    .header("Accept-Encoding", "br")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        let has_encoding = response
            .headers()
            .get("Content-Encoding")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());

        assert_eq!(
            has_encoding.as_deref(),
            Some("br"),
            "Expected Content-Encoding: br for brotli request, got: {has_encoding:?}"
        );
    }

    #[tokio::test]
    async fn compression_deflate_negotiation() {
        let app = test_app(&CompressionConfig::default());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/test")
                    .header("Accept-Encoding", "deflate")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        let has_encoding = response
            .headers()
            .get("Content-Encoding")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());

        assert_eq!(
            has_encoding.as_deref(),
            Some("deflate"),
            "Expected Content-Encoding: deflate, got: {has_encoding:?}"
        );
    }

    #[tokio::test]
    async fn compression_disabled() {
        let cfg = CompressionConfig {
            enabled: false,
            ..Default::default()
        };
        let app = test_app(&cfg);
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/test")
                    .header("Accept-Encoding", "gzip, deflate, br")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        // When compression is disabled, Content-Encoding should be absent.
        let has_encoding = response
            .headers()
            .get("Content-Encoding")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());

        assert_eq!(
            has_encoding, None,
            "Expected no Content-Encoding when disabled, got: {has_encoding:?}"
        );
    }

    #[tokio::test]
    async fn compression_algorithm_selection() {
        // Only allow gzip — brotli should NOT be used even if requested.
        let cfg = CompressionConfig {
            algorithms: vec!["gzip".into()],
            ..Default::default()
        };
        let app = test_app(&cfg);
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/test")
                    .header("Accept-Encoding", "br")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        let has_encoding = response
            .headers()
            .get("Content-Encoding")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());

        // Brotli was disallowed → no compression applied.
        assert_eq!(
            has_encoding, None,
            "Expected no Content-Encoding when only gzip is allowed for br request, got: {has_encoding:?}"
        );
    }

    #[tokio::test]
    async fn compression_with_no_accept_encoding() {
        // No Accept-Encoding header → no compression should be applied.
        let app = test_app(&CompressionConfig::default());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        let has_encoding = response
            .headers()
            .get("Content-Encoding")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());

        assert_eq!(
            has_encoding, None,
            "Expected no Content-Encoding without Accept-Encoding header, got: {has_encoding:?}"
        );
    }

    #[test]
    fn compression_level_mapping() {
        // Verify level mapping helper
        use tower_http::compression::CompressionLevel;

        // Helper to test the mapping logic (duplicated from build_compression_layer for testing)
        fn map_level(level: u32) -> CompressionLevel {
            match level {
                0..=3 => CompressionLevel::Fastest,
                7..=9 => CompressionLevel::Best,
                _ => CompressionLevel::Default,
            }
        }

        assert!(matches!(map_level(0), CompressionLevel::Fastest));
        assert!(matches!(map_level(3), CompressionLevel::Fastest));
        assert!(matches!(map_level(4), CompressionLevel::Default));
        assert!(matches!(map_level(6), CompressionLevel::Default));
        assert!(matches!(map_level(7), CompressionLevel::Best));
        assert!(matches!(map_level(9), CompressionLevel::Best));
        // Values above 9 default to Default (defensive fallback)
        assert!(matches!(map_level(10), CompressionLevel::Default));
        assert!(matches!(map_level(42), CompressionLevel::Default));
    }
}
