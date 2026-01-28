use std::path::PathBuf;
use std::sync::Arc;

use crate::storage::{LocalVectorStore, StorageConfig, VectorStore};
use crate::Error;

pub struct StorageFactory;

impl StorageFactory {
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
}
