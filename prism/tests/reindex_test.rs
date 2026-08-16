//! Reindex API tests: POST /admin/reindex re-embeds collections.

use prism::api::server::ApiServer;
use prism::backends::{TextBackend, VectorBackend};
use prism::collection::CollectionManager;
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::Arc;
use tempfile::TempDir;

async fn start_server() -> (TempDir, String) {
    let temp = TempDir::new().unwrap();
    let schemas_dir = temp.path().join("schemas");
    let data_dir = temp.path().join("data");
    std::fs::create_dir_all(&schemas_dir).unwrap();

    let text_backend = Arc::new(TextBackend::new(&data_dir).unwrap());
    let vector_backend = Arc::new(VectorBackend::new(&data_dir).unwrap());
    let manager =
        Arc::new(CollectionManager::new(&schemas_dir, text_backend, vector_backend, None).unwrap());
    manager.initialize().await.unwrap();

    let server = ApiServer::new(manager);
    let router = server.router().await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    (temp, base_url)
}

/// Hybrid schema: text field `body` is the embedding source, `embedding`
/// the target. No embedding provider is configured on the vector backend,
/// so reindex strips stale vectors and skips re-generation (documents keep
/// source text; auto-embed is a no-op without a provider).
fn hybrid_schema(name: &str) -> Value {
    json!({
        "collection": name,
        "backends": {
            "text": {
                "fields": [
                    {"name": "body", "type": "text", "stored": true, "indexed": true}
                ]
            },
            "vector": {
                "embedding_field": "embedding",
                "dimension": 4,
                "distance": "cosine"
            }
        },
        "embedding_generation": {
            "enabled": true,
            "model": "mock",
            "source_field": "body",
            "target_field": "embedding"
        }
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_reindex_endpoint_validates_input() {
    let (_t, base) = start_server().await;
    let client = Client::new();

    // empty collections list -> 400
    let r = client
        .post(format!("{}/admin/reindex", base))
        .json(&json!({"collections": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    // batch_size 0 -> 400
    let r = client
        .post(format!("{}/admin/reindex", base))
        .json(&json!({"collections": ["x"], "batch_size": 0}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    // unknown collection pattern -> 4xx with error body
    let r = client
        .post(format!("{}/admin/reindex", base))
        .json(&json!({"collections": ["no-such-*"]}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_client_error());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_reindex_text_only_collection_is_noop() {
    let (_t, base) = start_server().await;
    let client = Client::new();

    // Create a text-only collection
    let r = client
        .put(format!("{}/collections/reindex-text-only", base))
        .json(&json!({
            "collection": "reindex-text-only",
            "backends": {"text": {"fields": [
                {"name": "body", "type": "text", "stored": true, "indexed": true}
            ]}}
        }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "{}", r.text().await.unwrap_or_default());

    // Index a couple of docs
    for i in 0..3 {
        let r = client
            .post(format!("{}/collections/reindex-text-only/documents", base))
            .json(&json!({"documents": [{"id": format!("doc{}", i), "fields": {"body": format!("body {}", i)}}]}))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success(), "{}", r.text().await.unwrap_or_default());
    }

    // Reindex: no embedding config -> no-op, but reported as processed
    let r = client
        .post(format!("{}/admin/reindex", base))
        .json(&json!({"collections": ["reindex-text-only"]}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "{}", r.text().await.unwrap_or_default());
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["total_reembedded"], 0);
    assert_eq!(v["collections"][0]["collection"], "reindex-text-only");
}
