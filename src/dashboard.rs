//! Browser dashboard assets, embedded in the binary via `rust-embed` (DESIGN §9.2,
//! design/API-INTEGRATION.md §6 — single artifact, no separate web deploy, offline).
//!
//! The front-end is plain ES modules + CSS in `web/`; the daemon serves it from the same
//! HTTP server as the Control API. Unknown non-asset paths fall back to `index.html` so the
//! client-side router owns navigation.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/"]
struct WebAssets;

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        _ => "application/octet-stream",
    }
}

fn serve(path: &str) -> Option<Response> {
    let asset = WebAssets::get(path)?;
    Some(
        ([(header::CONTENT_TYPE, content_type(path))], asset.data).into_response(),
    )
}

/// Static asset handler with SPA fallback to `index.html`.
pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(resp) = serve(path) {
        return resp;
    }
    // Unknown path that isn't an asset request → SPA entry point.
    if !path.contains('.') {
        if let Some(resp) = serve("index.html") {
            return resp;
        }
    }
    (StatusCode::NOT_FOUND, "not found").into_response()
}
