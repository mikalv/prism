//! Object store (S3, GCS, Azure) backed Tantivy Directory implementation.
//!
//! Writes are buffered locally and uploaded on commit. Reads fetch from object store.
//! Uses CoW versioning via meta.json.{version} for atomic updates.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use bytes::Bytes;
use futures::StreamExt;
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use object_store::ObjectStore;
use tantivy::directory::error::{DeleteError, LockError, OpenReadError, OpenWriteError};
use tantivy::directory::{
    AntiCallToken, Directory, DirectoryLock, FileHandle, OwnedBytes, TerminatingWrite,
    WatchCallback, WatchHandle, WritePtr,
};
use tantivy::HasLen;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Handle;

use crate::cache::{LruCache, ObjectCacheStats};

const DEFAULT_CACHE_MAX_MB: usize = 512;

use std::fmt;

/// A Tantivy Directory implementation backed by object storage (S3, GCS, Azure).
///
/// Writes are buffered locally and uploaded on commit. Reads fetch from object store
/// with optional local caching. Uses CoW versioning for atomic updates.
#[derive(Clone)]
pub struct ObjectStoreDirectory {
    /// The object store backend
    store: Arc<dyn ObjectStore>,

    /// Base path in the object store (e.g., "indexes/my-collection")
    base_path: String,

    /// Current read version (from meta.json.{version})
    read_version: Option<u64>,

    /// Current write version (read_version + 1 during writes)
    write_version: u64,

    /// Local cache directory for write buffering
    cache_dir: Arc<PathBuf>,

    /// Local filesystem for cache operations
    local_fs: Arc<LocalFileSystem>,

    /// Tokio runtime handle for async operations
    rt: Arc<tokio::runtime::Runtime>,

    /// Lock for atomic read/write operations
    atomic_rw_lock: Arc<Mutex<()>>,

    /// Set of files that exist (cached from list operations)
    file_cache: Arc<RwLock<HashSet<PathBuf>>>,

    /// Local LRU cache for downloaded files
    object_cache: LruCache,
    /// Cache statistics for object cache
    cache_stats: Arc<ObjectCacheStats>,
}

impl std::fmt::Debug for ObjectStoreDirectory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectStoreDirectory")
            .field("base_path", &self.base_path)
            .field("read_version", &self.read_version)
            .field("write_version", &self.write_version)
            .finish()
    }
}

/// File handle for reading from object store.
#[derive(Debug)]
struct ObjectStoreFileHandle {
    store: Arc<dyn ObjectStore>,
    path: ObjectPath,
    len: usize,
    rt: Arc<tokio::runtime::Runtime>,
}

impl ObjectStoreFileHandle {
    fn new(
        store: Arc<dyn ObjectStore>,
        path: ObjectPath,
        len: usize,
        rt: Arc<tokio::runtime::Runtime>,
    ) -> Self {
        Self {
            store,
            path,
            len,
            rt,
        }
    }
}

impl HasLen for ObjectStoreFileHandle {
    fn len(&self) -> usize {
        self.len
    }
}

impl FileHandle for ObjectStoreFileHandle {
    fn read_bytes(&self, range: Range<usize>) -> std::io::Result<OwnedBytes> {
        self.rt.block_on(async {
            let bytes = self
                .store
                .get_range(&self.path, range)
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            Ok(OwnedBytes::new(bytes.to_vec()))
        })
    }
}

/// Write handle that buffers locally and uploads on terminate.
struct ObjectStoreWriteHandle {
    store: Arc<dyn ObjectStore>,
    location: ObjectPath,
    local_path: PathBuf,
    write_handle: File,
    shutdown: AtomicBool,
    rt: Arc<tokio::runtime::Runtime>,
}

impl ObjectStoreWriteHandle {
    fn new(
        store: Arc<dyn ObjectStore>,
        location: ObjectPath,
        cache_dir: &Path,
        rt: Arc<tokio::runtime::Runtime>,
    ) -> std::io::Result<Self> {
        let local_path = cache_dir.join(location.as_ref());

        // Create parent directories
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let write_handle = File::create(&local_path)?;

        Ok(Self {
            store,
            location,
            local_path,
            write_handle,
            shutdown: AtomicBool::new(false),
            rt,
        })
    }
}

impl Write for ObjectStoreWriteHandle {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "write handle has been shutdown",
            ));
        }
        self.write_handle.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "write handle has been shutdown",
            ));
        }
        self.write_handle.flush()
    }
}

impl TerminatingWrite for ObjectStoreWriteHandle {
    fn terminate_ref(&mut self, _: AntiCallToken) -> std::io::Result<()> {
        self.flush()?;
        self.shutdown.store(true, Ordering::SeqCst);

        self.rt.block_on(async {
            let data = tokio::fs::read(&self.local_path).await?;
            self.store
                .put(&self.location, Bytes::from(data).into())
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

            let _ = tokio::fs::remove_file(&self.local_path).await;
            Ok(())
        })
    }
}

/// No-op lock for object stores (concurrency handled externally).
struct NoOpLock;

impl ObjectStoreDirectory {
    /// Create a new ObjectStoreDirectory.
    ///
    /// # Arguments
    /// * `store` - The object store backend
    /// * `base_path` - Base path in the store (e.g., "indexes/my-collection")
    /// * `read_version` - Version to read from (None for new index)
    /// * `write_version` - Version to write to
    pub fn new(
        store: Arc<dyn ObjectStore>,
        base_path: &str,
        read_version: Option<u64>,
        write_version: u64,
    ) -> Result<Self, std::io::Error> {
        Self::with_cache_dir(store, base_path, read_version, write_version, None, None, None)
    }

    /// Create with explicit cache directory and runtime.
    pub fn with_cache_dir(
        store: Arc<dyn ObjectStore>,
        base_path: &str,
        read_version: Option<u64>,
        write_version: u64,
        cache_dir: Option<PathBuf>,
        cache_max_size_mb: Option<usize>,
        rt: Option<Arc<tokio::runtime::Runtime>>,
    ) -> Result<Self, std::io::Error> {
        if let Some(rv) = read_version {
            if rv > write_version {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "read version cannot be greater than write version",
                ));
            }
        }

        let cache_dir = cache_dir.unwrap_or_else(std::env::temp_dir);

        let rt = rt.unwrap_or_else(|| {
            Arc::new(
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create tokio runtime"),
            )
        });

        let cache_max = cache_max_size_mb.unwrap_or(DEFAULT_CACHE_MAX_MB);
        let object_cache = LruCache::new(cache_max);
        let cache_stats = object_cache.stats();

        Ok(Self {
            store,
            base_path: base_path.to_string(),
            read_version,
            write_version,
            cache_dir: Arc::new(cache_dir),
            local_fs: Arc::new(LocalFileSystem::new()),
            rt,
            atomic_rw_lock: Arc::new(Mutex::new(())),
            file_cache: Arc::new(RwLock::new(HashSet::new())),
            object_cache,
            cache_stats,
        })
    }

    /// Create from S3Config.
    pub async fn from_s3_config(
        config: &super::S3Config,
        collection_name: &str,
        read_version: Option<u64>,
        write_version: u64,
    ) -> Result<Self, std::io::Error> {
        use object_store::aws::AmazonS3Builder;

        let mut builder = AmazonS3Builder::new()
            .with_bucket_name(&config.bucket)
            .with_region(&config.region)
            .with_allow_http(true);

        if let Some(endpoint) = &config.endpoint {
            builder = builder.with_endpoint(endpoint);
        }

        if config.force_path_style {
            builder = builder.with_virtual_hosted_style_request(false);
        }

        let store = builder
            .build()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        let prefix = config.prefix.as_deref().unwrap_or("");
        let base_path = format!("{}{}", prefix, collection_name);

        let cache_dir = config.cache_dir.as_ref().map(PathBuf::from);

        Self::with_cache_dir(
            Arc::new(store),
            &base_path,
            read_version,
            write_version,
            cache_dir,
            config.cache_max_size_mb,
            None,
        )
    }

    /// Convert a Tantivy path to an object store path.
    fn to_object_path(&self, path: &Path) -> Result<ObjectPath, std::io::Error> {
        let p = path.to_str().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "non-utf8 path")
        })?;
        Ok(ObjectPath::from(format!("{}/{}", self.base_path, p)))
    }

    fn insert_into_cache(&self, cache_path: &Path, size: u64) {
        if let Some(evicted) = self.object_cache.put(cache_path.to_path_buf(), size) {
            let _ = fs::remove_file(evicted);
        }
    }

    /// Get file metadata via HEAD request.
    fn head(&self, path: &Path) -> Result<object_store::ObjectMeta, OpenReadError> {
        let location = self
            .to_object_path(path)
            .map_err(|e| OpenReadError::wrap_io_error(e, path.to_path_buf()))?;

        self.rt
            .block_on(async { self.store.head(&location).await })
            .map_err(|e| match e {
                object_store::Error::NotFound { .. } => {
                    OpenReadError::FileDoesNotExist(path.to_path_buf())
                }
                _ => OpenReadError::wrap_io_error(
                    std::io::Error::new(std::io::ErrorKind::Other, e),
                    path.to_path_buf(),
                ),
            })
    }

    /// List all files in the directory.
    pub fn list_files(&self) -> Vec<PathBuf> {
        let prefix = ObjectPath::from(format!("{}/", self.base_path));

        self.rt.block_on(async {
            let mut files = Vec::new();
            let mut list = self.store.list(Some(&prefix));

            while let Some(result) = list.next().await {
                if let Ok(meta) = result {
                    let rel_path = meta
                        .location
                        .as_ref()
                        .strip_prefix(&format!("{}/", self.base_path))
                        .unwrap_or(meta.location.as_ref());
                    files.push(PathBuf::from(rel_path));
                }
            }
            files
        })
    }

    /// Cache statistics for the local object cache
    pub fn cache_stats(&self) -> Arc<ObjectCacheStats> {
        Arc::clone(&self.cache_stats)
    }

    /// Check if the directory has any files.
    pub fn is_empty(&self) -> bool {
        self.list_files().is_empty()
    }
}

impl Directory for ObjectStoreDirectory {
    fn get_file_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>, OpenReadError> {
        let location = self
            .to_object_path(path)
            .map_err(|e| OpenReadError::wrap_io_error(e, path.to_path_buf()))?;

        // Check local cache first
        let cache_path = self.cache_dir.join(location.as_ref());
        let cache_obj_path = ObjectPath::from(cache_path.to_string_lossy().as_ref());

        if let Ok(meta) = self
            .rt
            .block_on(async { self.local_fs.head(&cache_obj_path).await })
        {
            if self.object_cache.get(&cache_path).is_none() {
                self.insert_into_cache(&cache_path, meta.size as u64);
            }

            return Ok(Arc::new(ObjectStoreFileHandle::new(
                self.local_fs.clone(),
                cache_obj_path,
                meta.size,
                self.rt.clone(),
            )));
        }

        // Fetch from object store
        self.cache_stats.miss();
        let len = self.head(path)?.size;

        // Download and write to cache
        let data = self
            .rt
            .block_on(async { self.store.get_range(&location, 0..len).await })
            .map_err(|e| OpenReadError::wrap_io_error(
                std::io::Error::new(std::io::ErrorKind::Other, e),
                path.to_path_buf(),
            ))?;

        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent).map_err(|e| OpenReadError::wrap_io_error(e, cache_path.clone()))?;
        }

        fs::write(&cache_path, data.as_ref())
            .map_err(|e| OpenReadError::wrap_io_error(e, cache_path.clone()))?;

        self.insert_into_cache(&cache_path, len as u64);

        Ok(Arc::new(ObjectStoreFileHandle::new(
            self.local_fs.clone(),
            cache_obj_path,
            len,
            self.rt.clone(),
        )))
    }

    fn exists(&self, path: &Path) -> Result<bool, OpenReadError> {
        match self.head(path) {
            Ok(_) => Ok(true),
            Err(OpenReadError::FileDoesNotExist(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
        // For meta.json, use versioning
        if path == Path::new("meta.json") || path == Path::new(".managed.json") {
            let path_str = format!("{}.{}", path.to_string_lossy(), self.write_version);

            // Try write version first
            if let Ok(f) = self.get_file_handle(Path::new(&path_str)) {
                return Ok(f
                    .read_bytes(0..f.len())
                    .map_err(|e| OpenReadError::wrap_io_error(e, path.to_path_buf()))?
                    .to_vec());
            }

            // Fall back to read version
            if let Some(rv) = self.read_version {
                let path_str = format!("{}.{}", path.to_string_lossy(), rv);
                let _lock = self.atomic_rw_lock.lock().unwrap();
                let f = self.get_file_handle(Path::new(&path_str))?;
                return Ok(f
                    .read_bytes(0..f.len())
                    .map_err(|e| OpenReadError::wrap_io_error(e, path.to_path_buf()))?
                    .to_vec());
            }

            return Err(OpenReadError::FileDoesNotExist(path.to_path_buf()));
        }

        // Regular files
        let f = self.get_file_handle(path)?;
        Ok(f.read_bytes(0..f.len())
            .map_err(|e| OpenReadError::wrap_io_error(e, path.to_path_buf()))?
            .to_vec())
    }

    fn atomic_write(&self, path: &Path, data: &[u8]) -> std::io::Result<()> {
        // For meta.json, use versioning
        let actual_path = if path == Path::new("meta.json") || path == Path::new(".managed.json") {
            format!("{}.{}", path.to_string_lossy(), self.write_version)
        } else {
            path.to_string_lossy().to_string()
        };

        let location = self.to_object_path(Path::new(&actual_path))?;

        let _lock = self.atomic_rw_lock.lock().unwrap();
        self.rt.block_on(async {
            self.store
                .put(&location, Bytes::from(data.to_vec()).into())
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            Ok(())
        })
    }

    fn delete(&self, _path: &Path) -> Result<(), DeleteError> {
        // Don't actually delete - preserve for versioning
        Ok(())
    }

    fn open_write(&self, path: &Path) -> Result<WritePtr, OpenWriteError> {
        let location = self
            .to_object_path(path)
            .map_err(|e| OpenWriteError::wrap_io_error(e, path.to_path_buf()))?;

        let write_handle = ObjectStoreWriteHandle::new(
            self.store.clone(),
            location,
            &self.cache_dir,
            self.rt.clone(),
        )
        .map_err(|e| OpenWriteError::wrap_io_error(e, path.to_path_buf()))?;

        Ok(WritePtr::new(Box::new(write_handle)))
    }

    fn sync_directory(&self) -> std::io::Result<()> {
        // Object stores are inherently durable after PUT completes
        Ok(())
    }

    fn watch(&self, _callback: WatchCallback) -> tantivy::Result<WatchHandle> {
        // Object stores don't support filesystem watches
        Ok(WatchHandle::empty())
    }

    fn acquire_lock(&self, _lock: &tantivy::directory::Lock) -> Result<DirectoryLock, LockError> {
        // Use no-op lock - concurrency handled externally
        Ok(DirectoryLock::from(Box::new(NoOpLock)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_dir() -> (TempDir, ObjectStoreDirectory) {
        let temp = TempDir::new().unwrap();
        let store = Arc::new(LocalFileSystem::new_with_prefix(temp.path()).unwrap());
        let dir = ObjectStoreDirectory::new(store, "test-index", None, 0).unwrap();
        (temp, dir)
    }

    #[test]
    fn test_create_directory() {
        let (_temp, dir) = create_test_dir();
        assert!(dir.list_files().is_empty());
    }

    #[test]
    fn test_atomic_read_write() {
        let (_temp, dir) = create_test_dir();

        // Write a regular file
        dir.atomic_write(Path::new("test.txt"), b"hello world")
            .unwrap();

        // Read it back
        let data = dir.atomic_read(Path::new("test.txt")).unwrap();
        assert_eq!(data, b"hello world");

        // Check exists
        assert!(dir.exists(Path::new("test.txt")).unwrap());
        assert!(!dir.exists(Path::new("nonexistent.txt")).unwrap());
    }

    #[test]
    fn test_file_handle_read() {
        let (_temp, dir) = create_test_dir();

        dir.atomic_write(Path::new("segment.idx"), b"0123456789")
            .unwrap();

        let handle = dir.get_file_handle(Path::new("segment.idx")).unwrap();

        // Read range
        let bytes = handle.read_bytes(2..5).unwrap();
        assert_eq!(bytes.as_slice(), b"234");

        // Check length
        assert_eq!(handle.len(), 10);
    }

    #[test]
    fn test_write_ptr() {
        let (_temp, dir) = create_test_dir();

        let mut writer = dir.open_write(Path::new("segment.dat")).unwrap();
        writer.write_all(b"hello ").unwrap();
        writer.write_all(b"world").unwrap();
        writer.terminate().unwrap();

        let data = dir.atomic_read(Path::new("segment.dat")).unwrap();
        assert_eq!(data, b"hello world");
    }

    #[test]
    fn test_cache_read_through_and_hits() {
        let store_temp = TempDir::new().unwrap();
        let cache_temp = TempDir::new().unwrap();
        let store = Arc::new(LocalFileSystem::new_with_prefix(store_temp.path()).unwrap());

        let dir = ObjectStoreDirectory::with_cache_dir(
            store,
            "test-index",
            None,
            0,
            Some(cache_temp.path().to_path_buf()),
            Some(10),
            None,
        )
        .unwrap();

        dir.atomic_write(Path::new("file1"), b"abc").unwrap();

        let data1 = dir.atomic_read(Path::new("file1")).unwrap();
        assert_eq!(data1, b"abc");

        let cache_path = cache_temp.path().join("test-index/file1");
        assert!(cache_path.exists());

        let stats = dir.cache_stats();
        assert_eq!(stats.misses(), 1);
        assert_eq!(stats.hits(), 0);

        let _ = dir.atomic_read(Path::new("file1")).unwrap();
        let stats = dir.cache_stats();
        assert_eq!(stats.hits(), 1);
    }

    #[test]
    fn test_cache_eviction_removes_old_files() {
        let store_temp = TempDir::new().unwrap();
        let cache_temp = TempDir::new().unwrap();
        let store = Arc::new(LocalFileSystem::new_with_prefix(store_temp.path()).unwrap());

        let dir = ObjectStoreDirectory::with_cache_dir(
            store,
            "test-index",
            None,
            0,
            Some(cache_temp.path().to_path_buf()),
            Some(1),
            None,
        )
        .unwrap();

        let big = vec![0u8; 800_000];

        dir.atomic_write(Path::new("f1"), &big).unwrap();
        dir.atomic_read(Path::new("f1")).unwrap();
        let path1 = cache_temp.path().join("test-index/f1");
        assert!(path1.exists());

        dir.atomic_write(Path::new("f2"), &big).unwrap();
        dir.atomic_read(Path::new("f2")).unwrap();
        let path2 = cache_temp.path().join("test-index/f2");
        assert!(path2.exists());

        // First file should be evicted due to cache size limit
        assert!(!path1.exists());
    }

    #[test]
    fn test_meta_json_versioning() {
        let temp = TempDir::new().unwrap();
        let store = Arc::new(LocalFileSystem::new_with_prefix(temp.path()).unwrap());

        // Write version 0
        let dir = ObjectStoreDirectory::new(store.clone(), "test-index", None, 0).unwrap();
        dir.atomic_write(Path::new("meta.json"), b"version 0")
            .unwrap();

        // Write version 1
        let dir = ObjectStoreDirectory::new(store.clone(), "test-index", Some(0), 1).unwrap();
        dir.atomic_write(Path::new("meta.json"), b"version 1")
            .unwrap();

        // Read version 0
        let dir = ObjectStoreDirectory::new(store.clone(), "test-index", None, 0).unwrap();
        let data = dir.atomic_read(Path::new("meta.json")).unwrap();
        assert_eq!(data, b"version 0");

        // Read version 1
        let dir = ObjectStoreDirectory::new(store.clone(), "test-index", None, 1).unwrap();
        let data = dir.atomic_read(Path::new("meta.json")).unwrap();
        assert_eq!(data, b"version 1");
    }

    #[test]
    fn test_list_files() {
        let (_temp, dir) = create_test_dir();

        dir.atomic_write(Path::new("file1.txt"), b"content1")
            .unwrap();
        dir.atomic_write(Path::new("file2.txt"), b"content2")
            .unwrap();

        let files = dir.list_files();
        assert_eq!(files.len(), 2);
    }
}
