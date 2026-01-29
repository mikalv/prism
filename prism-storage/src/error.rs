//! Storage error types.

use std::io;
use thiserror::Error;

/// Storage operation errors.
#[derive(Error, Debug)]
pub enum StorageError {
    /// I/O error during storage operation
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Path not found
    #[error("Path not found: {0}")]
    NotFound(String),

    /// Path already exists
    #[error("Path already exists: {0}")]
    AlreadyExists(String),

    /// Invalid path format
    #[error("Invalid path: {0}")]
    InvalidPath(String),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Storage backend error
    #[error("Backend error: {0}")]
    Backend(String),

    /// Object store error
    #[cfg(feature = "s3")]
    #[error("Object store error: {0}")]
    ObjectStore(#[from] object_store::Error),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Operation not supported
    #[error("Operation not supported: {0}")]
    NotSupported(String),
}

/// Result type for storage operations.
pub type Result<T> = std::result::Result<T, StorageError>;

impl StorageError {
    /// Check if this is a "not found" error.
    pub fn is_not_found(&self) -> bool {
        matches!(self, StorageError::NotFound(_))
            || matches!(self, StorageError::Io(e) if e.kind() == io::ErrorKind::NotFound)
    }

    /// Check if this is a permission error.
    pub fn is_permission_denied(&self) -> bool {
        matches!(self, StorageError::PermissionDenied(_))
            || matches!(self, StorageError::Io(e) if e.kind() == io::ErrorKind::PermissionDenied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_not_found() {
        let err = StorageError::NotFound("test".to_string());
        assert!(err.is_not_found());

        let io_err = StorageError::Io(io::Error::new(io::ErrorKind::NotFound, "not found"));
        assert!(io_err.is_not_found());
    }

    #[test]
    fn test_error_display() {
        let err = StorageError::NotFound("products/vector/index.bin".to_string());
        assert_eq!(
            err.to_string(),
            "Path not found: products/vector/index.bin"
        );
    }
}
