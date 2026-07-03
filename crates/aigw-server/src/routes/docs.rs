//! Swagger UI documentation endpoint
//!
//! Endpoints:
//! - GET /docs — Swagger UI page rendering /openapi.json

use axum::response::Html;

/// GET /docs — Swagger UI page that renders the OpenAPI spec from /openapi.json
pub async fn docs_ui() -> Html<&'static str> {
    Html(SWAGGER_HTML)
}

const SWAGGER_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>aigw API Documentation</title>
    <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
</head>
<body>
    <div id="swagger-ui"></div>
    <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js" crossorigin></script>
    <script>
        SwaggerUIBundle({
            url: "/openapi.json",
            dom_id: "#swagger-ui",
            presets: [SwaggerUIBundle.presets.apis, SwaggerUIBundle.SwaggerUIStandalonePreset],
            layout: "BaseLayout",
            deepLinking: true,
        });
    </script>
</body>
</html>"##;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{header, Method, Request},
        Router,
    };
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn test_docs_ui_returns_html() {
        let app = Router::new().route("/docs", axum::routing::get(docs_ui));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/docs")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        // Verify content-type is HTML
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            content_type.contains("text/html"),
            "Expected text/html content-type, got: {}",
            content_type
        );
    }

    #[tokio::test]
    async fn test_docs_ui_contains_swagger() {
        let app = Router::new().route("/docs", axum::routing::get(docs_ui));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/docs")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

        assert!(
            body_str.contains("swagger-ui"),
            "Response should contain 'swagger-ui'"
        );
        assert!(
            body_str.contains("openapi.json"),
            "Response should reference openapi.json"
        );
    }
}
