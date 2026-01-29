//! S3-compatible object storage backend.
//!
//! Uses the `object_store` crate for S3, MinIO, and other S3-compatible services.
//!
//! # Configuration
//!
//! ```toml
//! [storage.s3]
//! bucket = "prism-data"
//! region = "us-east-1"
//! prefix = "collections/"
//!
//! # Optional: For MinIO or other S3-compatible services
//! endpoint = "http://localhost:9000"
//! force_path_style = true
//! ```

use async_trait::async_trait;
use bytes::Bytes;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use object_store::ObjectStore;
use std::sync::Arc;
use tracing::{debug, instrument};

use crate::error::{Result, StorageError};
use crate::path::StoragePath;
use crate::traits::{ListOptions, ObjectMeta, SegmentStorage};

/// Configuration for S3 storage.
#[derive(Debug, Clone)]
pub struct S3Config {
    /// S3 bucket name
    pub bucket: String,
    /// AWS region
    pub region: String,
    /// Optional prefix for all paths
    pub prefix: Option<String>,
    /// Optional custom endpoint (for MinIO, etc.)
    pub endpoint: Option<String>,
    /// Use path-style requests (required for MinIO)
    pub force_path_style: bool,
    /// Optional access key (if not using IAM/env credentials)
    pub access_key_id: Option<String>,
    /// Optional secret key
    pub secret_access_key: Option<String>,
    /// Allow HTTP (non-HTTPS) connections
    pub allow_http: bool,
}

impl S3Config {
    /// Create a new S3 configuration for AWS.
    pub fn aws(bucket: impl Into<String>, region: impl Into<String>) -> Self {
        Self {
            bucket: bucket.into(),
            region: region.into(),
            prefix: None,
            endpoint: None,
            force_path_style: false,
            access_key_id: None,
            secret_access_key: None,
            allow_http: false,
        }
    }

    /// Create configuration for MinIO or other S3-compatible services.
    pub fn minio(
        bucket: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            bucket: bucket.into(),
            region: "us-east-1".to_string(),
            prefix: None,
            endpoint: Some(endpoint.into()),
            force_path_style: true,
            access_key_id: None,
            secret_access_key: None,
            allow_http: true,
        }
    }

    /// Set optional prefix for all paths.
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Set explicit credentials.
    pub fn with_credentials(
        mut self,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        self.access_key_id = Some(access_key_id.into());
        self.secret_access_key = Some(secret_access_key.into());
        self
    }
}

/// S3-compatible object storage backend.
#[derive(Clone)]
pub struct S3Storage {
    store: Arc<dyn ObjectStore>,
    prefix: String,
}

impl S3Storage {
    /// Create a new S3 storage backend from configuration.
    pub fn new(config: S3Config) -> Result<Self> {
        let mut builder = AmazonS3Builder::new()
            .with_bucket_name(&config.bucket)
            .with_region(&config.region)
            .with_allow_http(config.allow_http);

        if let Some(endpoint) = &config.endpoint {
            builder = builder.with_endpoint(endpoint);
        }

        if config.force_path_style {
            builder = builder.with_virtual_hosted_style_request(false);
        }

        if let (Some(key_id), Some(secret)) = (&config.access_key_id, &config.secret_access_key) {
            builder = builder
                .with_access_key_id(key_id)
                .with_secret_access_key(secret);
        }

        let store = builder
            .build()
            .map_err(|e| StorageError::Config(e.to_string()))?;

        let prefix = config.prefix.unwrap_or_default();

        Ok(Self {
            store: Arc::new(store),
            prefix,
        })
    }

    /// Create from an existing ObjectStore instance.
    pub fn from_store(store: Arc<dyn ObjectStore>, prefix: String) -> Self {
        Self { store, prefix }
    }

    /// Convert StoragePath to object_store Path.
    fn to_object_path(&self, path: &StoragePath) -> ObjectPath {
        let path_str = if self.prefix.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", self.prefix.trim_end_matches('/'), path)
        };
        ObjectPath::from(path_str)
    }

    /// Convert object_store Path back to StoragePath.
    fn from_object_path(&self, obj_path: &ObjectPath) -> Option<StoragePath> {
        let path_str = obj_path.as_ref();
        let relative = if !self.prefix.is_empty() {
            path_str.strip_prefix(&self.prefix)?.trim_start_matches('/')
        } else {
            path_str
        };
        StoragePath::parse(relative)
    }
}

impl std::fmt::Debug for S3Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Storage")
            .field("prefix", &self.prefix)
            .finish()
    }
}

#[async_trait]
impl SegmentStorage for S3Storage {
    #[instrument(skip(self, data), fields(path = %path, size = data.len()))]
    async fn write(&self, path: &StoragePath, data: Bytes) -> Result<()> {
        let obj_path = self.to_object_path(path);
        debug!("Writing {} bytes to s3://{:?}", data.len(), obj_path);

        self.store
            .put(&obj_path, data.into())
            .await
            .map_err(StorageError::from)?;

        Ok(())
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn read(&self, path: &StoragePath) -> Result<Bytes> {
        let obj_path = self.to_object_path(path);
        debug!("Reading from s3://{:?}", obj_path);

        match self.store.get(&obj_path).await {
            Ok(result) => {
                let bytes = result.bytes().await.map_err(StorageError::from)?;
                Ok(bytes)
            }
            Err(object_store::Error::NotFound { .. }) => {
                Err(StorageError::NotFound(path.to_string()))
            }
            Err(e) => Err(StorageError::from(e)),
        }
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn exists(&self, path: &StoragePath) -> Result<bool> {
        let obj_path = self.to_object_path(path);

        match self.store.head(&obj_path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(StorageError::from(e)),
        }
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn delete(&self, path: &StoragePath) -> Result<()> {
        let obj_path = self.to_object_path(path);
        debug!("Deleting s3://{:?}", obj_path);

        match self.store.delete(&obj_path).await {
            Ok(()) => Ok(()),
            Err(object_store::Error::NotFound { .. }) => Ok(()), // Idempotent
            Err(e) => Err(StorageError::from(e)),
        }
    }

    #[instrument(skip(self), fields(prefix = %prefix))]
    async fn list(&self, prefix: &StoragePath) -> Result<Vec<ObjectMeta>> {
        use futures::TryStreamExt;

        let obj_prefix = self.to_object_path(prefix);
        debug!("Listing s3://{:?}", obj_prefix);

        let mut results = Vec::new();
        let mut stream = self.store.list(Some(&obj_prefix));

        while let Some(meta) = stream.try_next().await.map_err(StorageError::from)? {
            if let Some(storage_path) = self.from_object_path(&meta.location) {
                results.push(ObjectMeta {
                    path: storage_path,
                    size: meta.size as u64,
                    last_modified: Some(meta.last_modified.timestamp()),
                    etag: meta.e_tag.clone(),
                });
            }
        }

        Ok(results)
    }

    async fn list_with_options(
        &self,
        prefix: &StoragePath,
        options: ListOptions,
    ) -> Result<Vec<ObjectMeta>> {
        let mut results = self.list(prefix).await?;

        if let Some(limit) = options.limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    #[instrument(skip(self), fields(from = %from, to = %to))]
    async fn rename(&self, from: &StoragePath, to: &StoragePath) -> Result<()> {
        let from_path = self.to_object_path(from);
        let to_path = self.to_object_path(to);
        debug!("Renaming s3://{:?} to s3://{:?}", from_path, to_path);

        // S3 doesn't have native rename, so copy + delete
        match self.store.copy(&from_path, &to_path).await {
            Ok(()) => {}
            Err(object_store::Error::NotFound { .. }) => {
                return Err(StorageError::NotFound(from.to_string()));
            }
            Err(e) => return Err(StorageError::from(e)),
        }

        // Delete original after successful copy
        self.store.delete(&from_path).await.ok();

        Ok(())
    }

    #[instrument(skip(self), fields(from = %from, to = %to))]
    async fn copy(&self, from: &StoragePath, to: &StoragePath) -> Result<()> {
        let from_path = self.to_object_path(from);
        let to_path = self.to_object_path(to);
        debug!("Copying s3://{:?} to s3://{:?}", from_path, to_path);

        match self.store.copy(&from_path, &to_path).await {
            Ok(()) => Ok(()),
            Err(object_store::Error::NotFound { .. }) => {
                Err(StorageError::NotFound(from.to_string()))
            }
            Err(e) => Err(StorageError::from(e)),
        }
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn head(&self, path: &StoragePath) -> Result<ObjectMeta> {
        let obj_path = self.to_object_path(path);

        match self.store.head(&obj_path).await {
            Ok(meta) => Ok(ObjectMeta {
                path: path.clone(),
                size: meta.size as u64,
                last_modified: Some(meta.last_modified.timestamp()),
                etag: meta.e_tag,
            }),
            Err(object_store::Error::NotFound { .. }) => {
                Err(StorageError::NotFound(path.to_string()))
            }
            Err(e) => Err(StorageError::from(e)),
        }
    }

    fn backend_name(&self) -> &'static str {
        "s3"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::StorageBackend;

    #[test]
    fn test_s3_config_aws() {
        let config = S3Config::aws("my-bucket", "us-west-2");
        assert_eq!(config.bucket, "my-bucket");
        assert_eq!(config.region, "us-west-2");
        assert!(!config.force_path_style);
        assert!(config.endpoint.is_none());
    }

    #[test]
    fn test_s3_config_minio() {
        let config = S3Config::minio("local-bucket", "http://localhost:9000");
        assert_eq!(config.bucket, "local-bucket");
        assert!(config.force_path_style);
        assert_eq!(config.endpoint, Some("http://localhost:9000".to_string()));
    }

    #[test]
    fn test_to_object_path_no_prefix() {
        let storage = S3Storage {
            store: Arc::new(object_store::memory::InMemory::new()),
            prefix: String::new(),
        };

        let path = StoragePath::vector("products", "shard_0", "index.bin");
        let obj_path = storage.to_object_path(&path);

        assert_eq!(obj_path.as_ref(), "products/vector/shard_0/index.bin");
    }

    #[test]
    fn test_to_object_path_with_prefix() {
        let storage = S3Storage {
            store: Arc::new(object_store::memory::InMemory::new()),
            prefix: "collections/".to_string(),
        };

        let path = StoragePath::vector("products", "shard_0", "index.bin");
        let obj_path = storage.to_object_path(&path);

        assert_eq!(
            obj_path.as_ref(),
            "collections/products/vector/shard_0/index.bin"
        );
    }

    #[test]
    fn test_from_object_path() {
        let storage = S3Storage {
            store: Arc::new(object_store::memory::InMemory::new()),
            prefix: "data/".to_string(),
        };

        let obj_path = ObjectPath::from("data/products/vector/shard_0/index.bin");
        let storage_path = storage.from_object_path(&obj_path).unwrap();

        assert_eq!(storage_path.collection, "products");
        assert_eq!(storage_path.backend, StorageBackend::Vector);
        assert_eq!(storage_path.shard, Some("shard_0".to_string()));
        assert_eq!(storage_path.segment, "index.bin");
    }

    // Integration tests require actual S3/MinIO - run with:
    // cargo test -p prism-storage --features s3 -- --ignored
    #[tokio::test]
    #[ignore]
    async fn test_s3_integration() {
        let config = S3Config::minio("test-bucket", "http://localhost:9000")
            .with_credentials("minioadmin", "minioadmin");

        let storage = S3Storage::new(config).unwrap();
        let path = StoragePath::vector("test", "shard_0", "integration_test.bin");
        let data = Bytes::from("integration test data");

        // Write
        storage.write(&path, data.clone()).await.unwrap();

        // Read
        let read = storage.read(&path).await.unwrap();
        assert_eq!(read, data);

        // Exists
        assert!(storage.exists(&path).await.unwrap());

        // Delete
        storage.delete(&path).await.unwrap();
        assert!(!storage.exists(&path).await.unwrap());
    }
}
