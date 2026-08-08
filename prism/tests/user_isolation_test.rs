//! Integration tests for user-isolation collection visibility.
//!
//! An authenticated identity must only see the collections it has been granted
//! access to. Here we call the `/admin/collections` handler directly with an
//! auth context and assert it filters via the permission model.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Extension;
use prism::api::routes::{
    get_collection_stats, get_document, list_collections, search, SearchRequest,
};
use prism::backends::{TextBackend, VectorBackend};
use prism::collection::CollectionManager;
use prism::config::{RoleConfig, SecurityConfig};
use prism::schema::CollectionSchema;
use prism::security::permissions::PermissionChecker;
use prism::security::types::AuthUser;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;

fn text_schema_yaml(name: &str) -> String {
    format!(
        "collection: {name}\nbackends:\n  text:\n    fields:\n      - name: title\n        type: text\n        indexed: true\n"
    )
}

async fn manager_with(names: &[&str]) -> (TempDir, Arc<CollectionManager>) {
    let temp = TempDir::new().unwrap();
    let schemas_dir = temp.path().join("schemas");
    let data_dir = temp.path().join("data");
    std::fs::create_dir_all(&schemas_dir).unwrap();
    let text = Arc::new(TextBackend::new(&data_dir).unwrap());
    let vector = Arc::new(VectorBackend::new(&data_dir).unwrap());
    let manager = Arc::new(CollectionManager::new(&schemas_dir, text, vector, None).unwrap());
    manager.initialize().await.unwrap();
    for name in names {
        let schema: CollectionSchema = serde_yaml::from_str(&text_schema_yaml(name)).unwrap();
        manager.add_collection(schema).await.unwrap();
    }
    (temp, manager)
}

fn checker_granting(role: &str, pattern: &str, perms: &[&str]) -> Arc<PermissionChecker> {
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
    Arc::new(PermissionChecker::new(&SecurityConfig {
        enabled: true,
        api_keys: vec![],
        roles,
        audit: Default::default(),
        isolation: false,
    }))
}

fn user_with_role(role: &str) -> AuthUser {
    AuthUser {
        name: role.to_string(),
        roles: vec![role.to_string()],
        key_prefix: String::new(),
        namespace: None,
    }
}

#[tokio::test]
async fn list_collections_filters_to_visible_when_authenticated() {
    let (_temp, manager) = manager_with(&["ws_mikalv_a", "ws_eyrmedical_b"]).await;
    let checker = checker_granting("mikalv", "ws_mikalv_*", &["search"]);
    let user = user_with_role("mikalv");

    let resp = list_collections(
        State(manager.clone()),
        Some(Extension(user)),
        Some(Extension(checker)),
    )
    .await;

    assert_eq!(resp.0.collections, vec!["ws_mikalv_a".to_string()]);
}

#[tokio::test]
async fn per_collection_search_denied_for_unauthorized_collection() {
    let (_temp, manager) = manager_with(&["ws_mikalv_a", "ws_eyrmedical_b"]).await;
    let checker = checker_granting("mikalv", "ws_mikalv_*", &["search"]);
    let user = user_with_role("mikalv");
    let req: SearchRequest = serde_json::from_value(serde_json::json!({"query": "*"})).unwrap();

    // Directly searching a collection outside the caller's grant is forbidden,
    // even though simple_search already filters — this closes the direct-name hole.
    let res = search(
        Path("ws_eyrmedical_b".to_string()),
        State(manager.clone()),
        Some(Extension(user)),
        Some(Extension(checker)),
        axum::Json(req),
    )
    .await;

    assert!(matches!(res, Err((StatusCode::FORBIDDEN, _))));
}

#[tokio::test]
async fn per_collection_get_document_denied_for_unauthorized_collection() {
    let (_temp, manager) = manager_with(&["ws_mikalv_a", "ws_eyrmedical_b"]).await;
    let checker = checker_granting("mikalv", "ws_mikalv_*", &["read"]);
    let user = user_with_role("mikalv");

    let res = get_document(
        Path(("ws_eyrmedical_b".to_string(), "doc1".to_string())),
        State(manager.clone()),
        Some(Extension(user)),
        Some(Extension(checker)),
    )
    .await;

    assert!(matches!(res, Err(StatusCode::FORBIDDEN)));
}

#[tokio::test]
async fn per_collection_stats_denied_for_unauthorized_collection() {
    let (_temp, manager) = manager_with(&["ws_mikalv_a", "ws_eyrmedical_b"]).await;
    let checker = checker_granting("mikalv", "ws_mikalv_*", &["read"]);
    let user = user_with_role("mikalv");

    // Own collection is allowed...
    let ok = get_collection_stats(
        Path("ws_mikalv_a".to_string()),
        State(manager.clone()),
        Some(Extension(user.clone())),
        Some(Extension(checker.clone())),
    )
    .await;
    assert!(ok.is_ok());

    // ...another user's is not.
    let denied = get_collection_stats(
        Path("ws_eyrmedical_b".to_string()),
        State(manager.clone()),
        Some(Extension(user)),
        Some(Extension(checker)),
    )
    .await;
    assert!(matches!(denied, Err(StatusCode::FORBIDDEN)));
}

#[tokio::test]
async fn list_collections_returns_all_when_unauthenticated() {
    let (_temp, manager) = manager_with(&["ws_mikalv_a", "ws_eyrmedical_b"]).await;

    let resp = list_collections(State(manager.clone()), None, None).await;

    let mut got = resp.0.collections.clone();
    got.sort();
    assert_eq!(
        got,
        vec!["ws_eyrmedical_b".to_string(), "ws_mikalv_a".to_string()]
    );
}
