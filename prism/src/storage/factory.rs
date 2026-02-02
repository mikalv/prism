//! Factory for creating unified SegmentStorage backends.

use std::path::PathBuf;
use std::sync::Arc;

use prism_storage::{LocalStorage, SegmentStorage};

use crate::storage::StorageConfig;
use crate::Error;

/// Factory for creating storage backends.
///
/// All backends (Text, Vector, Graph) use SegmentStorage for persistence.
pub struct StorageFactory;

impl StorageFactory {
    /// Create a unified SegmentStorage from configuration.
    ///
    /// This is the only method needed - all backends use SegmentStorage directly.
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

                if let (Some(key_id), Some(secret)) = (&s3.access_key_id, &s3.secret_access_key) {
                    s3_config = s3_config.with_credentials(key_id, secret);
                }

                let storage =
                    S3Storage::new(s3_config).map_err(|e| Error::Storage(e.to_string()))?;
                Ok(Arc::new(storage))
            }
            #[cfg(not(feature = "storage-s3"))]
            StorageConfig::S3(_) => Err(Error::Config(
                "S3 storage requires 'storage-s3' feature".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::LocalConfig;
    use tempfile::TempDir;

    #[test]
    fn test_create_segment_storage_local() {
        let temp = TempDir::new().unwrap();
        let config = StorageConfig::default();

        let storage = StorageFactory::create_segment_storage(&config, &temp.path().to_path_buf());
        assert!(storage.is_ok());
        assert_eq!(storage.unwrap().backend_name(), "local");
    }

    #[test]
    fn test_create_segment_storage_with_path() {
        let temp = TempDir::new().unwrap();
        let config = StorageConfig::Local(LocalConfig {
            path: Some(temp.path().to_string_lossy().to_string()),
        });

        let storage = StorageFactory::create_segment_storage(&config, &PathBuf::from("/fallback"));
        assert!(storage.is_ok());
    }

    #[tokio::test]
    async fn test_segment_storage_roundtrip() {
        use prism_storage::{Bytes, StoragePath};

        let temp = TempDir::new().unwrap();
        let config = StorageConfig::default();

        let storage =
            StorageFactory::create_segment_storage(&config, &temp.path().to_path_buf()).unwrap();

        let path = StoragePath::vector("test", "default", "data.bin");
        let data = Bytes::from_static(b"test data");

        storage.write(&path, data.clone()).await.unwrap();

        let loaded = storage.read(&path).await.unwrap();
        assert_eq!(loaded, data);
    }
}
