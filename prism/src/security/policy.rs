//! Collection policies: composable per-request decisions beyond role-based auth.
//!
//! The [`CollectionPolicy`] trait decouples *what* is enforced from *where* it
//! is enforced. The auth middleware builds a [`PolicyChain`] per request and
//! consults it alongside role-based permission checks. Future policies
//! (rate limiting, field masking, search filters, …) implement the trait and
//! slot into the chain without touching the middleware again.
//!
//! The first — and currently only — policy is [`RequireAuthPolicy`]:
//! collections matching configured glob patterns require an authenticated
//! caller even when the rest of the server is open (`security.enabled = false`).

use crate::config::SecurityConfig;

/// Everything a policy needs to decide a request.
pub struct RequestCtx<'a> {
    pub path: &'a str,
    pub method: &'a str,
    /// Collection name, when the path is collection-scoped (`/collections/:c/...`).
    pub collection: Option<&'a str>,
    /// Authenticated user, when a valid API key was presented.
    pub user: Option<&'a super::types::AuthUser>,
}

impl RequestCtx<'_> {
    /// Whether this request carries a valid authenticated user.
    pub fn is_authenticated(&self) -> bool {
        self.user.is_some()
    }
}

/// A policy verdict for a request.
pub enum Decision {
    /// No opinion — consult the next policy (or fall through to role checks).
    Allow,
    /// Reject with this HTTP status code (e.g. 404 to hide existence).
    Deny(u16),
    /// Authentication is required to proceed (401).
    Challenge,
}

/// A composable per-request policy.
pub trait CollectionPolicy: Send + Sync + AsAny {
    /// Stable name for logging/metrics.
    fn name(&self) -> &'static str;
    fn evaluate(&self, ctx: &RequestCtx<'_>) -> Decision;
}
/// Ordered chain of policies; the first non-`Allow` decision wins.
/// An empty chain allows everything (policies are purely additive).
pub struct PolicyChain {
    policies: Vec<Box<dyn CollectionPolicy>>,
}

impl PolicyChain {
    pub fn new(policies: Vec<Box<dyn CollectionPolicy>>) -> Self {
        Self { policies }
    }

    /// Evaluate all policies in order; first non-Allow decision wins.
    pub fn evaluate(&self, ctx: &RequestCtx<'_>) -> Decision {
        for p in &self.policies {
            match p.evaluate(ctx) {
                Decision::Allow => continue,
                other => return other,
            }
        }
        Decision::Allow
    }

    /// Whether any policy in the chain protects this collection (used by
    /// enumeration surfaces to hide protected collections from callers who
    /// cannot see them).
    pub fn protects(&self, collection: &str) -> bool {
        self.policies
            .iter()
            .filter_map(|p| {
                let any: &dyn std::any::Any = p.as_ref().as_any();
                any.downcast_ref::<RequireAuthPolicy>()
            })
            .any(|p| p.protects_collection(collection))
    }
}

/// Helper for downcasting: every CollectionPolicy knows its concrete type.
pub trait AsAny {
    fn as_any(&self) -> &dyn std::any::Any;
}

impl<T: 'static> AsAny for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Policy: require authentication (any valid API key) for the configured
/// collection patterns, regardless of `security.enabled`.
///
/// - Path outside all patterns → `Allow` (no opinion).
/// - Matching path, caller authenticated → `Allow` (role checks still apply
///   separately when security is enabled).
/// - Matching path, anonymous → `Deny(404)` (hide existence) or `Challenge`
///   (401) depending on configuration.
pub struct RequireAuthPolicy {
    /// Glob patterns (same syntax as roles: trailing `*`, exact, or `*`).
    patterns: Vec<String>,
    /// Deny anonymous callers with 404 (hide) instead of 401 (challenge).
    hide: bool,
}

impl RequireAuthPolicy {
    pub fn new(patterns: Vec<String>, hide: bool) -> Self {
        Self { patterns, hide }
    }

    fn matches(&self, collection: &str) -> bool {
        self.patterns
            .iter()
            .any(|p| super::permissions::glob_match(p, collection))
    }
}

impl CollectionPolicy for RequireAuthPolicy {
    fn name(&self) -> &'static str {
        "require_auth"
    }

    fn evaluate(&self, ctx: &RequestCtx<'_>) -> Decision {
        let Some(collection) = ctx.collection else {
            return Decision::Allow;
        };
        if !self.matches(collection) {
            return Decision::Allow;
        }
        if ctx.is_authenticated() {
            Decision::Allow
        } else if self.hide {
            Decision::Deny(404)
        } else {
            Decision::Challenge
        }
    }
}

impl RequireAuthPolicy {
    pub fn protects_collection(&self, collection: &str) -> bool {
        self.matches(collection)
    }
}

/// Build the configured policy chain from security config.
pub fn build_policy_chain(config: &SecurityConfig) -> PolicyChain {
    let mut policies: Vec<Box<dyn CollectionPolicy>> = Vec::new();

    if !config.require_auth.collections.is_empty() {
        policies.push(Box::new(RequireAuthPolicy::new(
            config.require_auth.collections.clone(),
            config.require_auth.hide_from_anonymous,
        )));
    }

    PolicyChain::new(policies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::types::AuthUser;

    fn user() -> AuthUser {
        AuthUser {
            name: "u".into(),
            roles: vec![],
            key_prefix: String::new(),
            namespace: None,
        }
    }

    fn ctx<'a>(path: &'a str, collection: Option<&'a str>, user: Option<&'a AuthUser>) -> RequestCtx<'a> {
        RequestCtx { path, method: "GET", collection, user }
    }

    #[test]
    fn require_auth_allows_unmatched_collections() {
        let p = RequireAuthPolicy::new(vec!["secret*".into()], true);
        let c = ctx("/collections/open/...", Some("open"), None);
        assert!(matches!(p.evaluate(&c), Decision::Allow));
    }

    #[test]
    fn require_auth_hides_protected_from_anonymous() {
        let p = RequireAuthPolicy::new(vec!["secret*".into()], true);
        let c = ctx("/collections/secretone/search", Some("secretone"), None);
        assert!(matches!(p.evaluate(&c), Decision::Deny(404)));
    }

    #[test]
    fn require_auth_challenges_when_not_hiding() {
        let p = RequireAuthPolicy::new(vec!["secret*".into()], false);
        let c = ctx("/collections/secretone/search", Some("secretone"), None);
        assert!(matches!(p.evaluate(&c), Decision::Challenge));
    }

    #[test]
    fn require_auth_allows_authenticated_on_protected() {
        let p = RequireAuthPolicy::new(vec!["secret*".into()], true);
        let u = user();
        let c = ctx("/collections/secretone/search", Some("secretone"), Some(&u));
        assert!(matches!(p.evaluate(&c), Decision::Allow));
    }

    #[test]
    fn chain_first_non_allow_wins() {
        struct DenyAll;
        impl CollectionPolicy for DenyAll {
            fn name(&self) -> &'static str { "deny_all" }
            fn evaluate(&self, _ctx: &RequestCtx<'_>) -> Decision { Decision::Deny(403) }
        }
        let chain = PolicyChain::new(vec![Box::new(DenyAll), Box::new(RequireAuthPolicy::new(vec!["*".into()], true))]);
        // DenyAll runs first and denies everything regardless of auth.
        let u = user();
        let c = ctx("/collections/x/search", Some("x"), Some(&u));
        assert!(matches!(chain.evaluate(&c), Decision::Deny(403)));
    }

    #[test]
    fn chain_empty_allows_all() {
        let chain = PolicyChain::new(vec![]);
        let c = ctx("/collections/x/search", Some("x"), None);
        assert!(matches!(chain.evaluate(&c), Decision::Allow));
    }

    #[test]
    fn protects_reports_matching_collections() {
        let p = RequireAuthPolicy::new(vec!["ltm-*".into()], true);
        assert!(p.protects_collection("ltm-memories"));
        assert!(!p.protects_collection("idx_clearnet"));
    }
}
