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
    RateLimited {
        message: String,
        /// Effective RPM limit (0 = unlimited) — emitted as `x-ratelimit-limit`.
        rpm_limit: i64,
        /// Remaining RPM budget after this check — `x-ratelimit-remaining`.
        rpm_remaining: i64,
    },
    /// A database error occurred during budget check.
    Internal(String),
}

impl axum::response::IntoResponse for LimitError {
    fn into_response(self) -> axum::response::Response {
        // Build the JSON error body first, then attach rate-limit headers
        // for the 429 variants so clients can observe the bucket state.
        let status = match self {
            LimitError::BudgetExceeded { .. } | LimitError::RateLimited { .. } => {
                StatusCode::TOO_MANY_REQUESTS
            }
            LimitError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let mut builder = axum::response::Response::builder()
            .status(status)
            .header("content-type", "application/json");
        match self {
            LimitError::BudgetExceeded {
                entity_type,
                spent,
                limit,
            } => {
                let body = json!({
                    "error": {
                        "message": format!("{} budget exceeded: spent {:.4}, limit {:.4}", entity_type, spent, limit),
                        "type": "budget_exceeded",
                        "param": null,
                        "code": 429,
                        "entity_type": entity_type,
                        "spent": spent,
                        "limit": limit,
                    }
                });
                builder
                    .body(axum::body::Body::from(
                        serde_json::to_string(&body).unwrap(),
                    ))
                    .unwrap()
            }
            LimitError::RateLimited {
                message,
                rpm_limit,
                rpm_remaining,
            } => {
                let body = json!({
                    "error": {
                        "message": message,
                        "type": "rate_limited",
                        "param": null,
                        "code": 429,
                    }
                });
                // x-ratelimit-limit only when a limit is actually enforced.
                if rpm_limit > 0 {
                    builder = builder
                        .header("x-ratelimit-limit", rpm_limit.to_string())
                        .header("x-ratelimit-remaining", rpm_remaining.to_string());
                }
                builder = builder.header("Retry-After", "1");
                builder
                    .body(axum::body::Body::from(
                        serde_json::to_string(&body).unwrap(),
                    ))
                    .unwrap()
            }
            LimitError::Internal(message) => {
                let body = json!({
                    "error": {
                        "message": message,
                        "type": "internal_error",
                        "param": null,
                        "code": 500,
                    }
                });
                builder
                    .body(axum::body::Body::from(
                        serde_json::to_string(&body).unwrap(),
                    ))
                    .unwrap()
            }
        }
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
            BudgetError::Exceeded {
                entity_type,
                spent,
                limit,
            } => LimitError::BudgetExceeded {
                entity_type,
                spent,
                limit,
            },
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
            .check_with_headers(
                &key.token_hash,
                rpm,
                tpm,
                token_estimate,
                std::time::Instant::now(),
            )
            .await
            .map_err(|e| match e {
                crate::rate_limiter::RateLimitError::RateLimited { message }
                | crate::rate_limiter::RateLimitError::TpmLimited { message } => {
                    LimitError::RateLimited {
                        message,
                        rpm_limit: rpm.unwrap_or(0).max(0),
                        rpm_remaining: 0,
                    }
                }
            })?;
    }

    Ok(())
}

/// Combined request entry guard (Stage 117 §3.1).
///
/// Runs the full enforcement chain on every LLM handler entry:
///
/// 1. **Multi-level budget** — `BudgetEnforcer::check_budget_multi`
///    (key → user → team → org) with soft_budget webhook alerting.
/// 2. **RPM/TPM rate limits** — `enforce_limits` (token-bucket pre-check).
///
/// On failure the returned `LimitError` implements `IntoResponse`, emitting
/// the 429/500 body **and** `x-ratelimit-*` headers so handlers can forward
/// the full response to the client without dropping them.
///
/// # Note on TOCTOU
///
/// Budget check reads `entity.spend` before the request runs and spend is
/// incremented asynchronously after completion — a ~ms race window where two
/// concurrent requests can both pass check and cumulatively exceed budget.
/// This is the documented litellm-equivalent trade-off (see `budget.rs`).
pub async fn check_request_limits(
    db: &Database,
    rate_limiter: &RateLimiter,
    key: &KeyIdentity,
    token_estimate: u32,
) -> Result<(), LimitError> {
    enforce_limits(db, rate_limiter, key, token_estimate).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::rate_limiter::RateLimiter;

    async fn setup() -> (Database, RateLimiter) {
        let db = Database::init("sqlite::memory:").await.expect("db init");
        let rl = RateLimiter::new();
        (db, rl)
    }

    fn master_key() -> KeyIdentity {
        KeyIdentity {
            token_hash: "master-hash".to_string(),
            key_alias: Some("master".to_string()),
            user_id: None,
            team_id: None,
            organization_id: None,
            is_master_key: true,
            user_role: Some("proxy_admin".to_string()),
        }
    }

    #[tokio::test]
    async fn test_enforce_limits_passes_for_master_key() {
        let (db, rl) = setup().await;
        let mk = master_key();
        let result = enforce_limits(&db, &rl, &mk, 0).await;
        assert!(result.is_ok(), "master key should bypass limits");
    }

    #[tokio::test]
    async fn test_limit_error_to_into_response() {
        use axum::response::IntoResponse;
        let err = LimitError::RateLimited {
            message: "RPM limit exceeded".to_string(),
            rpm_limit: 10,
            rpm_remaining: 0,
        };
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        // Rate-limit headers must be present for a 429.
        let headers = resp.headers();
        assert_eq!(
            headers
                .get("x-ratelimit-limit")
                .and_then(|v| v.to_str().ok()),
            Some("10")
        );
        assert!(headers.contains_key("Retry-After"));
    }

    #[tokio::test]
    async fn test_budget_exceeded_into_response() {
        use axum::response::IntoResponse;
        let err = LimitError::BudgetExceeded {
            entity_type: "key".to_string(),
            spent: 150.0,
            limit: 100.0,
        };
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_internal_error_into_response() {
        use axum::response::IntoResponse;
        let err = LimitError::Internal("DB connection lost".to_string());
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_limit_error_display() {
        let err = LimitError::RateLimited {
            message: "test".to_string(),
            rpm_limit: 0,
            rpm_remaining: 0,
        };
        let debug = format!("{:?}", err);
        assert!(debug.contains("RateLimited"));
    }
}
