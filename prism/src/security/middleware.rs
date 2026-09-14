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
const AUTH_WHITELIST: &[&str] = &["/health", "/stats/server", "/ui"];

/// Whether a path is publicly reachable without authentication.
///
/// Matches a whitelist entry only at a path-segment boundary: `w` matches
/// exactly (`/ui`) or as a parent segment (`/ui/...`), never as a bare string
/// prefix (`/uinvite` must NOT match `/ui`). This prevents an unanchored
/// allowlist bypass where a sensitive route sharing a textual prefix with a
/// public one would skip authentication.
fn is_public_path(path: &str) -> bool {
    AUTH_WHITELIST.iter().any(|w| {
        path == *w
            || path
                .strip_prefix(w)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// First path segments that are native (non-collection) endpoints, so their
/// leading segment must NOT be mistaken for an Elasticsearch index name.
/// `_`-prefixed ES endpoints (`/_search`, `/_bulk`, `/_cat/...`) are handled
/// separately by the `_` check.
const RESERVED_ROOTS: &[&str] = &[
    "collections", "api", "ui", "health", "stats", "metrics", "err", "ok", "sse", "admin",
];

/// Extract the target collection name(s) from a request path.
///
/// Two mounting styles carry a collection in the path:
/// - native:    `/collections/:collection/...`
/// - ES-compat: `/:index/...` and bare `/:index`, where the first segment is
///   neither a reserved root (see `RESERVED_ROOTS`) nor a `_`-prefixed ES
///   endpoint (`/_search`, `/_bulk`, `/_cat/indices`, ...).
///
/// The ES index segment may be a comma-separated list (`/a,b/_search`), so a
/// `Vec` is returned and the caller must authorize *every* named index.
/// Returns `None` when no collection is named in the path (cluster/meta routes
/// and the index-less cross-collection endpoints like `/_search`).
fn extract_collections(path: &str) -> Option<Vec<&str>> {
    if let Some(rest) = path.strip_prefix("/collections/") {
        let c = rest.split('/').next().filter(|s| !s.is_empty())?;
        return Some(vec![c]);
    }
    let first = path.strip_prefix('/')?.split('/').next()?;
    if first.is_empty() || first.starts_with('_') || RESERVED_ROOTS.contains(&first) {
        return None;
    }
    // ES index segment: split comma-lists, drop empties.
    let cols: Vec<&str> = first.split(',').filter(|s| !s.is_empty()).collect();
    if cols.is_empty() {
        None
    } else {
        Some(cols)
    }
}

/// Index-less Elasticsearch endpoints that read, write, or enumerate across
/// *all* collections. They carry no index in the path, so authorization falls
/// back to a check against the `*` collection: only a key granted the relevant
/// permission on all collections may use them. Per-result filtering for
/// narrowly-scoped keys is a separate, handler-level concern.
fn is_cross_collection_route(path: &str) -> bool {
    matches!(
        path,
        "/_search"
            | "/_msearch"
            | "/_mget"
            | "/_bulk"
            | "/_async_search"
            | "/_pit"
            | "/_field_caps"
            | "/_stats"
    ) || path.starts_with("/_cat/")
        || path.starts_with("/_resolve/")
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
/// - a collection-scoped path (`/collections/:c/...` or ES `/:index/...`) →
///   the method's permission on *every* named index.
/// - `/admin/*` and `/_admin/*` → global (`*`) Admin permission.
/// - index-less cross-collection ES routes (`/_search`, `/_cat/...`, ...) →
///   the method's permission on the `*` collection (all collections).
/// - anything else (cluster/meta routes) → authentication alone suffices.
fn is_authorized(
    checker: &PermissionChecker,
    user: &super::types::AuthUser,
    method: &axum::http::Method,
    path: &str,
) -> bool {
    if is_admin_path(path) {
        return checker.check_permission(user, "*", Permission::Admin);
    }
    if let Some(collections) = extract_collections(path) {
        let perm = required_permission(method, path);
        return collections
            .iter()
            .all(|c| checker.check_permission(user, c, perm));
    }
    if is_cross_collection_route(path) {
        return checker.check_permission(user, "*", required_permission(method, path));
    }
    true
}

/// Determine required permission from HTTP method and path
fn required_permission(method: &axum::http::Method, path: &str) -> Permission {
    if is_admin_path(path) {
        return Permission::Admin;
    }

    match *method {
        axum::http::Method::GET => Permission::Read,
        axum::http::Method::POST => {
            // Read-shaped ES/native verbs need only Search; everything else
            // POSTed is a write. `_delete_by_query` is intentionally treated as
            // a write (>= Delete would also pass a write-capable key).
            if path.ends_with("/search")
                || path.ends_with("/_search")
                || path.ends_with("/_msearch")
                || path.ends_with("/_count")
                || path.ends_with("/_field_caps")
                || path.ends_with("/_async_search")
                || path.ends_with("/_pit")
                || path.ends_with("/_suggest")
                || path.ends_with("/_mlt")
                || path.ends_with("/aggregate")
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

    // Store AuthUser and PermissionChecker in request extensions for handlers
    let mut request = request;
    request.extensions_mut().insert(user);
    request.extensions_mut().insert(checker);

    Ok(next.run(request).await)
}

/// Dynamic auth middleware that takes ownership of PermissionChecker
/// Used for hot-reload support where config is read from RwLock
///
/// Also carries the per-collection policy chain. Policies run for every
/// request — including anonymous ones — so `require_auth` collections stay
/// protected even when `security.enabled = false` (open server). When
/// `security.enabled = true`, global authentication and role checks apply
/// on top, exactly as before.
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

    // Extract API key from Authorization header (optional: policies decide
    // whether authentication is required for this request)
    let api_key = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let user = api_key.and_then(|k| checker.authenticate(k));

    // Per-collection policy chain: first non-Allow decision wins. ES paths may
    // name several indices (`/a,b/_search`); the policy chain is per-collection,
    // so use the first named index (the common single-index case).
    let collection = extract_collections(&path).and_then(|c| c.into_iter().next());
    let ctx = crate::security::policy::RequestCtx {
        path: &path,
        method: method.as_str(),
        collection,
        user: user.as_ref(),
    };
    match checker.policy_chain().evaluate(&ctx) {
        crate::security::policy::Decision::Allow => {}
        crate::security::policy::Decision::Deny(code) => {
            return Err(StatusCode::from_u16(code).unwrap_or(StatusCode::NOT_FOUND));
        }
        crate::security::policy::Decision::Challenge => {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    // Global auth (security.enabled): authentication is mandatory and role
    // checks apply. Without it the request proceeds (policies above were the
    // only gate).
    if checker.auth_required() {
        match user.as_ref() {
            None => return Err(StatusCode::UNAUTHORIZED),
            Some(u) => {
                if !is_authorized(&checker, u, &method, &path) {
                    return Err(StatusCode::FORBIDDEN);
                }
            }
        }
    }

    // Store AuthUser (when authenticated) and PermissionChecker in request
    // extensions for handlers
    let mut request = request;
    if let Some(user) = user {
        request.extensions_mut().insert(user);
    }
    request.extensions_mut().insert(Arc::new(checker));

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
                namespace: None,
            }],
            roles,
            audit: Default::default(),
            isolation: false,
            require_auth: Default::default(),
        };
        PermissionChecker::new(&config)
    }

    fn user(role: &str) -> AuthUser {
        AuthUser {
            name: "u".to_string(),
            roles: vec![role.to_string()],
            key_prefix: "k".to_string(),
            namespace: None,
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

    #[test]
    fn es_index_paths_are_collection_scoped() {
        // Regression: the ES-compat API mounts collections as `/:index/_*`, not
        // `/collections/:c/...`. A key scoped to one index must NOT reach
        // another via the ES routes (read, write, or delete).
        let checker = checker_with_role("alpha", &[("alpha", &["read", "search", "write", "delete"])]);
        let u = user("alpha");
        // Own index: allowed.
        assert!(is_authorized(&checker, &u, &axum::http::Method::POST, "/alpha/_search"));
        assert!(is_authorized(&checker, &u, &axum::http::Method::PUT, "/alpha/_doc/1"));
        assert!(is_authorized(&checker, &u, &axum::http::Method::GET, "/alpha/_doc/1"));
        // Other index: denied on every verb.
        assert!(!is_authorized(&checker, &u, &axum::http::Method::POST, "/beta/_search"));
        assert!(!is_authorized(&checker, &u, &axum::http::Method::PUT, "/beta/_doc/1"));
        assert!(!is_authorized(&checker, &u, &axum::http::Method::DELETE, "/beta/_doc/1"));
        assert!(!is_authorized(&checker, &u, &axum::http::Method::GET, "/beta/_doc/1"));
        // Bare index create/delete is scoped too.
        assert!(!is_authorized(&checker, &u, &axum::http::Method::PUT, "/beta"));
        assert!(!is_authorized(&checker, &u, &axum::http::Method::DELETE, "/beta"));
    }

    #[test]
    fn es_multi_index_requires_all() {
        // A comma-separated index list authorizes only if the key covers every
        // named index — one unauthorized index denies the whole request.
        let checker = checker_with_role("alpha", &[("alpha", &["read", "search"])]);
        let u = user("alpha");
        assert!(is_authorized(&checker, &u, &axum::http::Method::POST, "/alpha/_search"));
        assert!(!is_authorized(&checker, &u, &axum::http::Method::POST, "/alpha,beta/_search"));
    }

    #[test]
    fn cross_collection_routes_need_star_permission() {
        // Index-less endpoints span all collections. A key scoped to one index
        // must not read-all or enumerate; a key granted `*` may.
        let scoped = checker_with_role("alpha", &[("alpha", &["read", "search"])]);
        let us = user("alpha");
        for path in ["/_search", "/_msearch", "/_bulk", "/_cat/indices", "/_stats"] {
            assert!(
                !is_authorized(&scoped, &us, &axum::http::Method::POST, path),
                "scoped key must be denied on cross-collection route {path}"
            );
        }
        // `/_cat/indices` is a GET enumeration — still denied for a scoped key.
        assert!(!is_authorized(&scoped, &us, &axum::http::Method::GET, "/_cat/indices"));

        let all = checker_with_role("reader", &[("*", &["read", "search"])]);
        let ua = user("reader");
        assert!(is_authorized(&all, &ua, &axum::http::Method::POST, "/_search"));
        assert!(is_authorized(&all, &ua, &axum::http::Method::GET, "/_cat/indices"));
    }

    #[test]
    fn native_non_index_roots_are_not_treated_as_indices() {
        // First-segment roots like `api`/`stats` must not be read as an ES
        // index named "api" (which would demand a bogus collection permission).
        assert_eq!(extract_collections("/api/search"), None);
        assert_eq!(extract_collections("/stats/server"), None);
        assert_eq!(extract_collections("/_search"), None);
        assert_eq!(extract_collections("/health"), None);
        assert_eq!(extract_collections("/collections/logs/search"), Some(vec!["logs"]));
        assert_eq!(extract_collections("/myindex/_doc/1"), Some(vec!["myindex"]));
        assert_eq!(extract_collections("/a,b/_search"), Some(vec!["a", "b"]));
    }

    #[test]
    fn es_search_needs_only_search_permission() {
        // POST `/:index/_search` is a read; a search-only key must be able to
        // run it (it must not be misclassified as a write).
        let checker = checker_with_role("s", &[("alpha", &["search"])]);
        let u = user("s");
        assert!(is_authorized(&checker, &u, &axum::http::Method::POST, "/alpha/_search"));
        // But not write.
        assert!(!is_authorized(&checker, &u, &axum::http::Method::PUT, "/alpha/_doc/1"));
    }
}
