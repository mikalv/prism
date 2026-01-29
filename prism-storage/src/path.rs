//! Hierarchical storage paths for Prism data.
//!
//! Storage paths follow the pattern: `collection/backend/shard/segment`
//!
//! # Examples
//!
//! ```
//! use prism_storage::{StoragePath, StorageBackend};
//!
//! // Vector index segment
//! let path = StoragePath::new("products", StorageBackend::Vector)
//!     .with_shard("shard_0")
//!     .with_segment("hnsw_00001.bin");
//!
//! assert_eq!(path.to_string(), "products/vector/shard_0/hnsw_00001.bin");
//!
//! // Tantivy segment
//! let path = StoragePath::new("products", StorageBackend::Tantivy)
//!     .with_shard("shard_0")
//!     .with_segment("segment_00001.si");
//!
//! assert_eq!(path.to_string(), "products/tantivy/shard_0/segment_00001.si");
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Backend type for storage organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    /// Full-text search index (Tantivy)
    Tantivy,
    /// Vector/embedding index (HNSW, etc.)
    Vector,
    /// Graph/relationship storage
    Graph,
    /// Collection metadata
    Meta,
}

impl fmt::Display for StorageBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageBackend::Tantivy => write!(f, "tantivy"),
            StorageBackend::Vector => write!(f, "vector"),
            StorageBackend::Graph => write!(f, "graph"),
            StorageBackend::Meta => write!(f, "meta"),
        }
    }
}

impl StorageBackend {
    /// Parse backend from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "tantivy" | "text" => Some(StorageBackend::Tantivy),
            "vector" | "hnsw" => Some(StorageBackend::Vector),
            "graph" => Some(StorageBackend::Graph),
            "meta" | "metadata" => Some(StorageBackend::Meta),
            _ => None,
        }
    }
}

/// Hierarchical path for storage operations.
///
/// Format: `collection/backend/[shard/]segment`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoragePath {
    /// Collection name
    pub collection: String,
    /// Backend type (tantivy, vector, graph, meta)
    pub backend: StorageBackend,
    /// Optional shard identifier
    pub shard: Option<String>,
    /// Segment or file name
    pub segment: String,
}

impl StoragePath {
    /// Create a new storage path.
    pub fn new(collection: impl Into<String>, backend: StorageBackend) -> Self {
        Self {
            collection: collection.into(),
            backend,
            shard: None,
            segment: String::new(),
        }
    }

    /// Add shard to path.
    pub fn with_shard(mut self, shard: impl Into<String>) -> Self {
        self.shard = Some(shard.into());
        self
    }

    /// Add segment/file name to path.
    pub fn with_segment(mut self, segment: impl Into<String>) -> Self {
        self.segment = segment.into();
        self
    }

    /// Create path for collection metadata.
    pub fn collection_meta(collection: impl Into<String>, filename: impl Into<String>) -> Self {
        Self {
            collection: collection.into(),
            backend: StorageBackend::Meta,
            shard: None,
            segment: filename.into(),
        }
    }

    /// Create path for tantivy segment.
    pub fn tantivy(
        collection: impl Into<String>,
        shard: impl Into<String>,
        segment: impl Into<String>,
    ) -> Self {
        Self {
            collection: collection.into(),
            backend: StorageBackend::Tantivy,
            shard: Some(shard.into()),
            segment: segment.into(),
        }
    }

    /// Create path for vector index.
    pub fn vector(
        collection: impl Into<String>,
        shard: impl Into<String>,
        segment: impl Into<String>,
    ) -> Self {
        Self {
            collection: collection.into(),
            backend: StorageBackend::Vector,
            shard: Some(shard.into()),
            segment: segment.into(),
        }
    }

    /// Create path for graph storage.
    pub fn graph(
        collection: impl Into<String>,
        shard: impl Into<String>,
        segment: impl Into<String>,
    ) -> Self {
        Self {
            collection: collection.into(),
            backend: StorageBackend::Graph,
            shard: Some(shard.into()),
            segment: segment.into(),
        }
    }

    /// Get the directory prefix (without segment).
    pub fn prefix(&self) -> String {
        match &self.shard {
            Some(shard) => format!("{}/{}/{}", self.collection, self.backend, shard),
            None => format!("{}/{}", self.collection, self.backend),
        }
    }

    /// Convert to filesystem path.
    pub fn to_path_buf(&self, base: &std::path::Path) -> PathBuf {
        let mut path = base.join(&self.collection).join(self.backend.to_string());
        if let Some(shard) = &self.shard {
            path = path.join(shard);
        }
        if !self.segment.is_empty() {
            path = path.join(&self.segment);
        }
        path
    }

    /// Parse from string representation.
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() < 3 {
            return None;
        }

        let collection = parts[0].to_string();
        let backend = StorageBackend::from_str(parts[1])?;

        if parts.len() == 3 {
            // collection/backend/segment
            Some(Self {
                collection,
                backend,
                shard: None,
                segment: parts[2].to_string(),
            })
        } else {
            // collection/backend/shard/segment
            Some(Self {
                collection,
                backend,
                shard: Some(parts[2].to_string()),
                segment: parts[3..].join("/"),
            })
        }
    }

    /// Check if this path is a directory prefix (no segment).
    pub fn is_prefix(&self) -> bool {
        self.segment.is_empty()
    }
}

impl fmt::Display for StoragePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.shard {
            Some(shard) if !self.segment.is_empty() => {
                write!(f, "{}/{}/{}/{}", self.collection, self.backend, shard, self.segment)
            }
            Some(shard) => {
                write!(f, "{}/{}/{}", self.collection, self.backend, shard)
            }
            None if !self.segment.is_empty() => {
                write!(f, "{}/{}/{}", self.collection, self.backend, self.segment)
            }
            None => {
                write!(f, "{}/{}", self.collection, self.backend)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_path_display() {
        let path = StoragePath::new("products", StorageBackend::Vector)
            .with_shard("shard_0")
            .with_segment("hnsw_00001.bin");
        assert_eq!(path.to_string(), "products/vector/shard_0/hnsw_00001.bin");
    }

    #[test]
    fn test_storage_path_no_shard() {
        let path = StoragePath::collection_meta("products", "schema.json");
        assert_eq!(path.to_string(), "products/meta/schema.json");
    }

    #[test]
    fn test_storage_path_parse() {
        let path = StoragePath::parse("products/vector/shard_0/hnsw_00001.bin").unwrap();
        assert_eq!(path.collection, "products");
        assert_eq!(path.backend, StorageBackend::Vector);
        assert_eq!(path.shard, Some("shard_0".to_string()));
        assert_eq!(path.segment, "hnsw_00001.bin");
    }

    #[test]
    fn test_storage_path_parse_no_shard() {
        let path = StoragePath::parse("products/meta/schema.json").unwrap();
        assert_eq!(path.collection, "products");
        assert_eq!(path.backend, StorageBackend::Meta);
        assert_eq!(path.shard, None);
        assert_eq!(path.segment, "schema.json");
    }

    #[test]
    fn test_storage_path_prefix() {
        let path = StoragePath::tantivy("products", "shard_0", "segment.si");
        assert_eq!(path.prefix(), "products/tantivy/shard_0");
    }

    #[test]
    fn test_storage_path_to_path_buf() {
        let base = std::path::Path::new("/data");
        let path = StoragePath::vector("products", "shard_0", "index.bin");
        assert_eq!(
            path.to_path_buf(base),
            PathBuf::from("/data/products/vector/shard_0/index.bin")
        );
    }

    #[test]
    fn test_backend_from_str() {
        assert_eq!(StorageBackend::from_str("tantivy"), Some(StorageBackend::Tantivy));
        assert_eq!(StorageBackend::from_str("text"), Some(StorageBackend::Tantivy));
        assert_eq!(StorageBackend::from_str("VECTOR"), Some(StorageBackend::Vector));
        assert_eq!(StorageBackend::from_str("unknown"), None);
    }
}
