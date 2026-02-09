//! Compressed storage wrapper with LZ4/Zstd support.
//!
//! Provides transparent compression and decompression for stored data.
//! Wraps any `SegmentStorage` implementation to add compression.
//!
//! # Compression Algorithms
//!
//! | Algorithm | Speed | Ratio | Best For |
//! |-----------|-------|-------|----------|
//! | LZ4 | Very fast | Moderate | Latency-sensitive paths |
//! | Zstd | Fast | Good | Storage efficiency |
//!
//! # Usage
//!
//! ```ignore
//! use prism_storage::{CompressedStorage, CompressionConfig, LocalStorage};
//! use std::sync::Arc;
//!
//! let inner = Arc::new(LocalStorage::new("./data"));
//! let config = CompressionConfig::lz4();
//! let storage = CompressedStorage::new(inner, config);
//! ```
//!
//! # File Format
//!
//! Compressed files have a 4-byte header:
//! - Byte 0: Magic byte (0xC0)
//! - Byte 1: Algorithm (0x01 = LZ4, 0x02 = Zstd)
//! - Bytes 2-3: Reserved (for future use, e.g., compression level)

use async_trait::async_trait;
use bytes::Bytes;
use std::sync::Arc;
use tracing::{debug, instrument};

use crate::error::{Result, StorageError};
use crate::path::StoragePath;
use crate::traits::{ListOptions, ObjectMeta, SegmentStorage};

/// Magic byte for compressed data
const COMPRESSION_MAGIC: u8 = 0xC0;

/// Algorithm identifier bytes
const ALG_LZ4: u8 = 0x01;
const ALG_ZSTD: u8 = 0x02;

/// Compression algorithm selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionAlgorithm {
    /// LZ4 - Very fast compression/decompression, moderate ratio
    Lz4,
    /// Zstd - Fast with better compression ratio, configurable level
    Zstd {
        /// Compression level (1-22, default 3)
        level: i32,
    },
    /// No compression (passthrough)
    None,
}

impl Default for CompressionAlgorithm {
    fn default() -> Self {
        Self::Lz4
    }
}

impl CompressionAlgorithm {
    /// Create Zstd with default compression level.
    pub fn zstd() -> Self {
        Self::Zstd { level: 3 }
    }

    /// Create Zstd with custom compression level.
    pub fn zstd_level(level: i32) -> Self {
        Self::Zstd {
            level: level.clamp(1, 22),
        }
    }
}

/// Configuration for compressed storage.
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// Compression algorithm to use
    pub algorithm: CompressionAlgorithm,
    /// Minimum size (bytes) to compress. Files smaller than this are stored uncompressed.
    pub min_size: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algorithm: CompressionAlgorithm::Lz4,
            min_size: 512, // Don't compress tiny files
        }
    }
}

impl CompressionConfig {
    /// Create config with LZ4 compression.
    pub fn lz4() -> Self {
        Self {
            algorithm: CompressionAlgorithm::Lz4,
            ..Default::default()
        }
    }

    /// Create config with Zstd compression at default level.
    pub fn zstd() -> Self {
        Self {
            algorithm: CompressionAlgorithm::zstd(),
            ..Default::default()
        }
    }

    /// Create config with Zstd at specific level.
    pub fn zstd_level(level: i32) -> Self {
        Self {
            algorithm: CompressionAlgorithm::zstd_level(level),
            ..Default::default()
        }
    }

    /// Create config with no compression (passthrough).
    pub fn none() -> Self {
        Self {
            algorithm: CompressionAlgorithm::None,
            ..Default::default()
        }
    }

    /// Set minimum file size for compression.
    pub fn with_min_size(mut self, min_size: usize) -> Self {
        self.min_size = min_size;
        self
    }
}

/// Compressed storage wrapper.
///
/// Wraps any `SegmentStorage` to provide transparent compression.
pub struct CompressedStorage {
    /// Underlying storage backend
    inner: Arc<dyn SegmentStorage>,
    /// Compression configuration
    config: CompressionConfig,
}

impl CompressedStorage {
    /// Create a new compressed storage wrapper.
    pub fn new(inner: Arc<dyn SegmentStorage>, config: CompressionConfig) -> Self {
        Self { inner, config }
    }

    /// Create with default configuration (LZ4).
    pub fn with_lz4(inner: Arc<dyn SegmentStorage>) -> Self {
        Self::new(inner, CompressionConfig::lz4())
    }

    /// Create with Zstd compression.
    pub fn with_zstd(inner: Arc<dyn SegmentStorage>, level: i32) -> Self {
        Self::new(inner, CompressionConfig::zstd_level(level))
    }

    /// Compress data according to configuration.
    fn compress(&self, data: &[u8]) -> Result<Bytes> {
        // Skip compression for small files
        if data.len() < self.config.min_size {
            return Ok(Bytes::copy_from_slice(data));
        }

        match self.config.algorithm {
            CompressionAlgorithm::None => Ok(Bytes::copy_from_slice(data)),
            CompressionAlgorithm::Lz4 => self.compress_lz4(data),
            CompressionAlgorithm::Zstd { level } => self.compress_zstd(data, level),
        }
    }

    /// Decompress data, auto-detecting algorithm from header.
    fn decompress(&self, data: &[u8]) -> Result<Bytes> {
        // Check for compression header
        if data.len() < 4 || data[0] != COMPRESSION_MAGIC {
            // Not compressed, return as-is
            return Ok(Bytes::copy_from_slice(data));
        }

        match data[1] {
            ALG_LZ4 => self.decompress_lz4(&data[4..]),
            ALG_ZSTD => self.decompress_zstd(&data[4..]),
            unknown => Err(StorageError::Compression(format!(
                "Unknown compression algorithm: 0x{:02x}",
                unknown
            ))),
        }
    }

    #[cfg(feature = "compression-lz4")]
    fn compress_lz4(&self, data: &[u8]) -> Result<Bytes> {
        let compressed = lz4_flex::compress_prepend_size(data);
        let mut output = Vec::with_capacity(4 + compressed.len());
        output.extend_from_slice(&[COMPRESSION_MAGIC, ALG_LZ4, 0, 0]);
        output.extend_from_slice(&compressed);
        Ok(Bytes::from(output))
    }

    #[cfg(not(feature = "compression-lz4"))]
    fn compress_lz4(&self, _data: &[u8]) -> Result<Bytes> {
        Err(StorageError::Compression(
            "LZ4 compression requires 'compression-lz4' feature".to_string(),
        ))
    }

    #[cfg(feature = "compression-lz4")]
    fn decompress_lz4(&self, data: &[u8]) -> Result<Bytes> {
        let decompressed = lz4_flex::decompress_size_prepended(data)
            .map_err(|e| StorageError::Compression(format!("LZ4 decompression failed: {}", e)))?;
        Ok(Bytes::from(decompressed))
    }

    #[cfg(not(feature = "compression-lz4"))]
    fn decompress_lz4(&self, _data: &[u8]) -> Result<Bytes> {
        Err(StorageError::Compression(
            "LZ4 decompression requires 'compression-lz4' feature".to_string(),
        ))
    }

    #[cfg(feature = "compression-zstd")]
    fn compress_zstd(&self, data: &[u8], level: i32) -> Result<Bytes> {
        let compressed = zstd::encode_all(data, level)
            .map_err(|e| StorageError::Compression(format!("Zstd compression failed: {}", e)))?;
        let mut output = Vec::with_capacity(4 + compressed.len());
        output.extend_from_slice(&[COMPRESSION_MAGIC, ALG_ZSTD, level as u8, 0]);
        output.extend_from_slice(&compressed);
        Ok(Bytes::from(output))
    }

    #[cfg(not(feature = "compression-zstd"))]
    fn compress_zstd(&self, _data: &[u8], _level: i32) -> Result<Bytes> {
        Err(StorageError::Compression(
            "Zstd compression requires 'compression-zstd' feature".to_string(),
        ))
    }

    #[cfg(feature = "compression-zstd")]
    fn decompress_zstd(&self, data: &[u8]) -> Result<Bytes> {
        let decompressed = zstd::decode_all(data)
            .map_err(|e| StorageError::Compression(format!("Zstd decompression failed: {}", e)))?;
        Ok(Bytes::from(decompressed))
    }

    #[cfg(not(feature = "compression-zstd"))]
    fn decompress_zstd(&self, _data: &[u8]) -> Result<Bytes> {
        Err(StorageError::Compression(
            "Zstd decompression requires 'compression-zstd' feature".to_string(),
        ))
    }
}

impl std::fmt::Debug for CompressedStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompressedStorage")
            .field("inner", &self.inner.backend_name())
            .field("algorithm", &self.config.algorithm)
            .field("min_size", &self.config.min_size)
            .finish()
    }
}

#[async_trait]
impl SegmentStorage for CompressedStorage {
    #[instrument(skip(self, data), fields(path = %path, size = data.len()))]
    async fn write(&self, path: &StoragePath, data: Bytes) -> Result<()> {
        debug!("CompressedStorage write: {} ({} bytes)", path, data.len());

        let compressed = self.compress(&data)?;
        let compression_ratio = if data.len() > 0 {
            compressed.len() as f64 / data.len() as f64
        } else {
            1.0
        };

        debug!(
            "Compressed {} bytes -> {} bytes ({:.1}%)",
            data.len(),
            compressed.len(),
            compression_ratio * 100.0
        );

        self.inner.write(path, compressed).await
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn read(&self, path: &StoragePath) -> Result<Bytes> {
        debug!("CompressedStorage read: {}", path);

        let data = self.inner.read(path).await?;
        let decompressed = self.decompress(&data)?;

        debug!(
            "Decompressed {} bytes -> {} bytes",
            data.len(),
            decompressed.len()
        );

        Ok(decompressed)
    }

    async fn exists(&self, path: &StoragePath) -> Result<bool> {
        self.inner.exists(path).await
    }

    async fn delete(&self, path: &StoragePath) -> Result<()> {
        self.inner.delete(path).await
    }

    async fn list(&self, prefix: &StoragePath) -> Result<Vec<ObjectMeta>> {
        self.inner.list(prefix).await
    }

    async fn list_with_options(
        &self,
        prefix: &StoragePath,
        options: ListOptions,
    ) -> Result<Vec<ObjectMeta>> {
        self.inner.list_with_options(prefix, options).await
    }

    async fn rename(&self, from: &StoragePath, to: &StoragePath) -> Result<()> {
        self.inner.rename(from, to).await
    }

    async fn copy(&self, from: &StoragePath, to: &StoragePath) -> Result<()> {
        self.inner.copy(from, to).await
    }

    async fn head(&self, path: &StoragePath) -> Result<ObjectMeta> {
        // Note: size will be the compressed size
        self.inner.head(path).await
    }

    fn backend_name(&self) -> &'static str {
        "compressed"
    }
}

/// Statistics about compression performance.
#[derive(Debug, Clone, Default)]
pub struct CompressionStats {
    /// Total bytes before compression
    pub bytes_in: u64,
    /// Total bytes after compression
    pub bytes_out: u64,
    /// Number of compressed writes
    pub compress_count: u64,
    /// Number of decompressed reads
    pub decompress_count: u64,
}

impl CompressionStats {
    /// Calculate compression ratio (compressed / original).
    pub fn compression_ratio(&self) -> f64 {
        if self.bytes_in == 0 {
            1.0
        } else {
            self.bytes_out as f64 / self.bytes_in as f64
        }
    }

    /// Calculate space savings as percentage.
    pub fn space_savings_percent(&self) -> f64 {
        (1.0 - self.compression_ratio()) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LocalStorage;
    use tempfile::TempDir;

    async fn create_test_storage(config: CompressionConfig) -> (CompressedStorage, TempDir) {
        let dir = TempDir::new().unwrap();
        let inner = Arc::new(LocalStorage::new(dir.path()));
        let storage = CompressedStorage::new(inner, config);
        (storage, dir)
    }

    #[tokio::test]
    async fn test_roundtrip_no_compression() {
        let (storage, _dir) = create_test_storage(CompressionConfig::none()).await;

        let path = StoragePath::vector("test", "shard_0", "data.bin");
        let data = Bytes::from("Hello, World!");

        storage.write(&path, data.clone()).await.unwrap();
        let read = storage.read(&path).await.unwrap();

        assert_eq!(read, data);
    }

    #[tokio::test]
    #[cfg(feature = "compression-lz4")]
    async fn test_roundtrip_lz4() {
        let (storage, _dir) = create_test_storage(CompressionConfig::lz4()).await;

        let path = StoragePath::vector("test", "shard_0", "data.bin");
        // Create compressible data
        let data = Bytes::from("a".repeat(10000));

        storage.write(&path, data.clone()).await.unwrap();
        let read = storage.read(&path).await.unwrap();

        assert_eq!(read, data);
    }

    #[tokio::test]
    #[cfg(feature = "compression-zstd")]
    async fn test_roundtrip_zstd() {
        let (storage, _dir) = create_test_storage(CompressionConfig::zstd()).await;

        let path = StoragePath::vector("test", "shard_0", "data.bin");
        // Create compressible data
        let data = Bytes::from("b".repeat(10000));

        storage.write(&path, data.clone()).await.unwrap();
        let read = storage.read(&path).await.unwrap();

        assert_eq!(read, data);
    }

    #[tokio::test]
    #[cfg(feature = "compression-lz4")]
    async fn test_small_file_not_compressed() {
        let (storage, dir) =
            create_test_storage(CompressionConfig::lz4().with_min_size(1000)).await;

        let path = StoragePath::vector("test", "shard_0", "small.bin");
        let data = Bytes::from("tiny"); // Less than min_size

        storage.write(&path, data.clone()).await.unwrap();

        // Read raw file - should not have compression header
        let inner = LocalStorage::new(dir.path());
        let raw = inner.read(&path).await.unwrap();

        // No magic byte means not compressed
        assert!(raw.len() < 4 || raw[0] != COMPRESSION_MAGIC);

        // But roundtrip should still work
        let read = storage.read(&path).await.unwrap();
        assert_eq!(read, data);
    }

    #[tokio::test]
    #[cfg(feature = "compression-lz4")]
    async fn test_compression_actually_compresses() {
        let (storage, dir) = create_test_storage(CompressionConfig::lz4().with_min_size(0)).await;

        let path = StoragePath::vector("test", "shard_0", "data.bin");
        // Highly compressible data
        let data = Bytes::from("x".repeat(10000));

        storage.write(&path, data.clone()).await.unwrap();

        // Check that compressed size is smaller
        let inner = LocalStorage::new(dir.path());
        let raw = inner.read(&path).await.unwrap();

        assert!(
            raw.len() < data.len(),
            "Compressed size {} should be less than original {}",
            raw.len(),
            data.len()
        );
    }

    #[tokio::test]
    async fn test_exists_and_delete() {
        let (storage, _dir) = create_test_storage(CompressionConfig::none()).await;

        let path = StoragePath::vector("test", "shard_0", "data.bin");
        let data = Bytes::from("test data");

        assert!(!storage.exists(&path).await.unwrap());

        storage.write(&path, data).await.unwrap();
        assert!(storage.exists(&path).await.unwrap());

        storage.delete(&path).await.unwrap();
        assert!(!storage.exists(&path).await.unwrap());
    }

    #[test]
    fn test_compression_config_builders() {
        let lz4 = CompressionConfig::lz4();
        assert!(matches!(lz4.algorithm, CompressionAlgorithm::Lz4));

        let zstd = CompressionConfig::zstd();
        assert!(matches!(
            zstd.algorithm,
            CompressionAlgorithm::Zstd { level: 3 }
        ));

        let zstd_high = CompressionConfig::zstd_level(19);
        assert!(matches!(
            zstd_high.algorithm,
            CompressionAlgorithm::Zstd { level: 19 }
        ));

        let none = CompressionConfig::none();
        assert!(matches!(none.algorithm, CompressionAlgorithm::None));
    }

    #[test]
    fn test_compression_stats() {
        let stats = CompressionStats {
            bytes_in: 1000,
            bytes_out: 300,
            compress_count: 5,
            decompress_count: 3,
        };

        assert!((stats.compression_ratio() - 0.3).abs() < 0.001);
        assert!((stats.space_savings_percent() - 70.0).abs() < 0.1);
    }
}
