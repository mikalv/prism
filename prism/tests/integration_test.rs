//! Integration tests for engraph-core search pipeline
//!
//! Tests the full-stack: index -> search -> facets -> boosting

use prism::backends::{Document, Query, TextBackend, VectorBackend};
use prism::collection::CollectionManager;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

/// Setup test environment with a collection
async fn setup_test_environment() -> (TempDir, Arc<CollectionManager>) {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let schemas_dir = temp.path().join("schemas");
    let data_dir = temp.path().join("data");

    std::fs::create_dir_all(&schemas_dir).expect("Failed to create schemas dir");

    // Create a test schema with text backend
    // Note: "id" field is automatically added by TextBackend
    fs::write(
        schemas_dir.join("logs.yaml"),
        r#"
collection: logs
backends:
  text:
    fields:
      - name: message
        type: text
        indexed: true
        stored: true
      - name: level
        type: text
        indexed: true
        stored: true
      - name: timestamp
        type: text
        indexed: false
        stored: true
      - name: project_id
        type: text
        indexed: false
        stored: true
"#,
    )
    .expect("Failed to write schema");

    let text_backend =
        Arc::new(TextBackend::new(&data_dir).expect("Failed to create text backend"));
    let vector_backend =
        Arc::new(VectorBackend::new(&data_dir).expect("Failed to create vector backend"));
    let manager = Arc::new(
        CollectionManager::new(&schemas_dir, text_backend, vector_backend)
            .expect("Failed to create collection manager"),
    );
    manager
        .initialize()
        .await
        .expect("Failed to initialize manager");

    (temp, manager)
}

#[tokio::test]
async fn test_search_with_facets() {
    let (_temp, manager) = setup_test_environment().await;

    // Index test documents
    let docs = vec![
        Document {
            id: "log1".to_string(),
            fields: HashMap::from([
                ("message".to_string(), json!("Error in database connection")),
                ("level".to_string(), json!("error")),
                ("timestamp".to_string(), json!("2025-01-15T10:00:00Z")),
                ("project_id".to_string(), json!("proj-123")),
            ]),
        },
        Document {
            id: "log2".to_string(),
            fields: HashMap::from([
                ("message".to_string(), json!("Warning: high memory usage")),
                ("level".to_string(), json!("warning")),
                ("timestamp".to_string(), json!("2025-01-15T11:00:00Z")),
                ("project_id".to_string(), json!("proj-123")),
            ]),
        },
        Document {
            id: "log3".to_string(),
            fields: HashMap::from([
                ("message".to_string(), json!("Error parsing JSON input")),
                ("level".to_string(), json!("error")),
                ("timestamp".to_string(), json!("2025-01-16T09:00:00Z")),
                ("project_id".to_string(), json!("proj-456")),
            ]),
        },
    ];

    manager
        .index("logs", docs)
        .await
        .expect("Failed to index documents");

    // Search for errors
    let query = Query {
        query_string: "error".to_string(),
        fields: vec!["message".to_string()],
        limit: 10,
        offset: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    };

    let results = manager.search("logs", query, None).await.expect("Search failed");

    // Should find 2 error documents
    assert_eq!(results.total, 2, "Expected 2 error documents");
    assert!(
        results
            .results
            .iter()
            .all(|r| r.id == "log1" || r.id == "log3"),
        "Should only find log1 and log3"
    );
}

#[tokio::test]
async fn test_search_pagination() {
    let (_temp, manager) = setup_test_environment().await;

    // Index test documents
    let docs: Vec<Document> = (0..20)
        .map(|i| Document {
            id: format!("log{}", i),
            fields: HashMap::from([
                ("message".to_string(), json!(format!("Test message {}", i))),
                ("level".to_string(), json!("info")),
            ]),
        })
        .collect();

    manager
        .index("logs", docs)
        .await
        .expect("Failed to index documents");

    // First page
    let query1 = Query {
        query_string: "test".to_string(),
        fields: vec!["message".to_string()],
        limit: 5,
        offset: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    };

    let results1 = manager.search("logs", query1, None).await.expect("Search failed");
    assert_eq!(
        results1.results.len(),
        5,
        "Expected 5 results on first page"
    );

    // Second page
    let query2 = Query {
        query_string: "test".to_string(),
        fields: vec!["message".to_string()],
        limit: 5,
        offset: 5,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    };

    let results2 = manager.search("logs", query2, None).await.expect("Search failed");
    assert_eq!(
        results2.results.len(),
        5,
        "Expected 5 results on second page"
    );

    // Results should be different
    let ids1: Vec<_> = results1.results.iter().map(|r| &r.id).collect();
    let ids2: Vec<_> = results2.results.iter().map(|r| &r.id).collect();
    assert!(
        ids1.iter().all(|id| !ids2.contains(id)),
        "Pages should have different results"
    );
}

#[tokio::test]
async fn test_hybrid_search_text_only_fallback() {
    let (_temp, manager) = setup_test_environment().await;

    // Index test documents
    let docs = vec![Document {
        id: "doc1".to_string(),
        fields: HashMap::from([
            ("message".to_string(), json!("Important error message")),
            ("level".to_string(), json!("error")),
        ]),
    }];

    manager
        .index("logs", docs)
        .await
        .expect("Failed to index documents");

    // Hybrid search without vector (should fallback to text-only)
    let results = manager
        .hybrid_search(
            "logs", "error", None, // No vector
            10, None, None, None,
        )
        .await
        .expect("Hybrid search failed");

    assert_eq!(results.total, 1, "Should find 1 result");
    assert_eq!(results.results[0].id, "doc1");
}

#[tokio::test]
async fn test_collection_not_found() {
    let (_temp, manager) = setup_test_environment().await;

    let query = Query {
        query_string: "test".to_string(),
        fields: vec![],
        limit: 10,
        offset: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    };

    let result = manager.search("nonexistent", query, None).await;
    assert!(result.is_err(), "Should error on nonexistent collection");
}
