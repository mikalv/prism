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
        sort: Vec::new(),
        exists_fields: Vec::new(),
        not_exists_fields: Vec::new(),
    };
    let results = SearchBackend::search(&backend, "test2", query)
        .await
        .unwrap();
    assert_eq!(results.total, 2);
    assert_eq!(results.results[0].id, "d1");
}

#[tokio::test]
async fn test_search_offset_paginates() {
    use prism::backends::SearchBackend;
    use std::collections::HashMap;

    let temp_dir = TempDir::new().unwrap();
    let backend = VectorBackend::new(temp_dir.path()).unwrap();

    let schema = CollectionSchema {
        collection: "pg".to_string(),
        description: None,
        backends: Backends {
            text: None,
            vector: Some(VectorBackendConfig {
                embedding_field: "embedding".to_string(),
                dimension: 2,
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
    backend.initialize("pg", &schema).await.unwrap();

    // Three docs at increasing angle from the query vector [1,0]:
    // d1 exactly matches, d2 closer than d3.
    let docs: Vec<Document> = [("d1", [1.0, 0.0]), ("d2", [0.92, 0.39]), ("d3", [0.0, 1.0])]
        .into_iter()
        .map(|(id, v)| {
            let mut f = HashMap::new();
            f.insert("embedding".to_string(), serde_json::json!(v));
            Document {
                id: id.to_string(),
                fields: f,
            }
        })
        .collect();
    SearchBackend::index(&backend, "pg", docs).await.unwrap();

    let q = serde_json::to_string(&vec![1.0f32, 0.0]).unwrap();
    let base = prism::backends::r#trait::Query {
        query_string: q,
        fields: vec![],
        limit: 1,
        offset: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
        sort: Vec::new(),
        exists_fields: Vec::new(),
        not_exists_fields: Vec::new(),
    };

    // offset 0 -> nearest (d1)
    let page0 = SearchBackend::search(&backend, "pg", base.clone())
        .await
        .unwrap();
    assert_eq!(page0.results.len(), 1);
    assert_eq!(page0.results[0].id, "d1");

    // offset 1 -> second nearest (d2), NOT d1 again
    let mut q1 = base.clone();
    q1.offset = 1;
    let page1 = SearchBackend::search(&backend, "pg", q1).await.unwrap();
    assert_eq!(page1.results.len(), 1, "offset must skip the first hit");
    assert_eq!(page1.results[0].id, "d2");
}
