use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

use super::permissions::PermissionChecker;
use super::types::{AuthUser, Permission};

/// Routes that do not require authentication
const AUTH_WHITELIST: &[&str] = &["/health", "/stats/server"];

/// Extract collection name from URL path
fn extract_collection(path: &str) -> Option<&str> {
    // Match /collections/:collection/...
    let path = path.strip_prefix("/collections/")?;
    path.split('/').next()
}

/// Determine required permission from HTTP method and path
fn required_permission(method: &axum::http::Method, path: &str) -> Permission {
    if path.starts_with("/admin/") {
        return Permission::Admin;
    }

    match *method {
        axum::http::Method::GET => Permission::Read,
        axum::http::Method::POST => {
            if path.contains("/search") || path.contains("/_suggest") || path.contains("/_mlt") || path.contains("/aggregate") {
                Permission::Search
            } else {
                Permission::Write
            }
        }
        axum::http::Method::DELETE => Permission::Delete,
        axum::http::Method::PUT => Permission::Write,
        _ => Permission::Read,
    }
}

pub async fn auth_middleware(
    checker: Arc<PermissionChecker>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    // Skip auth for whitelisted routes
    if AUTH_WHITELIST.iter().any(|w| path.starts_with(w)) {
        return Ok(next.run(request).await);
    }

    // Extract API key from Authorization header
    let api_key = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let api_key = match api_key {
        Some(k) => k,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    // Authenticate
    let user = match checker.authenticate(api_key) {
        Some(u) => u,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    // Check collection-level permission
    if let Some(collection) = extract_collection(&path) {
        let perm = required_permission(&method, &path);
        if !checker.check_permission(&user, collection, perm) {
            return Err(StatusCode::FORBIDDEN);
        }
    } else if path.starts_with("/admin/") {
        if !checker.check_permission(&user, "*", Permission::Admin) {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    // Store AuthUser in request extensions for handlers
    let mut request = request;
    request.extensions_mut().insert(user);

    Ok(next.run(request).await)
}
