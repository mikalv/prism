use prism::config::{ApiKeyConfig, RoleConfig, SecurityConfig};
use prism::security::permissions::PermissionChecker;
use prism::security::types::Permission;
use std::collections::HashMap;

fn test_config() -> SecurityConfig {
    let mut roles = HashMap::new();
    roles.insert(
        "admin".to_string(),
        RoleConfig {
            collections: HashMap::from([("*".to_string(), vec!["*".to_string()])]),
        },
    );
    roles.insert(
        "analyst".to_string(),
        RoleConfig {
            collections: HashMap::from([
                (
                    "logs-*".to_string(),
                    vec!["read".to_string(), "search".to_string()],
                ),
                (
                    "metrics-*".to_string(),
                    vec!["read".to_string(), "search".to_string()],
                ),
            ]),
        },
    );
    roles.insert(
        "writer".to_string(),
        RoleConfig {
            collections: HashMap::from([(
                "products".to_string(),
                vec![
                    "read".to_string(),
                    "write".to_string(),
                    "delete".to_string(),
                ],
            )]),
        },
    );

    SecurityConfig {
        enabled: true,
        api_keys: vec![
            ApiKeyConfig {
                key: "prism_ak_admin".to_string(),
                name: "admin".to_string(),
                roles: vec!["admin".to_string()],
            },
            ApiKeyConfig {
                key: "prism_ak_analyst".to_string(),
                name: "analyst".to_string(),
                roles: vec!["analyst".to_string()],
            },
            ApiKeyConfig {
                key: "prism_ak_writer".to_string(),
                name: "writer".to_string(),
                roles: vec!["writer".to_string()],
            },
        ],
        roles,
        audit: Default::default(),
    }
}

#[test]
fn test_lookup_api_key() {
    let config = test_config();
    let checker = PermissionChecker::new(&config);

    let user = checker.authenticate("prism_ak_admin").unwrap();
    assert_eq!(user.name, "admin");
    assert_eq!(user.roles, vec!["admin"]);
    assert_eq!(user.key_prefix, "prism_ak_admi...");

    assert!(checker.authenticate("invalid_key").is_none());
}

#[test]
fn test_admin_can_access_everything() {
    let config = test_config();
    let checker = PermissionChecker::new(&config);
    let user = checker.authenticate("prism_ak_admin").unwrap();

    assert!(checker.check_permission(&user, "logs-2026", Permission::Read));
    assert!(checker.check_permission(&user, "products", Permission::Write));
    assert!(checker.check_permission(&user, "anything", Permission::Admin));
}

#[test]
fn test_analyst_read_only_matching_collections() {
    let config = test_config();
    let checker = PermissionChecker::new(&config);
    let user = checker.authenticate("prism_ak_analyst").unwrap();

    // Can read/search matching collections
    assert!(checker.check_permission(&user, "logs-2026", Permission::Read));
    assert!(checker.check_permission(&user, "logs-2026", Permission::Search));
    assert!(checker.check_permission(&user, "metrics-cpu", Permission::Read));

    // Cannot write
    assert!(!checker.check_permission(&user, "logs-2026", Permission::Write));

    // Cannot access non-matching collections
    assert!(!checker.check_permission(&user, "products", Permission::Read));
}

#[test]
fn test_writer_specific_collection() {
    let config = test_config();
    let checker = PermissionChecker::new(&config);
    let user = checker.authenticate("prism_ak_writer").unwrap();

    assert!(checker.check_permission(&user, "products", Permission::Read));
    assert!(checker.check_permission(&user, "products", Permission::Write));
    assert!(checker.check_permission(&user, "products", Permission::Delete));

    // Cannot access other collections
    assert!(!checker.check_permission(&user, "logs-2026", Permission::Read));
}

#[test]
fn test_glob_pattern_matching() {
    let config = test_config();
    let checker = PermissionChecker::new(&config);

    // logs-* should match logs-anything
    let user = checker.authenticate("prism_ak_analyst").unwrap();
    assert!(checker.check_permission(&user, "logs-production", Permission::Read));
    assert!(checker.check_permission(&user, "logs-", Permission::Read));
    assert!(!checker.check_permission(&user, "logs", Permission::Read)); // no dash, no match
}
