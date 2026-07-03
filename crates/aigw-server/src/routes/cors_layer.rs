//! CORS middleware — allows browser-based frontend to call aigw API.
//!
//! During development, the Vite dev server runs on a different origin
//! (e.g. localhost:5173) than aigw (localhost:4000). This middleware
//! adds permissive CORS headers to enable cross-origin requests.
//!
//! In production, when static files are served from the same origin,
//! CORS is not needed. The origin can be restricted at that point.

use axum::{
    extract::Request,
    http::{header, Method},
    middleware::Next,
    response::Response,
};

/// Axum middleware that adds CORS headers to all responses.
///
/// Headers added:
/// - `Access-Control-Allow-Origin: *`
/// - `Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS`
/// - `Access-Control-Allow-Headers: Authorization, Content-Type`
pub async fn add_cors_headers(request: Request, next: Next) -> Response {
    // Handle preflight (OPTIONS) requests by returning 200 OK immediately
    if request.method() == Method::OPTIONS {
        let mut response = Response::default();
        inject_cors_headers(response.headers_mut());
        return response;
    }

    let mut response = next.run(request).await;
    inject_cors_headers(response.headers_mut());
    response
}

/// Inject CORS headers into a response's header map.
fn inject_cors_headers(headers: &mut axum::http::HeaderMap) {
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        header::HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        header::HeaderValue::from_static("Authorization, Content-Type"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        middleware,
        routing::get,
        Router,
    };
    use serde_json::json;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_cors_headers_present() {
        async fn handler() -> axum::Json<serde_json::Value> {
            axum::Json(json!({"status": "ok"}))
        }

        let app = Router::new()
            .route("/test-cors", get(handler))
            .layer(middleware::from_fn(add_cors_headers));

        let req = Request::builder()
            .uri("/test-cors")
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
        assert_eq!(
            resp.headers()
                .get(header::ACCESS_CONTROL_ALLOW_METHODS)
                .unwrap(),
            "GET, POST, PUT, DELETE, OPTIONS"
        );
        assert_eq!(
            resp.headers()
                .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
                .unwrap(),
            "Authorization, Content-Type"
        );
    }

    #[tokio::test]
    async fn test_cors_preflight_handled() {
        async fn handler() -> axum::Json<serde_json::Value> {
            axum::Json(json!({"status": "ok"}))
        }

        let app = Router::new()
            .route("/test-cors", get(handler))
            .layer(middleware::from_fn(add_cors_headers));

        let req = Request::builder()
            .uri("/test-cors")
            .method("OPTIONS")
            .header("Origin", "http://localhost:5173")
            .header("Access-Control-Request-Method", "POST")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
    }

    #[tokio::test]
    async fn test_cors_headers_on_error_response() {
        async fn handler() -> Result<axum::Json<serde_json::Value>, StatusCode> {
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }

        let app = Router::new()
            .route("/test-cors-error", get(handler))
            .layer(middleware::from_fn(add_cors_headers));

        let req = Request::builder()
            .uri("/test-cors-error")
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        // CORS headers should still be present even on error responses
        assert_eq!(
            resp.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
    }
}
