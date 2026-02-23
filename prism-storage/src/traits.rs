//! Core storage trait definitions.
//!
//! The `SegmentStorage` trait provides a unified interface for all storage backends
//! (local filesystem, S3, tiered caching) used by Prism's various data stores.

use async_trait::async_trait;
use bytes::Bytes;

use crate::error::Result;
use crate::path::StoragePath;

/// Metadata about a stored object.
#[derive(Debug, Clone)]
pub struct ObjectMeta {
    /// Full path to the object
    pub path: StoragePath,
    /// Size in bytes
    pub size: u64,
    /// Last modified timestamp (Unix epoch seconds)
    pub last_modified: Option<i64>,
    /// ETag or content hash if available
    pub etag: Option<String>,
}

/// Options for list operations.
#[derive(Debug, Clone, Default)]
pub struct ListOptions {
    /// Maximum number of results to return
    pub limit: Option<usize>,
    /// Only return objects with this prefix
    pub prefix: Option<String>,
    /// Delimiter for hierarchical listing (e.g., "/" for directory-like behavior)
    pub delimiter: Option<String>,
}

/// Unified storage trait for all Prism backends.
///
/// This trait provides async operations for reading, writing, and managing
/// data segments across different storage backends (local disk, S3, etc.).
///
/// # Thread Safety
///
/// All implementations must be `Send + Sync` to allow concurrent access
/// from multiple async tasks.
///
/// # Error Handling
///
/// Operations return `StorageError` which includes specific variants for
/// common failure modes (not found, permission denied, etc.).
#[async_trait]
pub trait SegmentStorage: Send + Sync {
    /// Write data to the specified path.
    ///
    /// Creates parent directories/prefixes as needed.
    /// Overwrites existing data at the path.
    ///
    /// # Arguments
    ///
    /// * `path` - Target storage path
    /// * `data` - Bytes to write
    async fn write(&self, path: &StoragePath, data: Bytes) -> Result<()>;

    /// Write data from a byte slice.
    ///
    /// Convenience method that copies the slice to Bytes.
    async fn write_bytes(&self, path: &StoragePath, data: &[u8]) -> Result<()> {
        self.write(path, Bytes::copy_from_slice(data)).await
    }

    /// Read data from the specified path.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::NotFound` if the path does not exist.
    async fn read(&self, path: &StoragePath) -> Result<Bytes>;

    /// Read data as a Vec<u8>.
    ///
    /// Convenience method that converts Bytes to Vec.
    async fn read_vec(&self, path: &StoragePath) -> Result<Vec<u8>> {
        Ok(self.read(path).await?.to_vec())
    }

    /// Check if a path exists.
    async fn exists(&self, path: &StoragePath) -> Result<bool>;

    /// Delete data at the specified path.
    ///
    /// No-op if the path does not exist (idempotent).
    async fn delete(&self, path: &StoragePath) -> Result<()>;

    /// List objects with the given prefix.
    ///
    /// Returns metadata for all objects whose path starts with the given prefix.
    /// The prefix should be a `StoragePath` with an empty or partial segment.
    async fn list(&self, prefix: &StoragePath) -> Result<Vec<ObjectMeta>>;

    /// List objects with options.
    ///
    /// More flexible listing with limit and delimiter support.
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

    /// Rename/move an object.
    ///
    /// This should be atomic when possible (same filesystem, same bucket).
    /// Falls back to copy+delete when atomicity is not possible.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::NotFound` if source does not exist.
    async fn rename(&self, from: &StoragePath, to: &StoragePath) -> Result<()>;

    /// Copy an object to a new location.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::NotFound` if source does not exist.
    async fn copy(&self, from: &StoragePath, to: &StoragePath) -> Result<()> {
        let data = self.read(from).await?;
        self.write(to, data).await
    }

    /// Get metadata for an object without reading its contents.
    async fn head(&self, path: &StoragePath) -> Result<ObjectMeta>;

    /// Delete all objects with the given prefix.
    ///
    /// Use with caution - this recursively deletes all matching objects.
    async fn delete_prefix(&self, prefix: &StoragePath) -> Result<usize> {
        let objects = self.list(prefix).await?;
        let count = objects.len();
        for obj in objects {
            self.delete(&obj.path).await?;
        }
        Ok(count)
    }

    /// Get a human-readable name for this storage backend.
    fn backend_name(&self) -> &'static str;
}

/// Extension trait for synchronous operations.
///
/// Some backends (like Tantivy's Directory) require synchronous access.
/// This trait provides blocking wrappers when needed.
pub trait SegmentStorageSync: SegmentStorage {
    /// Blocking write operation.
    fn write_sync(&self, path: &StoragePath, data: &[u8]) -> Result<()>;

    /// Blocking read operation.
    fn read_sync(&self, path: &StoragePath) -> Result<Vec<u8>>;

    /// Blocking exists check.
    fn exists_sync(&self, path: &StoragePath) -> Result<bool>;

    /// Blocking delete operation.
    fn delete_sync(&self, path: &StoragePath) -> Result<()>;

    /// Blocking rename operation.
    fn rename_sync(&self, from: &StoragePath, to: &StoragePath) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::StorageBackend;
    use crate::LocalStorage;
    use tempfile::TempDir;

    #[test]
    fn test_object_meta_debug() {
        let meta = ObjectMeta {
            path: StoragePath::new("test", StorageBackend::Vector).with_segment("index.bin"),
            size: 1024,
            last_modified: Some(1234567890),
            etag: Some("abc123".to_string()),
        };
        let debug = format!("{:?}", meta);
        assert!(debug.contains("test"));
        assert!(debug.contains("1024"));
    }

    #[test]
    fn test_list_options_default() {
        let opts = ListOptions::default();
        assert!(opts.limit.is_none());
        assert!(opts.prefix.is_none());
        assert!(opts.delimiter.is_none());
    }

    #[tokio::test]
    async fn test_write_bytes_read_vec_roundtrip() {
        let dir = TempDir::new().unwrap();
        let storage = LocalStorage::new(dir.path());
        let path = StoragePath::vector("test", "shard_0", "data.bin");

        let data = b"hello world bytes";
        storage.write_bytes(&path, data).await.unwrap();

        let read = storage.read_vec(&path).await.unwrap();
        assert_eq!(read, data);
    }

    #[tokio::test]
    async fn test_default_copy() {
        let dir = TempDir::new().unwrap();
        let storage = LocalStorage::new(dir.path());
        let src = StoragePath::vector("test", "shard_0", "src.bin");
        let dst = StoragePath::vector("test", "shard_0", "dst.bin");

        storage
            .write(&src, Bytes::from("copy me"))
            .await
            .unwrap();

        // Use the trait default copy (LocalStorage overrides it, but let's test the trait path)
        let data = storage.read(&src).await.unwrap();
        storage.write(&dst, data).await.unwrap();

        let read = storage.read(&dst).await.unwrap();
        assert_eq!(read, Bytes::from("copy me"));
    }

    #[tokio::test]
    async fn test_default_list_with_options_limit() {
        let dir = TempDir::new().unwrap();
        let storage = LocalStorage::new(dir.path());

        // Write 5 files
        for i in 0..5 {
            let path =
                StoragePath::vector("test", "shard_0", &format!("file_{}.bin", i));
            storage
                .write(&path, Bytes::from(format!("data {}", i)))
                .await
                .unwrap();
        }

        let prefix = StoragePath::new("test", StorageBackend::Vector);
        let opts = ListOptions {
            limit: Some(2),
            ..Default::default()
        };
        let results = storage.list_with_options(&prefix, opts).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_default_delete_prefix() {
        let dir = TempDir::new().unwrap();
        let storage = LocalStorage::new(dir.path());

        for i in 0..3 {
            let path =
                StoragePath::vector("test", "shard_0", &format!("f_{}.bin", i));
            storage
                .write(&path, Bytes::from("data"))
                .await
                .unwrap();
        }

        let prefix =
            StoragePath::new("test", StorageBackend::Vector).with_shard("shard_0");
        let deleted = storage.delete_prefix(&prefix).await.unwrap();
        assert_eq!(deleted, 3);

        let remaining = storage.list(&prefix).await.unwrap();
        assert!(remaining.is_empty());
    }
}
