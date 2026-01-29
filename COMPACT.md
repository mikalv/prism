# Unified Storage Implementation - Session Compact

**Date:** 2026-01-29
**Issue:** [#42](https://github.com/mikalv/prism/issues/42)
**Design Doc:** `docs/plans/2026-01-29-federation-clustering-design.md`

## Goal

Create a shared `SegmentStorage` trait that all backends (Tantivy, Vector, Graph) use for persistence. This enables pluggable storage (local, S3, tiered) without changing backend code.

## Current State

Each backend has its own storage:
- **Tantivy:** Uses `tantivy::directory::MmapDirectory` directly
- **Vector:** Custom file-based storage (`VectorStore`, `S3VectorStore`)
- **Graph:** File-based storage

## Target Architecture

```
┌─────────────────────────────────────────────────┐
│  Backends                                       │
│  ┌───────────┐ ┌───────────┐ ┌───────────┐     │
│  │ Tantivy   │ │ Vector    │ │ Graph     │     │
│  └─────┬─────┘ └─────┬─────┘ └─────┬─────┘     │
│        │             │             │            │
│        └─────────────┼─────────────┘            │
│                      ▼                          │
│            ┌─────────────────┐                  │
│            │ SegmentStorage  │  ← Unified trait │
│            └────────┬────────┘                  │
│                     │                           │
│        ┌────────────┼────────────┐              │
│        ▼            ▼            ▼              │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐        │
│  │  Local   │ │   S3     │ │  Cached  │        │
│  └──────────┘ └──────────┘ └──────────┘        │
└─────────────────────────────────────────────────┘
```

## Implementation Plan

### Phase 1: Core Trait (prism-storage crate)

```rust
// prism-storage/src/lib.rs

#[async_trait]
pub trait SegmentStorage: Send + Sync {
    async fn write(&self, path: &StoragePath, data: &[u8]) -> Result<()>;
    async fn read(&self, path: &StoragePath) -> Result<Vec<u8>>;
    async fn exists(&self, path: &StoragePath) -> Result<bool>;
    async fn list(&self, prefix: &StoragePath) -> Result<Vec<StoragePath>>;
    async fn delete(&self, path: &StoragePath) -> Result<()>;
    async fn rename(&self, from: &StoragePath, to: &StoragePath) -> Result<()>;
}

/// Hierarchical path: collection/backend/shard/segment
pub struct StoragePath {
    pub collection: String,
    pub backend: StorageBackend,  // tantivy | vector | graph
    pub shard: Option<String>,
    pub segment: String,
}
```

### Phase 2: Implementations

**LocalStorage:**
```rust
pub struct LocalStorage {
    base_path: PathBuf,
}
```

**S3Storage:**
```rust
pub struct S3Storage {
    client: aws_sdk_s3::Client,
    bucket: String,
    prefix: String,
}
```

**CachedStorage (L1 local + L2 remote):**
```rust
pub struct CachedStorage {
    l1: LocalStorage,      // Fast local cache
    l2: Arc<dyn SegmentStorage>,  // S3 or other
    l1_max_size: u64,
}
```

### Phase 3: Tantivy Adapter

Tantivy expects `tantivy::directory::Directory` trait. Create adapter:

```rust
pub struct TantivyStorageAdapter {
    storage: Arc<dyn SegmentStorage>,
    collection: String,
    shard: String,
}

impl Directory for TantivyStorageAdapter {
    // Implement Tantivy's Directory trait using SegmentStorage
}
```

### Phase 4: Migrate Backends

1. **VectorBackend** - Replace `VectorStore`/`S3VectorStore` with `SegmentStorage`
2. **GraphBackend** - Use `SegmentStorage` for edge/node files
3. **TextBackend** - Use `TantivyStorageAdapter`

## Files to Create/Modify

### New Crate: `prism-storage/`
```
prism-storage/
├── Cargo.toml
└── src/
    ├── lib.rs           # SegmentStorage trait
    ├── path.rs          # StoragePath type
    ├── local.rs         # LocalStorage
    ├── s3.rs            # S3Storage
    ├── cached.rs        # CachedStorage
    └── tantivy.rs       # TantivyStorageAdapter
```

### Modify Existing
```
prism/Cargo.toml                      # Add prism-storage dependency
prism/src/backends/vector/backend.rs  # Use SegmentStorage
prism/src/backends/vector/store.rs    # Deprecate/remove
prism/src/backends/text.rs            # Use TantivyStorageAdapter
prism/src/config/mod.rs               # Add StorageConfig
```

## Configuration

```toml
[storage]
backend = "local"  # local | s3 | cached

[storage.local]
data_dir = "./data"

[storage.s3]
bucket = "prism-data"
region = "eu-west-1"
prefix = "collections/"

[storage.cached]
l1_path = "./cache"
l1_max_size_gb = 50
l2_backend = "s3"
```

## Segment Path Examples

```
products/tantivy/shard_0/segment_00001.si
products/tantivy/shard_0/segment_00001.fdx
products/vector/shard_0/hnsw_00001.bin
products/graph/shard_0/edges_00001.bin
```

## Dependencies

```toml
# prism-storage/Cargo.toml
[dependencies]
async-trait = "0.1"
tokio = { version = "1", features = ["fs"] }
aws-sdk-s3 = { version = "1", optional = true }
thiserror = "1"

[features]
default = []
s3 = ["aws-sdk-s3"]
```

## Testing Strategy

1. **Unit tests** per implementation (LocalStorage, S3Storage)
2. **Integration tests** with actual S3 (MinIO in CI)
3. **Backend tests** verifying Tantivy/Vector/Graph work with new storage

## Key Considerations

- **Tantivy Directory trait** is synchronous - need blocking adapter or async runtime bridge
- **Memory mapping** - LocalStorage should support mmap for Tantivy perf
- **Atomic operations** - rename must be atomic for crash safety
- **S3 eventual consistency** - handle with retries/versioning

## Related Issues

- #30 - Collection export/import (uses SegmentStorage)
- #40 - HNSW sharding (vector segments)
- #41 - Graph sharding (graph segments)
- #45 - ILM (tiered storage)
- #54 - Crate organization (prism-storage as first prism-* crate)

## Success Criteria

- [x] `SegmentStorage` trait defined
- [x] `LocalStorage` implementation working
- [x] `S3Storage` implementation working (41 tests pass)
- [x] `CachedStorage` (L1+L2) implementation working
- [x] `TantivyStorageAdapter` passes Tantivy tests (7 tests pass)
- [x] `SegmentStorageVectorAdapter` bridges to VectorStore trait
- [x] `StorageFactory` updated with `create_segment_storage()` and `create_vector_store_v2()`
- [x] All existing tests pass (workspace: 145+ tests)
- [x] VectorBackend has `with_segment_storage()` constructor for SegmentStorage
- [x] TextBackend has `initialize_with_segment_storage()` for unified storage/S3
- [x] New unified storage configuration (`UnifiedStorageConfig` in prism.toml)
- [x] GraphBackend uses SegmentStorage (7 tests pass)
