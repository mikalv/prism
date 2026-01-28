//! S3 Integration Tests
//!
//! Run with: cargo test --features storage-s3 --test s3_integration -- --ignored
//! Requires MinIO: docker-compose -f docker-compose.test.yml up -d

#![cfg(feature = "storage-s3")]

use object_store::aws::AmazonS3Builder;
use prism::storage::{ObjectStoreDirectory, S3Config};
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
    }
}

async fn create_minio_store() -> Arc<dyn object_store::ObjectStore> {
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

#[tokio::test]
#[ignore]
async fn test_s3_directory_operations() {
    let store = create_minio_store().await;

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

#[tokio::test]
#[ignore]
async fn test_s3_file_handle() {
    let store = create_minio_store().await;

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

#[tokio::test]
#[ignore]
async fn test_s3_write_ptr() {
    use std::io::Write;
    use tantivy::directory::TerminatingWrite;

    let store = create_minio_store().await;

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

#[tokio::test]
#[ignore]
async fn test_s3_from_config() {
    let config = minio_config();

    let dir = ObjectStoreDirectory::from_s3_config(&config, "config-test", None, 0)
        .await
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
