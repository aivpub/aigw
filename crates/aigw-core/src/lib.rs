//! aigw-core: AI Gateway core library
//!
//! This crate provides:
//! - Database schema and migrations (litellm-compatible)
//! - Data models (VerificationToken, SpendLog, Organization, Team, User, Project, Budget)
//! - Router strategies (shuffle, usage-based, latency-based)
//! - Auth middleware (virtual key + master key)
//! - Config parsing (litellm-compatible YAML format)
//!
//! # Architecture
//!
//! ```text
//! config.rs  →  models.rs  →  db.rs  →  router.rs  →  middleware.rs
//!                   ↑              ↑
//!              (schema)      (sqlite)
//! ```

pub mod adapter;
pub mod alerts;
pub mod async_task;
pub mod auth;
pub mod body_archive;
pub mod budget;
pub mod config;
pub mod crypto;
pub mod daily_spend_queue;
pub mod db;
pub mod deployment;
pub mod engine;
pub mod image_tokens;
pub mod instance;
pub mod metrics;
pub mod middleware;
pub mod models;
pub mod otel_tracing;
pub mod password;
pub mod provider;
pub mod rate_limiter;
pub mod request_id;
pub mod resolver;
pub mod router;
pub mod tenant;

// Re-export commonly used types
pub use async_task::{AsyncTask, JobLogEntry, NewStep, StepOutput};
pub use auth::{decode_jwt, encode_jwt, JwtClaims};
pub use config::{AigwConfig, BudgetResetConfig, GeneralSettings, ModelInfo, RouterSettings};
pub use crypto::{
    decode_base64_type15, decrypt_json_fields, decrypt_litellm_value, encrypt_litellm_value,
    hash_token, rotate_json_fields,
};
pub use db::CredentialsStore;
pub use deployment::{Deployment, ProviderType};
pub use engine::{Engine, EngineConfig};
pub use image_tokens::{
    calculate_image_tokens, decode_image_dimensions, estimate_image_tokens_from_blocks,
    estimate_single, extract_image_tokens_from_usage, infer_strategy, CalculationStrategy,
};
pub use metrics::{MetricsRecorder, RequestSummary};
pub use models::{
    Budget, Credential, DailySpendKind, DailySpendLog, Organization, OrganizationMembership,
    Project, ProxyModel, SpendLog, Team, TeamMembership, User, VirtualKey,
};
pub use otel_tracing::{extract_traceparent, inject_traceparent, OtelConfig};
pub use password::{hash_password, verify_password};
pub use router::{merge_router_overrides, Router, RouterConfig, RouterStrategy};
