//! Hierarchical storage paths for Prism data.
//!
//! Storage paths follow the pattern: `collection/backend/shard/segment`
//!
//! All path components are validated against directory traversal attacks.
//! Components containing ".", "..", null bytes, backslashes, or empty
//! segments are rejected at construction time.
//!
//! # Examples
//!
//! ```
//! use prism_storage::{StoragePath, StorageBackend};
//!
//! // Vector index segment
//! let path = StoragePath::new("products", StorageBackend::Vector)
//!     .unwrap()
//!     .with_shard("shard_0").unwrap()
//!     .with_segment("hnsw_00001.bin").unwrap();
//!
//! assert_eq!(path.to_string(), "products/vector/shard_0/hnsw_00001.bin");
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

// ─── Error type ──────────────────────────────────────────────────────────────

/// Errors from invalid storage path components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoragePathError {
    /// Component is empty (e.g. from consecutive slashes without other content)
    EmptyComponent,
    /// Component is "." or ".." (directory traversal)
    TraversalComponent(String),
    /// Component contains null bytes
    NullByte,
    /// Component contains backslash (Windows path separator)
    Backslash,
    /// Path has too few segments to be valid (need at least collection/backend/segment)
    TooFewSegments,
    /// Backend type is unknown
    UnknownBackend(String),
}

impl fmt::Display for StoragePathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyComponent => write!(f, "empty path component"),
            Self::TraversalComponent(c) => write!(f, "directory traversal component: {:?}", c),
            Self::NullByte => write!(f, "null byte in path component"),
            Self::Backslash => write!(f, "backslash in path component"),
            Self::TooFewSegments => write!(f, "path has too few segments"),
            Self::UnknownBackend(b) => write!(f, "unknown backend: {}", b),
        }
    }
}

impl std::error::Error for StoragePathError {}

// ─── Component validation ────────────────────────────────────────────────────

/// Validate a single path component.
///
/// Rejects:
/// - Empty strings (from consecutive slashes)
/// - "." and ".." (directory traversal)
/// - Strings containing null bytes (C-string truncation attacks)
/// - Strings containing backslashes (Windows path traversal)
fn validate_component(component: &str) -> Result<(), StoragePathError> {
    if component.is_empty() {
        return Err(StoragePathError::EmptyComponent);
    }
    if component == "." || component == ".." {
        return Err(StoragePathError::TraversalComponent(component.to_string()));
    }
    if component.as_bytes().contains(&0) {
        return Err(StoragePathError::NullByte);
    }
    if component.contains('\\') {
        return Err(StoragePathError::Backslash);
    }
    Ok(())
}

// ─── Backend enum ────────────────────────────────────────────────────────────

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

impl FromStr for StorageBackend {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "tantivy" | "text" => Ok(StorageBackend::Tantivy),
            "vector" | "hnsw" => Ok(StorageBackend::Vector),
            "graph" => Ok(StorageBackend::Graph),
            "meta" | "metadata" => Ok(StorageBackend::Meta),
            _ => Err(format!("unknown storage backend: {}", s)),
        }
    }
}

// ─── StoragePath ─────────────────────────────────────────────────────────────

/// Hierarchical path for storage operations.
///
/// Format: `collection/backend/[shard/]segment`
///
/// All components are validated at construction time. It is not possible to
/// create a `StoragePath` containing traversal sequences, null bytes, or
/// other dangerous characters.
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
    ///
    /// Returns an error if the collection name is invalid.
    pub fn new(
        collection: impl Into<String>,
        backend: StorageBackend,
    ) -> Result<Self, StoragePathError> {
        let collection = collection.into();
        validate_component(&collection)?;
        Ok(Self {
            collection,
            backend,
            shard: None,
            segment: String::new(),
        })
    }

    /// Add shard to path.
    ///
    /// Returns an error if the shard name is invalid.
    pub fn with_shard(mut self, shard: impl Into<String>) -> Result<Self, StoragePathError> {
        let shard = shard.into();
        validate_component(&shard)?;
        self.shard = Some(shard);
        Ok(self)
    }

    /// Add segment/file name to path.
    ///
    /// The segment may contain forward slashes for sub-paths (e.g. "subdir/file.bin").
    /// Each component is validated individually.
    ///
    /// Returns an error if any component is invalid.
    pub fn with_segment(mut self, segment: impl Into<String>) -> Result<Self, StoragePathError> {
        let segment = segment.into();
        for component in segment.split('/') {
            validate_component(component)?;
        }
        self.segment = segment;
        Ok(self)
    }

    /// Create path for collection metadata.
    ///
    /// Returns an error if collection or filename is invalid.
    pub fn collection_meta(
        collection: impl Into<String>,
        filename: impl Into<String>,
    ) -> Result<Self, StoragePathError> {
        let collection = collection.into();
        let filename = filename.into();
        validate_component(&collection)?;
        // Filename may contain sub-paths — validate each component
        for component in filename.split('/') {
            validate_component(component)?;
        }
        Ok(Self {
            collection,
            backend: StorageBackend::Meta,
            shard: None,
            segment: filename,
        })
    }

    /// Create path for tantivy segment.
    ///
    /// Returns an error if any component is invalid.
    pub fn tantivy(
        collection: impl Into<String>,
        shard: impl Into<String>,
        segment: impl Into<String>,
    ) -> Result<Self, StoragePathError> {
        let collection = collection.into();
        let shard = shard.into();
        let segment = segment.into();
        validate_component(&collection)?;
        validate_component(&shard)?;
        validate_component(&segment)?;
        Ok(Self {
            collection,
            backend: StorageBackend::Tantivy,
            shard: Some(shard),
            segment,
        })
    }

    /// Create path for vector index.
    ///
    /// Returns an error if any component is invalid.
    pub fn vector(
        collection: impl Into<String>,
        shard: impl Into<String>,
        segment: impl Into<String>,
    ) -> Result<Self, StoragePathError> {
        let collection = collection.into();
        let shard = shard.into();
        let segment = segment.into();
        validate_component(&collection)?;
        validate_component(&shard)?;
        validate_component(&segment)?;
        Ok(Self {
            collection,
            backend: StorageBackend::Vector,
            shard: Some(shard),
            segment,
        })
    }

    /// Create path for graph storage.
    ///
    /// Returns an error if any component is invalid.
    pub fn graph(
        collection: impl Into<String>,
        shard: impl Into<String>,
        segment: impl Into<String>,
    ) -> Result<Self, StoragePathError> {
        let collection = collection.into();
        let shard = shard.into();
        let segment = segment.into();
        validate_component(&collection)?;
        validate_component(&shard)?;
        validate_component(&segment)?;
        Ok(Self {
            collection,
            backend: StorageBackend::Graph,
            shard: Some(shard),
            segment,
        })
    }

    /// Get the directory prefix (without segment).
    pub fn prefix(&self) -> String {
        match &self.shard {
            Some(shard) => format!("{}/{}/{}", self.collection, self.backend, shard),
            None => format!("{}/{}", self.collection, self.backend),
        }
    }

    /// Convert to filesystem path.
    ///
    /// Safe because all components were validated at construction time.
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
    ///
    /// Consecutive slashes are collapsed. Leading/trailing slashes are ignored.
    /// Returns an error if any component fails validation.
    pub fn parse(s: &str) -> Result<Self, StoragePathError> {
        // Split and filter empty components (handles consecutive slashes and leading/trailing slashes)
        let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty()).collect();

        if parts.len() < 3 {
            return Err(StoragePathError::TooFewSegments);
        }

        // Validate every component
        for part in &parts {
            validate_component(part)?;
        }

        let collection = parts[0].to_string();
        let backend = parts[1]
            .parse::<StorageBackend>()
            .map_err(|_| StoragePathError::UnknownBackend(parts[1].to_string()))?;

        if parts.len() == 3 {
            // collection/backend/segment
            Ok(Self {
                collection,
                backend,
                shard: None,
                segment: parts[2].to_string(),
            })
        } else {
            // collection/backend/shard/segment[/subsegment...]
            Ok(Self {
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
                write!(
                    f,
                    "{}/{}/{}/{}",
                    self.collection, self.backend, shard, self.segment
                )
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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // --- Functional tests (updated for Result return types) ---

    #[test]
    fn test_storage_path_display() {
        let path = StoragePath::new("products", StorageBackend::Vector)
            .unwrap()
            .with_shard("shard_0")
            .unwrap()
            .with_segment("hnsw_00001.bin")
            .unwrap();
        assert_eq!(path.to_string(), "products/vector/shard_0/hnsw_00001.bin");
    }

    #[test]
    fn test_storage_path_no_shard() {
        let path = StoragePath::collection_meta("products", "schema.json").unwrap();
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
        let path = StoragePath::tantivy("products", "shard_0", "segment.si").unwrap();
        assert_eq!(path.prefix(), "products/tantivy/shard_0");
    }

    #[test]
    fn test_storage_path_to_path_buf() {
        let base = std::path::Path::new("/data");
        let path = StoragePath::vector("products", "shard_0", "index.bin").unwrap();
        assert_eq!(
            path.to_path_buf(base),
            PathBuf::from("/data/products/vector/shard_0/index.bin")
        );
    }

    #[test]
    fn test_backend_from_str() {
        assert_eq!(
            "tantivy".parse::<StorageBackend>().ok(),
            Some(StorageBackend::Tantivy)
        );
        assert_eq!(
            "text".parse::<StorageBackend>().ok(),
            Some(StorageBackend::Tantivy)
        );
        assert_eq!(
            "VECTOR".parse::<StorageBackend>().ok(),
            Some(StorageBackend::Vector)
        );
        assert!("unknown".parse::<StorageBackend>().is_err());
    }

    // --- Security tests ---

    #[test]
    fn test_parse_rejects_parent_traversal() {
        assert!(StoragePath::parse("products/vector/../../../etc/passwd").is_err());
        assert!(StoragePath::parse("products/vector/shard/../file").is_err());
        assert!(StoragePath::parse("../../../etc/passwd").is_err());
    }

    #[test]
    fn test_parse_rejects_dot_component() {
        assert!(StoragePath::parse("products/vector/./file.bin").is_err());
        assert!(StoragePath::parse("./products/vector/file.bin").is_err());
    }

    #[test]
    fn test_parse_collapses_consecutive_slashes() {
        // Consecutive slashes are filtered — resulting path is valid
        let path = StoragePath::parse("products///vector///file.bin").unwrap();
        assert_eq!(path.collection, "products");
        assert_eq!(path.segment, "file.bin");
    }

    #[test]
    fn test_parse_handles_leading_trailing_slashes() {
        let path = StoragePath::parse("/products/vector/file.bin/").unwrap();
        assert_eq!(path.collection, "products");
        assert_eq!(path.segment, "file.bin");
    }

    #[test]
    fn test_parse_rejects_null_bytes() {
        assert!(StoragePath::parse("products/vector/shard\0evil/file.bin").is_err());
        assert!(StoragePath::parse("products/vector/\0/file.bin").is_err());
    }

    #[test]
    fn test_parse_rejects_backslash() {
        assert!(StoragePath::parse("products/vector/..\\..\\etc\\passwd").is_err());
    }

    #[test]
    fn test_with_shard_rejects_traversal() {
        let base = StoragePath::new("products", StorageBackend::Vector).unwrap();
        assert!(base.clone().with_shard("..").is_err());
        assert!(base.clone().with_shard(".").is_err());
        assert!(base.clone().with_shard("").is_err());
        assert!(base.clone().with_shard("valid_shard").is_ok());
    }

    #[test]
    fn test_with_segment_rejects_traversal() {
        let base = StoragePath::new("products", StorageBackend::Vector).unwrap();
        assert!(base.clone().with_segment("../../etc/passwd").is_err());
        assert!(base.clone().with_segment("../file").is_err());
        assert!(base.clone().with_segment("valid/file.bin").is_ok());
    }

    #[test]
    fn test_new_rejects_bad_collection() {
        assert!(StoragePath::new("..", StorageBackend::Vector).is_err());
        assert!(StoragePath::new(".", StorageBackend::Vector).is_err());
        assert!(StoragePath::new("", StorageBackend::Vector).is_err());
        assert!(StoragePath::new("valid", StorageBackend::Vector).is_ok());
    }

    #[test]
    fn test_collection_meta_rejects_traversal() {
        assert!(StoragePath::collection_meta("..", "file.json").is_err());
        assert!(StoragePath::collection_meta("valid", "../secret").is_err());
    }

    // --- Fuzz crash reproductions ---

    #[test]
    fn test_fuzz_crash_1_dot_via_slash_collapse() {
        // Input: /vector////.////
        assert!(StoragePath::parse("/vector////.////").is_err());
    }

    #[test]
    fn test_fuzz_crash_2_parent_directory() {
        // Input: /vector///fddddddtd/..//
        assert!(StoragePath::parse("/vector///fddddddtd/..//").is_err());
    }

    #[test]
    fn test_fuzz_crash_3_null_bytes_and_dot() {
        // Input containing null bytes and ./
        assert!(StoragePath::parse("/vector/\x01///\x00/./").is_err());
    }

    #[test]
    fn test_to_path_buf_stays_under_base() {
        let base = std::path::Path::new("/data/storage");
        let path = StoragePath::parse("products/vector/shard_0/index.bin").unwrap();
        let buf = path.to_path_buf(base);
        let s = buf.to_string_lossy();
        assert!(
            s.starts_with("/data/storage/"),
            "Path escaped base directory: {}",
            s
        );
        assert!(!s.contains("/../"));
        assert!(!s.contains("/./"));
    }
}
