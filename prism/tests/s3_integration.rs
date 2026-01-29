//! S3 Integration Tests
//!
//! Run with: cargo test --features storage-s3 --test s3_integration -- --ignored
//! Requires MinIO: docker-compose -f docker-compose.test.yml up -d

#![cfg(feature = "storage-s3")]

use object_store::aws::AmazonS3Builder;
use prism::storage::{ObjectStoreDirectory, S3Config, S3VectorStore, VectorStore};
use std::path::Path;
use std::sync::Arc;
use tantivy::directory::Directory;

fn minio_config() -> S3Config {
    S3Config {
        bucket: "test-bucket".to_string(),
        region: "us-east-1".to_string(),
        prefix: Some("prism-test/".to_string()),
        endpoint: Some("http://localhost:9000".to_string()),
        force_path_style: true,
        cache_dir: None,
        cache_max_size_mb: None,
    }
}

fn create_minio_store() -> Arc<dyn object_store::ObjectStore> {
    Arc::new(
        AmazonS3Builder::new()
            .with_bucket_name("test-bucket")
            .with_region("us-east-1")
            .with_endpoint("http://localhost:9000")
            .with_virtual_hosted_style_request(false)
            .with_allow_http(true)
            .with_access_key_id("minioadmin")
            .with_secret_access_key("minioadmin")
            .build()
            .expect("Failed to create S3 store"),
    )
}

// =============================================================================
// ObjectStoreDirectory Tests
// Note: These use #[test] (not #[tokio::test]) because ObjectStoreDirectory
// creates its own internal runtime for sync operations.
// =============================================================================

#[test]
#[ignore]
fn test_s3_directory_operations() {
    let store = create_minio_store();

    let dir = ObjectStoreDirectory::new(store.clone(), "test-collection", None, 0)
        .expect("Failed to create directory");

    dir.atomic_write(Path::new("test.txt"), b"hello s3")
        .expect("Failed to write");

    let data = dir
        .atomic_read(Path::new("test.txt"))
        .expect("Failed to read");
    assert_eq!(data, b"hello s3");

    let files = dir.list_files();
    assert!(files.iter().any(|p| p.to_string_lossy().contains("test")));

    dir.atomic_write(Path::new("test.txt"), b"")
        .expect("Failed to cleanup");
}

#[test]
#[ignore]
fn test_s3_file_handle() {
    let store = create_minio_store();

    let dir = ObjectStoreDirectory::new(store.clone(), "test-filehandle", None, 0)
        .expect("Failed to create directory");

    dir.atomic_write(Path::new("segment.idx"), b"0123456789")
        .expect("Failed to write");

    let handle = dir
        .get_file_handle(Path::new("segment.idx"))
        .expect("Failed to get handle");

    let bytes = handle.read_bytes(2..5).expect("Failed to read range");
    assert_eq!(bytes.as_slice(), b"234");

    dir.atomic_write(Path::new("segment.idx"), b"")
        .expect("Failed to cleanup");
}

#[test]
#[ignore]
fn test_s3_write_ptr() {
    use std::io::Write;
    use tantivy::directory::TerminatingWrite;

    let store = create_minio_store();

    let dir = ObjectStoreDirectory::new(store.clone(), "test-writeptr", None, 0)
        .expect("Failed to create directory");

    let mut writer = dir
        .open_write(Path::new("segment.dat"))
        .expect("Failed to open write");
    writer.write_all(b"hello ").expect("Failed to write");
    writer.write_all(b"world").expect("Failed to write");
    writer.terminate().expect("Failed to terminate");

    let data = dir
        .atomic_read(Path::new("segment.dat"))
        .expect("Failed to read");
    assert_eq!(data, b"hello world");

    dir.atomic_write(Path::new("segment.dat"), b"")
        .expect("Failed to cleanup");
}

#[test]
#[ignore]
fn test_s3_from_config() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let config = minio_config();

    let dir = rt
        .block_on(ObjectStoreDirectory::from_s3_config(
            &config,
            "config-test",
            None,
            0,
        ))
        .expect("Failed to create from config");

    dir.atomic_write(Path::new("config-test.txt"), b"from config")
        .expect("Failed to write");

    let data = dir
        .atomic_read(Path::new("config-test.txt"))
        .expect("Failed to read");
    assert_eq!(data, b"from config");

    dir.atomic_write(Path::new("config-test.txt"), b"")
        .expect("Failed to cleanup");
}

// =============================================================================
// S3VectorStore Tests
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_s3_vector_store_roundtrip() {
    let store = create_minio_store();
    let vector_store = S3VectorStore::new(store, "vector-test/".to_string());

    let data = b"serialized hnsw index data for testing";
    vector_store
        .save("test-collection", data)
        .await
        .expect("Failed to save vector index");

    let loaded = vector_store
        .load("test-collection")
        .await
        .expect("Failed to load vector index");
    assert_eq!(loaded, Some(data.to_vec()));

    // Cleanup
    vector_store
        .delete("test-collection")
        .await
        .expect("Failed to delete");
}

#[tokio::test]
#[ignore]
async fn test_s3_vector_store_not_found() {
    let store = create_minio_store();
    let vector_store = S3VectorStore::new(store, "vector-test/".to_string());

    let loaded = vector_store
        .load("nonexistent-collection")
        .await
        .expect("Failed to load");
    assert_eq!(loaded, None);
}

#[tokio::test]
#[ignore]
async fn test_s3_vector_store_overwrite() {
    let store = create_minio_store();
    let vector_store = S3VectorStore::new(store, "vector-test/".to_string());

    // Write initial data
    vector_store
        .save("overwrite-test", b"version 1")
        .await
        .expect("Failed to save v1");

    // Overwrite with new data
    vector_store
        .save("overwrite-test", b"version 2")
        .await
        .expect("Failed to save v2");

    let loaded = vector_store
        .load("overwrite-test")
        .await
        .expect("Failed to load");
    assert_eq!(loaded, Some(b"version 2".to_vec()));

    // Cleanup
    vector_store.delete("overwrite-test").await.ok();
}

#[tokio::test]
#[ignore]
async fn test_s3_vector_store_delete() {
    let store = create_minio_store();
    let vector_store = S3VectorStore::new(store, "vector-test/".to_string());

    // Save data
    vector_store
        .save("delete-test", b"to be deleted")
        .await
        .expect("Failed to save");

    // Verify it exists
    let loaded = vector_store.load("delete-test").await.expect("Failed to load");
    assert!(loaded.is_some());

    // Delete
    vector_store
        .delete("delete-test")
        .await
        .expect("Failed to delete");

    // Verify it's gone
    let loaded = vector_store.load("delete-test").await.expect("Failed to load");
    assert_eq!(loaded, None);
}

#[tokio::test]
#[ignore]
async fn test_s3_vector_store_large_data() {
    let store = create_minio_store();
    let vector_store = S3VectorStore::new(store, "vector-test/".to_string());

    // Create ~1MB of data (simulating a real HNSW index)
    let large_data: Vec<u8> = (0..1_000_000).map(|i| (i % 256) as u8).collect();

    vector_store
        .save("large-index", &large_data)
        .await
        .expect("Failed to save large index");

    let loaded = vector_store
        .load("large-index")
        .await
        .expect("Failed to load large index");
    assert_eq!(loaded, Some(large_data));

    // Cleanup
    vector_store.delete("large-index").await.ok();
}
