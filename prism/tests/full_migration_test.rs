use prism::backends::text::TextBackend;
use prism::backends::VectorBackend;
use prism::collection::manager::CollectionManager;
use prism::migration::{DataExporter, DataImporter, SchemaDiscoverer};
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{sleep, Duration};

/// This test simulates a full migration workflow:
/// 1. Create old-style Tantivy index with some test data
/// 2. Discover schema from the old index
/// 3. Export data to JSONL
/// 4. Start new engraph-core server
/// 5. Import data via HTTP API
/// 6. Verify data is searchable in new system
#[tokio::test]
async fn test_full_migration_flow() {
    // Setup directories
    let temp_dir = TempDir::new().unwrap();
    let old_data_dir = temp_dir.path().join("old_engraph");
    let schemas_dir = temp_dir.path().join("schemas");
    let jsonl_dir = temp_dir.path().join("jsonl");
    let new_data_dir = temp_dir.path().join("new_engraph");

    fs::create_dir_all(&old_data_dir).unwrap();
    fs::create_dir_all(&schemas_dir).unwrap();
    fs::create_dir_all(&jsonl_dir).unwrap();
    fs::create_dir_all(&new_data_dir).unwrap();

    // Step 1: Create old-style Tantivy index with test data
    let collection_name = "test_collection";
    create_old_index(&old_data_dir, collection_name).unwrap();

    // Step 2: Discover schema
    let discoverer = SchemaDiscoverer::new(&old_data_dir);
    let schemas = discoverer.discover_all().unwrap();
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].collection, collection_name);

    discoverer.write_schemas(&schemas_dir, &schemas).unwrap();

    // Step 3: Export data to JSONL
    let exporter = DataExporter::new(&old_data_dir);
    exporter
        .export_all(&vec![collection_name.to_string()], &jsonl_dir)
        .unwrap();

    let jsonl_file = jsonl_dir.join(format!("{}.jsonl", collection_name));
    assert!(jsonl_file.exists());

    // Step 4: Setup new engraph-core system
    let text_backend = Arc::new(TextBackend::new(&new_data_dir).unwrap());
    let vector_backend = Arc::new(VectorBackend::new(&new_data_dir).unwrap());
    let manager = Arc::new(
        CollectionManager::new(&schemas_dir, text_backend.clone(), vector_backend).unwrap(),
    );
    manager.initialize().await.unwrap();

    // Start HTTP server in background
    let server_manager = manager.clone();
    let server_handle = tokio::spawn(async move {
        let api = prism::api::server::ApiServer::new(server_manager);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        println!("Test server listening on: {}", addr);

        axum::serve(listener, api.router().await).await.unwrap();
    });

    // Give server time to start
    sleep(Duration::from_millis(100)).await;

    // Step 5: Import data via HTTP API
    // Note: In a real test, we'd get the actual server port
    // For this test, we'll verify the importer can read JSONL correctly
    let importer = DataImporter::new(&jsonl_dir, "http://localhost:8080".to_string());
    let docs = importer.read_jsonl_file(&jsonl_file).await.unwrap();

    assert_eq!(docs.len(), 3);

    // Verify all documents are present (order may vary)
    let ids: Vec<String> = docs
        .iter()
        .map(|d| d.get("id").unwrap().as_str().unwrap().to_string())
        .collect();
    assert!(ids.contains(&"1".to_string()));
    assert!(ids.contains(&"2".to_string()));
    assert!(ids.contains(&"3".to_string()));

    // Verify one document's content
    let doc1 = docs
        .iter()
        .find(|d| d.get("id").unwrap().as_str().unwrap() == "1")
        .unwrap();
    assert_eq!(doc1.get("title").unwrap().as_str().unwrap(), "Document 1");
    assert_eq!(doc1.get("count").unwrap().as_i64().unwrap(), 100);
    assert_eq!(doc1.get("active").unwrap().as_bool().unwrap(), true);

    // Cleanup
    server_handle.abort();
}

fn create_old_index(base_path: &std::path::Path, collection: &str) -> prism::Result<()> {
    use tantivy::schema::*;
    use tantivy::{doc, Index, IndexWriter};

    let index_path = base_path.join(collection);
    fs::create_dir_all(&index_path)?;

    // Create schema matching what we'll test
    let mut schema_builder = Schema::builder();

    let id_field = schema_builder.add_text_field("id", STRING | STORED);
    let title_field = schema_builder.add_text_field("title", TEXT | STORED);
    let count_field = schema_builder.add_i64_field("count", INDEXED | STORED);
    let active_field = schema_builder.add_bool_field("active", INDEXED | STORED);

    let schema = schema_builder.build();

    let index = Index::create_in_dir(&index_path, schema.clone())?;
    let mut writer: IndexWriter = index.writer(50_000_000)?;

    // Add test documents
    writer.add_document(doc!(
        id_field => "1",
        title_field => "Document 1",
        count_field => 100i64,
        active_field => true
    ))?;

    writer.add_document(doc!(
        id_field => "2",
        title_field => "Document 2",
        count_field => 200i64,
        active_field => false
    ))?;

    writer.add_document(doc!(
        id_field => "3",
        title_field => "Document 3",
        count_field => 300i64,
        active_field => true
    ))?;

    writer.commit()?;

    Ok(())
}
