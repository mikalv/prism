//! Unified storage configuration.
//!
//! Supports local filesystem, S3, and tiered (cached) storage backends.
//!
//! # Configuration Examples
//!
//! ## Local Storage (Default)
//!
//! ```toml
//! [storage]
//! backend = "local"
//! data_dir = "~/.prism/data"
//! ```
//!
//! ## S3 Storage
//!
//! ```toml
//! [storage]
//! backend = "s3"
//!
//! [storage.s3]
//! bucket = "my-prism-bucket"
//! region = "us-east-1"
//! prefix = "collections/"
//! ```
//!
//! ## MinIO / S3-Compatible
//!
//! ```toml
//! [storage]
//! backend = "s3"
//!
//! [storage.s3]
//! bucket = "local-bucket"
//! region = "us-east-1"
//! endpoint = "http://localhost:9000"
//! force_path_style = true
//! ```
//!
//! ## Cached Storage (L1 local + L2 S3)
//!
//! ```toml
//! [storage]
//! backend = "cached"
//!
//! [storage.cache]
//! l1_path = "~/.prism/cache"
//! l1_max_size_gb = 10
//!
//! [storage.s3]
//! bucket = "my-prism-bucket"
//! region = "us-east-1"
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use prism_storage::{LocalStorage, SegmentStorage};

#[cfg(feature = "storage-s3")]
use prism_storage::{CacheConfig, CachedStorage};

/// Unified storage configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnifiedStorageConfig {
    /// Storage backend type: "local", "s3", or "cached"
    #[serde(default = "default_backend")]
    pub backend: String,

    /// Local storage path (used for "local" backend or as default)
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,

    /// S3 configuration (for "s3" or "cached" backends)
    #[serde(default)]
    pub s3: Option<S3StorageConfig>,

    /// Cache configuration (for "cached" backend)
    #[serde(default)]
    pub cache: Option<CacheStorageConfig>,

    /// Buffer directory for Tantivy writes (temporary local storage)
    #[serde(default = "default_buffer_dir")]
    pub buffer_dir: PathBuf,
}

fn default_backend() -> String {
    "local".to_string()
}

fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".prism")
        .join("data")
}

fn default_buffer_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".prism")
        .join("buffer")
}

/// S3-compatible storage configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct S3StorageConfig {
    /// S3 bucket name
    pub bucket: String,

    /// AWS region
    #[serde(default = "default_region")]
    pub region: String,

    /// Optional prefix for all paths
    #[serde(default)]
    pub prefix: Option<String>,

    /// Custom endpoint (for MinIO, etc.)
    #[serde(default)]
    pub endpoint: Option<String>,

    /// Use path-style requests (required for MinIO)
    #[serde(default)]
    pub force_path_style: bool,

    /// Access key ID (optional, uses AWS credential chain if not set)
    #[serde(default)]
    pub access_key_id: Option<String>,

    /// Secret access key (optional)
    #[serde(default)]
    pub secret_access_key: Option<String>,
}

fn default_region() -> String {
    "us-east-1".to_string()
}

/// Cache layer configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CacheStorageConfig {
    /// Path for L1 (local) cache
    #[serde(default = "default_cache_path")]
    pub l1_path: PathBuf,

    /// Maximum size of L1 cache in GB
    #[serde(default = "default_cache_size")]
    pub l1_max_size_gb: u64,

    /// Write-through to L2 on writes (default: true)
    #[serde(default = "default_true")]
    pub write_through: bool,
}

fn default_cache_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".prism")
        .join("cache")
}

fn default_cache_size() -> u64 {
    10 // 10 GB default
}

fn default_true() -> bool {
    true
}

impl Default for UnifiedStorageConfig {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            data_dir: default_data_dir(),
            s3: None,
            cache: None,
            buffer_dir: default_buffer_dir(),
        }
    }
}

impl Default for CacheStorageConfig {
    fn default() -> Self {
        Self {
            l1_path: default_cache_path(),
            l1_max_size_gb: default_cache_size(),
            write_through: true,
        }
    }
}

impl UnifiedStorageConfig {
    /// Create a SegmentStorage from this configuration.
    pub fn create_storage(&self) -> Result<Arc<dyn SegmentStorage>, crate::Error> {
        match self.backend.as_str() {
            "local" => Ok(Arc::new(LocalStorage::new(&self.data_dir))),
            #[cfg(feature = "storage-s3")]
            "s3" => {
                let s3_config = self.s3.as_ref().ok_or_else(|| {
                    crate::Error::Config("S3 backend requires [storage.s3] configuration".into())
                })?;

                use prism_storage::{S3Config, S3Storage};

                let mut config = if s3_config.endpoint.is_some() {
                    S3Config::minio(&s3_config.bucket, s3_config.endpoint.as_ref().unwrap())
                } else {
                    S3Config::aws(&s3_config.bucket, &s3_config.region)
                };

                if let Some(prefix) = &s3_config.prefix {
                    config = config.with_prefix(prefix);
                }

                if let (Some(key_id), Some(secret)) =
                    (&s3_config.access_key_id, &s3_config.secret_access_key)
                {
                    config = config.with_credentials(key_id, secret);
                }

                let storage =
                    S3Storage::new(config).map_err(|e| crate::Error::Storage(e.to_string()))?;
                Ok(Arc::new(storage))
            }
            #[cfg(feature = "storage-s3")]
            "cached" => {
                let s3_config = self.s3.as_ref().ok_or_else(|| {
                    crate::Error::Config(
                        "Cached backend requires [storage.s3] configuration for L2".into(),
                    )
                })?;
                let cache_config = self.cache.clone().unwrap_or_default();

                use prism_storage::{S3Config, S3Storage};

                // Create L2 (S3)
                let mut config = if s3_config.endpoint.is_some() {
                    S3Config::minio(&s3_config.bucket, s3_config.endpoint.as_ref().unwrap())
                } else {
                    S3Config::aws(&s3_config.bucket, &s3_config.region)
                };

                if let Some(prefix) = &s3_config.prefix {
                    config = config.with_prefix(prefix);
                }

                if let (Some(key_id), Some(secret)) =
                    (&s3_config.access_key_id, &s3_config.secret_access_key)
                {
                    config = config.with_credentials(key_id, secret);
                }

                let l2: Arc<dyn SegmentStorage> = Arc::new(
                    S3Storage::new(config).map_err(|e| crate::Error::Storage(e.to_string()))?,
                );

                // Create cached storage
                let prism_cache_config = CacheConfig {
                    max_size_bytes: cache_config.l1_max_size_gb * 1024 * 1024 * 1024,
                    write_through: cache_config.write_through,
                    populate_on_read: true,
                };

                let cached = CachedStorage::new(&cache_config.l1_path, l2, prism_cache_config);
                Ok(Arc::new(cached))
            }
            #[cfg(not(feature = "storage-s3"))]
            "s3" | "cached" => Err(crate::Error::Config(
                "S3 storage requires 'storage-s3' feature".into(),
            )),
            other => Err(crate::Error::Config(format!(
                "Unknown storage backend: '{}'. Valid options: local, s3, cached",
                other
            ))),
        }
    }

    /// Get the buffer directory for Tantivy writes.
    pub fn buffer_dir(&self) -> &PathBuf {
        &self.buffer_dir
    }

    /// Check if using remote storage (S3 or cached).
    pub fn is_remote(&self) -> bool {
        matches!(self.backend.as_str(), "s3" | "cached")
    }

    /// Check if using local-only storage.
    pub fn is_local(&self) -> bool {
        self.backend == "local"
    }

    /// Expand ~ in all paths.
    pub fn expand_paths(&mut self) -> Result<(), crate::Error> {
        self.data_dir = crate::config::expand_tilde(&self.data_dir)
            .map_err(|e| crate::Error::Config(e.to_string()))?;
        self.buffer_dir = crate::config::expand_tilde(&self.buffer_dir)
            .map_err(|e| crate::Error::Config(e.to_string()))?;

        if let Some(ref mut cache) = self.cache {
            cache.l1_path = crate::config::expand_tilde(&cache.l1_path)
                .map_err(|e| crate::Error::Config(e.to_string()))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = UnifiedStorageConfig::default();
        assert_eq!(config.backend, "local");
        assert!(config.data_dir.to_string_lossy().contains(".prism"));
    }

    #[test]
    fn test_local_storage_creation() {
        let config = UnifiedStorageConfig::default();
        let storage = config.create_storage().unwrap();
        assert_eq!(storage.backend_name(), "local");
    }

    #[test]
    fn test_config_parsing_local() {
        let toml = r#"
            backend = "local"
            data_dir = "/tmp/prism-test"
        "#;
        let config: UnifiedStorageConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.backend, "local");
        assert_eq!(config.data_dir, PathBuf::from("/tmp/prism-test"));
    }

    #[test]
    fn test_config_parsing_s3() {
        let toml = r#"
            backend = "s3"

            [s3]
            bucket = "my-bucket"
            region = "eu-west-1"
            prefix = "data/"
        "#;
        let config: UnifiedStorageConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.backend, "s3");
        let s3 = config.s3.unwrap();
        assert_eq!(s3.bucket, "my-bucket");
        assert_eq!(s3.region, "eu-west-1");
        assert_eq!(s3.prefix, Some("data/".to_string()));
    }

    #[test]
    fn test_config_parsing_cached() {
        let toml = r#"
            backend = "cached"

            [s3]
            bucket = "my-bucket"
            region = "us-east-1"

            [cache]
            l1_path = "/tmp/cache"
            l1_max_size_gb = 20
        "#;
        let config: UnifiedStorageConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.backend, "cached");
        assert!(config.s3.is_some());
        let cache = config.cache.unwrap();
        assert_eq!(cache.l1_max_size_gb, 20);
    }

    #[test]
    fn test_is_remote() {
        let mut config = UnifiedStorageConfig::default();
        assert!(!config.is_remote());
        assert!(config.is_local());

        config.backend = "s3".to_string();
        assert!(config.is_remote());
        assert!(!config.is_local());
    }
}
