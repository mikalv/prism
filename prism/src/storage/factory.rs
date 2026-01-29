//! Factory for creating storage backends.

use std::path::PathBuf;
use std::sync::Arc;

use prism_storage::{LocalStorage, SegmentStorage};

use crate::storage::{
    create_vector_store_from_segment_storage, LocalVectorStore, StorageConfig, VectorStore,
};
use crate::Error;

pub struct StorageFactory;

impl StorageFactory {
    /// Create a VectorStore from the legacy StorageConfig.
    ///
    /// This uses the old LocalVectorStore/S3VectorStore implementations.
    /// For new code, prefer `create_segment_storage` + `create_vector_store_from_segment_storage`.
    pub fn create_vector_store(
        config: &StorageConfig,
        base_path: &PathBuf,
    ) -> Result<Arc<dyn VectorStore>, Error> {
        match config {
            StorageConfig::Local(local) => {
                let path = local
                    .path
                    .as_ref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| base_path.clone());
                Ok(Arc::new(LocalVectorStore::new(path)))
            }
            #[cfg(feature = "storage-s3")]
            StorageConfig::S3(s3) => {
                use crate::storage::S3VectorStore;
                use object_store::aws::AmazonS3Builder;

                let mut builder = AmazonS3Builder::new()
                    .with_bucket_name(&s3.bucket)
                    .with_region(&s3.region)
                    .with_allow_http(true);

                if let Some(endpoint) = &s3.endpoint {
                    builder = builder.with_endpoint(endpoint);
                }
                if s3.force_path_style {
                    builder = builder.with_virtual_hosted_style_request(false);
                }

                // Use explicit credentials if provided
                if let (Some(key_id), Some(secret)) =
                    (&s3.access_key_id, &s3.secret_access_key)
                {
                    builder = builder
                        .with_access_key_id(key_id)
                        .with_secret_access_key(secret);
                }

                let store = Arc::new(builder.build().map_err(|e| Error::Storage(e.to_string()))?);

                let prefix = s3.prefix.clone().unwrap_or_default();
                Ok(Arc::new(S3VectorStore::new(store, prefix)))
            }
            #[cfg(not(feature = "storage-s3"))]
            StorageConfig::S3(_) => Err(Error::Config(
                "S3 storage requires 'storage-s3' feature".into(),
            )),
        }
    }

    /// Create a unified SegmentStorage from configuration.
    ///
    /// This is the preferred method for new code.
    pub fn create_segment_storage(
        config: &StorageConfig,
        base_path: &PathBuf,
    ) -> Result<Arc<dyn SegmentStorage>, Error> {
        match config {
            StorageConfig::Local(local) => {
                let path = local
                    .path
                    .as_ref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| base_path.clone());
                Ok(Arc::new(LocalStorage::new(path)))
            }
            #[cfg(feature = "storage-s3")]
            StorageConfig::S3(s3) => {
                use prism_storage::{S3Config as PrismS3Config, S3Storage};

                let mut s3_config = if s3.endpoint.is_some() {
                    PrismS3Config::minio(&s3.bucket, s3.endpoint.as_ref().unwrap())
                } else {
                    PrismS3Config::aws(&s3.bucket, &s3.region)
                };

                if let Some(prefix) = &s3.prefix {
                    s3_config = s3_config.with_prefix(prefix);
                }

                if let (Some(key_id), Some(secret)) =
                    (&s3.access_key_id, &s3.secret_access_key)
                {
                    s3_config = s3_config.with_credentials(key_id, secret);
                }

                let storage = S3Storage::new(s3_config).map_err(|e| Error::Storage(e.to_string()))?;
                Ok(Arc::new(storage))
            }
            #[cfg(not(feature = "storage-s3"))]
            StorageConfig::S3(_) => Err(Error::Config(
                "S3 storage requires 'storage-s3' feature".into(),
            )),
        }
    }

    /// Create a VectorStore backed by SegmentStorage.
    ///
    /// This is the migration path from legacy VectorStore to unified SegmentStorage.
    pub fn create_vector_store_v2(
        config: &StorageConfig,
        base_path: &PathBuf,
    ) -> Result<Arc<dyn VectorStore>, Error> {
        let segment_storage = Self::create_segment_storage(config, base_path)?;
        Ok(create_vector_store_from_segment_storage(segment_storage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::LocalConfig;
    use tempfile::TempDir;

    #[test]
    fn test_create_local_vector_store() {
        let temp = TempDir::new().unwrap();
        let config = StorageConfig::Local(LocalConfig {
            path: Some(temp.path().to_string_lossy().to_string()),
        });

        let store = StorageFactory::create_vector_store(&config, &temp.path().to_path_buf());
        assert!(store.is_ok());
    }

    #[test]
    fn test_create_default_local_store() {
        let temp = TempDir::new().unwrap();
        let config = StorageConfig::default();

        let store = StorageFactory::create_vector_store(&config, &temp.path().to_path_buf());
        assert!(store.is_ok());
    }

    #[test]
    fn test_create_segment_storage() {
        let temp = TempDir::new().unwrap();
        let config = StorageConfig::default();

        let storage = StorageFactory::create_segment_storage(&config, &temp.path().to_path_buf());
        assert!(storage.is_ok());
        assert_eq!(storage.unwrap().backend_name(), "local");
    }

    #[test]
    fn test_create_vector_store_v2() {
        let temp = TempDir::new().unwrap();
        let config = StorageConfig::default();

        let store = StorageFactory::create_vector_store_v2(&config, &temp.path().to_path_buf());
        assert!(store.is_ok());
    }

    #[tokio::test]
    async fn test_vector_store_v2_roundtrip() {
        let temp = TempDir::new().unwrap();
        let config = StorageConfig::default();

        let store = StorageFactory::create_vector_store_v2(&config, &temp.path().to_path_buf())
            .unwrap();

        let data = b"test data";
        store.save("test-collection", data).await.unwrap();

        let loaded = store.load("test-collection").await.unwrap();
        assert_eq!(loaded, Some(data.to_vec()));
    }
}
