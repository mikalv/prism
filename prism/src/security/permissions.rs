use super::types::{AuthUser, Permission};
use crate::config::SecurityConfig;
use std::collections::HashMap;
use subtle::ConstantTimeEq;

pub struct PermissionChecker {
    /// Stored API keys with metadata for constant-time lookup
    keys: Vec<(String, String, Vec<String>, Option<String>)>, // (key, name, roles, namespace)
    /// Role name -> collection patterns -> permissions
    roles: HashMap<String, Vec<(String, Vec<String>)>>,
    /// Opt-in isolation mode (namespace implicit grants + default-deny surfaces).
    isolation: bool,
}

impl PermissionChecker {
    pub fn new(config: &SecurityConfig) -> Self {
        let keys: Vec<(String, String, Vec<String>, Option<String>)> = config
            .api_keys
            .iter()
            .map(|ak| {
                (
                    ak.key.clone(),
                    ak.name.clone(),
                    ak.roles.clone(),
                    ak.namespace.clone(),
                )
            })
            .collect();

        let roles: HashMap<String, Vec<(String, Vec<String>)>> = config
            .roles
            .iter()
            .map(|(name, role_config)| {
                let patterns: Vec<(String, Vec<String>)> = role_config
                    .collections
                    .iter()
                    .map(|(pat, perms)| (pat.clone(), perms.clone()))
                    .collect();
                (name.clone(), patterns)
            })
            .collect();

        Self {
            keys,
            roles,
            isolation: config.isolation,
        }
    }

    /// Authenticate an API key using constant-time comparison.
    ///
    /// All stored keys are compared against the provided key regardless of
    /// whether a match is found early, preventing timing side-channels that
    /// could leak key prefixes.
    pub fn authenticate(&self, api_key: &str) -> Option<AuthUser> {
        let input_bytes = api_key.as_bytes();
        let mut matched: Option<(&str, &[String], &Option<String>)> = None;

        for (stored_key, name, roles, namespace) in &self.keys {
            let stored_bytes = stored_key.as_bytes();

            // Only compare if lengths match (length itself is not secret
            // since an attacker can enumerate valid key lengths from the
            // config format, but the content must remain hidden)
            if stored_bytes.len() == input_bytes.len() && stored_bytes.ct_eq(input_bytes).into() {
                matched = Some((name.as_str(), roles.as_slice(), namespace));
            }
        }

        // Always iterate all keys before returning to avoid early-exit timing
        matched.map(|(name, roles, namespace)| {
            let prefix = if api_key.len() > 13 {
                format!("{}...", &api_key[..13])
            } else {
                api_key.to_string()
            };
            AuthUser {
                name: name.to_string(),
                roles: roles.to_vec(),
                key_prefix: prefix,
                namespace: namespace.clone(),
            }
        })
    }

    /// Return the subset of `all` collections the user may act on with the given
    /// permission. This is the single resolver used by every enumeration and
    /// fan-out surface (collection listing, multi-search, ES-compat `_cat`, etc.)
    /// so isolation is enforced consistently and no surface is missed.
    pub fn visible_collections(
        &self,
        user: &AuthUser,
        all: Vec<String>,
        permission: Permission,
    ) -> Vec<String> {
        all.into_iter()
            .filter(|c| self.check_permission(user, c, permission))
            .collect()
    }

    /// Whether opt-in isolation mode is enabled. Surfaces use this to decide
    /// whether a denied single-collection access should 404 (hide existence)
    /// rather than 403.
    pub fn is_isolation(&self) -> bool {
        self.isolation
    }

    pub fn check_permission(
        &self,
        user: &AuthUser,
        collection: &str,
        permission: Permission,
    ) -> bool {
        for role_name in &user.roles {
            if let Some(patterns) = self.roles.get(role_name) {
                for (pattern, perms) in patterns {
                    if glob_match(pattern, collection)
                        && perms.iter().any(|p| p == "*" || p == permission.as_str())
                    {
                        return true;
                    }
                }
            }
        }

        // Isolation mode: an api key's namespace implicitly grants read+search
        // on `<namespace>*` (never write/delete/admin) without an explicit role.
        if self.isolation && matches!(permission, Permission::Read | Permission::Search) {
            if let Some(ns) = &user.namespace {
                if collection.starts_with(ns.as_str()) {
                    return true;
                }
            }
        }

        false
    }
}

/// Simple glob matching: only supports trailing `*` (e.g., `logs-*`, `*`)
fn glob_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        pattern == value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiKeyConfig, RoleConfig, SecurityConfig};

    fn checker_with_role(role: &str, pattern: &str, perms: &[&str]) -> PermissionChecker {
        let mut roles = HashMap::new();
        roles.insert(
            role.to_string(),
            RoleConfig {
                collections: HashMap::from([(
                    pattern.to_string(),
                    perms.iter().map(|p| p.to_string()).collect(),
                )]),
            },
        );
        let config = SecurityConfig {
            enabled: true,
            api_keys: vec![],
            roles,
            audit: Default::default(),
            isolation: false,
        };
        PermissionChecker::new(&config)
    }

    fn user_with_roles(roles: &[&str]) -> AuthUser {
        AuthUser {
            name: "u".into(),
            roles: roles.iter().map(|r| r.to_string()).collect(),
            key_prefix: String::new(),
            namespace: None,
        }
    }

    fn checker_with_namespace_key(namespace: &str, isolation: bool) -> PermissionChecker {
        let config = SecurityConfig {
            enabled: true,
            api_keys: vec![ApiKeyConfig {
                key: "k".into(),
                name: "mikalv".into(),
                roles: vec![],
                namespace: Some(namespace.to_string()),
            }],
            roles: HashMap::new(),
            audit: Default::default(),
            isolation,
        };
        PermissionChecker::new(&config)
    }

    #[test]
    fn visible_collections_returns_only_permitted() {
        let checker = checker_with_role("mikalv", "ws_mikalv_*", &["search"]);
        let user = user_with_roles(&["mikalv"]);
        let all = vec![
            "ws_mikalv_code_a".to_string(),
            "ws_mikalv_docs_b".to_string(),
            "ws_eyrmedical_code_c".to_string(),
            "mail".to_string(),
        ];

        let visible = checker.visible_collections(&user, all, Permission::Search);

        assert_eq!(
            visible,
            vec![
                "ws_mikalv_code_a".to_string(),
                "ws_mikalv_docs_b".to_string()
            ]
        );
    }

    #[test]
    fn isolation_namespace_grants_read_and_search_without_explicit_role() {
        let checker = checker_with_namespace_key("ws_mikalv_", true);
        let user = checker.authenticate("k").unwrap();

        // Namespace implicitly grants search+read within it...
        assert!(checker.check_permission(&user, "ws_mikalv_code_a", Permission::Search));
        assert!(checker.check_permission(&user, "ws_mikalv_code_a", Permission::Read));
        // ...but nothing outside it, and not write/delete/admin inside it.
        assert!(!checker.check_permission(&user, "ws_eyrmedical_b", Permission::Search));
        assert!(!checker.check_permission(&user, "ws_mikalv_code_a", Permission::Write));
    }

    #[test]
    fn namespace_does_not_grant_when_isolation_off() {
        let checker = checker_with_namespace_key("ws_mikalv_", false);
        let user = checker.authenticate("k").unwrap();

        // With isolation disabled the namespace is inert (default-deny).
        assert!(!checker.check_permission(&user, "ws_mikalv_code_a", Permission::Search));
    }
}
