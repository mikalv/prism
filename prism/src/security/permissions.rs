use super::types::{AuthUser, Permission};
use crate::config::SecurityConfig;
use std::collections::HashMap;

pub struct PermissionChecker {
    /// API key -> (name, roles)
    keys: HashMap<String, (String, Vec<String>)>,
    /// Role name -> collection patterns -> permissions
    roles: HashMap<String, Vec<(String, Vec<String>)>>,
}

impl PermissionChecker {
    pub fn new(config: &SecurityConfig) -> Self {
        let keys: HashMap<String, (String, Vec<String>)> = config
            .api_keys
            .iter()
            .map(|ak| (ak.key.clone(), (ak.name.clone(), ak.roles.clone())))
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

    pub fn authenticate(&self, api_key: &str) -> Option<AuthUser> {
        self.keys.get(api_key).map(|(name, roles)| {
            let prefix = if api_key.len() > 13 {
                format!("{}...", &api_key[..13])
            } else {
                api_key.to_string()
            };
            AuthUser {
                name: name.clone(),
                roles: roles.clone(),
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
