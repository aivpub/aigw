//! Request-id generation (TD-006)
//!
//! The aigw gateway assigns every request a UUID v7 `RequestId` (stored as
//! `spend_logs.call_id`). To let clients reconcile a SpendLog directly from the
//! response header (`x-gw-call-id`), the server propagates this id back via
//! `tower_http::PropagateRequestIdLayer`. This module owns the UUID v7 maker so
//! both `main.rs` (real server) and integration tests can mount the identical
//! `SetRequestIdLayer` + `PropagateRequestIdLayer` stack.

use tower_http::request_id::{MakeRequestId, RequestId};

/// Custom UUID v7 request ID generator.
///
/// Produces lexicographically sortable request IDs for log correlation and
/// SpendLog `call_id` (UUID v7 = gateway request id).
#[derive(Clone, Default)]
pub struct UuidV7RequestId;

impl MakeRequestId for UuidV7RequestId {
    fn make_request_id<B>(&mut self, _request: &axum::http::Request<B>) -> Option<RequestId> {
        let id = uuid::Uuid::now_v7().to_string();
        Some(RequestId::new(id.parse().ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[test]
    fn test_uuid_v7_request_id() {
        let mut maker = UuidV7RequestId;
        let req = Request::builder().uri("/").body(()).unwrap();
        let id = maker
            .make_request_id(&req)
            .expect("make_request_id returns Some");
        let val = id.header_value().to_str().unwrap();
        // UUID v7 format: 8-4-4-4-12 hex with version nibble 7
        assert_eq!(val.len(), 36);
        assert_eq!(&val[14..15], "7", "expected UUID v7 version nibble");
    }

    #[test]
    fn test_uuid_v7_request_ids_are_unique() {
        let mut maker = UuidV7RequestId;
        let req = Request::builder().uri("/").body(()).unwrap();
        let a = maker
            .make_request_id(&req)
            .unwrap()
            .header_value()
            .to_str()
            .unwrap()
            .to_string();
        let b = maker
            .make_request_id(&req)
            .unwrap()
            .header_value()
            .to_str()
            .unwrap()
            .to_string();
        assert_ne!(a, b);
    }
}
