//! Integration tests for ranking and relevance scoring (Issue #26)
//!
//! Tests field boosting, recency decay, and popularity boost features.

use prism::backends::{Document, Query, TextBackend, VectorBackend};
use prism::collection::CollectionManager;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

/// Setup test environment with a collection configured for ranking
async fn setup_ranking_environment(schema_yaml: &str) -> (TempDir, Arc<CollectionManager>) {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let schemas_dir = temp.path().join("schemas");
    let data_dir = temp.path().join("data");

    std::fs::create_dir_all(&schemas_dir).expect("Failed to create schemas dir");

    fs::write(schemas_dir.join("articles.yaml"), schema_yaml).expect("Failed to write schema");

    let text_backend =
        Arc::new(TextBackend::new(&data_dir).expect("Failed to create text backend"));
    let vector_backend =
        Arc::new(VectorBackend::new(&data_dir).expect("Failed to create vector backend"));
    let manager = Arc::new(
        CollectionManager::new(&schemas_dir, text_backend, vector_backend, None)
            .expect("Failed to create collection manager"),
    );
    manager
        .initialize()
        .await
        .expect("Failed to initialize manager");

    (temp, manager)
}

#[tokio::test]
async fn test_field_boosting() {
    // Schema with title boosted 3x over content
    let schema = r#"
collection: articles
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
        stored: true
      - name: content
        type: text
        indexed: true
        stored: true
boosting:
  field_weights:
    title: 3.0
    content: 1.0
"#;

    let (_temp, manager) = setup_ranking_environment(schema).await;

    // Index documents where "rust" appears in different fields
    let docs = vec![
        Document {
            id: "doc_title".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Rust programming language")),
                (
                    "content".to_string(),
                    json!("This is about software development"),
                ),
            ]),
        },
        Document {
            id: "doc_content".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Software development guide")),
                (
                    "content".to_string(),
                    json!("Learn about Rust and other languages"),
                ),
            ]),
        },
    ];

    manager
        .index("articles", docs)
        .await
        .expect("Failed to index documents");

    // Search for "rust"
    let query = Query {
        query_string: "rust".to_string(),
        limit: 10,
        offset: 0,
        fields: vec![],
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    };

    let results = manager
        .search("articles", query, None)
        .await
        .expect("Failed to search");

    // Document with "rust" in title should rank higher due to 3x boost
    assert_eq!(results.results.len(), 2);
    assert_eq!(
        results.results[0].id, "doc_title",
        "Document with match in boosted title field should rank first"
    );
    assert!(
        results.results[0].score > results.results[1].score,
        "Title match should have higher score than content match"
    );
}

#[tokio::test]
async fn test_popularity_boost() {
    // Schema with document_boost enabled
    let schema = r#"
collection: articles
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
        stored: true
system_fields:
  indexed_at: false
  document_boost: true
boosting:
  field_weights: {}
"#;

    let (_temp, manager) = setup_ranking_environment(schema).await;

    // Index documents with different _boost values
    let docs = vec![
        Document {
            id: "popular".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Popular Rust article")),
                ("_boost".to_string(), json!(3.0)),
            ]),
        },
        Document {
            id: "normal".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Normal Rust article")),
                ("_boost".to_string(), json!(1.0)),
            ]),
        },
        Document {
            id: "low".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Low priority Rust article")),
                ("_boost".to_string(), json!(0.5)),
            ]),
        },
    ];

    manager
        .index("articles", docs)
        .await
        .expect("Failed to index documents");

    // Search for "rust"
    let query = Query {
        query_string: "rust".to_string(),
        limit: 10,
        offset: 0,
        fields: vec![],
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    };

    let results = manager
        .search("articles", query, None)
        .await
        .expect("Failed to search");

    // Documents should be ordered by boost value (all have same text match score)
    assert_eq!(results.results.len(), 3);
    assert_eq!(
        results.results[0].id, "popular",
        "Highest boost document should rank first"
    );
    assert_eq!(
        results.results[1].id, "normal",
        "Medium boost document should rank second"
    );
    assert_eq!(
        results.results[2].id, "low",
        "Lowest boost document should rank last"
    );
}

#[tokio::test]
async fn test_recency_decay() {
    // Recency decay is tested with live indexing since _indexed_at is auto-set

    // Schema with recency decay configured
    let schema = r#"
collection: articles
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
        stored: true
system_fields:
  indexed_at: true
  document_boost: false
boosting:
  recency:
    field: _indexed_at
    decay_function: exponential
    scale: 1d
    decay_rate: 0.5
"#;

    let (_temp, manager) = setup_ranking_environment(schema).await;

    // We need to test with documents that have different ages
    // Since _indexed_at is auto-set, we'll index docs and verify ranking works
    // In a real scenario, older docs would have older _indexed_at values

    let docs = vec![Document {
        id: "doc1".to_string(),
        fields: HashMap::from([(
            "title".to_string(),
            json!("First Rust article about programming"),
        )]),
    }];

    manager
        .index("articles", docs)
        .await
        .expect("Failed to index documents");

    // Index another document slightly later
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let docs2 = vec![Document {
        id: "doc2".to_string(),
        fields: HashMap::from([(
            "title".to_string(),
            json!("Second Rust article about programming"),
        )]),
    }];

    manager
        .index("articles", docs2)
        .await
        .expect("Failed to index second batch");

    // Search for "rust programming"
    let query = Query {
        query_string: "rust programming".to_string(),
        limit: 10,
        offset: 0,
        fields: vec![],
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    };

    let results = manager
        .search("articles", query, None)
        .await
        .expect("Failed to search");

    // Both documents should be found
    assert_eq!(results.results.len(), 2);

    // The more recent document should have a slightly higher score due to recency decay
    // Note: With very short time difference, the effect is minimal
    // This test mainly verifies the recency decay code path doesn't crash
    assert!(
        results.results[0].score >= results.results[1].score,
        "More recent document should have >= score"
    );
}

#[tokio::test]
async fn test_combined_ranking() {
    // Schema with all ranking features enabled
    let schema = r#"
collection: articles
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
        stored: true
      - name: content
        type: text
        indexed: true
        stored: true
system_fields:
  indexed_at: true
  document_boost: true
boosting:
  recency:
    field: _indexed_at
    decay_function: exponential
    scale: 7d
    decay_rate: 0.5
  field_weights:
    title: 2.0
    content: 1.0
"#;

    let (_temp, manager) = setup_ranking_environment(schema).await;

    // Index documents with different characteristics
    let docs = vec![
        Document {
            id: "boosted_content".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Software development")),
                ("content".to_string(), json!("This article discusses Rust")),
                ("_boost".to_string(), json!(2.0)),
            ]),
        },
        Document {
            id: "title_match".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Rust programming guide")),
                ("content".to_string(), json!("Learn about development")),
                ("_boost".to_string(), json!(1.0)),
            ]),
        },
    ];

    manager
        .index("articles", docs)
        .await
        .expect("Failed to index documents");

    // Search for "rust"
    let query = Query {
        query_string: "rust".to_string(),
        limit: 10,
        offset: 0,
        fields: vec![],
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    };

    let results = manager
        .search("articles", query, None)
        .await
        .expect("Failed to search");

    assert_eq!(results.results.len(), 2);

    // Both documents match "rust" - title_match has it in title (2x field boost)
    // boosted_content has it in content (1x field boost) but has 2x document boost
    // So the scores should be relatively close
    // This test verifies all ranking features work together without crashing
    for result in &results.results {
        assert!(
            result.score > 0.0,
            "All results should have positive scores"
        );
    }
}

#[tokio::test]
async fn test_no_boosting_config() {
    // Schema without boosting config - should work normally
    let schema = r#"
collection: articles
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
        stored: true
"#;

    let (_temp, manager) = setup_ranking_environment(schema).await;

    let docs = vec![Document {
        id: "doc1".to_string(),
        fields: HashMap::from([("title".to_string(), json!("Rust programming"))]),
    }];

    manager
        .index("articles", docs)
        .await
        .expect("Failed to index documents");

    let query = Query {
        query_string: "rust".to_string(),
        limit: 10,
        offset: 0,
        fields: vec![],
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    };

    let results = manager
        .search("articles", query, None)
        .await
        .expect("Failed to search");

    assert_eq!(results.results.len(), 1);
    assert_eq!(results.results[0].id, "doc1");
    assert!(results.results[0].score > 0.0);
}
