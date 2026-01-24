use std::sync::Arc;
use searchcore::backends::vector::VectorBackend;
use searchcore::backends::Document;
use searchcore::backends::SearchBackend;
use searchcore::schema::CollectionSchema;

// A minimal test that indexes a document with a precomputed embedding to verify indexing path
#[tokio::test]
async fn test_index_with_embedding_field() {
    let tmp = tempfile::tempdir().unwrap();
    let backend = VectorBackend::new(tmp.path()).unwrap();

    let schema = CollectionSchema {
        collection: "test_collection".into(),
        description: None,
        backends: searchcore::schema::types::Backends { text: None, vector: Some(searchcore::schema::types::VectorBackendConfig { embedding_field: "embedding".into(), dimension: 8, distance: searchcore::schema::types::VectorDistance::Cosine, hnsw_m: 16, hnsw_ef_construction: 200, hnsw_ef_search: 100, vector_weight: 0.5 }), graph: None },
        indexing: searchcore::schema::types::IndexingConfig::default(),
        quota: searchcore::schema::types::QuotaConfig::default(),
        embedding_generation: None,
        facets: None,
            boosting: None,
    };
    backend.initialize("test_collection", &schema).await.unwrap();

    let docs = vec![Document {
        id: "doc1".to_string(),
        fields: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "embedding".to_string(),
                serde_json::Value::Array((0..8).map(|i| serde_json::Value::Number(serde_json::Number::from_f64((i as f64 + 1.0) / 8.0).unwrap())).collect()),
            );
            m.insert("text".to_string(), serde_json::Value::String("hello world".into()));
            m
        },
    }];

    backend
        .index("test_collection", docs)
        .await
        .expect("Indexing failed");
}
