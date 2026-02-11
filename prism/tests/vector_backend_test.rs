use prism::backends::r#trait::Document;
use prism::backends::SearchBackend;
use prism::backends::VectorBackend;
use prism::schema::types::{Backends, CollectionSchema, VectorBackendConfig, VectorDistance};
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn test_initialize_collection() {
    let temp_dir = TempDir::new().unwrap();
    let backend = Arc::new(VectorBackend::new(temp_dir.path()).unwrap());

    let schema = CollectionSchema {
        collection: "test".to_string(),
        description: None,
        backends: Backends {
            text: None,
            vector: Some(VectorBackendConfig {
                embedding_field: "embedding".to_string(),
                dimension: 384,
                distance: VectorDistance::Cosine,
                hnsw_m: 16,
                hnsw_ef_construction: 200,
                hnsw_ef_search: 100,
                vector_weight: 0.5,
                num_shards: 1,
                shard_oversample: 2.5,
                compaction: Default::default(),
            }),
            graph: None,
        },
        indexing: Default::default(),
        quota: Default::default(),
        embedding_generation: None,
        facets: None,
        boosting: None,
        storage: Default::default(),
        system_fields: Default::default(),
        hybrid: None,
        replication: None,
        reranking: None,
        ilm_policy: None,
    };

    backend.initialize("test", &schema).await.unwrap();

    // Verify index was created via initialize (no private access)
    // attempt to get a non-existing doc should return None until indexed
    let got = SearchBackend::get(&*backend, "test", "nope").await.unwrap();
    assert!(got.is_none());
}

#[tokio::test]
async fn test_index_and_search() {
    use prism::backends::SearchBackend;

    let temp_dir = TempDir::new().unwrap();
    let backend = VectorBackend::new(temp_dir.path()).unwrap();

    let schema = CollectionSchema {
        collection: "test2".to_string(),
        description: None,
        backends: Backends {
            text: None,
            vector: Some(VectorBackendConfig {
                embedding_field: "embedding".to_string(),
                dimension: 4,
                distance: VectorDistance::Cosine,
                hnsw_m: 16,
                hnsw_ef_construction: 200,
                hnsw_ef_search: 100,
                vector_weight: 0.5,
                num_shards: 1,
                shard_oversample: 2.5,
                compaction: Default::default(),
            }),
            graph: None,
        },
        indexing: Default::default(),
        quota: Default::default(),
        embedding_generation: None,
        facets: None,
        boosting: None,
        storage: Default::default(),
        system_fields: Default::default(),
        hybrid: None,
        replication: None,
        reranking: None,
        ilm_policy: None,
    };

    backend.initialize("test2", &schema).await.unwrap();

    // Index two documents
    let mut fields1 = std::collections::HashMap::new();
    fields1.insert(
        "embedding".to_string(),
        serde_json::json!([1.0, 0.0, 0.0, 0.0]),
    );
    let doc1 = Document {
        id: "d1".to_string(),
        fields: fields1,
    };

    let mut fields2 = std::collections::HashMap::new();
    fields2.insert(
        "embedding".to_string(),
        serde_json::json!([0.0, 1.0, 0.0, 0.0]),
    );
    let doc2 = Document {
        id: "d2".to_string(),
        fields: fields2,
    };

    SearchBackend::index(&backend, "test2", vec![doc1.clone(), doc2.clone()])
        .await
        .unwrap();

    // Query with vector close to doc1
    let q = serde_json::to_string(&vec![1.0f32, 0.0, 0.0, 0.0]).unwrap();
    let query = prism::backends::r#trait::Query {
        query_string: q,
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
    let results = SearchBackend::search(&backend, "test2", query)
        .await
        .unwrap();
    assert_eq!(results.total, 2);
    assert_eq!(results.results[0].id, "d1");
}
