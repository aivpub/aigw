//! Rate limiter — token-bucket based RPM/TPM enforcement
//!
//! Tracks per-key request-per-minute (RPM) and token-per-minute (TPM) usage
//! using an in-memory token bucket algorithm. Buckets are created lazily on
//! first access and persist for the lifetime of the process.
//!
//! # Algorithm
//!
//! The token bucket refills continuously at `limit / 60` tokens per second.
//! Requests consume 1 token from the RPM bucket and `token_estimate` tokens
//! from the TPM bucket. When a bucket is empty, the request is denied.
//!
//! # Concurrency
//!
//! All state is behind `Arc<tokio::sync::Mutex<>>`. The critical section is
//! brief — hashmap lookup + arithmetic — so contention is minimal.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// TokenBucket
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A token bucket that refills at a constant rate up to a maximum capacity.
#[derive(Debug, Clone)]
struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    /// Tokens added per second
    refill_rate: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(limit_per_minute: f64) -> Self {
        Self {
            tokens: limit_per_minute,
            max_tokens: limit_per_minute,
            refill_rate: limit_per_minute / 60.0,
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;
    }

    /// Number of tokens currently available (for `x-ratelimit-remaining`).
    fn remaining(&self) -> f64 {
        let mut t = self.tokens;
        // Refill lazily to report an accurate remaining value.
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        t = (t + elapsed * self.refill_rate).min(self.max_tokens);
        t
    }

    fn try_consume(&mut self, count: f64) -> bool {
        self.refill();
        if self.tokens >= count {
            self.tokens -= count;
            true
        } else {
            false
        }
    }
}

/// Result of a rate-limit check — used to emit `x-ratelimit-*` response headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitInfo {
    /// Effective RPM limit applied to this key (0 = no limit)
    pub rpm_limit: i64,
    /// Remaining requests in the RPM bucket after this check
    pub rpm_remaining: i64,
    /// Effective TPM limit applied to this key (0 = no limit)
    pub tpm_limit: i64,
}

/// Errors from the rate-limit guard (HTTP-level).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitError {
    /// RPM bucket exhausted.
    RateLimited { message: String },
    /// A TPM budget is exhausted for this request's estimated tokens.
    TpmLimited { message: String },
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RateLimitError::RateLimited { message } | RateLimitError::TpmLimited { message } => {
                write!(f, "{message}")
            }
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// RateLimiter
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// In-memory rate limiter for RPM and TPM enforcement.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    rpm_buckets: Arc<Mutex<HashMap<String, TokenBucket>>>,
    tpm_buckets: Arc<Mutex<HashMap<String, TokenBucket>>>,
}

impl RateLimiter {
    /// Create a new `RateLimiter` with empty bucket maps.
    pub fn new() -> Self {
        Self {
            rpm_buckets: Arc::new(Mutex::new(HashMap::new())),
            tpm_buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check whether a request identified by `key_hash` with estimated
    /// `token_count` is allowed under the given RPM and TPM limits.
    ///
    /// Both limits are optional; `None` means no limit. A value of
    /// `Some(0)` is treated as unlimited.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the request is within all applicable limits.
    /// * `Err(String)` with a rate-limit error message if denied.
    pub async fn check(
        &self,
        key_hash: &str,
        rpm_limit: Option<i64>,
        tpm_limit: Option<i64>,
        token_count: u32,
    ) -> Result<(), String> {
        // RPM check
        if let Some(rpm) = rpm_limit {
            if rpm > 0 {
                let mut buckets = self.rpm_buckets.lock().await;
                let bucket = buckets
                    .entry(key_hash.to_string())
                    .or_insert_with(|| TokenBucket::new(rpm as f64));
                if !bucket.try_consume(1.0) {
                    return Err(format!(
                        "RPM limit exceeded: {rpm} requests per minute",
                        rpm = rpm
                    ));
                }
            }
        }

        // TPM check
        if let Some(tpm) = tpm_limit {
            if tpm > 0 && token_count > 0 {
                let mut buckets = self.tpm_buckets.lock().await;
                let bucket = buckets
                    .entry(key_hash.to_string())
                    .or_insert_with(|| TokenBucket::new(tpm as f64));
                if !bucket.try_consume(token_count as f64) {
                    return Err(format!(
                        "TPM limit exceeded: {tpm} tokens per minute",
                        tpm = tpm
                    ));
                }
            }
        }

        Ok(())
    }

    /// Rate-limit check that also reports the remaining budget for
    /// `x-ratelimit-*` response headers. Unlike [`RateLimiter::check`], this
    /// returns a structured [`RateLimitInfo`] on success and a typed
    /// [`RateLimitError`] on denial, so the caller can emit
    /// `x-ratelimit-limit` / `x-ratelimit-remaining` / `Retry-After`.
    ///
    /// The `_now` parameter is unused (token buckets self-refill on access);
    /// it exists to keep the signature future-proof for a time-injectable
    /// clock in tests.
    pub async fn check_with_headers(
        &self,
        key_hash: &str,
        rpm_limit: Option<i64>,
        tpm_limit: Option<i64>,
        token_count: u32,
        _now: std::time::Instant,
    ) -> Result<RateLimitInfo, RateLimitError> {
        let mut rpm_remaining = 0i64;
        let mut tpm_limit_out = 0i64;

        // RPM check
        if let Some(rpm) = rpm_limit {
            if rpm > 0 {
                let mut buckets = self.rpm_buckets.lock().await;
                let bucket = buckets
                    .entry(key_hash.to_string())
                    .or_insert_with(|| TokenBucket::new(rpm as f64));
                if !bucket.try_consume(1.0) {
                    let msg = format!("RPM limit exceeded: {rpm} requests per minute");
                    return Err(RateLimitError::RateLimited { message: msg });
                }
                rpm_remaining = bucket.remaining().max(0.0) as i64;
            }
        }

        // TPM check
        if let Some(tpm) = tpm_limit {
            if tpm > 0 && token_count > 0 {
                let mut buckets = self.tpm_buckets.lock().await;
                let bucket = buckets
                    .entry(key_hash.to_string())
                    .or_insert_with(|| TokenBucket::new(tpm as f64));
                if !bucket.try_consume(token_count as f64) {
                    let msg = format!("TPM limit exceeded: {tpm} tokens per minute");
                    return Err(RateLimitError::TpmLimited { message: msg });
                }
                tpm_limit_out = tpm;
            }
        }

        Ok(RateLimitInfo {
            rpm_limit: rpm_limit.unwrap_or(0).max(0),
            rpm_remaining,
            tpm_limit: tpm_limit_out,
        })
    }

    /// Record token usage for a completed request.
    ///
    /// The initial `check()` already deducts the estimated token count from
    /// the TPM bucket. This method is a no-op hook for future adjustments.
    /// Actual spend tracking is handled via the spend_logs table in the DB.
    pub async fn record_usage(&self, _key_hash: &str, _token_count: u64) {}
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Unit tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_rate_limiter_allows_within_limit() {
        let limiter = RateLimiter::new();
        let key = "hash-allow-test";

        for _ in 0..10 {
            let result = limiter.check(key, Some(100), None, 0).await;
            assert!(result.is_ok(), "request within RPM should succeed");
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_when_exceeded() {
        let limiter = RateLimiter::new();
        let key = "hash-block-test";

        for i in 0..5 {
            let result = limiter.check(key, Some(5), None, 0).await;
            assert!(result.is_ok(), "request {} should succeed", i + 1);
        }

        let result = limiter.check(key, Some(5), None, 0).await;
        assert!(result.is_err(), "6th request should be rate limited");
        assert!(result.unwrap_err().contains("RPM limit exceeded"));
    }

    #[tokio::test]
    async fn test_rate_limiter_refills_after_time() {
        let limiter = RateLimiter::new();
        let key = "hash-refill-test";

        // 600 RPM = 10 tokens/sec; 250ms = 2.5 tokens refilled
        assert!(limiter.check(key, Some(600), None, 0).await.is_ok());
        assert!(limiter.check(key, Some(600), None, 0).await.is_ok());

        sleep(Duration::from_millis(250)).await;
        let result = limiter.check(key, Some(600), None, 0).await;
        assert!(result.is_ok(), "should be allowed after refill period");
    }

    #[tokio::test]
    async fn test_tpm_limiter_blocks_large_requests() {
        let limiter = RateLimiter::new();
        let key = "hash-tpm-test";

        // TPM = 100; first claims 60 (ok), second claims 60 (blocked, only 40 remain)
        assert!(limiter.check(key, None, Some(100), 60).await.is_ok());

        let result = limiter.check(key, None, Some(100), 60).await;
        assert!(result.is_err(), "should be TPM limited");
        assert!(result.unwrap_err().contains("TPM limit exceeded"));
    }

    #[tokio::test]
    async fn test_zero_limit_skips_check() {
        let limiter = RateLimiter::new();
        let key = "hash-zero-limit";

        assert!(limiter.check(key, Some(0), None, 0).await.is_ok());
        assert!(limiter.check(key, None, Some(0), 1000).await.is_ok());
    }

    #[tokio::test]
    async fn test_none_limits_always_pass() {
        let limiter = RateLimiter::new();
        let key = "hash-none-limits";

        assert!(limiter.check(key, None, None, 0).await.is_ok());
    }

    #[tokio::test]
    async fn test_independent_keys() {
        let limiter = RateLimiter::new();

        // Exhaust key-a
        assert!(limiter.check("key-a", Some(1), None, 0).await.is_ok());
        assert!(limiter.check("key-a", Some(1), None, 0).await.is_err());

        // key-b still works
        assert!(limiter.check("key-b", Some(1), None, 0).await.is_ok());
    }

    #[test]
    fn test_token_bucket_refill_math() {
        let mut bucket = TokenBucket::new(60.0);
        assert!(bucket.tokens >= 59.0);

        for _ in 0..60 {
            assert!(bucket.try_consume(1.0));
        }
        assert!(!bucket.try_consume(1.0));
    }

    // ── Stage 117: check_with_headers — x-ratelimit reporting ──

    #[tokio::test]
    async fn test_check_with_headers_reports_remaining() {
        let limiter = RateLimiter::new();
        let now = std::time::Instant::now();
        let info = limiter
            .check_with_headers("h-remaining", Some(10), None, 0, now)
            .await
            .expect("within limit");
        assert_eq!(info.rpm_limit, 10);
        assert_eq!(info.rpm_remaining, 9);
        assert_eq!(info.tpm_limit, 0);
    }

    #[tokio::test]
    async fn test_check_with_headers_rpm_denied() {
        let limiter = RateLimiter::new();
        let now = std::time::Instant::now();
        // Exhaust the RPM bucket.
        for _ in 0..2 {
            limiter
                .check_with_headers("h-deny", Some(2), None, 0, now)
                .await
                .unwrap();
        }
        let err = limiter
            .check_with_headers("h-deny", Some(2), None, 0, now)
            .await
            .unwrap_err();
        assert!(matches!(err, RateLimitError::RateLimited { .. }));
    }

    #[tokio::test]
    async fn test_check_with_headers_tpm_denied() {
        let limiter = RateLimiter::new();
        let now = std::time::Instant::now();
        assert!(limiter
            .check_with_headers("h-tpm", None, Some(100), 60, now)
            .await
            .is_ok());
        let err = limiter
            .check_with_headers("h-tpm", None, Some(100), 60, now)
            .await
            .unwrap_err();
        assert!(matches!(err, RateLimitError::TpmLimited { .. }));
    }

    #[tokio::test]
    async fn test_check_with_headers_no_limits() {
        let limiter = RateLimiter::new();
        let now = std::time::Instant::now();
        let info = limiter
            .check_with_headers("h-none", None, None, 0, now)
            .await
            .expect("no limits always pass");
        assert_eq!(info.rpm_limit, 0);
        assert_eq!(info.tpm_limit, 0);
    }
}
