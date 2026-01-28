use async_trait::async_trait;
use std::path::PathBuf;

use crate::Error;

#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn save(&self, collection: &str, data: &[u8]) -> Result<(), Error>;
    async fn load(&self, collection: &str) -> Result<Option<Vec<u8>>, Error>;
    async fn delete(&self, collection: &str) -> Result<(), Error>;
}

pub struct LocalVectorStore {
    base_path: PathBuf,
}

impl LocalVectorStore {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    fn index_path(&self, collection: &str) -> PathBuf {
        self.base_path.join(collection).join("vector_index.json")
    }
}

#[async_trait]
impl VectorStore for LocalVectorStore {
    async fn save(&self, collection: &str, data: &[u8]) -> Result<(), Error> {
        let path = self.index_path(collection);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, data).await?;
        Ok(())
    }

    async fn load(&self, collection: &str) -> Result<Option<Vec<u8>>, Error> {
        let path = self.index_path(collection);
        match tokio::fs::read(&path).await {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn delete(&self, collection: &str) -> Result<(), Error> {
        let path = self.index_path(collection);
        tokio::fs::remove_file(path).await.ok();
        Ok(())
    }
}

#[cfg(feature = "storage-s3")]
pub struct S3VectorStore {
    store: std::sync::Arc<dyn object_store::ObjectStore>,
    prefix: String,
}

#[cfg(feature = "storage-s3")]
impl S3VectorStore {
    pub fn new(store: std::sync::Arc<dyn object_store::ObjectStore>, prefix: String) -> Self {
        Self { store, prefix }
    }

    fn index_path(&self, collection: &str) -> object_store::path::Path {
        object_store::path::Path::from(format!(
            "{}{}/vector_index.json",
            self.prefix, collection
        ))
    }
}

#[cfg(feature = "storage-s3")]
#[async_trait]
impl VectorStore for S3VectorStore {
    async fn save(&self, collection: &str, data: &[u8]) -> Result<(), Error> {
        let path = self.index_path(collection);
        self.store
            .put(&path, bytes::Bytes::copy_from_slice(data).into())
            .await
            .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    async fn load(&self, collection: &str) -> Result<Option<Vec<u8>>, Error> {
        let path = self.index_path(collection);
        match self.store.get(&path).await {
            Ok(result) => {
                let bytes = result
                    .bytes()
                    .await
                    .map_err(|e| Error::Storage(e.to_string()))?;
                Ok(Some(bytes.to_vec()))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(Error::Storage(e.to_string())),
        }
    }

    async fn delete(&self, collection: &str) -> Result<(), Error> {
        let path = self.index_path(collection);
        self.store.delete(&path).await.ok();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_local_vector_store_roundtrip() {
        let temp = TempDir::new().unwrap();
        let store = LocalVectorStore::new(temp.path());

        let data = b"test vector index data";
        store.save("test-collection", data).await.unwrap();

        let loaded = store.load("test-collection").await.unwrap();
        assert_eq!(loaded, Some(data.to_vec()));
    }

    #[tokio::test]
    async fn test_local_vector_store_not_found() {
        let temp = TempDir::new().unwrap();
        let store = LocalVectorStore::new(temp.path());

        let loaded = store.load("nonexistent").await.unwrap();
        assert_eq!(loaded, None);
    }
}
