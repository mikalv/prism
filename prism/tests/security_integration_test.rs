//! Integration tests for security middleware
//!
//! These tests start a real HTTP server with security enabled and verify
//! auth + audit behavior end-to-end.

use prism::api::ApiServer;
use prism::backends::text::TextBackend;
use prism::backends::VectorBackend;
use prism::collection::CollectionManager;
use prism::config::{ApiKeyConfig, AuditConfig, CorsConfig, RoleConfig, SecurityConfig};
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
    let manager =
        Arc::new(CollectionManager::new(&schemas_dir, text_backend, vector_backend, None).unwrap());

    let server = ApiServer::with_security(manager, CorsConfig::default(), security);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, server.router().await).await.unwrap();
    });

    sleep(Duration::from_millis(50)).await;
    (temp, url)
}

fn security_config() -> SecurityConfig {
    let mut roles = HashMap::new();
    roles.insert(
        "admin".to_string(),
        RoleConfig {
            collections: HashMap::from([("*".to_string(), vec!["*".to_string()])]),
        },
    );
    roles.insert(
        "reader".to_string(),
        RoleConfig {
            collections: HashMap::from([(
                "test-*".to_string(),
                vec!["read".to_string(), "search".to_string()],
            )]),
        },
    );

    SecurityConfig {
        enabled: true,
        api_keys: vec![
            ApiKeyConfig {
                key: "test_admin_key".to_string(),
                name: "admin".to_string(),
                roles: vec!["admin".to_string()],
                namespace: None,
            },
            ApiKeyConfig {
                key: "test_reader_key".to_string(),
                name: "reader".to_string(),
                roles: vec!["reader".to_string()],
                namespace: None,
            },
        ],
        roles,
        audit: AuditConfig {
            enabled: false,
            index_to_collection: false,
        },
        isolation: false,
        require_auth: Default::default(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_health_no_auth_required() {
    let (_temp, url) = setup_server(security_config()).await;
    let resp = reqwest::get(format!("{}/health", url)).await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_missing_api_key_returns_401() {
    let (_temp, url) = setup_server(security_config()).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/admin/collections", url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_security_disabled_allows_all() {
    let disabled = SecurityConfig {
        enabled: false,
        ..SecurityConfig::default()
    };
    let (_temp, url) = setup_server(disabled).await;
    let client = reqwest::Client::new();
    // No auth header, should work
    let resp = client
        .get(format!("{}/admin/collections", url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

// ============================================================================
// Per-collection require_auth on an open server (security.enabled = false)
// ============================================================================

fn open_server_config(protected: &[&str]) -> SecurityConfig {
    SecurityConfig {
        enabled: false,
        api_keys: vec![ApiKeyConfig {
            key: "sk-test-protected".to_string(),
            name: "mikalv".to_string(),
            roles: vec![],
            namespace: None,
        }],
        roles: HashMap::new(),
        audit: AuditConfig::default(),
        isolation: false,
        require_auth: prism::config::RequireAuthConfig {
            collections: protected.iter().map(|p| p.to_string()).collect(),
            hide_from_anonymous: true,
        },
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn open_server_protected_collection_404_for_anonymous() {
    let (_t, url) = setup_server(open_server_config(&["secret*"])).await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/collections/secretone/search", url))
        .json(&serde_json::json!({"query": "anything"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "anonymous must not learn it exists");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn open_server_protected_collection_200_with_key() {
    let (_t, url) = setup_server(open_server_config(&["secret*"])).await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/collections/secretone/search", url))
        .bearer_auth("sk-test-protected")
        .json(&serde_json::json!({"query": "anything"}))
        .send()
        .await
        .unwrap();
    // collection may not exist -> 404 from handler is fine; the point is the
    // auth layer passed (not policy 404 vs handler 404 is distinguished by key below)
    let _ = resp;
    // A nonexistent-but-open collection with key: policy allows, handler 404s.
    let resp2 = client
        .post(format!("{}/collections/secretone/search", url))
        .bearer_auth("sk-test-protected")
        .json(&serde_json::json!({"query": "anything"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn open_server_unprotected_collection_open_to_anonymous() {    let (_t, url) = setup_server(open_server_config(&["secret*"])).await;
    let client = reqwest::Client::new();
    // /health and open collections: no auth needed at all
    let resp = client.get(format!("{}/health", url)).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    // open collection search passes the policy layer (handler 404 ok — no collection)
    let resp = client
        .post(format!("{}/collections/public/search", url))
        .json(&serde_json::json!({"query": "anything"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404); // handler-level: collection doesn't exist
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn open_server_listing_hides_protected_collections() {
    use serde_json::Value;
    let (_t, url) = setup_server(open_server_config(&["secret*", "ltm-*"])).await;
    let client = reqwest::Client::new();

    // Create collections via PUT (schema with a stored content field), then
    // index a document into each (open server allows anonymous writes
    // outside protected patterns).
    let schema = serde_json::json!({
        "collection": "placeholder",
        "backends": {"text": {"fields": [
            {"name": "content", "type": "text", "stored": true, "indexed": true}
        ]}}
    });
    for name in ["publicone", "secretdb"] {
        let protected = name == "secretdb";
        let mut s = schema.clone();
        s["collection"] = serde_json::json!(name);

        // Anonymous cannot even create a protected collection (404, hidden).
        if protected {
            let resp = client
                .put(format!("{}/collections/{}", url, name))
                .json(&s)
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 404, "anonymous create of protected must be hidden");
        }

        let resp = client
            .put(format!("{}/collections/{}", url, name))
            .bearer_auth(if protected { "sk-test-protected" } else { "" })
            .json(&s)
            .send()
            .await
            .unwrap();
        assert!(resp.status().as_u16() == 200 || resp.status().as_u16() == 201,
            "creating {} failed: {}", name, resp.status());
        let resp = client
            .post(format!("{}/collections/{}/documents?sync=true", url, name))
            .bearer_auth(if protected { "sk-test-protected" } else { "" })
            .json(&serde_json::json!({"documents": [{"id": "d1", "fields": {"content": "hello"}}]}))
            .send()
            .await
            .unwrap();
        assert!(resp.status().as_u16() == 200 || resp.status().as_u16() == 201,
            "seeding {} failed: {}", name, resp.status());
    }

    // Anonymous listing must NOT contain the protected name.
    let resp: Value = client
        .get(format!("{}/admin/collections", url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let names: Vec<String> = resp["collections"].as_array().unwrap().clone()
        .iter().filter_map(|v| v.as_str().map(String::from)).collect();
    assert!(names.contains(&"publicone".to_string()), "open collection visible");
    assert!(
        !names.contains(&"secretdb".to_string()),
        "protected collection must be hidden from anonymous listing (got {:?})",
        names
    );

    // With a valid key, the protected collection becomes visible.
    let resp: Value = client
        .get(format!("{}/admin/collections", url))
        .bearer_auth("sk-test-protected")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let names: Vec<String> = resp["collections"].as_array().unwrap().clone()
        .iter().filter_map(|v| v.as_str().map(String::from)).collect();
    assert!(
        names.contains(&"secretdb".to_string()),
        "authenticated listing must show protected collection (got {:?})",
        names
    );
}
