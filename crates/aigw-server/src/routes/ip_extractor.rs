//! Optional RightmostXForwardedFor extractor that tolerates missing/malformed headers.
//!
//! The built-in `RightmostXForwardedFor` rejects the request with 500 when the
//! header is missing or unparseable. This wrapper returns `None` instead,
//! allowing handlers to gracefully degrade when no proxy sets the header
//! (e.g. in tests, direct access, or non-proxied deployments).

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_client_ip::RightmostXForwardedFor;

/// Wraps `RightmostXForwardedFor` as an optional extractor — `None` on failure.
pub struct OptionalClientIp(pub Option<RightmostXForwardedFor>);

impl<S> FromRequestParts<S> for OptionalClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match RightmostXForwardedFor::from_request_parts(parts, state).await {
            Ok(ip) => Ok(OptionalClientIp(Some(ip))),
            Err(_) => Ok(OptionalClientIp(None)),
        }
    }
}
