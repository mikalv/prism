use super::types::{AuthUser, Permission};
use crate::config::SecurityConfig;
use std::collections::HashMap;
use subtle::ConstantTimeEq;

pub struct PermissionChecker {
    /// Stored API keys with metadata for constant-time lookup
    keys: Vec<(String, String, Vec<String>)>, // (key, name, roles)
    /// Role name -> collection patterns -> permissions
    roles: HashMap<String, Vec<(String, Vec<String>)>>,
}

impl PermissionChecker {
    pub fn new(config: &SecurityConfig) -> Self {
        let keys: Vec<(String, String, Vec<String>)> = config
            .api_keys
            .iter()
            .map(|ak| (ak.key.clone(), ak.name.clone(), ak.roles.clone()))
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

        Self { keys, roles }
    }

    /// Authenticate an API key using constant-time comparison.
    ///
    /// All stored keys are compared against the provided key regardless of
    /// whether a match is found early, preventing timing side-channels that
    /// could leak key prefixes.
    pub fn authenticate(&self, api_key: &str) -> Option<AuthUser> {
        let input_bytes = api_key.as_bytes();
        let mut matched: Option<(&str, &[String])> = None;

        for (stored_key, name, roles) in &self.keys {
            let stored_bytes = stored_key.as_bytes();

            // Only compare if lengths match (length itself is not secret
            // since an attacker can enumerate valid key lengths from the
            // config format, but the content must remain hidden)
            if stored_bytes.len() == input_bytes.len()
                && stored_bytes.ct_eq(input_bytes).into()
            {
                matched = Some((name.as_str(), roles.as_slice()));
            }
        }

        // Always iterate all keys before returning to avoid early-exit timing
        matched.map(|(name, roles)| {
            let prefix = if api_key.len() > 13 {
                format!("{}...", &api_key[..13])
            } else {
                api_key.to_string()
            };
            AuthUser {
                name: name.to_string(),
                roles: roles.to_vec(),
                key_prefix: prefix,
            }
        })
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
