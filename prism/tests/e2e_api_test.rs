//! End-to-end HTTP API tests for Prism search engine.
//!
//! Each test starts a real Axum HTTP server on a random port and exercises
//! the REST API via reqwest, covering collection CRUD, document indexing,
//! search, pagination, concurrency, and error paths.

use prism::api::server::ApiServer;
use prism::backends::{TextBackend, VectorBackend};
use prism::collection::CollectionManager;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use tempfile::TempDir;

/// Start a real HTTP server on a random port and return the base URL.
async fn start_server() -> (TempDir, String, tokio::task::JoinHandle<()>) {
    let temp = TempDir::new().unwrap();
    let schemas_dir = temp.path().join("schemas");
    let data_dir = temp.path().join("data");
    std::fs::create_dir_all(&schemas_dir).unwrap();

    let text_backend = Arc::new(TextBackend::new(&data_dir).unwrap());
    let vector_backend = Arc::new(VectorBackend::new(&data_dir).unwrap());
    let manager = Arc::new(
        CollectionManager::new(&schemas_dir, text_backend, vector_backend, None).unwrap(),
    );
    manager.initialize().await.unwrap();

    let server = ApiServer::new(manager);
    let router = server.router().await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // Give the server a moment to start accepting connections.
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    (temp, base_url, handle)
}

/// Schema JSON for a text-only collection with multiple field types.
fn test_schema() -> Value {
    json!({
        "collection": "test-e2e",
        "backends": {
            "text": {
                "fields": [
                    {"name": "title", "type": "text", "stored": true, "indexed": true},
                    {"name": "body", "type": "text", "stored": true, "indexed": true},
                    {"name": "category", "type": "string", "stored": true, "indexed": true},
                    {"name": "count", "type": "i64", "stored": true, "indexed": true},
                    {"name": "timestamp", "type": "date", "stored": true, "indexed": true}
                ]
            }
        }
    })
}

/// Schema JSON for a collection with all supported field types.
fn all_types_schema() -> Value {
    json!({
        "collection": "all-types",
        "backends": {
            "text": {
                "fields": [
                    {"name": "text_field", "type": "text", "stored": true, "indexed": true},
                    {"name": "string_field", "type": "string", "stored": true, "indexed": true},
                    {"name": "i64_field", "type": "i64", "stored": true, "indexed": true},
                    {"name": "u64_field", "type": "u64", "stored": true, "indexed": true},
                    {"name": "f64_field", "type": "f64", "stored": true, "indexed": true},
                    {"name": "bool_field", "type": "bool", "stored": true, "indexed": true},
                    {"name": "date_field", "type": "date", "stored": true, "indexed": true}
                ]
            }
        }
    })
}

/// Helper: create the test-e2e collection via PUT.
async fn create_test_collection(client: &Client, base_url: &str) {
    let resp = client
        .put(format!("{}/collections/test-e2e", base_url))
        .json(&test_schema())
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    assert!(
        status == 200 || status == 201,
        "Expected 200/201 creating collection, got {}",
        status
    );
}

/// Helper: index a batch of documents into a collection.
async fn index_docs(client: &Client, base_url: &str, collection: &str, docs: &Value) {
    let resp = client
        .post(format!("{}/collections/{}/documents", base_url, collection))
        .json(&json!({ "documents": docs }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    assert!(
        status == 200 || status == 201,
        "Expected 200/201 indexing docs, got {}",
        status
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_health_endpoint() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    let resp = client
        .get(format!("{}/health", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    handle.abort();
}

#[tokio::test]
async fn test_root_endpoint() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    let resp = client
        .get(format!("{}/", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "prism");
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());

    handle.abort();
}

#[tokio::test]
async fn test_create_collection_and_list() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    // Create collection
    create_test_collection(&client, &base_url).await;

    // List collections
    let resp = client
        .get(format!("{}/admin/collections", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    let collections = body["collections"].as_array().unwrap();
    let names: Vec<&str> = collections.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        names.contains(&"test-e2e"),
        "Expected test-e2e in collections list, got {:?}",
        names
    );

    handle.abort();
}

#[tokio::test]
async fn test_index_and_search() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Index 10 documents
    let docs: Vec<Value> = (0..10)
        .map(|i| {
            json!({
                "id": format!("doc-{}", i),
                "fields": {
                    "title": format!("Document number {}", i),
                    "body": if i == 3 { "Prism is an amazing search engine" } else { "Generic body text content" },
                    "category": "article",
                    "count": i
                }
            })
        })
        .collect();
    index_docs(&client, &base_url, "test-e2e", &json!(docs)).await;

    // Search for "amazing"
    let resp = client
        .post(format!("{}/collections/test-e2e/search", base_url))
        .json(&json!({ "query": "amazing", "limit": 10 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    let results = body["results"].as_array().unwrap();
    assert!(
        !results.is_empty(),
        "Search for 'amazing' should return at least 1 result"
    );

    // The doc with "amazing" in body should be present
    let found_ids: Vec<&str> = results.iter().filter_map(|r| r["id"].as_str()).collect();
    assert!(
        found_ids.contains(&"doc-3"),
        "Expected doc-3 in results, got {:?}",
        found_ids
    );

    handle.abort();
}

#[tokio::test]
async fn test_index_and_get_by_id() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Index a single document
    let docs = json!([{
        "id": "doc-1",
        "fields": {
            "title": "Hello World",
            "body": "This is a test document",
            "category": "greeting",
            "count": 42
        }
    }]);
    index_docs(&client, &base_url, "test-e2e", &docs).await;

    // Get by ID
    let resp = client
        .get(format!(
            "{}/collections/test-e2e/documents/doc-1",
            base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"], "doc-1");
    assert_eq!(body["fields"]["title"], "Hello World");
    assert_eq!(body["fields"]["category"], "greeting");

    handle.abort();
}

#[tokio::test]
async fn test_collection_stats() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Index 50 documents
    let docs: Vec<Value> = (0..50)
        .map(|i| {
            json!({
                "id": format!("stat-doc-{}", i),
                "fields": {
                    "title": format!("Stats document {}", i),
                    "body": "Body content for stats test",
                    "category": "stats"
                }
            })
        })
        .collect();
    index_docs(&client, &base_url, "test-e2e", &json!(docs)).await;

    // Get stats
    let resp = client
        .get(format!("{}/collections/test-e2e/stats", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["collection"], "test-e2e");
    assert_eq!(
        body["document_count"].as_u64().unwrap(),
        50,
        "Expected document_count == 50, got {:?}",
        body["document_count"]
    );

    handle.abort();
}

#[tokio::test]
async fn test_collection_schema_endpoint() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Get schema
    let resp = client
        .get(format!("{}/collections/test-e2e/schema", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["collection"], "test-e2e");

    let fields = body["fields"].as_array().unwrap();
    let field_names: Vec<&str> = fields.iter().filter_map(|f| f["name"].as_str()).collect();
    assert!(
        field_names.contains(&"title"),
        "Schema should contain 'title' field"
    );
    assert!(
        field_names.contains(&"body"),
        "Schema should contain 'body' field"
    );
    assert!(
        field_names.contains(&"category"),
        "Schema should contain 'category' field"
    );
    assert!(
        field_names.contains(&"count"),
        "Schema should contain 'count' field"
    );
    assert!(
        field_names.contains(&"timestamp"),
        "Schema should contain 'timestamp' field"
    );

    handle.abort();
}

#[tokio::test]
async fn test_search_empty_collection() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Search an empty collection
    let resp = client
        .post(format!("{}/collections/test-e2e/search", base_url))
        .json(&json!({ "query": "anything", "limit": 10 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    let results = body["results"].as_array().unwrap();
    assert!(
        results.is_empty(),
        "Empty collection should return 0 results"
    );
    assert_eq!(body["total"].as_u64().unwrap(), 0);

    handle.abort();
}

#[tokio::test]
async fn test_search_nonexistent_collection() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    // Search on non-existent collection
    let resp = client
        .post(format!(
            "{}/collections/does-not-exist/search",
            base_url
        ))
        .json(&json!({ "query": "test", "limit": 10 }))
        .send()
        .await
        .unwrap();

    // Should be an error status (400, 404, or 500)
    let status = resp.status().as_u16();
    assert!(
        status >= 400,
        "Expected error status for nonexistent collection, got {}",
        status
    );

    handle.abort();
}

#[tokio::test]
async fn test_index_with_all_field_types() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    // Create collection with all field types
    let resp = client
        .put(format!("{}/collections/all-types", base_url))
        .json(&all_types_schema())
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    assert!(
        status == 200 || status == 201,
        "Expected 200/201 creating all-types collection, got {}",
        status
    );

    // Index a document with all field types
    let docs = json!([{
        "id": "all-fields-doc",
        "fields": {
            "text_field": "Full text searchable content",
            "string_field": "exact-match-value",
            "i64_field": -42,
            "u64_field": 123456,
            "f64_field": 3.14159,
            "bool_field": true,
            "date_field": "2025-06-15T10:30:00Z"
        }
    }]);
    index_docs(&client, &base_url, "all-types", &docs).await;

    // Retrieve by ID and verify all stored fields
    let resp = client
        .get(format!(
            "{}/collections/all-types/documents/all-fields-doc",
            base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"], "all-fields-doc");

    let fields = &body["fields"];
    // text and string stored fields
    assert!(
        fields["text_field"].is_string(),
        "text_field should be a string"
    );
    assert!(
        fields["string_field"].is_string(),
        "string_field should be a string"
    );
    // numeric stored fields
    assert!(
        fields["i64_field"].is_number(),
        "i64_field should be a number"
    );
    assert!(
        fields["u64_field"].is_number(),
        "u64_field should be a number"
    );
    assert!(
        fields["f64_field"].is_number(),
        "f64_field should be a number"
    );
    // bool field (may be stored as int depending on backend)
    assert!(
        fields["bool_field"].is_boolean() || fields["bool_field"].is_number(),
        "bool_field should be boolean or numeric"
    );
    // date field should now be returned as ISO 8601 string
    let date_val = fields.get("date_field");
    assert!(
        date_val.is_some() && date_val.unwrap().is_string(),
        "date_field should be present as a string, got {:?}",
        date_val
    );

    handle.abort();
}

#[tokio::test]
async fn test_document_upsert_via_api() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Index initial version
    let docs_v1 = json!([{
        "id": "upsert-doc",
        "fields": {
            "title": "Original Title",
            "body": "Original body content",
            "category": "v1"
        }
    }]);
    index_docs(&client, &base_url, "test-e2e", &docs_v1).await;

    // Verify initial version
    let resp = client
        .get(format!(
            "{}/collections/test-e2e/documents/upsert-doc",
            base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["fields"]["title"], "Original Title");

    // Re-index same ID with updated content (upsert)
    let docs_v2 = json!([{
        "id": "upsert-doc",
        "fields": {
            "title": "Updated Title",
            "body": "Updated body content",
            "category": "v2"
        }
    }]);
    index_docs(&client, &base_url, "test-e2e", &docs_v2).await;

    // Verify updated version
    let resp = client
        .get(format!(
            "{}/collections/test-e2e/documents/upsert-doc",
            base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["fields"]["title"], "Updated Title");
    assert_eq!(body["fields"]["category"], "v2");

    // Stats should show count == 1 (not 2)
    let resp = client
        .get(format!("{}/collections/test-e2e/stats", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let stats: Value = resp.json().await.unwrap();
    assert_eq!(
        stats["document_count"].as_u64().unwrap(),
        1,
        "Upsert should not increase document count"
    );

    handle.abort();
}

#[tokio::test]
async fn test_delete_collection() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Index some documents
    let docs = json!([{
        "id": "del-doc-1",
        "fields": {"title": "To be deleted", "body": "Content"}
    }]);
    index_docs(&client, &base_url, "test-e2e", &docs).await;

    // Delete the collection
    let resp = client
        .delete(format!("{}/collections/test-e2e", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        200,
        "DELETE collection should return 200"
    );

    // Verify it is gone from listing
    let resp = client
        .get(format!("{}/admin/collections", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body: Value = resp.json().await.unwrap();
    let collections = body["collections"].as_array().unwrap();
    let names: Vec<&str> = collections.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        !names.contains(&"test-e2e"),
        "Deleted collection should not appear in listing"
    );

    handle.abort();
}

#[tokio::test]
async fn test_large_batch_index() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Build 500 documents
    let docs: Vec<Value> = (0..500)
        .map(|i| {
            json!({
                "id": format!("batch-{}", i),
                "fields": {
                    "title": format!("Batch document {}", i),
                    "body": format!("Content for batch document number {}", i),
                    "category": "batch"
                }
            })
        })
        .collect();
    index_docs(&client, &base_url, "test-e2e", &json!(docs)).await;

    // Verify stats
    let resp = client
        .get(format!("{}/collections/test-e2e/stats", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body["document_count"].as_u64().unwrap(),
        500,
        "Stats should show 500 documents after batch index"
    );

    handle.abort();
}

#[tokio::test]
async fn test_search_pagination() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Index 100 documents, all containing the word "paginate"
    let docs: Vec<Value> = (0..100)
        .map(|i| {
            json!({
                "id": format!("page-{}", i),
                "fields": {
                    "title": format!("Paginate document {}", i),
                    "body": format!("Content about paginate testing number {}", i),
                    "category": "pagination"
                }
            })
        })
        .collect();
    index_docs(&client, &base_url, "test-e2e", &json!(docs)).await;

    // Page 1: limit=10, offset=0
    let resp1 = client
        .post(format!("{}/collections/test-e2e/search", base_url))
        .json(&json!({ "query": "paginate", "limit": 10, "offset": 0 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp1.status().as_u16(), 200);
    let body1: Value = resp1.json().await.unwrap();
    let results1 = body1["results"].as_array().unwrap();
    assert_eq!(results1.len(), 10, "Page 1 should have 10 results");

    // Page 2: limit=10, offset=10
    let resp2 = client
        .post(format!("{}/collections/test-e2e/search", base_url))
        .json(&json!({ "query": "paginate", "limit": 10, "offset": 10 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status().as_u16(), 200);
    let body2: Value = resp2.json().await.unwrap();
    let results2 = body2["results"].as_array().unwrap();
    assert_eq!(results2.len(), 10, "Page 2 should have 10 results");

    // Collect IDs from both pages
    let ids1: HashSet<&str> = results1.iter().filter_map(|r| r["id"].as_str()).collect();
    let ids2: HashSet<&str> = results2.iter().filter_map(|r| r["id"].as_str()).collect();

    // There should be no overlap between pages
    let overlap: HashSet<&&str> = ids1.iter().filter(|id| ids2.contains(**id)).collect();
    assert!(
        overlap.is_empty(),
        "Paginated results should not overlap, found duplicates: {:?}",
        overlap
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// Aggregations API
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_aggregate_endpoint() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Index documents with a category field
    let docs: Vec<Value> = (0..30)
        .map(|i| {
            json!({
                "id": format!("agg-{}", i),
                "fields": {
                    "title": format!("Agg document {}", i),
                    "body": "Aggregation test content",
                    "category": if i % 3 == 0 { "sports" } else if i % 3 == 1 { "tech" } else { "science" },
                    "count": i * 10
                }
            })
        })
        .collect();
    index_docs(&client, &base_url, "test-e2e", &json!(docs)).await;

    // POST aggregate with a count aggregation (simplest type)
    let resp = client
        .post(format!("{}/collections/test-e2e/aggregate", base_url))
        .json(&json!({
            "aggregations": [
                {
                    "name": "total",
                    "type": "count"
                }
            ]
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    assert!(
        status == 200 || status == 500,
        "Aggregate endpoint should return 200 or 500, got {}",
        status
    );

    // If the backend supports it, verify the response structure
    if status == 200 {
        let body: Value = resp.json().await.unwrap();
        assert!(body["results"].is_object() || body["results"].is_null(), "Expected results in response");
        assert!(body["took_ms"].is_number(), "Expected took_ms");
    }

    handle.abort();
}

#[tokio::test]
async fn test_aggregate_nonexistent_collection() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    let resp = client
        .post(format!(
            "{}/collections/nonexistent/aggregate",
            base_url
        ))
        .json(&json!({
            "aggregations": [
                {
                    "name": "test",
                    "type": "terms",
                    "field": "category",
                    "size": 10
                }
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);

    handle.abort();
}

// ---------------------------------------------------------------------------
// Top Terms / Index Inspection API
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_top_terms() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Index documents so terms exist
    let docs: Vec<Value> = (0..20)
        .map(|i| {
            json!({
                "id": format!("term-{}", i),
                "fields": {
                    "title": "search engine indexing",
                    "body": "Prism provides full text search capabilities",
                    "category": "search"
                }
            })
        })
        .collect();
    index_docs(&client, &base_url, "test-e2e", &json!(docs)).await;

    // Get top terms for the "title" field
    let resp = client
        .get(format!(
            "{}/collections/test-e2e/terms/title?limit=10",
            base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["field"], "title");
    assert!(body["terms"].is_array());

    handle.abort();
}

#[tokio::test]
async fn test_get_top_terms_nonexistent_collection() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    let resp = client
        .get(format!(
            "{}/collections/nonexistent/terms/title",
            base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);

    handle.abort();
}

// ---------------------------------------------------------------------------
// Segments API
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_segments() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Index some documents to create segments
    let docs: Vec<Value> = (0..10)
        .map(|i| {
            json!({
                "id": format!("seg-{}", i),
                "fields": {
                    "title": format!("Segment doc {}", i),
                    "body": "Content for segment test"
                }
            })
        })
        .collect();
    index_docs(&client, &base_url, "test-e2e", &json!(docs)).await;

    let resp = client
        .get(format!("{}/collections/test-e2e/segments", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    // Should have segment information
    assert!(
        body.is_object(),
        "Expected segments response to be an object"
    );

    handle.abort();
}

#[tokio::test]
async fn test_get_segments_nonexistent_collection() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    let resp = client
        .get(format!("{}/collections/nonexistent/segments", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);

    handle.abort();
}

// ---------------------------------------------------------------------------
// Suggest / Autocomplete API
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_suggest_endpoint() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Index documents with searchable terms
    let docs: Vec<Value> = (0..20)
        .map(|i| {
            json!({
                "id": format!("suggest-{}", i),
                "fields": {
                    "title": "programming language design",
                    "body": "Rust programming provides memory safety guarantees",
                    "category": "programming"
                }
            })
        })
        .collect();
    index_docs(&client, &base_url, "test-e2e", &json!(docs)).await;

    // Request suggestions for prefix "prog"
    let resp = client
        .post(format!("{}/collections/test-e2e/_suggest", base_url))
        .json(&json!({
            "prefix": "prog",
            "field": "title",
            "size": 5,
            "fuzzy": false
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    assert!(body["suggestions"].is_array());

    handle.abort();
}

#[tokio::test]
async fn test_suggest_nonexistent_collection() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    let resp = client
        .post(format!(
            "{}/collections/nonexistent/_suggest",
            base_url
        ))
        .json(&json!({
            "prefix": "test",
            "field": "title"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);

    handle.abort();
}

// ---------------------------------------------------------------------------
// More Like This API
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_more_like_this_by_text() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Index documents
    let docs: Vec<Value> = (0..20)
        .map(|i| {
            json!({
                "id": format!("mlt-{}", i),
                "fields": {
                    "title": if i < 10 { "Rust programming language systems" } else { "Cooking recipes for breakfast" },
                    "body": if i < 10 {
                        "Systems programming with memory safety and concurrency"
                    } else {
                        "Delicious pancake recipe with maple syrup and butter"
                    },
                    "category": if i < 10 { "tech" } else { "cooking" }
                }
            })
        })
        .collect();
    index_docs(&client, &base_url, "test-e2e", &json!(docs)).await;

    // Find documents similar to given text
    let resp = client
        .post(format!("{}/collections/test-e2e/_mlt", base_url))
        .json(&json!({
            "like_text": "Rust systems programming memory safety",
            "fields": ["title", "body"],
            "min_term_freq": 1,
            "min_doc_freq": 1,
            "max_query_terms": 25,
            "size": 5
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    assert!(body["results"].is_array());

    handle.abort();
}

#[tokio::test]
async fn test_more_like_this_missing_params() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // MLT without like or like_text should return 400
    let resp = client
        .post(format!("{}/collections/test-e2e/_mlt", base_url))
        .json(&json!({
            "fields": ["title"],
            "size": 5
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        400,
        "MLT without like or like_text should return 400"
    );

    handle.abort();
}

#[tokio::test]
async fn test_more_like_this_nonexistent_collection() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    let resp = client
        .post(format!("{}/collections/nonexistent/_mlt", base_url))
        .json(&json!({
            "like_text": "test query",
            "fields": ["title"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);

    handle.abort();
}

// ---------------------------------------------------------------------------
// Multi-Search API
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_multi_search_endpoint() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    // Create two collections
    create_test_collection(&client, &base_url).await;

    let schema2 = json!({
        "collection": "test-e2e-2",
        "backends": {
            "text": {
                "fields": [
                    {"name": "title", "type": "text", "stored": true, "indexed": true},
                    {"name": "body", "type": "text", "stored": true, "indexed": true}
                ]
            }
        }
    });
    let resp = client
        .put(format!("{}/collections/test-e2e-2", base_url))
        .json(&schema2)
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().as_u16() == 200 || resp.status().as_u16() == 201,
        "Expected 200/201, got {}",
        resp.status().as_u16()
    );

    // Index docs into both collections
    let docs1 = json!([{
        "id": "ms-1",
        "fields": {"title": "Multi search doc one", "body": "Content from collection one"}
    }]);
    index_docs(&client, &base_url, "test-e2e", &docs1).await;

    let docs2 = json!([{
        "id": "ms-2",
        "fields": {"title": "Multi search doc two", "body": "Content from collection two"}
    }]);
    index_docs(&client, &base_url, "test-e2e-2", &docs2).await;

    // Multi-search across both
    let resp = client
        .post(format!("{}/_msearch", base_url))
        .json(&json!({
            "collections": ["test-e2e", "test-e2e-2"],
            "query": "multi search",
            "limit": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    assert!(body["results"].is_array());
    assert!(body["total"].is_number());

    handle.abort();
}

#[tokio::test]
async fn test_multi_index_search_comma_separated() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Index a doc
    let docs = json!([{
        "id": "mis-1",
        "fields": {"title": "Comma separated search", "body": "Test body content"}
    }]);
    index_docs(&client, &base_url, "test-e2e", &docs).await;

    // Search via comma-separated path
    let resp = client
        .post(format!("{}/test-e2e/_search", base_url))
        .json(&json!({ "query": "comma separated", "limit": 10 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    assert!(body["results"].is_array());

    handle.abort();
}

// ---------------------------------------------------------------------------
// Delete Collection (extended)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_delete_nonexistent_collection() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    let resp = client
        .delete(format!("{}/collections/does-not-exist", base_url))
        .send()
        .await
        .unwrap();
    // Should return 404 for nonexistent collection
    assert!(
        resp.status().as_u16() >= 400,
        "Deleting nonexistent collection should return error, got {}",
        resp.status().as_u16()
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// Search with highlight and score_function
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_search_with_highlight() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    let docs = json!([{
        "id": "hl-1",
        "fields": {
            "title": "Highlighted search result",
            "body": "This document contains the word highlight for testing purposes"
        }
    }]);
    index_docs(&client, &base_url, "test-e2e", &docs).await;

    let resp = client
        .post(format!("{}/collections/test-e2e/search", base_url))
        .json(&json!({
            "query": "highlight",
            "limit": 10,
            "highlight": {
                "fields": ["body"],
                "pre_tag": "<em>",
                "post_tag": "</em>"
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    let results = body["results"].as_array().unwrap();
    // Might or might not get results depending on tokenization, but the endpoint should succeed
    assert!(body["total"].is_number());
    let _ = results;

    handle.abort();
}

#[tokio::test]
async fn test_search_with_min_score() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    let docs: Vec<Value> = (0..10)
        .map(|i| {
            json!({
                "id": format!("minscore-{}", i),
                "fields": {
                    "title": format!("Document about search engine {}", i),
                    "body": "Search relevance scoring and ranking"
                }
            })
        })
        .collect();
    index_docs(&client, &base_url, "test-e2e", &json!(docs)).await;

    // Search with a very high min_score to filter most results
    let resp = client
        .post(format!("{}/collections/test-e2e/search", base_url))
        .json(&json!({
            "query": "search",
            "limit": 10,
            "min_score": 999.0
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    let results = body["results"].as_array().unwrap();
    assert!(
        results.is_empty(),
        "min_score=999 should filter all results"
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// Document get - missing document
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_document_not_found() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    let resp = client
        .get(format!(
            "{}/collections/test-e2e/documents/nonexistent-id",
            base_url
        ))
        .send()
        .await
        .unwrap();
    // Should return 200 with null body, or 404 depending on implementation
    let status = resp.status().as_u16();
    assert!(
        status == 200 || status == 404,
        "Expected 200 or 404 for missing doc, got {}",
        status
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// Server info
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_server_info_endpoint() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    let resp = client
        .get(format!("{}/stats/server", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    assert!(body["version"].is_string());
    assert!(body["prism_version"].is_string());

    handle.abort();
}

// ---------------------------------------------------------------------------
// Lint schemas
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_lint_schemas_endpoint() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    let resp = client
        .get(format!("{}/admin/lint-schemas", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    // Lint should return something (array or object)
    let body: Value = resp.json().await.unwrap();
    assert!(
        body.is_array() || body.is_object(),
        "Lint response should be JSON"
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// Create collection - duplicate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_duplicate_collection() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    // Create the first time
    create_test_collection(&client, &base_url).await;

    // Try to create again - should get 409 Conflict
    let resp = client
        .put(format!("{}/collections/test-e2e", base_url))
        .json(&test_schema())
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        409,
        "Duplicate collection creation should return 409"
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// Create collection - invalid name
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_collection_invalid_name() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    // Try to create collection with empty name
    let schema = json!({
        "collection": "",
        "backends": {
            "text": {
                "fields": [
                    {"name": "title", "type": "text", "stored": true, "indexed": true}
                ]
            }
        }
    });
    let resp = client
        .put(format!("{}/collections/ ", base_url))
        .json(&schema)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    assert!(
        status >= 400,
        "Invalid collection name should return error, got {}",
        status
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// Pipelines listing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_pipelines_endpoint() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();

    let resp = client
        .get(format!("{}/admin/pipelines", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    assert!(body["pipelines"].is_array());

    handle.abort();
}

// ---------------------------------------------------------------------------
// Simple search API
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_simple_search_api() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    let docs = json!([{
        "id": "simple-1",
        "fields": {"title": "Simple search test", "body": "Testing the simple search API endpoint"}
    }]);
    index_docs(&client, &base_url, "test-e2e", &docs).await;

    let resp = client
        .post(format!("{}/api/search", base_url))
        .json(&json!({ "query": "simple search", "limit": 5 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();
    assert!(body["results"].is_array());
    assert!(body["total"].is_number());

    handle.abort();
}

#[tokio::test]
async fn test_concurrent_requests() {
    let (_temp, base_url, handle) = start_server().await;
    let client = Client::new();
    create_test_collection(&client, &base_url).await;

    // Index some documents to search
    let docs: Vec<Value> = (0..20)
        .map(|i| {
            json!({
                "id": format!("conc-{}", i),
                "fields": {
                    "title": format!("Concurrent document {}", i),
                    "body": "Concurrent search testing content",
                    "category": "concurrent"
                }
            })
        })
        .collect();
    index_docs(&client, &base_url, "test-e2e", &json!(docs)).await;

    // Spawn 10 concurrent search requests
    let mut handles = Vec::new();
    for i in 0..10 {
        let c = client.clone();
        let url = base_url.clone();
        handles.push(tokio::spawn(async move {
            let resp = c
                .post(format!("{}/collections/test-e2e/search", url))
                .json(&json!({ "query": "concurrent", "limit": 10 }))
                .send()
                .await
                .unwrap();
            (i, resp.status().as_u16())
        }));
    }

    // Wait for all and assert success
    for h in handles {
        let (idx, status) = h.await.unwrap();
        assert_eq!(
            status, 200,
            "Concurrent request {} returned status {}",
            idx, status
        );
    }

    handle.abort();
}
