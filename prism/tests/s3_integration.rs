//! S3 Integration Tests
//!
//! Run with: cargo test --features storage-s3 --test s3_integration -- --ignored
//! Requires MinIO: docker-compose -f docker-compose.test.yml up -d

#![cfg(feature = "storage-s3")]

use prism::storage::{Bytes, S3Config, SegmentStorage, StorageBackend, StoragePath};
use prism_storage::{S3Config as PrismS3Config, S3Storage};
use std::sync::Arc;

fn minio_config() -> S3Config {
    S3Config {
        bucket: "test-bucket".to_string(),
        region: "us-east-1".to_string(),
        prefix: Some("prism-test/".to_string()),
        endpoint: Some("http://localhost:9000".to_string()),
        force_path_style: true,
        cache_dir: None,
        cache_max_size_mb: None,
        access_key_id: Some("minioadmin".to_string()),
        secret_access_key: Some("minioadmin".to_string()),
    }
}

fn create_s3_storage() -> Arc<dyn SegmentStorage> {
    let config = PrismS3Config::minio("test-bucket", "http://localhost:9000")
        .with_prefix("prism-test/")
        .with_credentials("minioadmin", "minioadmin");
    Arc::new(S3Storage::new(config).expect("Failed to create S3 storage"))
}

// =============================================================================
// SegmentStorage S3 Tests
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_s3_segment_storage_roundtrip() {
    let storage = create_s3_storage();

    let path = StoragePath::vector("test-collection", "default", "index.bin");
    let data = Bytes::from_static(b"serialized hnsw index data for testing");

    storage
        .write(&path, data.clone())
        .await
        .expect("Failed to write");

    let loaded = storage.read(&path).await.expect("Failed to read");
    assert_eq!(loaded, data);

    // Cleanup
    storage.delete(&path).await.expect("Failed to delete");
}

#[tokio::test]
#[ignore]
async fn test_s3_segment_storage_not_found() {
    let storage = create_s3_storage();

    let path = StoragePath::vector("nonexistent", "shard", "missing.bin");
    let result = storage.read(&path).await;

    // Should return error for non-existent file
    assert!(result.is_err());
}

#[tokio::test]
#[ignore]
async fn test_s3_segment_storage_overwrite() {
    let storage = create_s3_storage();
    let path = StoragePath::vector("overwrite-test", "default", "data.bin");

    // Write initial data
    storage
        .write(&path, Bytes::from_static(b"version 1"))
        .await
        .expect("Failed to write v1");

    // Overwrite with new data
    storage
        .write(&path, Bytes::from_static(b"version 2"))
        .await
        .expect("Failed to write v2");

    let loaded = storage.read(&path).await.expect("Failed to read");
    assert_eq!(loaded, Bytes::from_static(b"version 2"));

    // Cleanup
    storage.delete(&path).await.ok();
}

#[tokio::test]
#[ignore]
async fn test_s3_segment_storage_delete() {
    let storage = create_s3_storage();
    let path = StoragePath::vector("delete-test", "default", "data.bin");

    // Save data
    storage
        .write(&path, Bytes::from_static(b"to be deleted"))
        .await
        .expect("Failed to write");

    // Verify it exists
    let loaded = storage.read(&path).await.expect("Failed to read");
    assert_eq!(loaded, Bytes::from_static(b"to be deleted"));

    // Delete
    storage.delete(&path).await.expect("Failed to delete");

    // Verify it's gone
    let result = storage.read(&path).await;
    assert!(result.is_err());
}

#[tokio::test]
#[ignore]
async fn test_s3_segment_storage_large_data() {
    let storage = create_s3_storage();
    let path = StoragePath::vector("large-test", "default", "large.bin");

    // Create ~1MB of data (simulating a real HNSW index)
    let large_data: Vec<u8> = (0..1_000_000).map(|i| (i % 256) as u8).collect();
    let large_bytes = Bytes::from(large_data.clone());

    storage
        .write(&path, large_bytes.clone())
        .await
        .expect("Failed to write large data");

    let loaded = storage
        .read(&path)
        .await
        .expect("Failed to read large data");
    assert_eq!(loaded, large_bytes);

    // Cleanup
    storage.delete(&path).await.ok();
}

#[tokio::test]
#[ignore]
async fn test_s3_segment_storage_list() {
    let storage = create_s3_storage();
    let collection = "list-test";

    // Write multiple files
    let path1 = StoragePath::vector(collection, "shard1", "index.bin");
    let path2 = StoragePath::vector(collection, "shard2", "index.bin");

    storage
        .write(&path1, Bytes::from_static(b"shard1"))
        .await
        .unwrap();
    storage
        .write(&path2, Bytes::from_static(b"shard2"))
        .await
        .unwrap();

    // List collection files using StoragePath prefix
    let prefix = StoragePath::new(collection, StorageBackend::Vector);
    let files = storage.list(&prefix).await.expect("Failed to list");
    assert!(files.len() >= 2);

    // Cleanup
    storage.delete(&path1).await.ok();
    storage.delete(&path2).await.ok();
}

#[tokio::test]
#[ignore]
async fn test_s3_segment_storage_text_backend_path() {
    let storage = create_s3_storage();

    // Test text backend path format (uses tantivy for text search)
    let path = StoragePath::tantivy("documents", "default", "meta.json");
    let data = Bytes::from_static(b"{\"version\": 1}");

    storage
        .write(&path, data.clone())
        .await
        .expect("Failed to write text segment");

    let loaded = storage
        .read(&path)
        .await
        .expect("Failed to read text segment");
    assert_eq!(loaded, data);

    // Cleanup
    storage.delete(&path).await.ok();
}

#[tokio::test]
#[ignore]
async fn test_s3_segment_storage_graph_backend_path() {
    let storage = create_s3_storage();

    // Test graph backend path format
    let path = StoragePath::graph("knowledge", "default", "nodes.json");
    let data = Bytes::from_static(b"{}");

    storage
        .write(&path, data.clone())
        .await
        .expect("Failed to write graph segment");

    let loaded = storage
        .read(&path)
        .await
        .expect("Failed to read graph segment");
    assert_eq!(loaded, data);

    // Cleanup
    storage.delete(&path).await.ok();
}

#[test]
#[ignore]
fn test_s3_config_from_prism_config() {
    let config = minio_config();

    // Verify config can be converted to prism-storage config
    let prism_config = if config.endpoint.is_some() {
        PrismS3Config::minio(&config.bucket, config.endpoint.as_ref().unwrap())
    } else {
        PrismS3Config::aws(&config.bucket, &config.region)
    };

    let prism_config = if let Some(prefix) = &config.prefix {
        prism_config.with_prefix(prefix)
    } else {
        prism_config
    };

    let prism_config =
        if let (Some(key), Some(secret)) = (&config.access_key_id, &config.secret_access_key) {
            prism_config.with_credentials(key, secret)
        } else {
            prism_config
        };

    // Should be able to create storage from config
    let storage = S3Storage::new(prism_config);
    assert!(storage.is_ok());
}
