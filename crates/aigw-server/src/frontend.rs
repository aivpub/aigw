//! Embedded frontend SPA serving via rust-embed.
//!
//! The `dist/` directory from the Vite build is embedded at compile time
//! and served at `/admin` with client-side routing fallback to `index.html`.

use axum::body::Body;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

/// Embedded frontend assets from `crates/aigw-frontend/dist/`.
#[derive(RustEmbed)]
#[folder = "../aigw-frontend/dist/"]
struct FrontendAssets;

/// Serve an embedded frontend file by path.
///
/// Maps `/admin` → `index.html`, `/admin/` → `index.html`,
/// `/admin/assets/foo.js` → `assets/foo.js`.
/// Unknown paths fall back to `index.html` for SPA client-side routing.
pub async fn serve_frontend(uri: Uri) -> impl IntoResponse {
    let path = uri.path();

    // Strip /admin prefix to get the embedded file path
    let relative = path.strip_prefix("/admin").unwrap_or(path);
    let relative = relative.strip_prefix('/').unwrap_or(relative);

    // Serve index.html for root and SPA routes
    let file_path = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };

    match FrontendAssets::get(file_path) {
        Some(content) => {
            let mime = mime_guess::from_path(file_path).first_or_octet_stream();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .header(
                    header::CACHE_CONTROL,
                    if file_path.starts_with("assets/") {
                        "public, max-age=31536000, immutable"
                    } else {
                        "no-cache"
                    },
                )
                .body(Body::from(content.data))
                .unwrap()
        }
        None => {
            // SPA fallback: return index.html for client-side routing
            if let Some(index) = FrontendAssets::get("index.html") {
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/html")
                    .header(header::CACHE_CONTROL, "no-cache")
                    .body(Body::from(index.data))
                    .unwrap()
            } else {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("Not found"))
                    .unwrap()
            }
        }
    }
}
