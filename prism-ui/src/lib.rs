//! Embedded Web UI for Prism
//!
//! This crate provides a web UI that can be embedded into prism-server.
//! The UI assets are compiled into the binary at build time.
//!
//! ## Development Mode
//!
//! For rapid development, if a `webui/` directory exists in the current
//! working directory, files will be served from disk instead of the
//! embedded assets. This allows hot-reloading with tools like Vite.
//!
//! ```bash
//! # Symlink for development
//! ln -s websearch-ui/dist webui
//!
//! # Or run vite in watch mode
//! cd websearch-ui && npm run build -- --watch
//! ```

use axum::{
    body::Body,
    extract::Path,
    http::{header, Response, StatusCode},
    routing::get,
    Router,
};
use rust_embed::Embed;
use std::path::PathBuf;
use tracing::debug;

/// Embedded UI assets from websearch-ui/dist
#[derive(Embed)]
#[folder = "../websearch-ui/dist"]
#[include = "*.html"]
#[include = "*.js"]
#[include = "*.css"]
#[include = "*.ico"]
#[include = "*.svg"]
#[include = "*.png"]
#[include = "*.jpg"]
#[include = "*.woff"]
#[include = "*.woff2"]
#[include = "assets/*"]
struct EmbeddedAssets;

/// Create the UI router
///
/// Mount this at `/ui` in your main application:
///
/// ```ignore
/// let app = Router::new()
///     .merge(api_routes())
///     .nest("/ui", prism_ui::ui_router());
/// ```
pub fn ui_router() -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/*path", get(serve_asset))
}

/// Check if we should use development mode (serve from disk)
fn dev_webui_dir() -> Option<PathBuf> {
    let webui_dir = std::env::current_dir().ok()?.join("webui");
    if webui_dir.is_dir() {
        Some(webui_dir)
    } else {
        None
    }
}

async fn serve_index() -> Response<Body> {
    serve_file("index.html").await
}

async fn serve_asset(Path(path): Path<String>) -> Response<Body> {
    serve_file(&path).await
}

async fn serve_file(path: &str) -> Response<Body> {
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    // Development mode: check local webui/ directory first
    if let Some(webui_dir) = dev_webui_dir() {
        let file_path = webui_dir.join(path);
        if file_path.is_file() {
            debug!("Serving from dev directory: {:?}", file_path);
            return serve_from_disk(&file_path).await;
        }
    }

    // Production mode: serve from embedded assets
    match EmbeddedAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            debug!("Serving embedded asset: {} ({})", path, mime);

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .header(header::CACHE_CONTROL, cache_control_for(path))
                .body(Body::from(content.data.into_owned()))
                .unwrap()
        }
        None => {
            // SPA fallback: try serving index.html for client-side routing
            if !path.contains('.') {
                if let Some(index) = EmbeddedAssets::get("index.html") {
                    debug!("SPA fallback to index.html for: {}", path);
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                        .body(Body::from(index.data.into_owned()))
                        .unwrap();
                }
            }

            debug!("Asset not found: {}", path);
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header(header::CONTENT_TYPE, "text/plain")
                .body(Body::from("Not found"))
                .unwrap()
        }
    }
}

async fn serve_from_disk(file_path: &std::path::Path) -> Response<Body> {
    match tokio::fs::read(file_path).await {
        Ok(content) => {
            let mime = mime_guess::from_path(file_path).first_or_octet_stream();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                // No caching in dev mode
                .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
                .body(Body::from(content))
                .unwrap()
        }
        Err(_) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::from("Not found"))
            .unwrap(),
    }
}

/// Determine cache-control header based on file type
fn cache_control_for(path: &str) -> &'static str {
    if path.ends_with(".html") {
        // HTML files: no cache (always check for updates)
        "no-cache, must-revalidate"
    } else if path.contains("/assets/") || path.ends_with(".js") || path.ends_with(".css") {
        // Hashed assets: cache forever (1 year)
        "public, max-age=31536000, immutable"
    } else {
        // Other static files: cache for 1 hour
        "public, max-age=3600"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_control() {
        assert!(cache_control_for("index.html").contains("no-cache"));
        assert!(cache_control_for("assets/main.js").contains("immutable"));
        assert!(cache_control_for("main.css").contains("immutable"));
        assert!(cache_control_for("favicon.ico").contains("3600"));
    }
}
