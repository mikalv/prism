use searchcore::backends::r#trait::{Document, Query, SearchBackend};
use searchcore::backends::{TextBackend, VectorBackend, HybridSearchCoordinator};
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::test]
async fn test_hybrid_merge_behaviour() {
    // Create simple backends
    let tmp = tempfile::TempDir::new().unwrap();
    let text_path = tmp.path().join("tantivy");
    let vector_path = tmp.path().join("vector");
    let text = Arc::new(TextBackend::new(text_path).unwrap());
    let vector = Arc::new(VectorBackend::new(vector_path).unwrap());

    let hybrid = HybridSearchCoordinator::new(text.clone(), vector.clone(), 0.6);

    // Initialize simple collection schema for both backends
    let schema = searchcore::schema::types::CollectionSchema {
        collection: "col".to_string(),
        description: None,
        backends: searchcore::schema::types::Backends { text: Some(searchcore::schema::types::TextBackendConfig { fields: vec![searchcore::schema::types::TextField { name: "text".to_string(), field_type: searchcore::schema::types::FieldType::Text, stored: true, indexed: true }] }), vector: Some(searchcore::schema::types::VectorBackendConfig { embedding_field: "embedding".to_string(), dimension: 3, distance: searchcore::schema::types::VectorDistance::Cosine, hnsw_m: 16, hnsw_ef_construction: 200, hnsw_ef_search: 100, vector_weight: 0.5 }), graph: None },
        indexing: Default::default(),
        quota: Default::default(),
        embedding_generation: None,
        facets: None,
            boosting: None,
    };

    text.initialize("col", &schema).await.unwrap();
    vector.initialize("col", &schema).await.unwrap();

    // Add documents via hybrid
    let doc1 = Document { id: "d1".to_string(), fields: { let mut m = HashMap::new(); m.insert("text".to_string(), serde_json::json!("hello world")); m.insert("embedding".to_string(), serde_json::json!([1.0,0.0,0.0])); m }};
    let doc2 = Document { id: "d2".to_string(), fields: { let mut m = HashMap::new(); m.insert("text".to_string(), serde_json::json!("foo bar")); m.insert("embedding".to_string(), serde_json::json!([0.0,1.0,0.0])); m }};

    hybrid.index("col", vec![doc1.clone(), doc2.clone()]).await.unwrap();

    // For this test, set query_string to the vector JSON and include text field
    let q = Query { query_string: serde_json::to_string(&vec![1.0f32, 0.0, 0.0]).unwrap(), fields: vec!["text".to_string()], limit: 10, offset: 0, merge_strategy: None, text_weight: None, vector_weight: None };
    let res = hybrid.search("col", q).await.unwrap();

    // Expect results contain at least one document
    assert!(res.results.len() >= 1);
}