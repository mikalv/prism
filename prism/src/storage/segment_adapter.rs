//! Adapter that bridges SegmentStorage to the legacy VectorStore trait.
//!
//! This allows gradual migration from VectorStore to SegmentStorage.

use async_trait::async_trait;
use prism_storage::{Bytes, SegmentStorage, StoragePath};
use std::sync::Arc;

use crate::Error;

use super::VectorStore;

/// Adapter that implements VectorStore using SegmentStorage.
///
/// Uses a fixed path pattern: `{collection}/vector/default/vector_index.json`
pub struct SegmentStorageVectorAdapter {
    storage: Arc<dyn SegmentStorage>,
}

impl SegmentStorageVectorAdapter {
    /// Create a new adapter wrapping a SegmentStorage implementation.
    pub fn new(storage: Arc<dyn SegmentStorage>) -> Self {
        Self { storage }
    }

    /// Get the storage path for a collection's vector index.
    fn index_path(collection: &str) -> StoragePath {
        StoragePath::vector(collection, "default", "vector_index.json")
    }
}

#[async_trait]
impl VectorStore for SegmentStorageVectorAdapter {
    async fn save(&self, collection: &str, data: &[u8]) -> Result<(), Error> {
        let path = Self::index_path(collection);
        self.storage
            .write(&path, Bytes::copy_from_slice(data))
            .await
            .map_err(|e| Error::Storage(e.to_string()))
    }

    async fn load(&self, collection: &str) -> Result<Option<Vec<u8>>, Error> {
        let path = Self::index_path(collection);
        match self.storage.read(&path).await {
            Ok(data) => Ok(Some(data.to_vec())),
            Err(prism_storage::StorageError::NotFound(_)) => Ok(None),
            Err(e) => Err(Error::Storage(e.to_string())),
        }
    }

    async fn delete(&self, collection: &str) -> Result<(), Error> {
        let path = Self::index_path(collection);
        self.storage
            .delete(&path)
            .await
            .map_err(|e| Error::Storage(e.to_string()))
    }
}

/// Create a VectorStore from SegmentStorage configuration.
pub fn create_vector_store_from_segment_storage(
    storage: Arc<dyn SegmentStorage>,
) -> Arc<dyn VectorStore> {
    Arc::new(SegmentStorageVectorAdapter::new(storage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_storage::LocalStorage;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_segment_adapter_roundtrip() {
        let temp = TempDir::new().unwrap();
        let storage = Arc::new(LocalStorage::new(temp.path()));
        let adapter = SegmentStorageVectorAdapter::new(storage);

        let data = b"test vector index data";
        adapter.save("test-collection", data).await.unwrap();

        let loaded = adapter.load("test-collection").await.unwrap();
        assert_eq!(loaded, Some(data.to_vec()));
    }

    #[tokio::test]
    async fn test_segment_adapter_not_found() {
        let temp = TempDir::new().unwrap();
        let storage = Arc::new(LocalStorage::new(temp.path()));
        let adapter = SegmentStorageVectorAdapter::new(storage);

        let loaded = adapter.load("nonexistent").await.unwrap();
        assert_eq!(loaded, None);
    }

    #[tokio::test]
    async fn test_segment_adapter_delete() {
        let temp = TempDir::new().unwrap();
        let storage = Arc::new(LocalStorage::new(temp.path()));
        let adapter = SegmentStorageVectorAdapter::new(storage);

        adapter.save("test", b"data").await.unwrap();
        assert!(adapter.load("test").await.unwrap().is_some());

        adapter.delete("test").await.unwrap();
        assert!(adapter.load("test").await.unwrap().is_none());
    }
}
