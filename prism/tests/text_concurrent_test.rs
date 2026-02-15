//! Tests for concurrent text backend operations.
//!
//! Validates that rapid index/search interleaving doesn't crash with
//! "Path not found" errors (the bug caused by background merge threads
//! deleting segment files that concurrent readers reference).

use prism::backends::{Document, Query, SearchBackend, TextBackend};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;

fn make_schema_yaml() -> String {
    r#"
collection: test_concurrent
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
        stored: true
      - name: body
        type: text
        indexed: true
        stored: true
      - name: category
        type: string
        indexed: true
        stored: true
"#
    .to_string()
}

fn make_doc(id: &str, title: &str, body: &str, category: &str) -> Document {
    Document {
        id: id.to_string(),
        fields: HashMap::from([
            ("title".to_string(), json!(title)),
            ("body".to_string(), json!(body)),
            ("category".to_string(), json!(category)),
        ]),
    }
}

fn make_query(query_string: &str, limit: usize) -> Query {
    Query {
        query_string: query_string.to_string(),
        fields: vec![],
        limit,
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

async fn setup_text_backend() -> (TempDir, Arc<TextBackend>) {
    let temp = TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();

    let backend = Arc::new(TextBackend::new(&data_dir).unwrap());

    let schema: prism::schema::CollectionSchema =
        serde_yaml::from_str(&make_schema_yaml()).unwrap();
    backend
        .initialize("test_concurrent", &schema)
        .await
        .unwrap();

    (temp, backend)
}

/// Basic: index documents and immediately search
#[tokio::test]
async fn test_index_then_search() {
    let (_temp, backend) = setup_text_backend().await;

    let docs: Vec<Document> = (0..50)
        .map(|i| {
            make_doc(
                &format!("doc-{i}"),
                &format!("Title {i}"),
                "some body text",
                "cat-a",
            )
        })
        .collect();

    backend.index("test_concurrent", docs).await.unwrap();

    let results = backend
        .search("test_concurrent", make_query("body", 10))
        .await
        .unwrap();

    assert!(results.total > 0, "should find documents after indexing");
}

/// Multiple small commits followed by search — the scenario that triggered
/// background merges and "Path not found" errors.
#[tokio::test]
async fn test_many_small_commits_then_search() {
    let (_temp, backend) = setup_text_backend().await;

    // 20 separate commits of 5 docs each = 20 segments
    for batch in 0..20 {
        let docs: Vec<Document> = (0..5)
            .map(|i| {
                let id = format!("doc-{}-{}", batch, i);
                make_doc(
                    &id,
                    &format!("Batch {batch} doc {i}"),
                    "searchable content here",
                    "test",
                )
            })
            .collect();
        backend.index("test_concurrent", docs).await.unwrap();
    }

    // Search should work without crashing
    let results = backend
        .search("test_concurrent", make_query("searchable", 100))
        .await
        .unwrap();

    assert_eq!(results.total, 100, "all 100 documents should be findable");
}

/// Rapid interleaved index + search operations — stress test for concurrent access.
#[tokio::test]
async fn test_interleaved_index_and_search() {
    let (_temp, backend) = setup_text_backend().await;

    // Seed some initial data
    let seed_docs: Vec<Document> = (0..10)
        .map(|i| {
            make_doc(
                &format!("seed-{i}"),
                "initial document",
                "seed body text",
                "seed",
            )
        })
        .collect();
    backend.index("test_concurrent", seed_docs).await.unwrap();

    // Interleave: index a batch, then search, repeat
    for round in 0..15 {
        let docs: Vec<Document> = (0..3)
            .map(|i| {
                make_doc(
                    &format!("round-{round}-{i}"),
                    &format!("Round {round} title"),
                    "interleaved test content",
                    "interleaved",
                )
            })
            .collect();
        backend.index("test_concurrent", docs).await.unwrap();

        let results = backend
            .search("test_concurrent", make_query("content", 200))
            .await
            .unwrap();

        assert!(
            results.total > 0,
            "search must return results at round {round}"
        );
    }

    // Final count check
    let stats = backend.stats("test_concurrent").await.unwrap();
    assert_eq!(
        stats.document_count,
        10 + 15 * 3,
        "total docs = 10 seed + 45 interleaved"
    );
}

/// Concurrent search tasks while indexing runs — the exact production pattern
/// that triggered "Path not found" on .store/.term/.fieldnorm files.
#[tokio::test]
async fn test_concurrent_search_during_indexing() {
    let (_temp, backend) = setup_text_backend().await;

    // Seed data so searches have something to find
    let seed: Vec<Document> = (0..20)
        .map(|i| {
            make_doc(
                &format!("seed-{i}"),
                "concurrent test",
                "searchable body",
                "cat",
            )
        })
        .collect();
    backend.index("test_concurrent", seed).await.unwrap();

    let b1 = backend.clone();
    let b2 = backend.clone();

    // Spawn indexing task
    let indexer = tokio::spawn(async move {
        for batch in 0..10 {
            let docs: Vec<Document> = (0..5)
                .map(|i| {
                    make_doc(
                        &format!("new-{batch}-{i}"),
                        "newly indexed",
                        "fresh content",
                        "new",
                    )
                })
                .collect();
            b1.index("test_concurrent", docs).await.unwrap();
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }
    });

    // Spawn concurrent search tasks
    let searcher = tokio::spawn(async move {
        for _ in 0..30 {
            let result = b2
                .search("test_concurrent", make_query("body OR content", 50))
                .await;
            // Must not crash with "Path not found"
            assert!(result.is_ok(), "search failed: {:?}", result.err());
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        }
    });

    // Both tasks must complete without panic
    let (idx_result, search_result) = tokio::join!(indexer, searcher);
    idx_result.unwrap();
    search_result.unwrap();
}

/// Delete + re-index cycle — ensures deleted segments don't cause crashes.
#[tokio::test]
async fn test_delete_and_reindex() {
    let (_temp, backend) = setup_text_backend().await;

    // Index initial batch
    let docs: Vec<Document> = (0..20)
        .map(|i| {
            make_doc(
                &format!("del-{i}"),
                "deletable doc",
                "will be removed",
                "temp",
            )
        })
        .collect();
    backend.index("test_concurrent", docs).await.unwrap();

    // Delete half
    let ids_to_delete: Vec<String> = (0..10).map(|i| format!("del-{i}")).collect();
    backend
        .delete("test_concurrent", ids_to_delete)
        .await
        .unwrap();

    // Re-index new documents
    let new_docs: Vec<Document> = (0..20)
        .map(|i| {
            make_doc(
                &format!("new-{i}"),
                "replacement doc",
                "fresh content",
                "perm",
            )
        })
        .collect();
    backend.index("test_concurrent", new_docs).await.unwrap();

    // Search should work
    let results = backend
        .search("test_concurrent", make_query("fresh", 50))
        .await
        .unwrap();

    assert_eq!(results.total, 20, "should find all 20 new docs");
}

/// Bulk indexing — large batch in one commit.
#[tokio::test]
async fn test_bulk_index() {
    let (_temp, backend) = setup_text_backend().await;

    let docs: Vec<Document> = (0..500)
        .map(|i| {
            make_doc(
                &format!("bulk-{i}"),
                &format!("Document number {i}"),
                &format!("Body content for document {i} with searchable text"),
                if i % 2 == 0 { "even" } else { "odd" },
            )
        })
        .collect();

    backend.index("test_concurrent", docs).await.unwrap();

    let stats = backend.stats("test_concurrent").await.unwrap();
    assert_eq!(stats.document_count, 500);

    let results = backend
        .search("test_concurrent", make_query("searchable", 10))
        .await
        .unwrap();

    assert!(results.total > 0);
}

/// Get by ID after multiple commits.
#[tokio::test]
async fn test_get_after_multiple_commits() {
    let (_temp, backend) = setup_text_backend().await;

    // Three separate commits
    for batch in 0..3 {
        let docs: Vec<Document> = (0..5)
            .map(|i| {
                make_doc(
                    &format!("get-{batch}-{i}"),
                    &format!("Batch {batch}"),
                    "retrievable",
                    "test",
                )
            })
            .collect();
        backend.index("test_concurrent", docs).await.unwrap();
    }

    // Get a doc from each batch
    for batch in 0..3 {
        let doc = backend
            .get("test_concurrent", &format!("get-{batch}-2"))
            .await
            .unwrap();
        assert!(doc.is_some(), "doc get-{batch}-2 should exist");
        let doc = doc.unwrap();
        assert_eq!(
            doc.fields.get("title").and_then(|v| v.as_str()),
            Some(&*format!("Batch {batch}"))
        );
    }
}
