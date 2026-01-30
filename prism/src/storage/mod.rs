//! Unified storage backends for all Prism data.
//!
//! All backends (Text/Tantivy, Vector, Graph) use the SegmentStorage trait
//! from prism-storage for persistence. This enables pluggable storage backends
//! (local filesystem, S3, cached) without changing backend code.
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

mod config;
mod factory;

pub use config::{LocalConfig, S3Config, StorageConfig};
pub use factory::StorageFactory;

// Re-export prism-storage types for convenience (the unified storage layer)
pub use prism_storage::{
    Bytes, CacheConfig, CacheStats, CachedStorage, LocalStorage, SegmentStorage,
    StorageBackend, StoragePath, TantivyStorageAdapter,
};

#[cfg(feature = "storage-s3")]
pub use prism_storage::S3Storage;
