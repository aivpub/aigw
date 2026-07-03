//! aigw-server — AI Gateway HTTP server binary
//!
//! Provides:
//! - `/v1/chat/completions` — OpenAI-compatible chat endpoint (SSE streaming)
//! - `/v1/models` — model list
//! - `/key/*` — virtual key management
//! - `/spend/*`, `/global/spend/*` — usage tracking
//! - `/health`, `/health/readiness`, `/health/liveliness` — health checks
//! - `/docs` — OpenAPI documentation (Stage 4)

use clap::Parser;
use tracing_subscriber::EnvFilter;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    tracing::info!("aigw starting (mode: {}, bind: {})", cli.deployment_mode, cli.bind);
    tracing::info!("Config: {}", cli.config);

    // TODO (Stage 1-2): Parse config, init DB, build axum router, start server
    tracing::warn!("Server skeleton — full implementation in Stage 1-2");

    Ok(())
}
