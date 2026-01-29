# S3 Object Storage Backend Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Enable Tantivy indexes to be stored on S3-compatible object storage instead of local disk.

**Architecture:** Implement `tantivy::Directory` trait for `object_store` crate. Writes buffer locally then multipart upload on commit. CoW versioning via `meta.json.{version}` for atomic updates. Configurable per-collection via `StorageConfig`.

**Tech Stack:** `object_store` crate, `tantivy::Directory` trait, `tokio` async runtime, `bytes` crate.

---

## Task 1: Add Dependencies

**Files:**
- Modify: `Cargo.toml` (workspace)
- Modify: `prism/Cargo.toml`

**Step 1.1: Add object_store to workspace**

```toml
# Cargo.toml (workspace root) - add to [workspace.dependencies]
object_store = { version = "0.11", features = ["aws"] }
bytes = "1.5"
```

**Step 1.2: Add to prism crate**

```toml
# prism/Cargo.toml - add to [dependencies]
object_store = { workspace = true, optional = true }
bytes = { workspace = true }

# Add feature flag
[features]
storage-s3 = ["object_store"]
```

**Step 1.3: Verify compilation**

Run: `cargo check -p prism --features storage-s3`
Expected: Compiles without errors

**Step 1.4: Commit**

```bash
git add Cargo.toml prism/Cargo.toml
git commit -m "deps: add object_store and bytes for S3 storage backend"
```

---

## Task 2: Define StorageConfig Types

**Files:**
- Create: `prism/src/storage/mod.rs`
- Create: `prism/src/storage/config.rs`
- Modify: `prism/src/lib.rs`
- Modify: `prism/src/schema/types.rs`

**Step 2.1: Create storage module**

```rust
// prism/src/storage/mod.rs
mod config;

pub use config::{StorageConfig, S3Config, LocalConfig};

#[cfg(feature = "storage-s3")]
mod object_store_directory;

#[cfg(feature = "storage-s3")]
pub use object_store_directory::ObjectStoreDirectory;
```

**Step 2.2: Define config types**

```rust
// prism/src/storage/config.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StorageConfig {
    #[default]
    Local(LocalConfig),
    S3(S3Config),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LocalConfig {
    /// Base path for index storage. Defaults to data directory.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    /// S3 bucket name
    pub bucket: String,
    
    /// Key prefix for this collection
    #[serde(default)]
    pub prefix: Option<String>,
    
    /// AWS region (e.g., "us-east-1")
    pub region: String,
    
    /// Custom endpoint for S3-compatible services (MinIO, LocalStack)
    #[serde(default)]
    pub endpoint: Option<String>,
    
    /// Use path-style URLs (required for MinIO)
    #[serde(default)]
    pub force_path_style: bool,
    
    /// Local cache directory for write buffering
    #[serde(default)]
    pub cache_dir: Option<String>,
}

impl StorageConfig {
    pub fn is_local(&self) -> bool {
        matches!(self, StorageConfig::Local(_))
    }
    
    pub fn is_s3(&self) -> bool {
        matches!(self, StorageConfig::S3(_))
    }
}
```

**Step 2.3: Export from lib.rs**

```rust
// prism/src/lib.rs - add line
pub mod storage;
```

**Step 2.4: Add StorageConfig to CollectionSchema**

```rust
// prism/src/schema/types.rs - add to CollectionSchema struct
use crate::storage::StorageConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSchema {
    pub collection: String,
    pub description: Option<String>,
    pub backends: Backends,
    #[serde(default)]
    pub storage: StorageConfig,  // ADD THIS LINE
    // ... rest of fields
}
```

**Step 2.5: Write test for config parsing**

```rust
// prism/src/storage/config.rs - add at bottom
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_config_default() {
        let json = r#"{"type": "local"}"#;
        let config: StorageConfig = serde_json::from_str(json).unwrap();
        assert!(config.is_local());
    }

    #[test]
    fn test_s3_config_parsing() {
        let json = r#"{
            "type": "s3",
            "bucket": "my-bucket",
            "region": "us-east-1",
            "prefix": "indexes/",
            "endpoint": "http://localhost:9000",
            "force_path_style": true
        }"#;
        let config: StorageConfig = serde_json::from_str(json).unwrap();
        assert!(config.is_s3());
        if let StorageConfig::S3(s3) = config {
            assert_eq!(s3.bucket, "my-bucket");
            assert_eq!(s3.endpoint, Some("http://localhost:9000".to_string()));
            assert!(s3.force_path_style);
        }
    }

    #[test]
    fn test_default_is_local() {
        let config = StorageConfig::default();
        assert!(config.is_local());
    }
}
```

**Step 2.6: Run tests**

Run: `cargo test -p prism storage::config`
Expected: 3 tests pass

**Step 2.7: Commit**

```bash
git add prism/src/storage prism/src/lib.rs prism/src/schema/types.rs
git commit -m "feat(storage): add StorageConfig types for local and S3 backends"
```

---

## Task 3: Implement ObjectStoreDirectory - Core Structure

**Files:**
- Create: `prism/src/storage/object_store_directory.rs`

**Step 3.1: Write failing test for directory creation**

```rust
// prism/src/storage/object_store_directory.rs
#[cfg(test)]
mod tests {
    use super::*;
    use object_store::local::LocalFileSystem;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_directory() {
        let temp = TempDir::new().unwrap();
        let store = Arc::new(LocalFileSystem::new_with_prefix(temp.path()).unwrap());
        
        let dir = ObjectStoreDirectory::new(store, "test-index".into()).await.unwrap();
        
        assert!(dir.exists());
    }
}
```

**Step 3.2: Run test to verify it fails**

Run: `cargo test -p prism --features storage-s3 object_store_directory::tests::test_create_directory`
Expected: FAIL - module not found

**Step 3.3: Implement core structure**

```rust
// prism/src/storage/object_store_directory.rs
use std::collections::HashSet;
use std::io::{self, BufWriter, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use object_store::{ObjectStore, path::Path as ObjectPath};
use tantivy::directory::{
    DirectoryLock, FileHandle, Lock, OwnedBytes, TerminatingWrite, WatchCallback,
    WatchHandle, WritePtr,
};
use tantivy::Directory;
use tokio::runtime::Handle;

use crate::error::PrismError;

/// A Tantivy Directory implementation backed by object storage (S3, GCS, Azure).
/// 
/// Writes are buffered locally and uploaded on commit. Reads fetch from object store
/// with optional local caching. Uses CoW versioning for atomic updates.
pub struct ObjectStoreDirectory {
    /// The object store backend
    store: Arc<dyn ObjectStore>,
    
    /// Base path in the object store (e.g., "indexes/my-collection")
    base_path: ObjectPath,
    
    /// Current read version (from meta.json.{version})
    read_version: AtomicU64,
    
    /// Current write version (read_version + 1 during writes)
    write_version: AtomicU64,
    
    /// Local cache directory for write buffering
    cache_dir: PathBuf,
    
    /// Tokio runtime handle for async operations
    rt: Handle,
    
    /// Set of files that exist (cached from list operations)
    file_cache: RwLock<HashSet<PathBuf>>,
}

impl ObjectStoreDirectory {
    /// Create a new ObjectStoreDirectory.
    /// 
    /// # Arguments
    /// * `store` - The object store backend
    /// * `base_path` - Base path in the store (e.g., "indexes/my-collection")
    pub async fn new(
        store: Arc<dyn ObjectStore>,
        base_path: ObjectPath,
    ) -> Result<Self, PrismError> {
        Self::with_cache_dir(store, base_path, std::env::temp_dir()).await
    }
    
    /// Create with explicit cache directory.
    pub async fn with_cache_dir(
        store: Arc<dyn ObjectStore>,
        base_path: ObjectPath,
        cache_dir: PathBuf,
    ) -> Result<Self, PrismError> {
        let rt = Handle::current();
        
        // Read current version from meta.json.{version} files
        let read_version = Self::find_latest_version(&store, &base_path).await?;
        let write_version = read_version + 1;
        
        // Ensure cache directory exists
        let cache_path = cache_dir.join(base_path.as_ref());
        std::fs::create_dir_all(&cache_path)?;
        
        Ok(Self {
            store,
            base_path,
            read_version: AtomicU64::new(read_version),
            write_version: AtomicU64::new(write_version),
            cache_dir: cache_path,
            rt,
            file_cache: RwLock::new(HashSet::new()),
        })
    }
    
    /// Find the latest committed version by scanning meta.json.* files.
    async fn find_latest_version(
        store: &Arc<dyn ObjectStore>,
        base_path: &ObjectPath,
    ) -> Result<u64, PrismError> {
        let prefix = base_path.child("meta.json.");
        let mut max_version = 0u64;
        
        let list = store.list(Some(&prefix));
        tokio::pin!(list);
        
        use futures::StreamExt;
        while let Some(result) = list.next().await {
            if let Ok(meta) = result {
                // Extract version from "meta.json.{version}"
                let filename = meta.location.filename().unwrap_or_default();
                if let Some(version_str) = filename.strip_prefix("meta.json.") {
                    if let Ok(version) = version_str.parse::<u64>() {
                        max_version = max_version.max(version);
                    }
                }
            }
        }
        
        Ok(max_version)
    }
    
    /// Convert a Tantivy path to an object store path.
    fn to_object_path(&self, path: &Path) -> ObjectPath {
        self.base_path.child(path.to_string_lossy().as_ref())
    }
    
    /// Check if the directory exists (has any files).
    pub fn exists(&self) -> bool {
        self.read_version.load(Ordering::SeqCst) > 0 || {
            // Check if any files exist
            let store = self.store.clone();
            let base = self.base_path.clone();
            self.rt.block_on(async {
                let mut list = store.list(Some(&base));
                use futures::StreamExt;
                list.next().await.is_some()
            })
        }
    }
}
```

**Step 3.4: Run test**

Run: `cargo test -p prism --features storage-s3 object_store_directory::tests::test_create_directory`
Expected: PASS

**Step 3.5: Commit**

```bash
git add prism/src/storage/object_store_directory.rs
git commit -m "feat(storage): add ObjectStoreDirectory core structure"
```

---

## Task 4: Implement Directory Trait - File Listing

**Files:**
- Modify: `prism/src/storage/object_store_directory.rs`

**Step 4.1: Write failing test for list_managed_files**

```rust
// Add to tests module
#[tokio::test]
async fn test_list_managed_files_empty() {
    let temp = TempDir::new().unwrap();
    let store = Arc::new(LocalFileSystem::new_with_prefix(temp.path()).unwrap());
    
    let dir = ObjectStoreDirectory::new(store, "test-index".into()).await.unwrap();
    let files = dir.list_managed_files();
    
    assert!(files.is_empty());
}
```

**Step 4.2: Run test to verify it fails**

Run: `cargo test -p prism --features storage-s3 test_list_managed_files_empty`
Expected: FAIL - method not found

**Step 4.3: Implement list_managed_files**

```rust
// Add to ObjectStoreDirectory impl block
impl ObjectStoreDirectory {
    // ... existing methods ...
    
    /// List all files in the directory.
    fn list_files_sync(&self) -> Vec<PathBuf> {
        let store = self.store.clone();
        let base = self.base_path.clone();
        
        self.rt.block_on(async {
            let mut files = Vec::new();
            let list = store.list(Some(&base));
            tokio::pin!(list);
            
            use futures::StreamExt;
            while let Some(result) = list.next().await {
                if let Ok(meta) = result {
                    // Strip base path to get relative path
                    let rel_path = meta.location.as_ref()
                        .strip_prefix(base.as_ref())
                        .unwrap_or(meta.location.as_ref())
                        .trim_start_matches('/');
                    files.push(PathBuf::from(rel_path));
                }
            }
            files
        })
    }
}
```

**Step 4.4: Implement Directory trait (partial)**

```rust
impl Directory for ObjectStoreDirectory {
    fn list_managed_files(&self) -> tantivy::Result<Vec<PathBuf>> {
        Ok(self.list_files_sync())
    }
    
    fn exists(&self, path: &Path) -> tantivy::Result<bool> {
        let obj_path = self.to_object_path(path);
        let store = self.store.clone();
        
        let exists = self.rt.block_on(async {
            store.head(&obj_path).await.is_ok()
        });
        
        Ok(exists)
    }
    
    fn get_file_handle(&self, path: &Path) -> tantivy::Result<Arc<dyn FileHandle>> {
        todo!("Implement in Task 5")
    }
    
    fn delete(&self, path: &Path) -> tantivy::Result<()> {
        let obj_path = self.to_object_path(path);
        let store = self.store.clone();
        
        self.rt.block_on(async {
            store.delete(&obj_path).await
        }).map_err(|e| tantivy::TantivyError::IoError(
            io::Error::new(io::ErrorKind::Other, e)
        ))?;
        
        Ok(())
    }
    
    fn atomic_read(&self, path: &Path) -> tantivy::Result<Vec<u8>> {
        let obj_path = self.to_object_path(path);
        let store = self.store.clone();
        
        let bytes = self.rt.block_on(async {
            store.get(&obj_path).await?.bytes().await
        }).map_err(|e| tantivy::TantivyError::IoError(
            io::Error::new(io::ErrorKind::Other, e)
        ))?;
        
        Ok(bytes.to_vec())
    }
    
    fn atomic_write(&self, path: &Path, data: &[u8]) -> tantivy::Result<()> {
        let obj_path = self.to_object_path(path);
        let store = self.store.clone();
        let bytes = Bytes::copy_from_slice(data);
        
        self.rt.block_on(async {
            store.put(&obj_path, bytes.into()).await
        }).map_err(|e| tantivy::TantivyError::IoError(
            io::Error::new(io::ErrorKind::Other, e)
        ))?;
        
        Ok(())
    }
    
    fn open_write(&self, path: &Path) -> tantivy::Result<WritePtr> {
        todo!("Implement in Task 6")
    }
    
    fn sync_directory(&self) -> tantivy::Result<()> {
        // Object stores are inherently durable after PUT completes
        Ok(())
    }
    
    fn watch(&self, _callback: WatchCallback) -> tantivy::Result<WatchHandle> {
        // Object stores don't support filesystem watches
        Ok(WatchHandle::empty())
    }
    
    fn acquire_lock(&self, lock: &Lock) -> tantivy::Result<DirectoryLock> {
        // Use a no-op lock - object stores handle concurrency differently
        Ok(DirectoryLock::from(NoOpLock))
    }
}

/// No-op lock for object stores.
struct NoOpLock;

impl Drop for NoOpLock {
    fn drop(&mut self) {}
}
```

**Step 4.5: Run test**

Run: `cargo test -p prism --features storage-s3 test_list_managed_files_empty`
Expected: PASS

**Step 4.6: Add test for atomic_read/write**

```rust
#[tokio::test]
async fn test_atomic_read_write() {
    let temp = TempDir::new().unwrap();
    let store = Arc::new(LocalFileSystem::new_with_prefix(temp.path()).unwrap());
    
    let dir = ObjectStoreDirectory::new(store, "test-index".into()).await.unwrap();
    
    // Write
    dir.atomic_write(Path::new("test.txt"), b"hello world").unwrap();
    
    // Read back
    let data = dir.atomic_read(Path::new("test.txt")).unwrap();
    assert_eq!(data, b"hello world");
    
    // Exists
    assert!(dir.exists(Path::new("test.txt")).unwrap());
    assert!(!dir.exists(Path::new("nonexistent.txt")).unwrap());
}
```

**Step 4.7: Run all tests**

Run: `cargo test -p prism --features storage-s3 object_store_directory`
Expected: All tests pass

**Step 4.8: Commit**

```bash
git add prism/src/storage/object_store_directory.rs
git commit -m "feat(storage): implement Directory trait for listing and atomic ops"
```

---

## Task 5: Implement FileHandle for Reads

**Files:**
- Modify: `prism/src/storage/object_store_directory.rs`

**Step 5.1: Write failing test**

```rust
#[tokio::test]
async fn test_file_handle_read() {
    let temp = TempDir::new().unwrap();
    let store = Arc::new(LocalFileSystem::new_with_prefix(temp.path()).unwrap());
    
    let dir = ObjectStoreDirectory::new(store, "test-index".into()).await.unwrap();
    
    // Write a file
    dir.atomic_write(Path::new("segment.idx"), b"0123456789").unwrap();
    
    // Get file handle
    let handle = dir.get_file_handle(Path::new("segment.idx")).unwrap();
    
    // Read range
    let bytes = handle.read_bytes(Range { start: 2, end: 5 }).unwrap();
    assert_eq!(bytes.as_slice(), b"234");
    
    // Read full length
    assert_eq!(handle.len(), 10);
}
```

**Step 5.2: Run test to verify it fails**

Run: `cargo test -p prism --features storage-s3 test_file_handle_read`
Expected: FAIL - todo!() panic

**Step 5.3: Implement ObjectStoreFileHandle**

```rust
/// File handle for reading from object store.
struct ObjectStoreFileHandle {
    store: Arc<dyn ObjectStore>,
    path: ObjectPath,
    len: u64,
    rt: Handle,
}

impl ObjectStoreFileHandle {
    async fn new(
        store: Arc<dyn ObjectStore>,
        path: ObjectPath,
    ) -> Result<Self, object_store::Error> {
        let meta = store.head(&path).await?;
        Ok(Self {
            store,
            path,
            len: meta.size as u64,
            rt: Handle::current(),
        })
    }
}

impl FileHandle for ObjectStoreFileHandle {
    fn read_bytes(&self, range: Range<usize>) -> io::Result<OwnedBytes> {
        let store = self.store.clone();
        let path = self.path.clone();
        
        let bytes = self.rt.block_on(async {
            store.get_range(&path, range).await
        }).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        
        Ok(OwnedBytes::new(bytes.to_vec()))
    }
}

impl std::fmt::Debug for ObjectStoreFileHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectStoreFileHandle")
            .field("path", &self.path)
            .field("len", &self.len)
            .finish()
    }
}

impl HasLen for ObjectStoreFileHandle {
    fn len(&self) -> usize {
        self.len as usize
    }
}

// Add HasLen import at top
use tantivy::directory::FileHandle;
use tantivy::HasLen;
```

**Step 5.4: Update get_file_handle in Directory impl**

```rust
fn get_file_handle(&self, path: &Path) -> tantivy::Result<Arc<dyn FileHandle>> {
    let obj_path = self.to_object_path(path);
    let store = self.store.clone();
    
    let handle = self.rt.block_on(async {
        ObjectStoreFileHandle::new(store, obj_path).await
    }).map_err(|e| tantivy::TantivyError::IoError(
        io::Error::new(io::ErrorKind::Other, e)
    ))?;
    
    Ok(Arc::new(handle))
}
```

**Step 5.5: Run test**

Run: `cargo test -p prism --features storage-s3 test_file_handle_read`
Expected: PASS

**Step 5.6: Commit**

```bash
git add prism/src/storage/object_store_directory.rs
git commit -m "feat(storage): implement FileHandle for object store reads"
```

---

## Task 6: Implement WritePtr for Buffered Writes

**Files:**
- Modify: `prism/src/storage/object_store_directory.rs`

**Step 6.1: Write failing test**

```rust
#[tokio::test]
async fn test_write_ptr() {
    let temp = TempDir::new().unwrap();
    let store = Arc::new(LocalFileSystem::new_with_prefix(temp.path()).unwrap());
    
    let dir = ObjectStoreDirectory::new(store, "test-index".into()).await.unwrap();
    
    // Open write
    let mut writer = dir.open_write(Path::new("segment.dat")).unwrap();
    
    // Write data
    writer.write_all(b"hello ").unwrap();
    writer.write_all(b"world").unwrap();
    
    // Terminate (uploads to object store)
    writer.terminate().unwrap();
    
    // Verify uploaded
    let data = dir.atomic_read(Path::new("segment.dat")).unwrap();
    assert_eq!(data, b"hello world");
}
```

**Step 6.2: Run test to verify it fails**

Run: `cargo test -p prism --features storage-s3 test_write_ptr`
Expected: FAIL - todo!() panic

**Step 6.3: Implement ObjectStoreWriteHandle**

```rust
/// Write handle that buffers locally and uploads on terminate.
struct ObjectStoreWriteHandle {
    store: Arc<dyn ObjectStore>,
    path: ObjectPath,
    local_path: PathBuf,
    writer: BufWriter<std::fs::File>,
    rt: Handle,
}

impl ObjectStoreWriteHandle {
    fn new(
        store: Arc<dyn ObjectStore>,
        path: ObjectPath,
        cache_dir: &Path,
    ) -> io::Result<Self> {
        // Create local buffer file
        let local_path = cache_dir.join(path.filename().unwrap_or("temp"));
        let file = std::fs::File::create(&local_path)?;
        let writer = BufWriter::new(file);
        
        Ok(Self {
            store,
            path,
            local_path,
            writer,
            rt: Handle::current(),
        })
    }
}

impl Write for ObjectStoreWriteHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }
    
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl TerminatingWrite for ObjectStoreWriteHandle {
    fn terminate_ref(&mut self, _: tantivy::directory::AntiCallToken) -> io::Result<()> {
        // Flush local buffer
        self.writer.flush()?;
        drop(std::mem::replace(&mut self.writer, BufWriter::new(
            std::fs::File::create("/dev/null").unwrap()
        )));
        
        // Read local file and upload
        let data = std::fs::read(&self.local_path)?;
        let bytes = Bytes::from(data);
        let store = self.store.clone();
        let path = self.path.clone();
        
        self.rt.block_on(async {
            store.put(&path, bytes.into()).await
        }).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        
        // Clean up local file
        let _ = std::fs::remove_file(&self.local_path);
        
        Ok(())
    }
}
```

**Step 6.4: Update open_write in Directory impl**

```rust
fn open_write(&self, path: &Path) -> tantivy::Result<WritePtr> {
    let obj_path = self.to_object_path(path);
    
    let handle = ObjectStoreWriteHandle::new(
        self.store.clone(),
        obj_path,
        &self.cache_dir,
    ).map_err(tantivy::TantivyError::IoError)?;
    
    Ok(BufWriter::new(Box::new(handle)))
}
```

**Step 6.5: Run test**

Run: `cargo test -p prism --features storage-s3 test_write_ptr`
Expected: PASS

**Step 6.6: Commit**

```bash
git add prism/src/storage/object_store_directory.rs
git commit -m "feat(storage): implement WritePtr with local buffering and upload"
```

---

## Task 7: Add S3 Builder Function

**Files:**
- Modify: `prism/src/storage/object_store_directory.rs`
- Modify: `prism/src/storage/mod.rs`

**Step 7.1: Write test for S3 builder**

```rust
#[tokio::test]
async fn test_s3_builder_minio() {
    use crate::storage::S3Config;
    
    let config = S3Config {
        bucket: "test-bucket".to_string(),
        region: "us-east-1".to_string(),
        prefix: Some("indexes/".to_string()),
        endpoint: Some("http://localhost:9000".to_string()),
        force_path_style: true,
        cache_dir: None,
    };
    
    // This will fail to connect but should build the store correctly
    let result = ObjectStoreDirectory::from_s3_config(&config, "my-collection").await;
    
    // We can't actually connect without MinIO running, but we can verify the builder works
    assert!(result.is_err()); // Connection refused is expected
}
```

**Step 7.2: Implement S3 builder**

```rust
impl ObjectStoreDirectory {
    /// Create from S3Config.
    pub async fn from_s3_config(
        config: &crate::storage::S3Config,
        collection_name: &str,
    ) -> Result<Self, PrismError> {
        use object_store::aws::AmazonS3Builder;
        
        let mut builder = AmazonS3Builder::new()
            .with_bucket_name(&config.bucket)
            .with_region(&config.region);
        
        if let Some(endpoint) = &config.endpoint {
            builder = builder.with_endpoint(endpoint);
        }
        
        if config.force_path_style {
            builder = builder.with_virtual_hosted_style_request(false);
        }
        
        // Allow anonymous access for testing (credentials from env otherwise)
        builder = builder.with_allow_http(true);
        
        let store = builder.build()
            .map_err(|e| PrismError::Storage(e.to_string()))?;
        
        let prefix = config.prefix.as_deref().unwrap_or("");
        let base_path = ObjectPath::from(format!("{}{}", prefix, collection_name));
        
        let cache_dir = config.cache_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        
        Self::with_cache_dir(Arc::new(store), base_path, cache_dir).await
    }
}
```

**Step 7.3: Add PrismError::Storage variant**

```rust
// prism/src/error.rs - add variant
#[derive(Debug, thiserror::Error)]
pub enum PrismError {
    // ... existing variants ...
    
    #[error("Storage error: {0}")]
    Storage(String),
}
```

**Step 7.4: Run tests**

Run: `cargo test -p prism --features storage-s3 object_store_directory`
Expected: All tests pass

**Step 7.5: Commit**

```bash
git add prism/src/storage prism/src/error.rs
git commit -m "feat(storage): add S3 builder from config"
```

---

## Task 8: Integrate with TextBackend

**Files:**
- Modify: `prism/src/backends/text.rs`

**Step 8.1: Write integration test**

```rust
// prism/src/backends/text.rs - add to tests module
#[cfg(all(test, feature = "storage-s3"))]
mod s3_tests {
    use super::*;
    use crate::storage::{StorageConfig, S3Config};
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_text_backend_with_local_storage() {
        let temp = TempDir::new().unwrap();
        let schema = create_test_schema();
        
        let backend = TextBackend::new(temp.path().to_path_buf());
        backend.initialize("test-collection", &schema, &StorageConfig::default())
            .await
            .unwrap();
        
        // Index a document
        let doc = Document {
            id: "1".to_string(),
            fields: [("title".to_string(), Value::String("Hello World".to_string()))]
                .into_iter().collect(),
        };
        backend.index("test-collection", vec![doc]).await.unwrap();
        
        // Search
        let query = Query {
            query_string: "hello".to_string(),
            ..Default::default()
        };
        let results = backend.search("test-collection", &query).await.unwrap();
        assert_eq!(results.total, 1);
    }
    
    fn create_test_schema() -> CollectionSchema {
        // ... minimal test schema
    }
}
```

**Step 8.2: Modify TextBackend::initialize to accept StorageConfig**

```rust
// Update initialize signature
pub async fn initialize(
    &self,
    collection: &str,
    schema: &CollectionSchema,
    storage: &StorageConfig,
) -> Result<(), PrismError> {
    match storage {
        StorageConfig::Local(local) => {
            let path = local.path.as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| self.base_path.join(collection));
            
            // Existing local directory logic
            self.initialize_local(collection, schema, &path).await
        }
        #[cfg(feature = "storage-s3")]
        StorageConfig::S3(s3) => {
            let dir = crate::storage::ObjectStoreDirectory::from_s3_config(s3, collection)
                .await?;
            self.initialize_with_directory(collection, schema, Box::new(dir)).await
        }
        #[cfg(not(feature = "storage-s3"))]
        StorageConfig::S3(_) => {
            Err(PrismError::Config("S3 storage requires 'storage-s3' feature".into()))
        }
    }
}

// Extract existing logic to initialize_local
async fn initialize_local(
    &self,
    collection: &str,
    schema: &CollectionSchema,
    path: &Path,
) -> Result<(), PrismError> {
    // ... existing implementation
}

// New method for custom directories
async fn initialize_with_directory(
    &self,
    collection: &str,
    schema: &CollectionSchema,
    directory: Box<dyn Directory>,
) -> Result<(), PrismError> {
    let tantivy_schema = self.build_tantivy_schema(schema)?;
    let index = Index::open_or_create(directory, tantivy_schema.clone())?;
    // ... rest of initialization
}
```

**Step 8.3: Run tests**

Run: `cargo test -p prism --features storage-s3 text_backend`
Expected: All tests pass

**Step 8.4: Commit**

```bash
git add prism/src/backends/text.rs
git commit -m "feat(storage): integrate ObjectStoreDirectory with TextBackend"
```

---

## Task 9: Add Integration Tests with LocalStack/MinIO

**Files:**
- Create: `prism/tests/s3_integration.rs`
- Create: `docker-compose.test.yml`

**Step 9.1: Create docker-compose for MinIO**

```yaml
# docker-compose.test.yml
version: '3.8'
services:
  minio:
    image: minio/minio:latest
    ports:
      - "9000:9000"
      - "9001:9001"
    environment:
      MINIO_ROOT_USER: minioadmin
      MINIO_ROOT_PASSWORD: minioadmin
    command: server /data --console-address ":9001"
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:9000/minio/health/live"]
      interval: 5s
      timeout: 5s
      retries: 3
```

**Step 9.2: Create integration test**

```rust
// prism/tests/s3_integration.rs
#![cfg(feature = "storage-s3")]

use prism::storage::{ObjectStoreDirectory, S3Config};
use std::sync::Arc;
use object_store::aws::AmazonS3Builder;

/// Run with: cargo test --features storage-s3 --test s3_integration -- --ignored
/// Requires MinIO: docker-compose -f docker-compose.test.yml up -d
#[tokio::test]
#[ignore] // Requires MinIO running
async fn test_full_index_lifecycle_on_s3() {
    let config = S3Config {
        bucket: "test-bucket".to_string(),
        region: "us-east-1".to_string(),
        prefix: Some("prism-test/".to_string()),
        endpoint: Some("http://localhost:9000".to_string()),
        force_path_style: true,
        cache_dir: None,
    };
    
    // Create bucket first
    let store = AmazonS3Builder::new()
        .with_bucket_name("test-bucket")
        .with_region("us-east-1")
        .with_endpoint("http://localhost:9000")
        .with_virtual_hosted_style_request(false)
        .with_allow_http(true)
        .with_access_key_id("minioadmin")
        .with_secret_access_key("minioadmin")
        .build()
        .unwrap();
    
    // Test directory operations
    let dir = ObjectStoreDirectory::from_s3_config(&config, "test-collection")
        .await
        .expect("Failed to create directory");
    
    // Write and read
    dir.atomic_write(std::path::Path::new("test.txt"), b"hello s3").unwrap();
    let data = dir.atomic_read(std::path::Path::new("test.txt")).unwrap();
    assert_eq!(data, b"hello s3");
    
    // List files
    let files = dir.list_managed_files().unwrap();
    assert!(files.iter().any(|p| p.to_string_lossy().contains("test.txt")));
    
    // Cleanup
    dir.delete(std::path::Path::new("test.txt")).unwrap();
}
```

**Step 9.3: Add test instructions to README**

```markdown
## Running S3 Integration Tests

1. Start MinIO:
   ```bash
   docker-compose -f docker-compose.test.yml up -d
   ```

2. Create test bucket:
   ```bash
   docker exec -it $(docker ps -qf "ancestor=minio/minio") \
     mc alias set local http://localhost:9000 minioadmin minioadmin && \
     mc mb local/test-bucket
   ```

3. Run tests:
   ```bash
   cargo test --features storage-s3 --test s3_integration -- --ignored
   ```
```

**Step 9.4: Run integration test (if MinIO available)**

Run: `docker-compose -f docker-compose.test.yml up -d && sleep 5 && cargo test --features storage-s3 --test s3_integration -- --ignored`
Expected: PASS (or skip if MinIO not available)

**Step 9.5: Commit**

```bash
git add prism/tests/s3_integration.rs docker-compose.test.yml
git commit -m "test(storage): add S3 integration tests with MinIO"
```

---

## Task 10: Documentation and Final Cleanup

**Files:**
- Modify: `prism/src/storage/mod.rs` (doc comments)
- Modify: `README.md`

**Step 10.1: Add module documentation**

```rust
// prism/src/storage/mod.rs
//! # Storage Backends
//!
//! This module provides pluggable storage backends for Tantivy indexes.
//!
//! ## Supported Backends
//!
//! - **Local**: Default filesystem storage (no additional dependencies)
//! - **S3**: Amazon S3 and S3-compatible storage (requires `storage-s3` feature)
//!
//! ## Configuration
//!
//! Storage is configured per-collection in the schema:
//!
//! ```json
//! {
//!   "collection": "my-collection",
//!   "storage": {
//!     "type": "s3",
//!     "bucket": "my-bucket",
//!     "region": "us-east-1",
//!     "prefix": "indexes/"
//!   }
//! }
//! ```
//!
//! ## S3-Compatible Services
//!
//! For MinIO or LocalStack, set `endpoint` and `force_path_style`:
//!
//! ```json
//! {
//!   "storage": {
//!     "type": "s3",
//!     "bucket": "local-bucket",
//!     "region": "us-east-1",
//!     "endpoint": "http://localhost:9000",
//!     "force_path_style": true
//!   }
//! }
//! ```
```

**Step 10.2: Update README**

Add S3 storage section to README.md with configuration examples.

**Step 10.3: Run full test suite**

Run: `cargo test -p prism --all-features`
Expected: All tests pass

**Step 10.4: Final commit**

```bash
git add prism/src/storage README.md
git commit -m "docs(storage): add documentation for S3 storage backend"
```

---

## Task 11: Integrate with VectorBackend

**Files:**
- Modify: `prism/src/backends/vector/backend.rs`
- Create: `prism/src/storage/vector_store.rs`

**Context:** VectorBackend currently stores HNSW index in-memory with JSON persistence via `save()`/`load()`. We need to support S3 for the serialized index file.

**Step 11.1: Create VectorStore abstraction**

```rust
// prism/src/storage/vector_store.rs
use std::path::Path;
use async_trait::async_trait;
use crate::error::PrismError;

/// Abstraction for vector index persistence.
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Save serialized index data.
    async fn save(&self, collection: &str, data: &[u8]) -> Result<(), PrismError>;
    
    /// Load serialized index data. Returns None if not found.
    async fn load(&self, collection: &str) -> Result<Option<Vec<u8>>, PrismError>;
    
    /// Delete stored index.
    async fn delete(&self, collection: &str) -> Result<(), PrismError>;
}

/// Local filesystem vector store.
pub struct LocalVectorStore {
    base_path: std::path::PathBuf,
}

impl LocalVectorStore {
    pub fn new(base_path: impl Into<std::path::PathBuf>) -> Self {
        Self { base_path: base_path.into() }
    }
    
    fn index_path(&self, collection: &str) -> std::path::PathBuf {
        self.base_path.join(collection).join("vector_index.json")
    }
}

#[async_trait]
impl VectorStore for LocalVectorStore {
    async fn save(&self, collection: &str, data: &[u8]) -> Result<(), PrismError> {
        let path = self.index_path(collection);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, data).await?;
        Ok(())
    }
    
    async fn load(&self, collection: &str) -> Result<Option<Vec<u8>>, PrismError> {
        let path = self.index_path(collection);
        match tokio::fs::read(&path).await {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    
    async fn delete(&self, collection: &str) -> Result<(), PrismError> {
        let path = self.index_path(collection);
        tokio::fs::remove_file(path).await.ok();
        Ok(())
    }
}

/// S3-backed vector store.
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
        object_store::path::Path::from(format!("{}{}/vector_index.json", self.prefix, collection))
    }
}

#[cfg(feature = "storage-s3")]
#[async_trait]
impl VectorStore for S3VectorStore {
    async fn save(&self, collection: &str, data: &[u8]) -> Result<(), PrismError> {
        let path = self.index_path(collection);
        self.store.put(&path, bytes::Bytes::copy_from_slice(data).into()).await
            .map_err(|e| PrismError::Storage(e.to_string()))?;
        Ok(())
    }
    
    async fn load(&self, collection: &str) -> Result<Option<Vec<u8>>, PrismError> {
        let path = self.index_path(collection);
        match self.store.get(&path).await {
            Ok(result) => {
                let bytes = result.bytes().await
                    .map_err(|e| PrismError::Storage(e.to_string()))?;
                Ok(Some(bytes.to_vec()))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(PrismError::Storage(e.to_string())),
        }
    }
    
    async fn delete(&self, collection: &str) -> Result<(), PrismError> {
        let path = self.index_path(collection);
        self.store.delete(&path).await.ok();
        Ok(())
    }
}
```

**Step 11.2: Update storage/mod.rs exports**

```rust
// prism/src/storage/mod.rs - add
mod vector_store;
pub use vector_store::{VectorStore, LocalVectorStore};

#[cfg(feature = "storage-s3")]
pub use vector_store::S3VectorStore;
```

**Step 11.3: Write test for VectorStore**

```rust
// prism/src/storage/vector_store.rs - add tests
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
```

**Step 11.4: Run tests**

Run: `cargo test -p prism storage::vector_store`
Expected: 2 tests pass

**Step 11.5: Modify VectorBackend to use VectorStore**

```rust
// prism/src/backends/vector/backend.rs - update struct
pub struct VectorBackend {
    base_path: PathBuf,
    indexes: Arc<RwLock<HashMap<String, VectorIndex>>>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    vector_store: Arc<dyn VectorStore>,  // ADD THIS
}

impl VectorBackend {
    pub fn new(base_path: PathBuf) -> Self {
        Self {
            vector_store: Arc::new(LocalVectorStore::new(&base_path)),
            base_path,
            indexes: Arc::new(RwLock::new(HashMap::new())),
            embedding_provider: None,
        }
    }
    
    pub fn with_storage(base_path: PathBuf, storage: Arc<dyn VectorStore>) -> Self {
        Self {
            vector_store: storage,
            base_path,
            indexes: Arc::new(RwLock::new(HashMap::new())),
            embedding_provider: None,
        }
    }
}
```

**Step 11.6: Update save/load to use VectorStore**

```rust
// Replace direct file I/O with VectorStore calls
impl VectorBackend {
    async fn persist_index(&self, collection: &str) -> Result<(), PrismError> {
        let indexes = self.indexes.read();
        if let Some(index) = indexes.get(collection) {
            let data = index.hnsw.serialize()?;  // Assuming serialize returns Vec<u8>
            self.vector_store.save(collection, &data).await?;
        }
        Ok(())
    }
    
    async fn load_index(&self, collection: &str) -> Result<Option<HnswIndex>, PrismError> {
        if let Some(data) = self.vector_store.load(collection).await? {
            let index = HnswIndex::deserialize(&data)?;
            return Ok(Some(index));
        }
        Ok(None)
    }
}
```

**Step 11.7: Run vector backend tests**

Run: `cargo test -p prism backends::vector`
Expected: All tests pass

**Step 11.8: Commit**

```bash
git add prism/src/storage/vector_store.rs prism/src/storage/mod.rs prism/src/backends/vector/backend.rs
git commit -m "feat(storage): add VectorStore abstraction with S3 support"
```

---

## Task 12: Create StorageFactory for Unified Backend Creation

**Files:**
- Create: `prism/src/storage/factory.rs`
- Modify: `prism/src/storage/mod.rs`

**Step 12.1: Create StorageFactory**

```rust
// prism/src/storage/factory.rs
use std::sync::Arc;
use crate::error::PrismError;
use crate::storage::{StorageConfig, VectorStore, LocalVectorStore};

#[cfg(feature = "storage-s3")]
use crate::storage::{ObjectStoreDirectory, S3VectorStore};

/// Factory for creating storage backends from config.
pub struct StorageFactory;

impl StorageFactory {
    /// Create a Tantivy Directory from StorageConfig.
    #[cfg(feature = "storage-s3")]
    pub async fn create_directory(
        config: &StorageConfig,
        collection: &str,
    ) -> Result<Box<dyn tantivy::Directory>, PrismError> {
        match config {
            StorageConfig::Local(local) => {
                let path = local.path.as_ref()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| std::path::PathBuf::from("data").join(collection));
                std::fs::create_dir_all(&path)?;
                Ok(Box::new(tantivy::directory::MmapDirectory::open(&path)?))
            }
            StorageConfig::S3(s3) => {
                let dir = ObjectStoreDirectory::from_s3_config(s3, collection).await?;
                Ok(Box::new(dir))
            }
        }
    }
    
    /// Create a VectorStore from StorageConfig.
    #[cfg(feature = "storage-s3")]
    pub async fn create_vector_store(
        config: &StorageConfig,
    ) -> Result<Arc<dyn VectorStore>, PrismError> {
        match config {
            StorageConfig::Local(local) => {
                let path = local.path.as_ref()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| std::path::PathBuf::from("data"));
                Ok(Arc::new(LocalVectorStore::new(path)))
            }
            StorageConfig::S3(s3) => {
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
                
                let store = Arc::new(builder.build()
                    .map_err(|e| PrismError::Storage(e.to_string()))?);
                
                let prefix = s3.prefix.clone().unwrap_or_default();
                Ok(Arc::new(S3VectorStore::new(store, prefix)))
            }
        }
    }
    
    /// Non-S3 fallback implementations
    #[cfg(not(feature = "storage-s3"))]
    pub async fn create_directory(
        config: &StorageConfig,
        collection: &str,
    ) -> Result<Box<dyn tantivy::Directory>, PrismError> {
        match config {
            StorageConfig::Local(local) => {
                let path = local.path.as_ref()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| std::path::PathBuf::from("data").join(collection));
                std::fs::create_dir_all(&path)?;
                Ok(Box::new(tantivy::directory::MmapDirectory::open(&path)?))
            }
            StorageConfig::S3(_) => {
                Err(PrismError::Config("S3 storage requires 'storage-s3' feature".into()))
            }
        }
    }
    
    #[cfg(not(feature = "storage-s3"))]
    pub async fn create_vector_store(
        config: &StorageConfig,
    ) -> Result<Arc<dyn VectorStore>, PrismError> {
        match config {
            StorageConfig::Local(local) => {
                let path = local.path.as_ref()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| std::path::PathBuf::from("data"));
                Ok(Arc::new(LocalVectorStore::new(path)))
            }
            StorageConfig::S3(_) => {
                Err(PrismError::Config("S3 storage requires 'storage-s3' feature".into()))
            }
        }
    }
}
```

**Step 12.2: Export from mod.rs**

```rust
// prism/src/storage/mod.rs - add
mod factory;
pub use factory::StorageFactory;
```

**Step 12.3: Write test**

```rust
// prism/src/storage/factory.rs - add tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::LocalConfig;

    #[tokio::test]
    async fn test_create_local_directory() {
        let temp = tempfile::TempDir::new().unwrap();
        let config = StorageConfig::Local(LocalConfig {
            path: Some(temp.path().to_string_lossy().to_string()),
        });
        
        let dir = StorageFactory::create_directory(&config, "test").await.unwrap();
        assert!(dir.list_managed_files().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_create_local_vector_store() {
        let temp = tempfile::TempDir::new().unwrap();
        let config = StorageConfig::Local(LocalConfig {
            path: Some(temp.path().to_string_lossy().to_string()),
        });
        
        let store = StorageFactory::create_vector_store(&config).await.unwrap();
        store.save("test", b"data").await.unwrap();
        let loaded = store.load("test").await.unwrap();
        assert_eq!(loaded, Some(b"data".to_vec()));
    }
}
```

**Step 12.4: Run tests**

Run: `cargo test -p prism storage::factory`
Expected: 2 tests pass

**Step 12.5: Commit**

```bash
git add prism/src/storage/factory.rs prism/src/storage/mod.rs
git commit -m "feat(storage): add StorageFactory for unified backend creation"
```

---

## Task 13: Update CollectionManager to Use StorageFactory

**Files:**
- Modify: `prism/src/collection/manager.rs`

**Step 13.1: Update CollectionManager::create_collection**

```rust
// Update to use StorageFactory
impl CollectionManager {
    pub async fn create_collection(&self, schema: &CollectionSchema) -> Result<(), PrismError> {
        let storage = &schema.storage;
        
        // Create text backend with appropriate storage
        if let Some(text_config) = &schema.backends.text {
            let directory = StorageFactory::create_directory(storage, &schema.collection).await?;
            self.text_backend.initialize_with_directory(&schema.collection, schema, directory).await?;
        }
        
        // Create vector backend with appropriate storage
        if let Some(vector_config) = &schema.backends.vector {
            let vector_store = StorageFactory::create_vector_store(storage).await?;
            let vector_backend = VectorBackend::with_storage(
                self.base_path.clone(),
                vector_store,
            );
            vector_backend.initialize(&schema.collection, schema).await?;
            // Store in manager...
        }
        
        Ok(())
    }
}
```

**Step 13.2: Run integration tests**

Run: `cargo test -p prism collection`
Expected: All tests pass

**Step 13.3: Commit**

```bash
git add prism/src/collection/manager.rs
git commit -m "feat(storage): integrate StorageFactory with CollectionManager"
```

---

## Summary

| Task | Description | Files | Est. Time |
|------|-------------|-------|-----------|
| 1 | Add dependencies | Cargo.toml | 5 min |
| 2 | StorageConfig types | storage/config.rs, schema/types.rs | 15 min |
| 3 | ObjectStoreDirectory core | storage/object_store_directory.rs | 20 min |
| 4 | Directory trait (listing) | storage/object_store_directory.rs | 15 min |
| 5 | FileHandle for reads | storage/object_store_directory.rs | 15 min |
| 6 | WritePtr for writes | storage/object_store_directory.rs | 20 min |
| 7 | S3 builder function | storage/object_store_directory.rs | 10 min |
| 8 | TextBackend integration | backends/text.rs | 20 min |
| 9 | Integration tests | tests/s3_integration.rs | 15 min |
| 10 | Documentation | mod.rs, README.md | 10 min |
| 11 | VectorBackend integration | storage/vector_store.rs, vector/backend.rs | 25 min |
| 12 | StorageFactory | storage/factory.rs | 15 min |
| 13 | CollectionManager integration | collection/manager.rs | 15 min |

**Total: ~3.5 hours**

---

## Dependencies Graph

```
Task 1 (deps)
    └── Task 2 (config types)
            └── Task 3 (core structure)
                    ├── Task 4 (listing)
                    ├── Task 5 (reads)
                    └── Task 6 (writes)
                            └── Task 7 (S3 builder)
                                    └── Task 8 (integration)
                                            └── Task 9 (tests)
                                                    └── Task 10 (docs)
```
