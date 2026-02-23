//! Local filesystem storage implementation.
//!
//! This is the default storage backend for single-node Prism deployments.

use async_trait::async_trait;
use bytes::Bytes;
use std::path::PathBuf;
use tokio::fs;
use tracing::{debug, instrument};

use crate::error::{Result, StorageError};
use crate::path::StoragePath;
use crate::traits::{ListOptions, ObjectMeta, SegmentStorage, SegmentStorageSync};

/// Local filesystem storage backend.
///
/// Stores data in a hierarchical directory structure matching the `StoragePath` format:
/// `base_path/collection/backend/shard/segment`
#[derive(Debug, Clone)]
pub struct LocalStorage {
    base_path: PathBuf,
}

impl LocalStorage {
    /// Create a new local storage backend.
    ///
    /// The base path will be created if it doesn't exist.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    /// Get the base path for this storage.
    pub fn base_path(&self) -> &std::path::Path {
        &self.base_path
    }

    /// Convert a storage path to a filesystem path.
    fn to_fs_path(&self, path: &StoragePath) -> PathBuf {
        path.to_path_buf(&self.base_path)
    }

    /// Ensure parent directories exist for a path.
    async fn ensure_parent(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl SegmentStorage for LocalStorage {
    #[instrument(skip(self, data), fields(path = %path, size = data.len()))]
    async fn write(&self, path: &StoragePath, data: Bytes) -> Result<()> {
        let fs_path = self.to_fs_path(path);
        self.ensure_parent(&fs_path).await?;

        debug!("Writing {} bytes to {:?}", data.len(), fs_path);
        fs::write(&fs_path, &data).await?;
        Ok(())
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn read(&self, path: &StoragePath) -> Result<Bytes> {
        let fs_path = self.to_fs_path(path);
        debug!("Reading from {:?}", fs_path);

        match fs::read(&fs_path).await {
            Ok(data) => Ok(Bytes::from(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StorageError::NotFound(path.to_string()))
            }
            Err(e) => Err(e.into()),
        }
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn exists(&self, path: &StoragePath) -> Result<bool> {
        let fs_path = self.to_fs_path(path);
        Ok(fs_path.exists())
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn delete(&self, path: &StoragePath) -> Result<()> {
        let fs_path = self.to_fs_path(path);
        debug!("Deleting {:?}", fs_path);

        match fs::remove_file(&fs_path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    #[instrument(skip(self), fields(prefix = %prefix))]
    async fn list(&self, prefix: &StoragePath) -> Result<Vec<ObjectMeta>> {
        let fs_prefix = self.to_fs_path(prefix);
        let prefix_str = prefix.to_string();
        let mut results = Vec::new();

        // If the path is a file, return just that file
        if fs_prefix.is_file() {
            let metadata = fs::metadata(&fs_prefix).await?;
            results.push(ObjectMeta {
                path: prefix.clone(),
                size: metadata.len(),
                last_modified: metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64),
                etag: None,
            });
            return Ok(results);
        }

        // If it's a directory (or prefix path), list contents recursively
        let dir_path = if prefix.is_prefix() {
            fs_prefix
        } else {
            // Prefix is a partial path, use parent directory
            match fs_prefix.parent() {
                Some(p) => p.to_path_buf(),
                None => return Ok(results),
            }
        };

        if !dir_path.exists() {
            return Ok(results);
        }

        self.list_recursive(&dir_path, &prefix_str, &mut results)
            .await?;

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
        let from_path = self.to_fs_path(from);
        let to_path = self.to_fs_path(to);

        if !from_path.exists() {
            return Err(StorageError::NotFound(from.to_string()));
        }

        self.ensure_parent(&to_path).await?;
        debug!("Renaming {:?} to {:?}", from_path, to_path);
        fs::rename(&from_path, &to_path).await?;
        Ok(())
    }

    #[instrument(skip(self), fields(from = %from, to = %to))]
    async fn copy(&self, from: &StoragePath, to: &StoragePath) -> Result<()> {
        let from_path = self.to_fs_path(from);
        let to_path = self.to_fs_path(to);

        if !from_path.exists() {
            return Err(StorageError::NotFound(from.to_string()));
        }

        self.ensure_parent(&to_path).await?;
        debug!("Copying {:?} to {:?}", from_path, to_path);
        fs::copy(&from_path, &to_path).await?;
        Ok(())
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn head(&self, path: &StoragePath) -> Result<ObjectMeta> {
        let fs_path = self.to_fs_path(path);

        let metadata = match fs::metadata(&fs_path).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StorageError::NotFound(path.to_string()));
            }
            Err(e) => return Err(e.into()),
        };

        Ok(ObjectMeta {
            path: path.clone(),
            size: metadata.len(),
            last_modified: metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64),
            etag: None,
        })
    }

    fn backend_name(&self) -> &'static str {
        "local"
    }
}

impl LocalStorage {
    /// Recursively list files in a directory.
    #[async_recursion::async_recursion]
    async fn list_recursive(
        &self,
        dir: &std::path::Path,
        prefix: &str,
        results: &mut Vec<ObjectMeta>,
    ) -> Result<()> {
        let mut entries = fs::read_dir(dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let metadata = entry.metadata().await?;

            if metadata.is_dir() {
                // Use Box::pin for recursive async call
                Box::pin(self.list_recursive(&path, prefix, results)).await?;
            } else if metadata.is_file() {
                // Convert filesystem path back to StoragePath
                let relative = path
                    .strip_prefix(&self.base_path)
                    .map_err(|_| StorageError::InvalidPath(path.display().to_string()))?;
                let path_str = relative.to_string_lossy().to_string();

                // Only include if it matches the prefix
                if path_str.starts_with(prefix) || prefix.is_empty() {
                    if let Some(storage_path) = StoragePath::parse(&path_str) {
                        results.push(ObjectMeta {
                            path: storage_path,
                            size: metadata.len(),
                            last_modified: metadata
                                .modified()
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_secs() as i64),
                            etag: None,
                        });
                    }
                }
            }
        }

        Ok(())
    }
}

impl SegmentStorageSync for LocalStorage {
    fn write_sync(&self, path: &StoragePath, data: &[u8]) -> Result<()> {
        let fs_path = self.to_fs_path(path);
        if let Some(parent) = fs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&fs_path, data)?;
        Ok(())
    }

    fn read_sync(&self, path: &StoragePath) -> Result<Vec<u8>> {
        let fs_path = self.to_fs_path(path);
        match std::fs::read(&fs_path) {
            Ok(data) => Ok(data),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StorageError::NotFound(path.to_string()))
            }
            Err(e) => Err(e.into()),
        }
    }

    fn exists_sync(&self, path: &StoragePath) -> Result<bool> {
        let fs_path = self.to_fs_path(path);
        Ok(fs_path.exists())
    }

    fn delete_sync(&self, path: &StoragePath) -> Result<()> {
        let fs_path = self.to_fs_path(path);
        match std::fs::remove_file(&fs_path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn rename_sync(&self, from: &StoragePath, to: &StoragePath) -> Result<()> {
        let from_path = self.to_fs_path(from);
        let to_path = self.to_fs_path(to);

        if !from_path.exists() {
            return Err(StorageError::NotFound(from.to_string()));
        }

        if let Some(parent) = to_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&from_path, &to_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::StorageBackend;
    use tempfile::TempDir;

    async fn create_test_storage() -> (LocalStorage, TempDir) {
        let temp = TempDir::new().unwrap();
        let storage = LocalStorage::new(temp.path());
        (storage, temp)
    }

    #[tokio::test]
    async fn test_write_and_read() {
        let (storage, _temp) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "index.bin");
        let data = Bytes::from("hello world");

        storage.write(&path, data.clone()).await.unwrap();
        let read = storage.read(&path).await.unwrap();

        assert_eq!(read, data);
    }

    #[tokio::test]
    async fn test_read_not_found() {
        let (storage, _temp) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "nonexistent.bin");
        let result = storage.read(&path).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_exists() {
        let (storage, _temp) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "index.bin");

        assert!(!storage.exists(&path).await.unwrap());

        storage.write(&path, Bytes::from("data")).await.unwrap();

        assert!(storage.exists(&path).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete() {
        let (storage, _temp) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "index.bin");
        storage.write(&path, Bytes::from("data")).await.unwrap();

        assert!(storage.exists(&path).await.unwrap());

        storage.delete(&path).await.unwrap();

        assert!(!storage.exists(&path).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_idempotent() {
        let (storage, _temp) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "nonexistent.bin");

        // Should not error on non-existent file
        storage.delete(&path).await.unwrap();
    }

    #[tokio::test]
    async fn test_rename() {
        let (storage, _temp) = create_test_storage().await;

        let from = StoragePath::vector("test", "shard_0", "old.bin");
        let to = StoragePath::vector("test", "shard_0", "new.bin");
        let data = Bytes::from("test data");

        storage.write(&from, data.clone()).await.unwrap();
        storage.rename(&from, &to).await.unwrap();

        assert!(!storage.exists(&from).await.unwrap());
        assert!(storage.exists(&to).await.unwrap());
        assert_eq!(storage.read(&to).await.unwrap(), data);
    }

    #[tokio::test]
    async fn test_copy() {
        let (storage, _temp) = create_test_storage().await;

        let from = StoragePath::vector("test", "shard_0", "source.bin");
        let to = StoragePath::vector("test", "shard_1", "copy.bin");
        let data = Bytes::from("test data");

        storage.write(&from, data.clone()).await.unwrap();
        storage.copy(&from, &to).await.unwrap();

        assert!(storage.exists(&from).await.unwrap());
        assert!(storage.exists(&to).await.unwrap());
        assert_eq!(storage.read(&to).await.unwrap(), data);
    }

    #[tokio::test]
    async fn test_list() {
        let (storage, _temp) = create_test_storage().await;

        // Write some files
        storage
            .write(
                &StoragePath::vector("test", "shard_0", "a.bin"),
                Bytes::from("a"),
            )
            .await
            .unwrap();
        storage
            .write(
                &StoragePath::vector("test", "shard_0", "b.bin"),
                Bytes::from("bb"),
            )
            .await
            .unwrap();
        storage
            .write(
                &StoragePath::vector("test", "shard_1", "c.bin"),
                Bytes::from("ccc"),
            )
            .await
            .unwrap();

        // List all in collection
        let prefix = StoragePath::new("test", StorageBackend::Vector);
        let results = storage.list(&prefix).await.unwrap();

        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_head() {
        let (storage, _temp) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "index.bin");
        let data = Bytes::from("hello world");

        storage.write(&path, data.clone()).await.unwrap();
        let meta = storage.head(&path).await.unwrap();

        assert_eq!(meta.size, data.len() as u64);
        assert!(meta.last_modified.is_some());
    }

    #[tokio::test]
    async fn test_sync_operations() {
        let (storage, _temp) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "sync.bin");
        let data = b"sync test data";

        storage.write_sync(&path, data).unwrap();
        assert!(storage.exists_sync(&path).unwrap());

        let read = storage.read_sync(&path).unwrap();
        assert_eq!(read, data);

        storage.delete_sync(&path).unwrap();
        assert!(!storage.exists_sync(&path).unwrap());
    }

    #[tokio::test]
    async fn test_delete_prefix() {
        let (storage, _temp) = create_test_storage().await;

        // Write some files
        storage
            .write(
                &StoragePath::vector("test", "shard_0", "a.bin"),
                Bytes::from("a"),
            )
            .await
            .unwrap();
        storage
            .write(
                &StoragePath::vector("test", "shard_0", "b.bin"),
                Bytes::from("b"),
            )
            .await
            .unwrap();

        let prefix = StoragePath::new("test", StorageBackend::Vector).with_shard("shard_0");
        let deleted = storage.delete_prefix(&prefix).await.unwrap();

        assert_eq!(deleted, 2);

        let remaining = storage.list(&prefix).await.unwrap();
        assert!(remaining.is_empty());
    }

    #[tokio::test]
    async fn test_rename_not_found() {
        let (storage, _temp) = create_test_storage().await;
        let from = StoragePath::vector("test", "shard_0", "missing.bin");
        let to = StoragePath::vector("test", "shard_0", "new.bin");

        let result = storage.rename(&from, &to).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_head_not_found() {
        let (storage, _temp) = create_test_storage().await;
        let path = StoragePath::vector("test", "shard_0", "missing.bin");

        let result = storage.head(&path).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_copy_not_found() {
        let (storage, _temp) = create_test_storage().await;
        let from = StoragePath::vector("test", "shard_0", "missing.bin");
        let to = StoragePath::vector("test", "shard_0", "copy.bin");

        let result = storage.copy(&from, &to).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StorageError::NotFound(_)));
    }

    #[test]
    fn test_sync_read_not_found() {
        let temp = TempDir::new().unwrap();
        let storage = LocalStorage::new(temp.path());
        let path = StoragePath::vector("test", "shard_0", "missing.bin");

        let result = storage.read_sync(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StorageError::NotFound(_)));
    }

    #[test]
    fn test_sync_rename_not_found() {
        let temp = TempDir::new().unwrap();
        let storage = LocalStorage::new(temp.path());
        let from = StoragePath::vector("test", "shard_0", "missing.bin");
        let to = StoragePath::vector("test", "shard_0", "new.bin");

        let result = storage.rename_sync(&from, &to);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StorageError::NotFound(_)));
    }

    #[test]
    fn test_backend_name() {
        let temp = TempDir::new().unwrap();
        let storage = LocalStorage::new(temp.path());
        assert_eq!(storage.backend_name(), "local");
    }

    #[test]
    fn test_base_path() {
        let temp = TempDir::new().unwrap();
        let storage = LocalStorage::new(temp.path());
        assert_eq!(storage.base_path(), temp.path());
    }

    #[tokio::test]
    async fn test_list_empty_prefix() {
        let (storage, _temp) = create_test_storage().await;

        storage
            .write(
                &StoragePath::vector("test", "shard_0", "a.bin"),
                Bytes::from("a"),
            )
            .await
            .unwrap();

        let prefix = StoragePath::new("test", StorageBackend::Vector).with_shard("shard_0");
        let results = storage.list(&prefix).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].size, 1);
    }

    #[tokio::test]
    async fn test_list_with_options_limit() {
        let (storage, _temp) = create_test_storage().await;

        for i in 0..5 {
            storage
                .write(
                    &StoragePath::vector("test", "shard_0", &format!("f{}.bin", i)),
                    Bytes::from("x"),
                )
                .await
                .unwrap();
        }

        let prefix = StoragePath::new("test", StorageBackend::Vector);
        let opts = ListOptions {
            limit: Some(3),
            ..Default::default()
        };
        let results = storage.list_with_options(&prefix, opts).await.unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_sync_delete_idempotent() {
        let temp = TempDir::new().unwrap();
        let storage = LocalStorage::new(temp.path());
        let path = StoragePath::vector("test", "shard_0", "missing.bin");
        // Should not error
        storage.delete_sync(&path).unwrap();
    }

    #[tokio::test]
    async fn test_overwrite() {
        let (storage, _temp) = create_test_storage().await;
        let path = StoragePath::vector("test", "shard_0", "data.bin");

        storage
            .write(&path, Bytes::from("version 1"))
            .await
            .unwrap();
        storage
            .write(&path, Bytes::from("version 2"))
            .await
            .unwrap();

        let read = storage.read(&path).await.unwrap();
        assert_eq!(read, Bytes::from("version 2"));
    }
}
