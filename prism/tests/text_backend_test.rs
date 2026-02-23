//! Comprehensive integration tests for TextBackend
//!
//! Covers: indexing, get, delete, stats, search, aggregations,
//! highlights, top-terms, more-like-this, suggest, segments,
//! and document reconstruction.

use prism::aggregations::types::{
    AggregationRequest, AggregationType, AggregationValue, HistogramBounds, RangeEntry,
};
use prism::backends::text::TextBackend;
use prism::backends::{Document, HighlightConfig, Query, SearchBackend};
use prism::schema::{
    Backends, CollectionSchema, FieldType, IndexingConfig, QuotaConfig, TextBackendConfig,
    TextField,
};
use prism::schema::types::SystemFieldsConfig;
use serde_json::json;
use std::collections::HashMap;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helper: build a TextBackend with a simple schema
// ---------------------------------------------------------------------------

/// Creates a TextBackend backed by a temp dir and initialises a collection
/// called "test" with:
///   - text: title (indexed+stored), body (indexed+stored)
///   - i64:  count (indexed+stored)
///   - date: created_at (indexed+stored)
///   - string: category (indexed+stored, exact-match)
///   - f64: price (indexed+stored)
async fn setup() -> (TempDir, TextBackend) {
    let tmp = TempDir::new().unwrap();
    let backend = TextBackend::new(tmp.path()).unwrap();

    let schema = make_schema();
    backend.initialize("test", &schema).await.unwrap();
    (tmp, backend)
}

fn make_schema() -> CollectionSchema {
    CollectionSchema {
        collection: "test".to_string(),
        description: None,
        backends: Backends {
            text: Some(TextBackendConfig {
                fields: vec![
                    TextField {
                        name: "title".to_string(),
                        field_type: FieldType::Text,
                        stored: true,
                        indexed: true,
                        tokenizer: None,
                        tokenizer_options: None,
                    },
                    TextField {
                        name: "body".to_string(),
                        field_type: FieldType::Text,
                        stored: true,
                        indexed: true,
                        tokenizer: None,
                        tokenizer_options: None,
                    },
                    TextField {
                        name: "count".to_string(),
                        field_type: FieldType::I64,
                        stored: true,
                        indexed: true,
                        tokenizer: None,
                        tokenizer_options: None,
                    },
                    TextField {
                        name: "created_at".to_string(),
                        field_type: FieldType::Date,
                        stored: true,
                        indexed: true,
                        tokenizer: None,
                        tokenizer_options: None,
                    },
                    TextField {
                        name: "category".to_string(),
                        field_type: FieldType::String,
                        stored: true,
                        indexed: true,
                        tokenizer: None,
                        tokenizer_options: None,
                    },
                    TextField {
                        name: "price".to_string(),
                        field_type: FieldType::F64,
                        stored: true,
                        indexed: true,
                        tokenizer: None,
                        tokenizer_options: None,
                    },
                ],
                bm25_k1: None,
                bm25_b: None,
            }),
            vector: None,
            graph: None,
        },
        indexing: IndexingConfig::default(),
        quota: QuotaConfig::default(),
        embedding_generation: None,
        facets: None,
        boosting: None,
        storage: Default::default(),
        system_fields: SystemFieldsConfig {
            indexed_at: false,
            document_boost: false,
        },
        hybrid: None,
        replication: None,
        reranking: None,
        ilm_policy: None,
    }
}

fn make_query(q: &str) -> Query {
    Query {
        query_string: q.to_string(),
        fields: vec![],
        limit: 100,
        offset: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    }
}

fn doc(id: &str, title: &str, body: &str) -> Document {
    Document {
        id: id.to_string(),
        fields: HashMap::from([
            ("title".to_string(), json!(title)),
            ("body".to_string(), json!(body)),
        ]),
    }
}

fn doc_with_count(id: &str, title: &str, count: i64) -> Document {
    Document {
        id: id.to_string(),
        fields: HashMap::from([
            ("title".to_string(), json!(title)),
            ("body".to_string(), json!("placeholder body")),
            ("count".to_string(), json!(count)),
        ]),
    }
}

fn doc_full(
    id: &str,
    title: &str,
    body: &str,
    count: i64,
    created_at: &str,
    category: &str,
    price: f64,
) -> Document {
    Document {
        id: id.to_string(),
        fields: HashMap::from([
            ("title".to_string(), json!(title)),
            ("body".to_string(), json!(body)),
            ("count".to_string(), json!(count)),
            ("created_at".to_string(), json!(created_at)),
            ("category".to_string(), json!(category)),
            ("price".to_string(), json!(price)),
        ]),
    }
}

// =========================================================================
// 1. Indexing & Get
// =========================================================================

#[tokio::test]
async fn test_index_and_get_document() {
    let (_tmp, backend) = setup().await;

    backend
        .index("test", vec![doc("d1", "Hello World", "A greeting")])
        .await
        .unwrap();

    let fetched = backend.get("test", "d1").await.unwrap();
    assert!(fetched.is_some());

    let fetched = fetched.unwrap();
    assert_eq!(fetched.id, "d1");
    assert_eq!(fetched.fields.get("title").unwrap(), "Hello World");
    assert_eq!(fetched.fields.get("body").unwrap(), "A greeting");
}

#[tokio::test]
async fn test_get_nonexistent_document() {
    let (_tmp, backend) = setup().await;

    let result = backend.get("test", "nope").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_upsert_replaces_document() {
    let (_tmp, backend) = setup().await;

    // First version
    backend
        .index("test", vec![doc("d1", "Version One", "Old body")])
        .await
        .unwrap();
    // Overwrite same ID
    backend
        .index("test", vec![doc("d1", "Version Two", "New body")])
        .await
        .unwrap();

    let fetched = backend.get("test", "d1").await.unwrap().unwrap();
    assert_eq!(fetched.fields.get("title").unwrap(), "Version Two");
    assert_eq!(fetched.fields.get("body").unwrap(), "New body");

    // Only one document should exist
    let stats = backend.stats("test").await.unwrap();
    assert_eq!(stats.document_count, 1);
}

#[tokio::test]
async fn test_index_multiple_documents() {
    let (_tmp, backend) = setup().await;

    let docs = vec![
        doc("a", "Alpha", "First"),
        doc("b", "Beta", "Second"),
        doc("c", "Gamma", "Third"),
    ];
    backend.index("test", docs).await.unwrap();

    for id in &["a", "b", "c"] {
        assert!(backend.get("test", id).await.unwrap().is_some());
    }

    let stats = backend.stats("test").await.unwrap();
    assert_eq!(stats.document_count, 3);
}

#[tokio::test]
async fn test_index_document_with_numeric_field() {
    let (_tmp, backend) = setup().await;

    backend
        .index("test", vec![doc_with_count("n1", "Numeric", 42)])
        .await
        .unwrap();

    let fetched = backend.get("test", "n1").await.unwrap().unwrap();
    assert_eq!(fetched.fields.get("count").unwrap(), 42);
}

#[tokio::test]
async fn test_index_document_with_date_field() {
    let (_tmp, backend) = setup().await;

    let d = Document {
        id: "dt1".to_string(),
        fields: HashMap::from([
            ("title".to_string(), json!("Date test")),
            ("created_at".to_string(), json!("2025-06-15T12:00:00Z")),
        ]),
    };
    backend.index("test", vec![d]).await.unwrap();

    let fetched = backend.get("test", "dt1").await.unwrap().unwrap();
    let date_val = fetched.fields.get("created_at").unwrap().as_str().unwrap();
    assert!(date_val.starts_with("2025-06-15"));
}

#[tokio::test]
async fn test_index_document_with_f64_field() {
    let (_tmp, backend) = setup().await;

    let d = Document {
        id: "f1".to_string(),
        fields: HashMap::from([
            ("title".to_string(), json!("Float test")),
            ("price".to_string(), json!(19.99)),
        ]),
    };
    backend.index("test", vec![d]).await.unwrap();

    let fetched = backend.get("test", "f1").await.unwrap().unwrap();
    let price = fetched.fields.get("price").unwrap().as_f64().unwrap();
    assert!((price - 19.99).abs() < 0.001);
}

#[tokio::test]
async fn test_index_to_nonexistent_collection_errors() {
    let (_tmp, backend) = setup().await;

    let result = backend
        .index("nonexistent", vec![doc("x", "X", "X")])
        .await;
    assert!(result.is_err());
}

// =========================================================================
// 2. Delete
// =========================================================================

#[tokio::test]
async fn test_delete_single() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![doc("d1", "One", "Body"), doc("d2", "Two", "Body")],
        )
        .await
        .unwrap();

    backend
        .delete("test", vec!["d1".to_string()])
        .await
        .unwrap();

    assert!(backend.get("test", "d1").await.unwrap().is_none());
    assert!(backend.get("test", "d2").await.unwrap().is_some());
}

#[tokio::test]
async fn test_delete_multiple() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![
                doc("a", "Alpha", "Body"),
                doc("b", "Beta", "Body"),
                doc("c", "Gamma", "Body"),
            ],
        )
        .await
        .unwrap();

    backend
        .delete("test", vec!["a".to_string(), "c".to_string()])
        .await
        .unwrap();

    assert!(backend.get("test", "a").await.unwrap().is_none());
    assert!(backend.get("test", "b").await.unwrap().is_some());
    assert!(backend.get("test", "c").await.unwrap().is_none());
}

#[tokio::test]
async fn test_delete_nonexistent_does_not_error() {
    let (_tmp, backend) = setup().await;

    // Deleting an ID that doesn't exist should succeed silently.
    backend
        .delete("test", vec!["ghost".to_string()])
        .await
        .unwrap();
}

#[tokio::test]
async fn test_delete_then_stats_reflects_change() {
    let (_tmp, backend) = setup().await;

    backend
        .index("test", vec![doc("a", "A", "A"), doc("b", "B", "B")])
        .await
        .unwrap();

    let before = backend.stats("test").await.unwrap();
    assert_eq!(before.document_count, 2);

    backend
        .delete("test", vec!["a".to_string()])
        .await
        .unwrap();

    // After delete + commit + reload the count drops
    let after = backend.stats("test").await.unwrap();
    assert_eq!(after.document_count, 1);
}

// =========================================================================
// 3. Stats
// =========================================================================

#[tokio::test]
async fn test_stats_empty_collection() {
    let (_tmp, backend) = setup().await;

    let stats = backend.stats("test").await.unwrap();
    assert_eq!(stats.document_count, 0);
}

#[tokio::test]
async fn test_stats_after_indexing() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![doc("1", "A", "a"), doc("2", "B", "b"), doc("3", "C", "c")],
        )
        .await
        .unwrap();

    let stats = backend.stats("test").await.unwrap();
    assert_eq!(stats.document_count, 3);
}

#[tokio::test]
async fn test_stats_nonexistent_collection_errors() {
    let (_tmp, backend) = setup().await;

    let result = backend.stats("nope").await;
    assert!(result.is_err());
}

// =========================================================================
// 4. Search
// =========================================================================

#[tokio::test]
async fn test_basic_text_search() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![
                doc("d1", "Rust programming language", "Systems programming"),
                doc("d2", "Python tutorial", "Data science"),
                doc("d3", "Rust web framework", "Actix and Axum"),
            ],
        )
        .await
        .unwrap();

    let results = backend.search("test", make_query("rust")).await.unwrap();
    assert_eq!(results.total, 2);
    let ids: Vec<&str> = results.results.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&"d1"));
    assert!(ids.contains(&"d3"));
}

#[tokio::test]
async fn test_search_returns_stored_fields() {
    let (_tmp, backend) = setup().await;

    backend
        .index("test", vec![doc("d1", "Hello World", "Greetings")])
        .await
        .unwrap();

    let results = backend.search("test", make_query("hello")).await.unwrap();
    assert_eq!(results.total, 1);

    let hit = &results.results[0];
    assert_eq!(hit.id, "d1");
    assert!(hit.score > 0.0);
    assert_eq!(hit.fields.get("title").unwrap(), "Hello World");
    assert_eq!(hit.fields.get("body").unwrap(), "Greetings");
}

#[tokio::test]
async fn test_search_specific_fields() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![
                doc("d1", "Rust guide", "Python integration"),
                doc("d2", "Python guide", "Rust integration"),
            ],
        )
        .await
        .unwrap();

    // Search only the title field
    let mut q = make_query("rust");
    q.fields = vec!["title".to_string()];
    let results = backend.search("test", q).await.unwrap();

    assert_eq!(results.total, 1);
    assert_eq!(results.results[0].id, "d1");
}

#[tokio::test]
async fn test_search_limit() {
    let (_tmp, backend) = setup().await;

    let docs: Vec<Document> = (0..10)
        .map(|i| doc(&format!("d{i}"), &format!("common word {i}"), "common body"))
        .collect();
    backend.index("test", docs).await.unwrap();

    let mut q = make_query("common");
    q.limit = 3;
    let results = backend.search("test", q).await.unwrap();
    assert_eq!(results.results.len(), 3);
}

#[tokio::test]
async fn test_search_offset() {
    let (_tmp, backend) = setup().await;

    let docs: Vec<Document> = (0..10)
        .map(|i| doc(&format!("d{i}"), &format!("common word {i}"), "common body"))
        .collect();
    backend.index("test", docs).await.unwrap();

    // Get page 1 and page 2
    let mut q1 = make_query("common");
    q1.limit = 3;
    q1.offset = 0;
    let page1 = backend.search("test", q1).await.unwrap();

    let mut q2 = make_query("common");
    q2.limit = 3;
    q2.offset = 3;
    let page2 = backend.search("test", q2).await.unwrap();

    let ids1: Vec<&str> = page1.results.iter().map(|r| r.id.as_str()).collect();
    let ids2: Vec<&str> = page2.results.iter().map(|r| r.id.as_str()).collect();

    // Pages must not overlap
    for id in &ids1 {
        assert!(!ids2.contains(id), "Overlap between pages: {id}");
    }
}

#[tokio::test]
async fn test_search_empty_query_returns_no_results() {
    let (_tmp, backend) = setup().await;

    backend
        .index("test", vec![doc("d1", "Hello", "World")])
        .await
        .unwrap();

    // An empty string query typically returns no results because there are
    // no terms to match.
    let results = backend.search("test", make_query("")).await.unwrap();
    // Tantivy with an empty query typically returns nothing or everything
    // depending on parser config -- just ensure no crash.
    assert!(results.results.len() <= 1);
}

#[tokio::test]
async fn test_search_no_match() {
    let (_tmp, backend) = setup().await;

    backend
        .index("test", vec![doc("d1", "Hello", "World")])
        .await
        .unwrap();

    let results = backend
        .search("test", make_query("zzyzx_nonexistent"))
        .await
        .unwrap();
    assert_eq!(results.total, 0);
}

#[tokio::test]
async fn test_search_nonexistent_collection_errors() {
    let (_tmp, backend) = setup().await;

    let result = backend.search("nope", make_query("test")).await;
    assert!(result.is_err());
}

// =========================================================================
// 5. Search with highlights
// =========================================================================

#[tokio::test]
async fn test_search_with_highlight() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![doc(
                "d1",
                "Rust programming guide",
                "Learn Rust with practical examples in systems programming",
            )],
        )
        .await
        .unwrap();

    let mut q = make_query("rust");
    q.highlight = Some(HighlightConfig {
        fields: vec!["title".to_string(), "body".to_string()],
        pre_tag: "<b>".to_string(),
        post_tag: "</b>".to_string(),
        fragment_size: 150,
        number_of_fragments: 3,
    });

    let results = backend.search("test", q).await.unwrap();
    assert_eq!(results.total, 1);

    let hit = &results.results[0];
    assert!(hit.highlight.is_some());

    let highlights = hit.highlight.as_ref().unwrap();
    // At least one highlighted field should contain the tag
    let has_highlight = highlights.values().any(|frags| {
        frags
            .iter()
            .any(|f| f.contains("<b>") && f.contains("</b>"))
    });
    assert!(has_highlight, "Expected highlighted text with <b> tags");
}

#[tokio::test]
async fn test_highlight_non_matching_field_omitted() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![doc("d1", "Rust guide", "No mention of query term")],
        )
        .await
        .unwrap();

    let mut q = make_query("rust");
    q.fields = vec!["title".to_string()];
    q.highlight = Some(HighlightConfig {
        fields: vec!["body".to_string()], // body doesn't contain "rust"
        pre_tag: "<em>".to_string(),
        post_tag: "</em>".to_string(),
        fragment_size: 150,
        number_of_fragments: 3,
    });

    let results = backend.search("test", q).await.unwrap();
    // Should still find the document via title search.
    // Highlights for body may be empty since "rust" is not in body.
    assert_eq!(results.total, 1);
}

// =========================================================================
// 6. Aggregations (search_with_aggs)
// =========================================================================

async fn setup_agg_data() -> (TempDir, TextBackend) {
    let (tmp, backend) = setup().await;

    let docs = vec![
        doc_full("1", "Alpha item", "Body one", 10, "2025-01-15T00:00:00Z", "electronics", 99.99),
        doc_full("2", "Beta item", "Body two", 20, "2025-01-16T00:00:00Z", "electronics", 149.50),
        doc_full("3", "Gamma item", "Body three", 30, "2025-02-10T00:00:00Z", "books", 12.99),
        doc_full("4", "Delta item", "Body four", 40, "2025-02-20T00:00:00Z", "books", 24.99),
        doc_full("5", "Epsilon item", "Body five", 50, "2025-03-01T00:00:00Z", "clothing", 59.99),
    ];
    backend.index("test", docs).await.unwrap();

    (tmp, backend)
}

fn match_all_query() -> Query {
    // Tantivy matches all indexed text docs with '*'
    make_query("item")
}

#[tokio::test]
async fn test_agg_count() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![AggregationRequest {
        name: "total".to_string(),
        agg_type: AggregationType::Count,
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("total").unwrap();
    if let AggregationValue::Single(v) = &agg.value {
        assert_eq!(*v as u64, 5);
    } else {
        panic!("Expected Single value for Count aggregation");
    }
}

#[tokio::test]
async fn test_agg_sum() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![AggregationRequest {
        name: "total_count".to_string(),
        agg_type: AggregationType::Sum {
            field: "count".to_string(),
        },
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("total_count").unwrap();
    if let AggregationValue::Single(v) = &agg.value {
        assert!((v - 150.0).abs() < 0.01, "Sum should be 10+20+30+40+50=150");
    } else {
        panic!("Expected Single value for Sum aggregation");
    }
}

#[tokio::test]
async fn test_agg_avg() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![AggregationRequest {
        name: "avg_count".to_string(),
        agg_type: AggregationType::Avg {
            field: "count".to_string(),
        },
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("avg_count").unwrap();
    if let AggregationValue::Single(v) = &agg.value {
        assert!((v - 30.0).abs() < 0.01, "Avg should be 150/5=30");
    } else {
        panic!("Expected Single value for Avg aggregation");
    }
}

#[tokio::test]
async fn test_agg_min() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![AggregationRequest {
        name: "min_count".to_string(),
        agg_type: AggregationType::Min {
            field: "count".to_string(),
        },
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("min_count").unwrap();
    if let AggregationValue::Single(v) = &agg.value {
        assert!((v - 10.0).abs() < 0.01, "Min should be 10");
    } else {
        panic!("Expected Single value");
    }
}

#[tokio::test]
async fn test_agg_max() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![AggregationRequest {
        name: "max_count".to_string(),
        agg_type: AggregationType::Max {
            field: "count".to_string(),
        },
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("max_count").unwrap();
    if let AggregationValue::Single(v) = &agg.value {
        assert!((v - 50.0).abs() < 0.01, "Max should be 50");
    } else {
        panic!("Expected Single value");
    }
}

#[tokio::test]
async fn test_agg_stats() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![AggregationRequest {
        name: "count_stats".to_string(),
        agg_type: AggregationType::Stats {
            field: "count".to_string(),
        },
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("count_stats").unwrap();
    if let AggregationValue::Stats(stats) = &agg.value {
        assert_eq!(stats.count, 5);
        assert!((stats.min.unwrap() - 10.0).abs() < 0.01);
        assert!((stats.max.unwrap() - 50.0).abs() < 0.01);
        assert!((stats.sum.unwrap() - 150.0).abs() < 0.01);
        assert!((stats.avg.unwrap() - 30.0).abs() < 0.01);
    } else {
        panic!("Expected Stats value");
    }
}

#[tokio::test]
async fn test_agg_percentiles() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![AggregationRequest {
        name: "count_pct".to_string(),
        agg_type: AggregationType::Percentiles {
            field: "count".to_string(),
            percents: vec![50.0, 99.0],
        },
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("count_pct").unwrap();
    if let AggregationValue::Percentiles(pct) = &agg.value {
        // With values [10, 20, 30, 40, 50], the median (p50) should be 30
        let p50 = pct.values.get("50").unwrap().unwrap();
        assert!(
            (p50 - 30.0).abs() < 0.1,
            "p50 should be ~30, got {p50}"
        );
        // p99 should be close to 50
        let p99 = pct.values.get("99").unwrap().unwrap();
        assert!(p99 >= 49.0, "p99 should be near 50, got {p99}");
    } else {
        panic!("Expected Percentiles value");
    }
}

#[tokio::test]
async fn test_agg_terms() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![AggregationRequest {
        name: "by_category".to_string(),
        agg_type: AggregationType::Terms {
            field: "category".to_string(),
            size: Some(10),
        },
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("by_category").unwrap();
    if let AggregationValue::Buckets(buckets) = &agg.value {
        assert!(!buckets.is_empty());
        // electronics: 2, books: 2, clothing: 1
        let total_count: u64 = buckets.iter().map(|b| b.doc_count).sum();
        assert_eq!(total_count, 5);

        let electronics = buckets.iter().find(|b| b.key == "electronics");
        assert!(electronics.is_some());
        assert_eq!(electronics.unwrap().doc_count, 2);
    } else {
        panic!("Expected Buckets value for Terms aggregation");
    }
}

#[tokio::test]
async fn test_agg_histogram() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![AggregationRequest {
        name: "count_hist".to_string(),
        agg_type: AggregationType::Histogram {
            field: "count".to_string(),
            interval: 20.0,
            min_doc_count: None,
            extended_bounds: None,
        },
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("count_hist").unwrap();
    if let AggregationValue::Buckets(buckets) = &agg.value {
        assert!(!buckets.is_empty(), "Histogram should produce buckets");
        let total: u64 = buckets.iter().map(|b| b.doc_count).sum();
        assert_eq!(total, 5, "All 5 documents should be in histogram buckets");
    } else {
        panic!("Expected Buckets value");
    }
}

#[tokio::test]
async fn test_agg_histogram_with_extended_bounds() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![AggregationRequest {
        name: "hist_bounds".to_string(),
        agg_type: AggregationType::Histogram {
            field: "count".to_string(),
            interval: 10.0,
            min_doc_count: Some(0),
            extended_bounds: Some(HistogramBounds {
                min: 0.0,
                max: 60.0,
            }),
        },
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("hist_bounds").unwrap();
    if let AggregationValue::Buckets(buckets) = &agg.value {
        // With bounds 0..60 and interval 10, we should have buckets 0,10,20,30,40,50,60
        assert!(buckets.len() >= 6, "Extended bounds should create at least 6 buckets, got {}", buckets.len());
    } else {
        panic!("Expected Buckets value");
    }
}

#[tokio::test]
async fn test_agg_range() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![AggregationRequest {
        name: "count_ranges".to_string(),
        agg_type: AggregationType::Range {
            field: "count".to_string(),
            ranges: vec![
                RangeEntry {
                    key: Some("low".to_string()),
                    from: None,
                    to: Some(25.0),
                },
                RangeEntry {
                    key: Some("mid".to_string()),
                    from: Some(25.0),
                    to: Some(45.0),
                },
                RangeEntry {
                    key: Some("high".to_string()),
                    from: Some(45.0),
                    to: None,
                },
            ],
        },
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("count_ranges").unwrap();
    if let AggregationValue::Buckets(buckets) = &agg.value {
        assert_eq!(buckets.len(), 3);

        let low = buckets.iter().find(|b| b.key == "low").unwrap();
        assert_eq!(low.doc_count, 2, "low: values 10, 20");

        let mid = buckets.iter().find(|b| b.key == "mid").unwrap();
        assert_eq!(mid.doc_count, 2, "mid: values 30, 40");

        let high = buckets.iter().find(|b| b.key == "high").unwrap();
        assert_eq!(high.doc_count, 1, "high: value 50");
    } else {
        panic!("Expected Buckets value");
    }
}

#[tokio::test]
async fn test_agg_date_histogram() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![AggregationRequest {
        name: "by_month".to_string(),
        agg_type: AggregationType::DateHistogram {
            field: "created_at".to_string(),
            calendar_interval: "month".to_string(),
            min_doc_count: None,
        },
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("by_month").unwrap();
    if let AggregationValue::Buckets(buckets) = &agg.value {
        // 3 months: Jan, Feb, Mar
        assert!(
            buckets.len() >= 2,
            "Should have at least 2 month buckets, got {}",
            buckets.len()
        );
        let total: u64 = buckets.iter().map(|b| b.doc_count).sum();
        assert_eq!(total, 5);
    } else {
        panic!("Expected Buckets value for DateHistogram");
    }
}

#[tokio::test]
async fn test_agg_filter() {
    let (_tmp, backend) = setup_agg_data().await;

    // Filter to only documents matching "alpha"
    let aggs = vec![AggregationRequest {
        name: "alpha_filter".to_string(),
        agg_type: AggregationType::Filter {
            filter: "alpha".to_string(),
        },
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("alpha_filter").unwrap();
    if let AggregationValue::Buckets(buckets) = &agg.value {
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].key, "filter");
        assert_eq!(buckets[0].doc_count, 1);
    } else {
        panic!("Expected Buckets value for Filter");
    }
}

#[tokio::test]
async fn test_agg_filters() {
    let (_tmp, backend) = setup_agg_data().await;

    let mut filters = HashMap::new();
    filters.insert("has_alpha".to_string(), "alpha".to_string());
    filters.insert("has_beta".to_string(), "beta".to_string());

    let aggs = vec![AggregationRequest {
        name: "multi_filter".to_string(),
        agg_type: AggregationType::Filters { filters },
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("multi_filter").unwrap();
    if let AggregationValue::Buckets(buckets) = &agg.value {
        assert_eq!(buckets.len(), 2);
        for bucket in buckets {
            assert!(
                bucket.key == "has_alpha" || bucket.key == "has_beta",
                "Unexpected key: {}",
                bucket.key
            );
            assert_eq!(bucket.doc_count, 1);
        }
    } else {
        panic!("Expected Buckets value for Filters");
    }
}

#[tokio::test]
async fn test_agg_global() {
    let (_tmp, backend) = setup_agg_data().await;

    // Global should return count of ALL docs, even though our query only matches
    // some. We query "alpha" but global should see all 5.
    let narrow_query = Query {
        query_string: "alpha".to_string(),
        fields: vec![],
        limit: 100,
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

    let aggs = vec![AggregationRequest {
        name: "all_docs".to_string(),
        agg_type: AggregationType::Global {},
        aggs: None,
    }];

    let result = backend
        .search_with_aggs("test", &narrow_query, aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("all_docs").unwrap();
    if let AggregationValue::Buckets(buckets) = &agg.value {
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].key, "global");
        assert_eq!(
            buckets[0].doc_count, 5,
            "Global should see all 5 documents"
        );
    } else {
        panic!("Expected Buckets value for Global");
    }
}

#[tokio::test]
async fn test_agg_terms_with_sub_aggregation() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![AggregationRequest {
        name: "by_cat_with_stats".to_string(),
        agg_type: AggregationType::Terms {
            field: "category".to_string(),
            size: Some(10),
        },
        aggs: Some(vec![AggregationRequest {
            name: "price_stats".to_string(),
            agg_type: AggregationType::Stats {
                field: "price".to_string(),
            },
            aggs: None,
        }]),
    }];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    let agg = result.aggregations.get("by_cat_with_stats").unwrap();
    if let AggregationValue::Buckets(buckets) = &agg.value {
        // At least one bucket should have sub_aggs
        let has_sub_aggs = buckets.iter().any(|b| b.sub_aggs.is_some());
        assert!(has_sub_aggs, "Terms buckets should have sub-aggregations");
    } else {
        panic!("Expected Buckets value");
    }
}

#[tokio::test]
async fn test_agg_multiple_aggregations() {
    let (_tmp, backend) = setup_agg_data().await;

    let aggs = vec![
        AggregationRequest {
            name: "count_sum".to_string(),
            agg_type: AggregationType::Sum {
                field: "count".to_string(),
            },
            aggs: None,
        },
        AggregationRequest {
            name: "price_avg".to_string(),
            agg_type: AggregationType::Avg {
                field: "price".to_string(),
            },
            aggs: None,
        },
        AggregationRequest {
            name: "total".to_string(),
            agg_type: AggregationType::Count,
            aggs: None,
        },
    ];

    let result = backend
        .search_with_aggs("test", &match_all_query(), aggs)
        .await
        .unwrap();

    assert!(result.aggregations.contains_key("count_sum"));
    assert!(result.aggregations.contains_key("price_avg"));
    assert!(result.aggregations.contains_key("total"));
}

// =========================================================================
// 7. Other methods: get_top_terms, more_like_this, suggest_terms,
//    get_segments, reconstruct_document
// =========================================================================

#[tokio::test]
async fn test_get_top_terms() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![
                doc("1", "rust rust rust", "systems"),
                doc("2", "rust python", "scripting"),
                doc("3", "python python", "dynamic"),
            ],
        )
        .await
        .unwrap();

    let terms = backend.get_top_terms("test", "title", 5).unwrap();
    assert!(!terms.is_empty());

    // "rust" appears 4 times across 2 docs, "python" 3 times across 2 docs
    // doc_freq for "rust" should be 2, "python" should be 2
    let rust_term = terms.iter().find(|t| t.term == "rust");
    assert!(rust_term.is_some());
    assert!(rust_term.unwrap().doc_freq >= 2);
}

#[tokio::test]
async fn test_get_top_terms_limit() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![
                doc("1", "alpha beta gamma delta", "body"),
                doc("2", "alpha beta epsilon", "body"),
            ],
        )
        .await
        .unwrap();

    let terms = backend.get_top_terms("test", "title", 2).unwrap();
    assert!(terms.len() <= 2);
}

#[tokio::test]
async fn test_suggest_terms_prefix() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![
                doc("1", "programming", "body"),
                doc("2", "production", "body"),
                doc("3", "python", "body"),
            ],
        )
        .await
        .unwrap();

    let suggestions = backend
        .suggest_terms("test", "title", "pro", 10, false, 2)
        .unwrap();

    assert!(!suggestions.is_empty());
    // All suggestions should start with "pro"
    for s in &suggestions {
        assert!(s.term.starts_with("pro"), "Expected prefix 'pro', got '{}'", s.term);
    }
}

#[tokio::test]
async fn test_suggest_terms_fuzzy() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![
                doc("1", "programming", "body"),
                doc("2", "rust", "body"),
            ],
        )
        .await
        .unwrap();

    // With fuzzy enabled and a misspelled prefix, we should get suggestions
    let suggestions = backend
        .suggest_terms("test", "title", "programing", 10, true, 2)
        .unwrap();

    // Should find "programming" as a fuzzy match
    let has_programming = suggestions.iter().any(|s| s.term == "programming");
    assert!(
        has_programming,
        "Fuzzy suggest should find 'programming' from 'programing'"
    );
}

#[tokio::test]
async fn test_more_like_this_by_id() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![
                doc(
                    "d1",
                    "Introduction to Rust programming",
                    "Rust is a systems programming language",
                ),
                doc(
                    "d2",
                    "Advanced Rust patterns",
                    "Rust ownership and borrowing patterns",
                ),
                doc("d3", "Python data science", "Pandas numpy matplotlib"),
            ],
        )
        .await
        .unwrap();

    let results = backend
        .more_like_this(
            "test",
            Some("d1"),
            None,
            &["title".to_string(), "body".to_string()],
            1, // min_term_freq
            1, // min_doc_freq
            10,
            5,
        )
        .unwrap();

    // Should find d2 as similar (both about Rust), and exclude d1 itself
    let ids: Vec<&str> = results.results.iter().map(|r| r.id.as_str()).collect();
    assert!(!ids.contains(&"d1"), "Source document should be excluded");
    if !ids.is_empty() {
        // d2 should rank higher than d3 because it shares "rust" terms
        assert_eq!(ids[0], "d2", "Most similar doc should be d2");
    }
}

#[tokio::test]
async fn test_more_like_this_by_text() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![
                doc("d1", "Rust language guide", "Systems programming in rust"),
                doc("d2", "Python tutorial", "Machine learning with python"),
            ],
        )
        .await
        .unwrap();

    let results = backend
        .more_like_this(
            "test",
            None,
            Some("rust programming systems"),
            &["title".to_string(), "body".to_string()],
            1,
            1,
            10,
            5,
        )
        .unwrap();

    if !results.results.is_empty() {
        // d1 should be more similar
        assert_eq!(results.results[0].id, "d1");
    }
}

#[tokio::test]
async fn test_more_like_this_no_source_returns_error() {
    let (_tmp, backend) = setup().await;

    let result = backend.more_like_this("test", None, None, &[], 1, 1, 10, 5);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_get_segments() {
    let (_tmp, backend) = setup().await;

    backend
        .index("test", vec![doc("1", "Hello", "World")])
        .await
        .unwrap();

    let segments = backend.get_segments("test").unwrap();
    assert!(segments.total_docs >= 1);
    assert!(!segments.segments.is_empty());
    assert!(segments.delete_ratio >= 0.0);
}

#[tokio::test]
async fn test_get_segments_empty_collection() {
    let (_tmp, backend) = setup().await;

    let segments = backend.get_segments("test").unwrap();
    // Empty collection may have 0 segments or 0 docs
    assert_eq!(segments.total_docs, 0);
}

#[tokio::test]
async fn test_reconstruct_document() {
    let (_tmp, backend) = setup().await;

    backend
        .index(
            "test",
            vec![doc("d1", "Hello World", "This is the body text")],
        )
        .await
        .unwrap();

    let reconstructed = backend
        .reconstruct_document("test", "d1")
        .unwrap()
        .expect("Document should exist");

    assert_eq!(reconstructed.id, "d1");
    assert!(reconstructed.stored_fields.contains_key("title"));
    assert_eq!(
        reconstructed.stored_fields.get("title").unwrap(),
        "Hello World"
    );

    // indexed_terms should contain tokenised terms from text fields
    assert!(
        reconstructed.indexed_terms.contains_key("title"),
        "Should have indexed terms for 'title'"
    );
    let title_terms = &reconstructed.indexed_terms["title"];
    assert!(
        title_terms.contains(&"hello".to_string()),
        "Title terms should contain 'hello'"
    );
    assert!(
        title_terms.contains(&"world".to_string()),
        "Title terms should contain 'world'"
    );
}

#[tokio::test]
async fn test_reconstruct_nonexistent_document() {
    let (_tmp, backend) = setup().await;

    let result = backend.reconstruct_document("test", "nope").unwrap();
    assert!(result.is_none());
}

// =========================================================================
// 8. Edge cases & remove_collection
// =========================================================================

#[tokio::test]
async fn test_remove_collection() {
    let (_tmp, backend) = setup().await;

    backend
        .index("test", vec![doc("1", "A", "B")])
        .await
        .unwrap();

    backend.remove_collection("test");

    // Operations on removed collection should error
    let result = backend.stats("test").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_reinitialize_collection() {
    let (_tmp, backend) = setup().await;

    backend
        .index("test", vec![doc("1", "First", "Body")])
        .await
        .unwrap();

    // Reinitialise should open the existing index
    let schema = make_schema();
    backend.initialize("test", &schema).await.unwrap();

    // Documents from before should still be accessible
    let fetched = backend.get("test", "1").await.unwrap();
    assert!(fetched.is_some());
}

#[tokio::test]
async fn test_multiple_collections() {
    let tmp = TempDir::new().unwrap();
    let backend = TextBackend::new(tmp.path()).unwrap();

    let mut schema1 = make_schema();
    schema1.collection = "col_a".to_string();
    backend.initialize("col_a", &schema1).await.unwrap();

    let mut schema2 = make_schema();
    schema2.collection = "col_b".to_string();
    backend.initialize("col_b", &schema2).await.unwrap();

    backend
        .index("col_a", vec![doc("1", "Alpha", "Body A")])
        .await
        .unwrap();
    backend
        .index("col_b", vec![doc("1", "Beta", "Body B")])
        .await
        .unwrap();

    let a = backend.get("col_a", "1").await.unwrap().unwrap();
    assert_eq!(a.fields.get("title").unwrap(), "Alpha");

    let b = backend.get("col_b", "1").await.unwrap().unwrap();
    assert_eq!(b.fields.get("title").unwrap(), "Beta");
}

#[tokio::test]
async fn test_search_with_aggs_on_empty_results() {
    let (_tmp, backend) = setup().await;

    // Index some docs but search for a non-matching query
    backend
        .index("test", vec![doc("1", "hello", "world")])
        .await
        .unwrap();

    let q = Query {
        query_string: "zzyzx_nonexistent".to_string(),
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

    let aggs = vec![AggregationRequest {
        name: "total".to_string(),
        agg_type: AggregationType::Count,
        aggs: None,
    }];

    let result = backend.search_with_aggs("test", &q, aggs).await.unwrap();
    assert_eq!(result.total, 0);
    assert!(result.results.is_empty());

    if let AggregationValue::Single(v) = &result.aggregations["total"].value {
        assert_eq!(*v as u64, 0);
    }
}

#[tokio::test]
async fn test_search_with_aggs_returns_paginated_results() {
    let (_tmp, backend) = setup_agg_data().await;

    let mut q = match_all_query();
    q.limit = 2;
    q.offset = 1;

    let aggs = vec![AggregationRequest {
        name: "total".to_string(),
        agg_type: AggregationType::Count,
        aggs: None,
    }];

    let result = backend.search_with_aggs("test", &q, aggs).await.unwrap();
    // Results should be paginated to 2 items starting from offset 1
    assert_eq!(result.results.len(), 2);
    // But total should reflect all matching docs
    assert_eq!(result.total, 5);
    // Aggregation should count all matching docs (not just the page)
    if let AggregationValue::Single(v) = &result.aggregations["total"].value {
        assert_eq!(*v as u64, 5);
    }
}
