use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use std::sync::Arc;

use super::permissions::PermissionChecker;
use super::types::Permission;

/// Routes that do not require authentication.
///
/// `/ui` is the embedded web UI: its static shell (HTML/JS/CSS) must load
/// without an API key, because the UI has to render before it can authenticate
/// its own API calls. The UI's data requests (`/api/*`, `/collections/*`) still
/// go through auth normally.
const AUTH_WHITELIST: &[&str] = &["/health", "/stats/server", "/stats/load", "/admin/tasks", "/ui"];

/// Whether a path is publicly reachable without authentication.
///
/// Matches a whitelist entry only at a path-segment boundary: `w` matches
/// exactly (`/ui`) or as a parent segment (`/ui/...`), never as a bare string
/// prefix (`/uinvite` must NOT match `/ui`). This prevents an unanchored
/// allowlist bypass where a sensitive route sharing a textual prefix with a
/// public one would skip authentication.
fn is_public_path(path: &str) -> bool {
    AUTH_WHITELIST
        .iter()
        .any(|w| path == *w || path.strip_prefix(w).is_some_and(|rest| rest.starts_with('/')))
}

/// Extract collection name from URL path
fn extract_collection(path: &str) -> Option<&str> {
    // Match /collections/:collection/...
    let path = path.strip_prefix("/collections/")?;
    path.split('/').next()
}

/// Whether a path is an administrative endpoint requiring the Admin permission.
/// Covers both `/admin/*` and `/_admin/*` — the latter hosts the most
/// destructive operations (encrypted export/import, detach/attach, key
/// generation) and was previously left ungated (any authenticated key).
fn is_admin_path(path: &str) -> bool {
    path.starts_with("/admin/") || path.starts_with("/_admin/")
}

/// Decide whether an authenticated user may perform this request.
///
/// - `/collections/:c/...` → collection-scoped permission for the method.
/// - `/admin/*` and `/_admin/*` → global (`*`) Admin permission.
/// - anything else → authentication alone suffices.
fn is_authorized(
    checker: &PermissionChecker,
    user: &super::types::AuthUser,
    method: &axum::http::Method,
    path: &str,
) -> bool {
    if let Some(collection) = extract_collection(path) {
        checker.check_permission(user, collection, required_permission(method, path))
    } else if is_admin_path(path) {
        checker.check_permission(user, "*", Permission::Admin)
    } else {
        true
    }
}

/// Determine required permission from HTTP method and path
fn required_permission(method: &axum::http::Method, path: &str) -> Permission {
    if is_admin_path(path) {
        return Permission::Admin;
    }

    match *method {
        axum::http::Method::GET => Permission::Read,
        axum::http::Method::POST => {
            if path.contains("/search")
                || path.contains("/_suggest")
                || path.contains("/_mlt")
                || path.contains("/aggregate")
            {
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
    if is_public_path(&path) {
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

    // Check authorization (collection-scoped or admin, per the path).
    if !is_authorized(&checker, &user, &method, &path) {
        return Err(StatusCode::FORBIDDEN);
    }

    // Store AuthUser in request extensions for handlers
    let mut request = request;
    request.extensions_mut().insert(user);

    Ok(next.run(request).await)
}

/// Dynamic auth middleware that takes ownership of PermissionChecker
/// Used for hot-reload support where config is read from RwLock
pub async fn auth_middleware_dynamic(
    checker: PermissionChecker,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    // Skip auth for whitelisted routes
    if is_public_path(&path) {
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

    // Check authorization (collection-scoped or admin, per the path).
    if !is_authorized(&checker, &user, &method, &path) {
        return Err(StatusCode::FORBIDDEN);
    }

    // Store AuthUser in request extensions for handlers
    let mut request = request;
    request.extensions_mut().insert(user);

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiKeyConfig, RoleConfig, SecurityConfig};
    use crate::security::types::AuthUser;
    use std::collections::HashMap;

    fn checker_with_role(role: &str, patterns: &[(&str, &[&str])]) -> PermissionChecker {
        let mut collections = HashMap::new();
        for (pat, perms) in patterns {
            collections.insert(
                pat.to_string(),
                perms.iter().map(|p| p.to_string()).collect(),
            );
        }
        let mut roles = HashMap::new();
        roles.insert(role.to_string(), RoleConfig { collections });
        let config = SecurityConfig {
            enabled: true,
            api_keys: vec![ApiKeyConfig {
                key: "k".to_string(),
                name: "u".to_string(),
                roles: vec![role.to_string()],
            }],
            roles,
            audit: Default::default(),
        };
        PermissionChecker::new(&config)
    }

    fn user(role: &str) -> AuthUser {
        AuthUser {
            name: "u".to_string(),
            roles: vec![role.to_string()],
            key_prefix: "k".to_string(),
        }
    }

    #[test]
    fn admin_paths_require_admin_permission() {
        assert!(is_admin_path("/admin/collections"));
        assert!(is_admin_path("/_admin/export/encrypted"));
        assert!(is_admin_path("/_admin/collections/foo/detach"));
        assert!(!is_admin_path("/collections/foo/search"));
        assert_eq!(
            required_permission(&axum::http::Method::POST, "/_admin/export/encrypted"),
            Permission::Admin
        );
    }

    #[test]
    fn non_admin_key_is_denied_on_underscore_admin_routes() {
        // Regression: /_admin/* was only authentication-gated. A read-only key
        // on all collections must NOT reach these destructive endpoints.
        let checker = checker_with_role("reader", &[("*", &["read", "search"])]);
        let u = user("reader");
        for path in [
            "/_admin/export/encrypted",
            "/_admin/import/encrypted",
            "/_admin/encryption/generate-key",
            "/_admin/collections/secret/detach",
            "/admin/collections",
        ] {
            assert!(
                !is_authorized(&checker, &u, &axum::http::Method::POST, path),
                "reader must be denied on admin route {path}"
            );
        }
        // Its allowed collection access still works.
        assert!(is_authorized(
            &checker,
            &u,
            &axum::http::Method::POST,
            "/collections/logs/search"
        ));
    }

    #[test]
    fn admin_key_is_allowed_on_underscore_admin_routes() {
        let checker = checker_with_role("root", &[("*", &["admin"])]);
        let u = user("root");
        assert!(is_authorized(
            &checker,
            &u,
            &axum::http::Method::POST,
            "/_admin/export/encrypted"
        ));
    }

    #[test]
    fn static_ui_is_public_but_api_is_not() {
        // The embedded web UI shell must load without an API key (a UI has to
        // render before it can authenticate its own API calls). The UI's data
        // calls (/api/*, /collections/*) still require auth.
        assert!(is_public_path("/ui"));
        assert!(is_public_path("/ui/"));
        assert!(is_public_path("/ui/assets/index-abc123.js"));
        assert!(is_public_path("/health"));
        assert!(is_public_path("/stats/server"));
        assert!(!is_public_path("/api/search"));
        assert!(!is_public_path("/collections/logs/search"));
        assert!(!is_public_path("/admin/collections"));
    }

    #[test]
    fn whitelist_matches_only_at_segment_boundary() {
        // Unanchored-allowlist bypass guard: a route that merely shares a
        // textual prefix with a public path must still require auth.
        assert!(!is_public_path("/uinvite"));
        assert!(!is_public_path("/ui-admin/secrets"));
        assert!(!is_public_path("/healthz-internal"));
        assert!(!is_public_path("/stats/server-secret"));
    }
}
