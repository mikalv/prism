//! Integration tests for security middleware
//!
//! These tests start a real HTTP server with security enabled and verify
//! auth + audit behavior end-to-end.

use prism::api::ApiServer;
use prism::backends::text::TextBackend;
use prism::backends::VectorBackend;
use prism::collection::CollectionManager;
use prism::config::{SecurityConfig, ApiKeyConfig, RoleConfig, AuditConfig, CorsConfig};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{sleep, Duration};

async fn setup_server(security: SecurityConfig) -> (TempDir, String) {
    let temp = TempDir::new().unwrap();
    let schemas_dir = temp.path().join("schemas");
    std::fs::create_dir_all(&schemas_dir).unwrap();

    let text_backend = Arc::new(TextBackend::new(temp.path()).unwrap());
    let vector_backend = Arc::new(VectorBackend::new(temp.path()).unwrap());
    let manager = Arc::new(
        CollectionManager::new(&schemas_dir, text_backend, vector_backend).unwrap(),
    );

    let server = ApiServer::with_security(manager, CorsConfig::default(), security);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, server.router()).await.unwrap();
    });

    sleep(Duration::from_millis(50)).await;
    (temp, url)
}

fn security_config() -> SecurityConfig {
    let mut roles = HashMap::new();
    roles.insert("admin".to_string(), RoleConfig {
        collections: HashMap::from([("*".to_string(), vec!["*".to_string()])]),
    });
    roles.insert("reader".to_string(), RoleConfig {
        collections: HashMap::from([("test-*".to_string(), vec!["read".to_string(), "search".to_string()])]),
    });

    SecurityConfig {
        enabled: true,
        api_keys: vec![
            ApiKeyConfig { key: "test_admin_key".to_string(), name: "admin".to_string(), roles: vec!["admin".to_string()] },
            ApiKeyConfig { key: "test_reader_key".to_string(), name: "reader".to_string(), roles: vec!["reader".to_string()] },
        ],
        roles,
        audit: AuditConfig { enabled: false, index_to_collection: false },
    }
}

#[tokio::test]
async fn test_health_no_auth_required() {
    let (_temp, url) = setup_server(security_config()).await;
    let resp = reqwest::get(format!("{}/health", url)).await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_missing_api_key_returns_401() {
    let (_temp, url) = setup_server(security_config()).await;
    let client = reqwest::Client::new();
    let resp = client.get(format!("{}/admin/collections", url)).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_invalid_api_key_returns_401() {
    let (_temp, url) = setup_server(security_config()).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/admin/collections", url))
        .header("Authorization", "Bearer bad_key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_valid_admin_key_returns_200() {
    let (_temp, url) = setup_server(security_config()).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/admin/collections", url))
        .header("Authorization", "Bearer test_admin_key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_reader_cannot_access_admin() {
    let (_temp, url) = setup_server(security_config()).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/admin/collections", url))
        .header("Authorization", "Bearer test_reader_key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_security_disabled_allows_all() {
    let disabled = SecurityConfig::default(); // enabled = false
    let (_temp, url) = setup_server(disabled).await;
    let client = reqwest::Client::new();
    // No auth header, should work
    let resp = client.get(format!("{}/admin/collections", url)).send().await.unwrap();
    assert_eq!(resp.status(), 200);
}
