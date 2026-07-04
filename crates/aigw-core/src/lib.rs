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

pub mod budget;
pub mod adapter;
pub mod config;
pub mod crypto;
pub mod db;
pub mod instance;
pub mod middleware;
pub mod models;
pub mod provider;
pub mod rate_limiter;
pub mod router;
pub mod tenant;

// Re-export commonly used types
pub use config::{AigwConfig, GeneralSettings, ModelInfo, RouterSettings};
pub use models::{
    Budget, Organization, OrganizationMembership, Project, SpendLog, Team, TeamMembership, User,
    VirtualKey,
};
