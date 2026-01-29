//! Tantivy Directory adapter for SegmentStorage.
//!
//! Bridges the async `SegmentStorage` trait with Tantivy's synchronous `Directory` trait.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────┐
//! │  Tantivy IndexWriter│
//! └──────────┬──────────┘
//!            │ (sync calls)
//!            ▼
//! ┌─────────────────────┐
//! │ TantivyStorageAdapter│
//! │  (Directory impl)   │
//! └──────────┬──────────┘
//!            │ (block_on)
//!            ▼
//! ┌─────────────────────┐
//! │   SegmentStorage    │
//! │  (async trait)      │
//! └─────────────────────┘
//! ```
//!
//! # Write Strategy
//!
//! Writes are buffered locally and uploaded when `terminate()` is called on the write handle.
//! This matches Tantivy's expected behavior where segment files are written completely
//! before being committed.

use std::io::{self, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use tantivy::directory::error::{DeleteError, LockError, OpenReadError, OpenWriteError};
use tantivy::directory::{
    AntiCallToken, Directory, DirectoryLock, FileHandle, Lock, OwnedBytes, TerminatingWrite,
    WatchCallback, WatchHandle, WritePtr,
};
use tantivy::HasLen;
use tokio::runtime::Runtime;
use tracing::debug;

use crate::error::StorageError;
use crate::path::StoragePath;
use crate::traits::SegmentStorage;

/// Tantivy Directory adapter that uses SegmentStorage as backend.
///
/// Implements Tantivy's `Directory` trait by delegating to a `SegmentStorage` implementation.
/// Handles the async/sync bridge using a Tokio runtime.
pub struct TantivyStorageAdapter {
    /// The underlying storage backend
    storage: Arc<dyn SegmentStorage>,
    /// Collection name for path construction
    collection: String,
    /// Shard identifier for path construction
    shard: String,
    /// Tokio runtime for blocking on async operations (shared via Arc)
    runtime: Arc<Runtime>,
    /// Local buffer directory for write operations
    buffer_dir: PathBuf,
    /// Lock for coordinating atomic operations
    atomic_lock: Mutex<()>,
    /// Cache of known files for faster exists checks
    file_cache: RwLock<std::collections::HashSet<String>>,
}

impl TantivyStorageAdapter {
    /// Create a new Tantivy storage adapter.
    ///
    /// # Arguments
    ///
    /// * `storage` - The underlying SegmentStorage backend
    /// * `collection` - Collection name for path construction
    /// * `shard` - Shard identifier for path construction
    /// * `buffer_dir` - Local directory for buffering writes
    pub fn new(
        storage: Arc<dyn SegmentStorage>,
        collection: impl Into<String>,
        shard: impl Into<String>,
        buffer_dir: impl Into<PathBuf>,
    ) -> io::Result<Self> {
        let buffer_dir = buffer_dir.into();
        std::fs::create_dir_all(&buffer_dir)?;

        // Create a dedicated runtime for this adapter
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?,
        );

        Ok(Self {
            storage,
            collection: collection.into(),
            shard: shard.into(),
            runtime,
            buffer_dir,
            atomic_lock: Mutex::new(()),
            file_cache: RwLock::new(std::collections::HashSet::new()),
        })
    }

    /// Create with an explicit Tokio runtime.
    pub fn with_runtime(
        storage: Arc<dyn SegmentStorage>,
        collection: impl Into<String>,
        shard: impl Into<String>,
        buffer_dir: impl Into<PathBuf>,
        runtime: Arc<Runtime>,
    ) -> io::Result<Self> {
        let buffer_dir = buffer_dir.into();
        std::fs::create_dir_all(&buffer_dir)?;

        Ok(Self {
            storage,
            collection: collection.into(),
            shard: shard.into(),
            runtime,
            buffer_dir,
            atomic_lock: Mutex::new(()),
            file_cache: RwLock::new(std::collections::HashSet::new()),
        })
    }

    /// Convert a Tantivy path to a StoragePath.
    fn to_storage_path(&self, path: &Path) -> StoragePath {
        let segment = path.to_string_lossy().to_string();
        StoragePath::tantivy(&self.collection, &self.shard, segment)
    }

    /// Read file size from storage.
    fn get_file_size(&self, path: &Path) -> Result<usize, OpenReadError> {
        let storage_path = self.to_storage_path(path);

        self.runtime
            .block_on(async { self.storage.head(&storage_path).await })
            .map(|meta| meta.size as usize)
            .map_err(|e| match e {
                StorageError::NotFound(_) => OpenReadError::FileDoesNotExist(path.to_path_buf()),
                _ => OpenReadError::wrap_io_error(
                    io::Error::new(io::ErrorKind::Other, e.to_string()),
                    path.to_path_buf(),
                ),
            })
    }
}

impl std::fmt::Debug for TantivyStorageAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TantivyStorageAdapter")
            .field("collection", &self.collection)
            .field("shard", &self.shard)
            .field("backend", &self.storage.backend_name())
            .finish()
    }
}

impl Clone for TantivyStorageAdapter {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            collection: self.collection.clone(),
            shard: self.shard.clone(),
            runtime: self.runtime.clone(), // Arc clone - shares the same runtime
            buffer_dir: self.buffer_dir.clone(),
            atomic_lock: Mutex::new(()),
            file_cache: RwLock::new(self.file_cache.read().clone()),
        }
    }
}

/// File handle for reading from SegmentStorage.
struct StorageFileHandle {
    storage: Arc<dyn SegmentStorage>,
    path: StoragePath,
    len: usize,
    runtime: Arc<Runtime>,
}

impl StorageFileHandle {
    fn new(
        storage: Arc<dyn SegmentStorage>,
        path: StoragePath,
        len: usize,
        runtime: Arc<Runtime>,
    ) -> Self {
        Self {
            storage,
            path,
            len,
            runtime,
        }
    }
}

impl std::fmt::Debug for StorageFileHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageFileHandle")
            .field("path", &self.path.to_string())
            .field("len", &self.len)
            .finish()
    }
}

impl HasLen for StorageFileHandle {
    fn len(&self) -> usize {
        self.len
    }
}

impl FileHandle for StorageFileHandle {
    fn read_bytes(&self, range: Range<usize>) -> io::Result<OwnedBytes> {
        self.runtime.block_on(async {
            let data = self
                .storage
                .read(&self.path)
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

            // Extract the requested range
            if range.end > data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!("Range {:?} exceeds file size {}", range, data.len()),
                ));
            }

            let slice = &data[range];
            Ok(OwnedBytes::new(slice.to_vec()))
        })
    }
}

/// Write handle that buffers locally and uploads on terminate.
struct StorageWriteHandle {
    storage: Arc<dyn SegmentStorage>,
    path: StoragePath,
    local_path: PathBuf,
    file: std::fs::File,
    shutdown: AtomicBool,
    runtime: Arc<Runtime>,
}

impl StorageWriteHandle {
    fn new(
        storage: Arc<dyn SegmentStorage>,
        path: StoragePath,
        buffer_dir: &Path,
        runtime: Arc<Runtime>,
    ) -> io::Result<Self> {
        let local_path = buffer_dir.join(path.to_string().replace('/', "_"));

        // Create parent directories
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = std::fs::File::create(&local_path)?;

        Ok(Self {
            storage,
            path,
            local_path,
            file,
            shutdown: AtomicBool::new(false),
            runtime,
        })
    }
}

impl Write for StorageWriteHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "write handle has been shutdown",
            ));
        }
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "write handle has been shutdown",
            ));
        }
        self.file.flush()
    }
}

impl TerminatingWrite for StorageWriteHandle {
    fn terminate_ref(&mut self, _: AntiCallToken) -> io::Result<()> {
        self.flush()?;
        self.shutdown.store(true, Ordering::SeqCst);

        // Read the buffered file and upload to storage
        let data = std::fs::read(&self.local_path)?;
        debug!(
            "Uploading {} bytes to {}",
            data.len(),
            self.path.to_string()
        );

        self.runtime.block_on(async {
            self.storage
                .write(&self.path, Bytes::from(data))
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
        })?;

        // Clean up local buffer
        let _ = std::fs::remove_file(&self.local_path);

        Ok(())
    }
}

/// No-op directory lock for storage backends.
/// Concurrency is handled at a higher level (collection locks, etc.).
struct NoOpLock;

impl Directory for TantivyStorageAdapter {
    fn get_file_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>, OpenReadError> {
        let storage_path = self.to_storage_path(path);
        let len = self.get_file_size(path)?;

        Ok(Arc::new(StorageFileHandle::new(
            self.storage.clone(),
            storage_path,
            len,
            self.runtime.clone(),
        )))
    }

    fn exists(&self, path: &Path) -> Result<bool, OpenReadError> {
        // Check cache first
        {
            let cache = self.file_cache.read();
            let path_str = path.to_string_lossy().to_string();
            if cache.contains(&path_str) {
                return Ok(true);
            }
        }

        let storage_path = self.to_storage_path(path);

        self.runtime
            .block_on(async { self.storage.exists(&storage_path).await })
            .map_err(|e| {
                OpenReadError::wrap_io_error(
                    io::Error::new(io::ErrorKind::Other, e.to_string()),
                    path.to_path_buf(),
                )
            })
    }

    fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
        let _lock = self.atomic_lock.lock();
        let storage_path = self.to_storage_path(path);

        self.runtime
            .block_on(async { self.storage.read(&storage_path).await })
            .map(|b| b.to_vec())
            .map_err(|e| match e {
                StorageError::NotFound(_) => OpenReadError::FileDoesNotExist(path.to_path_buf()),
                _ => OpenReadError::wrap_io_error(
                    io::Error::new(io::ErrorKind::Other, e.to_string()),
                    path.to_path_buf(),
                ),
            })
    }

    fn atomic_write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let _lock = self.atomic_lock.lock();
        let storage_path = self.to_storage_path(path);

        debug!(
            "atomic_write: {} ({} bytes)",
            storage_path.to_string(),
            data.len()
        );

        self.runtime.block_on(async {
            self.storage
                .write(&storage_path, Bytes::copy_from_slice(data))
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
        })?;

        // Update cache
        {
            let mut cache = self.file_cache.write();
            cache.insert(path.to_string_lossy().to_string());
        }

        Ok(())
    }

    fn delete(&self, path: &Path) -> Result<(), DeleteError> {
        let storage_path = self.to_storage_path(path);

        self.runtime
            .block_on(async { self.storage.delete(&storage_path).await })
            .map_err(|e| DeleteError::IoError {
                io_error: Arc::new(io::Error::new(io::ErrorKind::Other, e.to_string())),
                filepath: path.to_path_buf(),
            })?;

        // Update cache
        {
            let mut cache = self.file_cache.write();
            cache.remove(&path.to_string_lossy().to_string());
        }

        Ok(())
    }

    fn open_write(&self, path: &Path) -> Result<WritePtr, OpenWriteError> {
        let storage_path = self.to_storage_path(path);

        let write_handle = StorageWriteHandle::new(
            self.storage.clone(),
            storage_path,
            &self.buffer_dir,
            self.runtime.clone(),
        )
        .map_err(|e| OpenWriteError::wrap_io_error(e, path.to_path_buf()))?;

        Ok(WritePtr::new(Box::new(write_handle)))
    }

    fn sync_directory(&self) -> io::Result<()> {
        // Storage backends handle durability internally
        Ok(())
    }

    fn watch(&self, _callback: WatchCallback) -> tantivy::Result<WatchHandle> {
        // Remote storage doesn't support filesystem watches
        Ok(WatchHandle::empty())
    }

    fn acquire_lock(&self, _lock: &Lock) -> Result<DirectoryLock, LockError> {
        // Use no-op lock - concurrency handled externally at collection level
        Ok(DirectoryLock::from(Box::new(NoOpLock)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LocalStorage;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_adapter() -> (TempDir, TempDir, TantivyStorageAdapter) {
        let storage_dir = TempDir::new().unwrap();
        let buffer_dir = TempDir::new().unwrap();

        let storage = Arc::new(LocalStorage::new(storage_dir.path()));
        let adapter = TantivyStorageAdapter::new(
            storage,
            "test-collection",
            "shard_0",
            buffer_dir.path(),
        )
        .unwrap();

        (storage_dir, buffer_dir, adapter)
    }

    #[test]
    fn test_atomic_read_write() {
        let (_storage_dir, _buffer_dir, adapter) = create_test_adapter();

        // Write
        adapter
            .atomic_write(Path::new("test.txt"), b"hello world")
            .unwrap();

        // Read
        let data = adapter.atomic_read(Path::new("test.txt")).unwrap();
        assert_eq!(data, b"hello world");
    }

    #[test]
    fn test_exists() {
        let (_storage_dir, _buffer_dir, adapter) = create_test_adapter();

        assert!(!adapter.exists(Path::new("nonexistent.txt")).unwrap());

        adapter
            .atomic_write(Path::new("exists.txt"), b"data")
            .unwrap();

        assert!(adapter.exists(Path::new("exists.txt")).unwrap());
    }

    #[test]
    fn test_file_handle() {
        let (_storage_dir, _buffer_dir, adapter) = create_test_adapter();

        adapter
            .atomic_write(Path::new("segment.idx"), b"0123456789")
            .unwrap();

        let handle = adapter.get_file_handle(Path::new("segment.idx")).unwrap();

        assert_eq!(handle.len(), 10);

        let bytes = handle.read_bytes(2..5).unwrap();
        assert_eq!(bytes.as_slice(), b"234");
    }

    #[test]
    fn test_write_ptr() {
        let (_storage_dir, _buffer_dir, adapter) = create_test_adapter();

        let mut writer = adapter.open_write(Path::new("segment.dat")).unwrap();
        writer.write_all(b"hello ").unwrap();
        writer.write_all(b"world").unwrap();
        writer.terminate().unwrap();

        let data = adapter.atomic_read(Path::new("segment.dat")).unwrap();
        assert_eq!(data, b"hello world");
    }

    #[test]
    fn test_delete() {
        let (_storage_dir, _buffer_dir, adapter) = create_test_adapter();

        adapter
            .atomic_write(Path::new("to_delete.txt"), b"delete me")
            .unwrap();

        assert!(adapter.exists(Path::new("to_delete.txt")).unwrap());

        adapter.delete(Path::new("to_delete.txt")).unwrap();

        assert!(!adapter.exists(Path::new("to_delete.txt")).unwrap());
    }

    #[test]
    fn test_debug_format() {
        let (_storage_dir, _buffer_dir, adapter) = create_test_adapter();
        let debug = format!("{:?}", adapter);
        assert!(debug.contains("test-collection"));
        assert!(debug.contains("shard_0"));
    }

    #[test]
    fn test_clone() {
        let (_storage_dir, _buffer_dir, adapter) = create_test_adapter();

        adapter
            .atomic_write(Path::new("shared.txt"), b"shared data")
            .unwrap();

        let cloned = adapter.clone();

        // Both should see the same data
        let data = cloned.atomic_read(Path::new("shared.txt")).unwrap();
        assert_eq!(data, b"shared data");
    }
}
