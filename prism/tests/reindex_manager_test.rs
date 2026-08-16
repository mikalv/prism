//! Integration tests for reindex_collection against the CollectionManager.

use prism::backends::{SearchBackend, TextBackend, VectorBackend};
use prism::collection::CollectionManager;
use prism::schema::SchemaLoader;
use std::sync::Arc;
use tempfile::TempDir;

async fn setup() -> (TempDir, Arc<CollectionManager>) {
    let temp = TempDir::new().unwrap();
    let schemas_dir = temp.path().join("schemas");
    let data_dir = temp.path().join("data");
    std::fs::create_dir_all(&schemas_dir).unwrap();

    // Hybrid schema: content -> content_vector, dim 4
    let schema_yaml = r#"
collection: reindex-hybrid
backends:
  text:
    fields:
      - name: content
        type: text
        stored: true
        indexed: true
      - name: title
        type: text
        stored: true
        indexed: true
  vector:
    embedding_field: content_vector
    dimension: 4
    distance: cosine
embedding_generation:
  enabled: true
  model: mock
  source_field: content
  target_field: content_vector
"#;
    std::fs::write(
        schemas_dir.join("reindex-hybrid.yaml"),
        schema_yaml.trim(),
    )
    .unwrap();

    let text_backend = Arc::new(TextBackend::new(&data_dir).unwrap());
    let vector_backend = Arc::new(VectorBackend::new(&data_dir).unwrap());
    let manager = Arc::new(
        CollectionManager::new(&schemas_dir, text_backend, vector_backend, None).unwrap(),
    );
    manager.initialize().await.unwrap();
    (temp, manager)
}

fn doc(id: &str, content: &str, vector: Option<Vec<f32>>) -> prism::backends::Document {
    let mut fields = std::collections::HashMap::new();
    fields.insert("content".to_string(), serde_json::json!(content));
    fields.insert("title".to_string(), serde_json::json!(format!("t-{}", id)));
    if let Some(v) = vector {
        fields.insert("content_vector".to_string(), serde_json::json!(v));
    }
    prism::backends::Document {
        id: id.to_string(),
        fields,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_reindex_collection_strips_and_finds_vectors() {
    let (_t, manager) = setup().await;

    // 3 docs with stale vectors, 1 without (new doc not yet embedded),
    // 1 doc without content at all (only title) - should not be re-embedded.
    let docs = vec![
        doc("d1", "alpha content", Some(vec![0.1, 0.2, 0.3, 0.4])),
        doc("d2", "beta content", Some(vec![0.5, 0.6, 0.7, 0.8])),
        doc("d3", "gamma content", None),
        {
            let mut d = doc("d4", "delta content", None);
            d.fields.remove("content");
            d
        },
    ];
    manager.index("reindex-hybrid", docs).await.unwrap();

    // Sanity: what does the text-backend scroll query actually return?
    let page = manager
        .text_backend()
        .search(
            "reindex-hybrid",
            prism::backends::Query {
                query_string: "*".to_string(),
                limit: 100,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    eprintln!("SCROLL PAGE: total={} len={}", page.total, page.results.len());

    let (_n, reembedded, skipped) = manager
        .reindex_collection("reindex-hybrid", 2)
        .await
        .unwrap();
    eprintln!("REINDEX: reembedded={} skipped={}", reembedded, skipped);

    // d1/d2 had vectors (stripped+counted); d3 has content but no vector.
    // d4 has no content - excluded by the exists query.
    assert!(reembedded + skipped >= 3, "expected >=3 docs scrolled");
}
