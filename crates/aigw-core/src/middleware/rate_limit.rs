//! Rate limit and budget enforcement guard function
//!
//! This module provides `enforce_limits`, a guard function that should be
//! called at the start of each LLM request handler. It performs:
//!
//! 1. **Multi-level budget check** — verifies key → user → team → org spend against max_budget
//! 2. **Rate limit check** — verifies RPM/TPM limits are not exceeded
//!
//! # Usage
//!
//! ```rust,ignore
//! use aigw_core::middleware::rate_limit::{enforce_limits, LimitError};
//!
//! async fn chat_handler(
//!     auth: KeyIdentity,
//!     state: State<SharedState>,
//! ) -> Result<Json<Value>, LimitError> {
//!     enforce_limits(&state.db, &state.rate_limiter, &auth, token_estimate).await?;
//!     // ... handle request
//! }
//! ```

use axum::http::StatusCode;
use axum::Json;
use serde_json::json;

use crate::budget::BudgetEnforcer;
use crate::budget::BudgetError;
use crate::db::Database;
use crate::middleware::KeyIdentity;
use crate::rate_limiter::RateLimiter;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Error types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Errors that can occur during limit enforcement.
///
/// Implements `IntoResponse` so it can be returned directly from handlers
/// using the `?` operator.
#[derive(Debug)]
pub enum LimitError {
    /// The key has exceeded its budget.
    BudgetExceeded {
        entity_type: String,
        spent: f64,
        limit: f64,
    },
    /// The key has exceeded its RPM or TPM limit.
    RateLimited { message: String },
    /// A database error occurred during budget check.
    Internal(String),
}

impl axum::response::IntoResponse for LimitError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_body) = match self {
            LimitError::BudgetExceeded { entity_type, spent, limit } => (
                StatusCode::TOO_MANY_REQUESTS,
                json!({
                    "error": {
                        "message": format!("{} budget exceeded: spent {:.4}, limit {:.4}", entity_type, spent, limit),
                        "type": "budget_exceeded",
                        "param": null,
                        "code": 429,
                        "entity_type": entity_type,
                        "spent": spent,
                        "limit": limit,
                    }
                }),
            ),
            LimitError::RateLimited { message } => (
                StatusCode::TOO_MANY_REQUESTS,
                json!({
                    "error": {
                        "message": message,
                        "type": "rate_limited",
                        "param": null,
                        "code": 429,
                    }
                }),
            ),
            LimitError::Internal(message) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({
                    "error": {
                        "message": message,
                        "type": "internal_error",
                        "param": null,
                        "code": 500,
                    }
                }),
            ),
        };

        (status, Json(error_body)).into_response()
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Enforcement guard
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Enforce budget and rate limits for a request.
///
/// This should be called at the beginning of every LLM request handler.
/// Master keys bypass all checks.
///
/// # Arguments
///
/// * `db` - The database for budget lookups
/// * `rate_limiter` - The shared rate limiter instance
/// * `key` - The authenticated key identity
/// * `token_estimate` - Estimated token count for this request (for TPM)
///
/// # Note
///
/// `token_estimate` is an approximation used for TPM pre-check.
/// Actual token counts are recorded via spend logs after the request completes.
/// For pre-check purposes, a reasonable default is 0 (skip TPM pre-check,
/// only enforce RPM and budget) or use max_tokens from the request body.
pub async fn enforce_limits(
    db: &Database,
    rate_limiter: &RateLimiter,
    key: &KeyIdentity,
    token_estimate: u32,
) -> Result<(), LimitError> {
    // Master keys bypass all limits
    if key.is_master_key {
        return Ok(());
    }

    // 1. Multi-level budget check (key → user → team → org)
    BudgetEnforcer::check_budget_multi(db, key)
        .await
        .map_err(|e| match e {
            BudgetError::Exceeded { entity_type, spent, limit } => {
                LimitError::BudgetExceeded { entity_type, spent, limit }
            }
            BudgetError::DbError(err) => {
                LimitError::Internal(format!("Budget check failed: {}", err))
            }
        })?;

    // 2. Check rate limits
    // We need the key's RPM/TPM limits. Since we already looked up the key
    // for budget, we'd need to look it up again here. To avoid a second DB
    // call, the caller should pass the limits directly.
    // However, for now, we use a simple approach: the KeyIdentity currently
    // doesn't carry RPM/TPM limits. Let's look them up.

    let key_data = db
        .get_key_by_token(&key.token_hash)
        .await
        .map_err(|e| LimitError::Internal(format!("Key lookup failed: {}", e)))?;

    if let Some(k) = key_data {
        let rpm = k.rpm_limit_i64();
        let tpm = k.tpm_limit_i64();

        rate_limiter
            .check(&key.token_hash, rpm, tpm, token_estimate)
            .await
            .map_err(|msg| LimitError::RateLimited { message: msg })?;
    }

    Ok(())
}
