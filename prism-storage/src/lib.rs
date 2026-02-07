//! Unified storage abstraction for Prism search engine.
//!
//! This crate provides a common `SegmentStorage` trait that all Prism backends
//! (Tantivy, Vector, Graph) use for data persistence. This enables pluggable
//! storage backends (local filesystem, S3, tiered caching) without changing
//! backend code.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │  Backends                                       │
//! │  ┌───────────┐ ┌───────────┐ ┌───────────┐     │
//! │  │ Tantivy   │ │ Vector    │ │ Graph     │     │
//! │  └─────┬─────┘ └─────┬─────┘ └─────┬─────┘     │
//! │        │             │             │            │
//! │        └─────────────┼─────────────┘            │
//! │                      ▼                          │
//! │            ┌─────────────────┐                  │
//! │            │ SegmentStorage  │  ← Unified trait │
//! │            └────────┬────────┘                  │
//! │                     │                           │
//! │        ┌────────────┼────────────┐              │
//! │        ▼            ▼            ▼              │
//! │  ┌──────────┐ ┌──────────┐ ┌──────────┐        │
//! │  │  Local   │ │   S3     │ │  Cached  │        │
//! │  └──────────┘ └──────────┘ └──────────┘        │
//! └─────────────────────────────────────────────────┘
//! ```
//!
//! # Storage Paths
//!
//! All data is organized using hierarchical [`StoragePath`]s:
//!
//! ```text
//! collection/backend/shard/segment
//!
//! products/tantivy/shard_0/segment_00001.si
//! products/vector/shard_0/hnsw_00001.bin
//! products/graph/shard_0/edges_00001.bin
//! ```
//!
//! # Quick Start
//!
//! ```no_run
//! use prism_storage::{LocalStorage, SegmentStorage, StoragePath, StorageBackend};
//! use bytes::Bytes;
//!
//! # async fn example() -> prism_storage::Result<()> {
//! // Create local storage
//! let storage = LocalStorage::new("./data");
//!
//! // Write vector index data
//! let path = StoragePath::vector("products", "shard_0", "hnsw_index.bin");
//! storage.write(&path, Bytes::from("binary data")).await?;
//!
//! // Read it back
//! let data = storage.read(&path).await?;
//!
//! // List all vector indexes for a collection
//! let prefix = StoragePath::new("products", StorageBackend::Vector);
//! let files = storage.list(&prefix).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # S3 Storage (requires `s3` feature)
//!
//! ```ignore
//! use prism_storage::{S3Storage, S3Config, SegmentStorage};
//!
//! // AWS S3
//! let config = S3Config::aws("my-bucket", "us-east-1");
//! let storage = S3Storage::new(config)?;
//!
//! // MinIO / S3-compatible
//! let config = S3Config::minio("local-bucket", "http://localhost:9000")
//!     .with_credentials("minioadmin", "minioadmin");
//! let storage = S3Storage::new(config)?;
//! ```
//!
//! # Cached Storage (L1 local + L2 remote)
//!
//! ```ignore
//! use prism_storage::{CachedStorage, CacheConfig, LocalStorage, SegmentStorage};
//! use std::sync::Arc;
//!
//! // Create L2 (remote) storage
//! let l2: Arc<dyn SegmentStorage> = Arc::new(S3Storage::new(s3_config)?);
//!
//! // Create cached storage with 10GB L1 cache
//! let config = CacheConfig::with_max_size_gb(10);
//! let storage = CachedStorage::new("./cache", l2, config);
//! ```
//!
//! # Features
//!
//! - `s3` - Enable S3/object storage backend (requires `object_store`)
//! - `tantivy-adapter` - Enable Tantivy Directory adapter
//! - `full` - Enable all features

mod cached;
mod compressed;
mod error;
mod local;
mod path;
mod traits;

#[cfg(feature = "s3")]
mod s3;

#[cfg(feature = "tantivy-adapter")]
mod tantivy_adapter;

pub use cached::{CacheConfig, CacheStats, CachedStorage};
pub use compressed::{
    CompressedStorage, CompressionAlgorithm, CompressionConfig, CompressionStats,
};
pub use error::{Result, StorageError};
pub use local::LocalStorage;
pub use path::{StorageBackend, StoragePath};
pub use traits::{ListOptions, ObjectMeta, SegmentStorage, SegmentStorageSync};

#[cfg(feature = "s3")]
pub use s3::{S3Config, S3Storage};

#[cfg(feature = "tantivy-adapter")]
pub use tantivy_adapter::TantivyStorageAdapter;

// Re-export bytes for convenience
pub use bytes::Bytes;

/// Create a storage backend from configuration.
///
/// This is a convenience function for creating the appropriate storage
/// backend based on configuration.
pub fn create_storage(config: &StorageConfig) -> Result<Box<dyn SegmentStorage>> {
    match config {
        StorageConfig::Local { path } => Ok(Box::new(LocalStorage::new(path))),
        #[cfg(feature = "s3")]
        StorageConfig::S3(s3_config) => Ok(Box::new(S3Storage::new(s3_config.clone())?)),
        #[cfg(not(feature = "s3"))]
        StorageConfig::S3(_) => Err(StorageError::Config(
            "S3 storage requires 's3' feature".to_string(),
        )),
        StorageConfig::Cached {
            l1_path,
            l1_max_size_gb,
            l2,
        } => {
            let l2_storage = create_storage(l2)?;
            let config = CacheConfig::with_max_size_gb(*l1_max_size_gb);
            Ok(Box::new(CachedStorage::new(
                l1_path,
                std::sync::Arc::from(l2_storage),
                config,
            )))
        }
        StorageConfig::Compressed {
            algorithm,
            inner,
            min_size,
        } => {
            let inner_storage = create_storage(inner)?;
            let compression_config = match algorithm.as_str() {
                "lz4" => CompressionConfig::lz4(),
                "zstd" => CompressionConfig::zstd(),
                "none" => CompressionConfig::none(),
                other => {
                    // Check for zstd with level (e.g., "zstd:9")
                    if let Some(level_str) = other.strip_prefix("zstd:") {
                        let level: i32 = level_str.parse().map_err(|_| {
                            StorageError::Config(format!("Invalid zstd level: {}", level_str))
                        })?;
                        CompressionConfig::zstd_level(level)
                    } else {
                        return Err(StorageError::Config(format!(
                            "Unknown compression algorithm: {}. Use 'lz4', 'zstd', 'zstd:LEVEL', or 'none'",
                            other
                        )));
                    }
                }
            }
            .with_min_size(*min_size);

            Ok(Box::new(CompressedStorage::new(
                std::sync::Arc::from(inner_storage),
                compression_config,
            )))
        }
    }
}

/// Storage configuration enum.
#[derive(Debug, Clone)]
pub enum StorageConfig {
    /// Local filesystem storage
    Local {
        /// Base path for data
        path: std::path::PathBuf,
    },
    /// S3-compatible object storage
    #[cfg(feature = "s3")]
    S3(S3Config),
    /// S3 placeholder when feature is disabled
    #[cfg(not(feature = "s3"))]
    S3(S3ConfigPlaceholder),
    /// Cached storage (L1 local + L2 remote)
    Cached {
        /// Path for L1 cache
        l1_path: std::path::PathBuf,
        /// Maximum L1 cache size in GB
        l1_max_size_gb: u64,
        /// L2 backend configuration
        l2: Box<StorageConfig>,
    },
    /// Compressed storage wrapper
    Compressed {
        /// Compression algorithm: "lz4", "zstd", "zstd:LEVEL", or "none"
        algorithm: String,
        /// Minimum file size to compress (bytes)
        min_size: usize,
        /// Inner storage configuration
        inner: Box<StorageConfig>,
    },
}

/// Placeholder for S3 config when feature is disabled.
#[cfg(not(feature = "s3"))]
#[derive(Debug, Clone)]
pub struct S3ConfigPlaceholder {
    _private: (),
}

impl Default for StorageConfig {
    fn default() -> Self {
        StorageConfig::Local {
            path: std::path::PathBuf::from("./data"),
        }
    }
}
