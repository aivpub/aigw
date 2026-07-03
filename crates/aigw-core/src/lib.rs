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

pub mod config;
pub mod db;
pub mod middleware;
pub mod models;
pub mod router;

// Re-export commonly used types
pub use config::{AigwConfig, GeneralSettings, ModelInfo, RouterSettings};
pub use models::{
    SpendLog, VirtualKey,
    LiteLLM_OrganizationTable, LiteLLM_TeamTable, LiteLLM_UserTable, LiteLLM_ProjectTable,
    LiteLLM_BudgetTable, LiteLLM_OrganizationMembership, LiteLLM_TeamMembership,
};
