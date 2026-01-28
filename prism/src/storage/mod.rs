//! Storage backends for Tantivy indexes.
//!
//! Supports local filesystem (default) and S3-compatible object storage.
//!
//! # Configuration
//!
//! ```json
//! { "storage": { "type": "local" } }
//! ```
//!
//! ```json
//! {
//!   "storage": {
//!     "type": "s3",
//!     "bucket": "my-bucket",
//!     "region": "us-east-1",
//!     "prefix": "indexes/"
//!   }
//! }
//! ```
//!
//! # S3-Compatible (MinIO)
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

mod config;
mod vector_store;

pub use config::{LocalConfig, S3Config, StorageConfig};
pub use vector_store::{LocalVectorStore, VectorStore};

#[cfg(feature = "storage-s3")]
pub use vector_store::S3VectorStore;

#[cfg(feature = "storage-s3")]
mod object_store_directory;

#[cfg(feature = "storage-s3")]
pub use object_store_directory::ObjectStoreDirectory;
