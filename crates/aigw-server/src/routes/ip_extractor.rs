//! Optional RightmostXForwardedFor extractor that tolerates missing/malformed headers.
//!
//! The built-in `RightmostXForwardedFor` rejects the request with 500 when the
//! header is missing or unparseable. This wrapper returns `None` only when all
//! three fallback layers fail:
//!   1. X-Forwarded-For  (standard proxy header)
//!   2. X-Real-IP        (nginx convention)
//!   3. ConnectInfo      (TCP peer address, direct connection fallback)
//!
//! In tests, direct access, or non-proxied deployments, ConnectInfo ensures we
//! always have a client IP.

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum_client_ip::RightmostXForwardedFor;
use std::net::{IpAddr, SocketAddr};

/// Wraps `RightmostXForwardedFor` as an optional extractor with three-layer fallback.
pub struct OptionalClientIp(pub Option<RightmostXForwardedFor>);

impl<S> FromRequestParts<S> for OptionalClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Layer 1: X-Forwarded-For (standard proxy header)
        if let Ok(ip) = RightmostXForwardedFor::from_request_parts(parts, state).await {
            return Ok(OptionalClientIp(Some(ip)));
        }

        // Layer 2: X-Real-IP header (nginx convention)
        if let Some(real_ip) = parts.headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
            if let Ok(addr) = real_ip.parse::<IpAddr>() {
                return Ok(OptionalClientIp(Some(RightmostXForwardedFor(addr))));
            }
        }

        // Layer 3: TCP peer address (direct connection / no proxy)
        if let Ok(ConnectInfo(addr)) =
            ConnectInfo::<SocketAddr>::from_request_parts(parts, state).await
        {
            return Ok(OptionalClientIp(Some(RightmostXForwardedFor(addr.ip()))));
        }

        Ok(OptionalClientIp(None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    fn build_request_with_header(
        addr: Option<SocketAddr>,
        header_name: &str,
        header_value: &str,
    ) -> Request<Body> {
        let mut req = Request::get("/")
            .header(header_name, header_value)
            .body(Body::empty())
            .unwrap();
        if let Some(addr) = addr {
            req.extensions_mut().insert(ConnectInfo(addr));
        }
        req
    }

    fn empty_request(addr: Option<SocketAddr>) -> Request<Body> {
        let mut req = Request::get("/").body(Body::empty()).unwrap();
        if let Some(addr) = addr {
            req.extensions_mut().insert(ConnectInfo(addr));
        }
        req
    }

    #[tokio::test]
    async fn x_forwarded_for_single_ip() {
        let req = build_request_with_header(None, "x-forwarded-for", "10.0.0.1");
        let (mut parts, _) = req.into_parts();
        let result = OptionalClientIp::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        let OptionalClientIp(Some(ip)) = result.unwrap() else {
            panic!("Expected Some(RightmostXForwardedFor)");
        };
        assert_eq!(ip.0, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[tokio::test]
    async fn x_forwarded_for_multiple_ips_extracts_rightmost() {
        let req = build_request_with_header(
            None,
            "x-forwarded-for",
            "203.0.113.1, 198.51.100.2, 10.0.0.99",
        );
        let (mut parts, _) = req.into_parts();
        let result = OptionalClientIp::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        let OptionalClientIp(Some(ip)) = result.unwrap() else {
            panic!("Expected Some(RightmostXForwardedFor)");
        };
        assert_eq!(ip.0, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 99)));
    }

    #[tokio::test]
    async fn fallback_to_x_real_ip_when_x_forwarded_for_missing() {
        let req = Request::get("/")
            .header("x-real-ip", "172.16.0.5")
            .body(Body::empty())
            .unwrap();
        let (mut parts, _) = req.into_parts();
        let result = OptionalClientIp::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        let OptionalClientIp(Some(ip)) = result.unwrap() else {
            panic!("Expected Some(RightmostXForwardedFor)");
        };
        assert_eq!(ip.0, IpAddr::V4(Ipv4Addr::new(172, 16, 0, 5)));
    }

    #[tokio::test]
    async fn x_forwarded_for_malformed_falls_back_to_x_real_ip() {
        let req = Request::get("/")
            .header("x-forwarded-for", "not-an-ip") // malformed, will be rejected by RightmostXForwardedFor
            .header("x-real-ip", "10.0.1.1")
            .body(Body::empty())
            .unwrap();
        let (mut parts, _) = req.into_parts();
        let result = OptionalClientIp::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        let OptionalClientIp(Some(ip)) = result.unwrap() else {
            panic!("Expected Some(RightmostXForwardedFor)");
        };
        assert_eq!(ip.0, IpAddr::V4(Ipv4Addr::new(10, 0, 1, 1)));
    }

    #[tokio::test]
    async fn fallback_to_connect_info_when_no_headers() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 54321);
        let req = empty_request(Some(addr));
        let (mut parts, _) = req.into_parts();
        let result = OptionalClientIp::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        let OptionalClientIp(Some(ip)) = result.unwrap() else {
            panic!("Expected Some(RightmostXForwardedFor) from ConnectInfo");
        };
        assert_eq!(ip.0, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)));
    }

    #[tokio::test]
    async fn returns_none_when_all_layers_fail() {
        // No headers, no ConnectInfo extension
        let req = Request::get("/").body(Body::empty()).unwrap();
        let (mut parts, _) = req.into_parts();
        let result = OptionalClientIp::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().0.is_none());
    }

    #[tokio::test]
    async fn ipv6_address_support() {
        let req = build_request_with_header(None, "x-forwarded-for", "::1");
        let (mut parts, _) = req.into_parts();
        let result = OptionalClientIp::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        let OptionalClientIp(Some(ip)) = result.unwrap() else {
            panic!("Expected Some for IPv6");
        };
        assert_eq!(ip.0, IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)));
    }

    #[tokio::test]
    async fn x_real_ip_malformed_and_no_connect_info_returns_none() {
        let req = Request::get("/")
            .header("x-real-ip", "not-an-ip")
            .body(Body::empty())
            .unwrap();
        let (mut parts, _) = req.into_parts();
        let result = OptionalClientIp::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().0.is_none());
    }
}
