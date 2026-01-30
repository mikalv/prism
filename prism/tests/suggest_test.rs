//! Integration tests for autocomplete/suggestions API (Issue #47)
//!
//! Tests prefix-based term suggestions with frequency ranking.

use prism::backends::{Document, TextBackend, VectorBackend};
use prism::collection::CollectionManager;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

/// Setup test environment with a basic collection for suggestions
async fn setup_suggest_environment() -> (TempDir, Arc<CollectionManager>) {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let schemas_dir = temp.path().join("schemas");
    let data_dir = temp.path().join("data");

    std::fs::create_dir_all(&schemas_dir).expect("Failed to create schemas dir");

    let schema = r#"
collection: articles
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
        stored: true
      - name: tags
        type: text
        indexed: true
        stored: true
"#;

    fs::write(schemas_dir.join("articles.yaml"), schema).expect("Failed to write schema");

    let text_backend = Arc::new(TextBackend::new(&data_dir).expect("Failed to create text backend"));
    let vector_backend = Arc::new(VectorBackend::new(&data_dir).expect("Failed to create vector backend"));
    let manager = Arc::new(
        CollectionManager::new(&schemas_dir, text_backend, vector_backend)
            .expect("Failed to create collection manager"),
    );
    manager.initialize().await.expect("Failed to initialize manager");

    (temp, manager)
}

#[tokio::test]
async fn test_prefix_suggestion() {
    let (_temp, manager) = setup_suggest_environment().await;

    // Index documents with terms that share prefixes
    let docs = vec![
        Document {
            id: "doc1".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Rust programming language")),
                ("tags".to_string(), json!("rust systems programming")),
            ]),
        },
        Document {
            id: "doc2".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Russia travel guide")),
                ("tags".to_string(), json!("travel europe russia")),
            ]),
        },
        Document {
            id: "doc3".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Ruby on Rails tutorial")),
                ("tags".to_string(), json!("ruby web framework")),
            ]),
        },
        // Add more rust documents to increase its doc_freq
        Document {
            id: "doc4".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Advanced Rust patterns")),
                ("tags".to_string(), json!("rust advanced memory")),
            ]),
        },
        Document {
            id: "doc5".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Rust web development")),
                ("tags".to_string(), json!("rust web actix")),
            ]),
        },
    ];

    manager.index("articles", docs).await.expect("Failed to index documents");

    // Query prefix "ru" using suggest (non-fuzzy mode)
    let suggestions = manager
        .suggest("articles", "title", "ru", 5, false, 2)
        .expect("Failed to get suggestions");

    assert!(!suggestions.is_empty(), "Should have suggestions for prefix 'ru'");

    // Verify all returned terms start with "ru"
    for entry in &suggestions {
        assert!(
            entry.term.starts_with("ru"),
            "Suggestion '{}' should start with 'ru'",
            entry.term
        );
    }

    // "rust" should be most frequent (appears in 3 documents)
    let first = &suggestions[0];
    assert_eq!(first.term, "rust", "Most frequent term should be 'rust'");
    assert!(first.doc_freq >= 3, "rust should appear in at least 3 documents");
}

#[tokio::test]
async fn test_empty_prefix_returns_top_terms() {
    let (_temp, manager) = setup_suggest_environment().await;

    // Index documents
    let docs = vec![
        Document {
            id: "doc1".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("programming rust language")),
            ]),
        },
        Document {
            id: "doc2".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("programming python language")),
            ]),
        },
        Document {
            id: "doc3".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("programming java language")),
            ]),
        },
    ];

    manager.index("articles", docs).await.expect("Failed to index documents");

    // Empty prefix should return most frequent terms overall
    let suggestions = manager
        .suggest("articles", "title", "", 10, false, 2)
        .expect("Failed to get suggestions");

    assert!(!suggestions.is_empty(), "Should have suggestions for empty prefix");

    // "programming" and "language" should appear in all 3 docs
    let terms: Vec<&str> = suggestions.iter().map(|s| s.term.as_str()).collect();
    assert!(terms.contains(&"programming"), "Should contain 'programming'");
    assert!(terms.contains(&"language"), "Should contain 'language'");

    // Verify sorting by score (descending, based on doc_freq)
    for i in 0..suggestions.len() - 1 {
        assert!(
            suggestions[i].doc_freq >= suggestions[i + 1].doc_freq,
            "Suggestions should be sorted by doc_freq descending"
        );
    }
}

#[tokio::test]
async fn test_no_matches() {
    let (_temp, manager) = setup_suggest_environment().await;

    // Index some documents
    let docs = vec![
        Document {
            id: "doc1".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Rust programming")),
            ]),
        },
    ];

    manager.index("articles", docs).await.expect("Failed to index documents");

    // Query prefix that doesn't match any terms
    let suggestions = manager
        .suggest("articles", "title", "xyz", 5, false, 2)
        .expect("Failed to get suggestions");

    assert!(suggestions.is_empty(), "Should have no suggestions for prefix 'xyz'");
}

#[tokio::test]
async fn test_limit_parameter() {
    let (_temp, manager) = setup_suggest_environment().await;

    // Index documents with many unique terms
    let docs = vec![
        Document {
            id: "doc1".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("alpha beta gamma delta epsilon")),
            ]),
        },
        Document {
            id: "doc2".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("alpha zeta eta theta iota")),
            ]),
        },
    ];

    manager.index("articles", docs).await.expect("Failed to index documents");

    // Request only 2 suggestions
    let suggestions = manager
        .suggest("articles", "title", "", 2, false, 2)
        .expect("Failed to get suggestions");

    assert!(suggestions.len() <= 2, "Should respect limit parameter");

    // "alpha" should be first (appears in both docs)
    if !suggestions.is_empty() {
        assert_eq!(suggestions[0].term, "alpha", "Most frequent term should be 'alpha'");
    }
}

#[tokio::test]
async fn test_field_specific_suggestions() {
    let (_temp, manager) = setup_suggest_environment().await;

    // Index documents with different terms in different fields
    let docs = vec![
        Document {
            id: "doc1".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Rust programming")),
                ("tags".to_string(), json!("backend systems")),
            ]),
        },
    ];

    manager.index("articles", docs).await.expect("Failed to index documents");

    // Suggestions from title field
    let title_suggestions = manager
        .suggest("articles", "title", "ru", 5, false, 2)
        .expect("Failed to get title suggestions");

    // Suggestions from tags field
    let tags_suggestions = manager
        .suggest("articles", "tags", "sy", 5, false, 2)
        .expect("Failed to get tags suggestions");

    // Title should have "rust"
    let title_terms: Vec<&str> = title_suggestions.iter().map(|s| s.term.as_str()).collect();
    assert!(title_terms.contains(&"rust"), "Title field should contain 'rust'");

    // Tags should have "systems"
    let tag_terms: Vec<&str> = tags_suggestions.iter().map(|s| s.term.as_str()).collect();
    assert!(tag_terms.contains(&"systems"), "Tags field should contain 'systems'");

    // Title shouldn't have "systems" (it's only in tags)
    let title_sys_suggestions = manager
        .suggest("articles", "title", "sy", 5, false, 2)
        .expect("Failed to get title suggestions");
    assert!(
        title_sys_suggestions.is_empty() ||
        !title_sys_suggestions.iter().any(|s| s.term == "systems"),
        "Title field should not contain 'systems'"
    );
}

#[tokio::test]
async fn test_case_sensitivity() {
    let (_temp, manager) = setup_suggest_environment().await;

    // Index documents - Tantivy's default tokenizer lowercases terms
    let docs = vec![
        Document {
            id: "doc1".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("RUST Programming Language")),
            ]),
        },
    ];

    manager.index("articles", docs).await.expect("Failed to index documents");

    // Lowercase prefix should match (since terms are lowercased)
    let suggestions = manager
        .suggest("articles", "title", "rust", 5, false, 2)
        .expect("Failed to get suggestions");

    assert!(!suggestions.is_empty(), "Should find 'rust' with lowercase prefix");

    // Uppercase prefix won't match lowercased terms
    let upper_suggestions = manager
        .suggest("articles", "title", "RUST", 5, false, 2)
        .expect("Failed to get suggestions");

    assert!(upper_suggestions.is_empty(), "Uppercase prefix shouldn't match lowercased terms");
}

#[tokio::test]
async fn test_nonexistent_field() {
    let (_temp, manager) = setup_suggest_environment().await;

    // Index a document
    let docs = vec![
        Document {
            id: "doc1".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Test document")),
            ]),
        },
    ];

    manager.index("articles", docs).await.expect("Failed to index documents");

    // Try to get suggestions from a field that doesn't exist
    let result = manager.suggest("articles", "nonexistent_field", "test", 5, false, 2);

    assert!(result.is_err(), "Should error on nonexistent field");
}

#[tokio::test]
async fn test_nonexistent_collection() {
    let (_temp, manager) = setup_suggest_environment().await;

    // Try to get suggestions from a collection that doesn't exist
    let result = manager.suggest("nonexistent_collection", "title", "test", 5, false, 2);

    assert!(result.is_err(), "Should error on nonexistent collection");
}
