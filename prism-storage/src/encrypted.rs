//! Encrypted storage wrapper with AES-256-GCM support.
//!
//! Provides transparent encryption and decryption for stored data.
//! Wraps any `SegmentStorage` implementation to add encryption at rest.
//!
//! # Security
//!
//! - AES-256-GCM provides authenticated encryption (confidentiality + integrity)
//! - Unique 96-bit nonce per write (cryptographically random)
//! - Keys should be provided via environment variables or KMS (never stored on disk)
//!
//! # Usage
//!
//! ```ignore
//! use prism_storage::{EncryptedStorage, EncryptionConfig, LocalStorage};
//! use std::sync::Arc;
//!
//! let inner = Arc::new(LocalStorage::new("./data"));
//! let config = EncryptionConfig::from_env("MY_ENCRYPTION_KEY")?;
//! let storage = EncryptedStorage::new(inner, config)?;
//! ```
//!
//! # File Format
//!
//! Encrypted files have the following structure:
//! ```text
//! [Magic: 1 byte (0xE0)] [Version: 1 byte] [Nonce: 12 bytes] [Ciphertext + Tag: N bytes]
//! ```
//!
//! The authentication tag (16 bytes) is appended to the ciphertext by AES-GCM.

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use async_trait::async_trait;
use bytes::Bytes;
use std::sync::Arc;
use tracing::{debug, instrument};

use crate::error::{Result, StorageError};
use crate::path::StoragePath;
use crate::traits::{ListOptions, ObjectMeta, SegmentStorage};

/// Magic byte for encrypted data
const ENCRYPTION_MAGIC: u8 = 0xE0;

/// Current encryption format version
const ENCRYPTION_VERSION: u8 = 0x01;

/// Header size: magic (1) + version (1) + nonce (12)
const HEADER_SIZE: usize = 14;

/// Nonce size for AES-GCM (96 bits)
const NONCE_SIZE: usize = 12;

/// Key size for AES-256 (256 bits)
const KEY_SIZE: usize = 32;

/// Configuration for encrypted storage.
#[derive(Clone)]
pub struct EncryptionConfig {
    /// 256-bit encryption key
    key: [u8; KEY_SIZE],
    /// Key identifier (for logging/debugging, not the actual key)
    key_id: String,
}

impl std::fmt::Debug for EncryptionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptionConfig")
            .field("key_id", &self.key_id)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl EncryptionConfig {
    /// Create config from a raw 32-byte key.
    ///
    /// # Arguments
    /// * `key` - 256-bit (32-byte) encryption key
    /// * `key_id` - Identifier for logging (not the key itself)
    pub fn from_key(key: [u8; KEY_SIZE], key_id: impl Into<String>) -> Self {
        Self {
            key,
            key_id: key_id.into(),
        }
    }

    /// Create config from hex-encoded key string.
    ///
    /// # Arguments
    /// * `hex_key` - 64-character hex string (256 bits)
    /// * `key_id` - Identifier for logging
    pub fn from_hex(hex_key: &str, key_id: impl Into<String>) -> Result<Self> {
        let bytes = hex::decode(hex_key)
            .map_err(|e| StorageError::Encryption(format!("Invalid hex key: {}", e)))?;

        if bytes.len() != KEY_SIZE {
            return Err(StorageError::Encryption(format!(
                "Key must be {} bytes, got {}",
                KEY_SIZE,
                bytes.len()
            )));
        }

        let mut key = [0u8; KEY_SIZE];
        key.copy_from_slice(&bytes);

        Ok(Self {
            key,
            key_id: key_id.into(),
        })
    }

    /// Create config from environment variable.
    ///
    /// The environment variable should contain a 64-character hex string.
    ///
    /// # Arguments
    /// * `env_var` - Name of the environment variable containing the hex key
    pub fn from_env(env_var: &str) -> Result<Self> {
        let hex_key = std::env::var(env_var).map_err(|_| {
            StorageError::Encryption(format!("Environment variable '{}' not set", env_var))
        })?;

        Self::from_hex(&hex_key, format!("env:{}", env_var))
    }

    /// Create config from base64-encoded key string.
    ///
    /// # Arguments
    /// * `b64_key` - Base64-encoded 32-byte key
    /// * `key_id` - Identifier for logging
    pub fn from_base64(b64_key: &str, key_id: impl Into<String>) -> Result<Self> {
        use base64::{engine::general_purpose::STANDARD, Engine};

        let bytes = STANDARD
            .decode(b64_key)
            .map_err(|e| StorageError::Encryption(format!("Invalid base64 key: {}", e)))?;

        if bytes.len() != KEY_SIZE {
            return Err(StorageError::Encryption(format!(
                "Key must be {} bytes, got {}",
                KEY_SIZE,
                bytes.len()
            )));
        }

        let mut key = [0u8; KEY_SIZE];
        key.copy_from_slice(&bytes);

        Ok(Self {
            key,
            key_id: key_id.into(),
        })
    }

    /// Generate a new random encryption key.
    ///
    /// Useful for initial setup. The key should be saved securely
    /// (e.g., in a secrets manager) and provided via `from_env` in production.
    pub fn generate(key_id: impl Into<String>) -> Self {
        use aes_gcm::aead::rand_core::RngCore;

        let mut key = [0u8; KEY_SIZE];
        OsRng.fill_bytes(&mut key);

        Self {
            key,
            key_id: key_id.into(),
        }
    }

    /// Export key as hex string (for saving to secrets manager).
    ///
    /// # Security Warning
    /// Handle the returned string carefully - it contains the raw key material.
    pub fn to_hex(&self) -> String {
        hex::encode(&self.key)
    }

    /// Get the key identifier.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
}

/// Encrypted storage wrapper.
///
/// Wraps any `SegmentStorage` to provide transparent encryption at rest.
pub struct EncryptedStorage {
    /// Underlying storage backend
    inner: Arc<dyn SegmentStorage>,
    /// AES-256-GCM cipher instance
    cipher: Aes256Gcm,
    /// Key identifier for logging
    key_id: String,
}

impl EncryptedStorage {
    /// Create a new encrypted storage wrapper.
    pub fn new(inner: Arc<dyn SegmentStorage>, config: EncryptionConfig) -> Result<Self> {
        let cipher = Aes256Gcm::new_from_slice(&config.key)
            .map_err(|e| StorageError::Encryption(format!("Failed to initialize cipher: {}", e)))?;

        Ok(Self {
            inner,
            cipher,
            key_id: config.key_id,
        })
    }

    /// Encrypt data with a random nonce.
    fn encrypt(&self, data: &[u8]) -> Result<Bytes> {
        use aes_gcm::aead::rand_core::RngCore;

        // Generate random nonce
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt
        let ciphertext = self
            .cipher
            .encrypt(nonce, data)
            .map_err(|e| StorageError::Encryption(format!("Encryption failed: {}", e)))?;

        // Build output: magic + version + nonce + ciphertext
        let mut output = Vec::with_capacity(HEADER_SIZE + ciphertext.len());
        output.push(ENCRYPTION_MAGIC);
        output.push(ENCRYPTION_VERSION);
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        Ok(Bytes::from(output))
    }

    /// Decrypt data, extracting nonce from header.
    fn decrypt(&self, data: &[u8]) -> Result<Bytes> {
        // Check minimum size
        if data.len() < HEADER_SIZE {
            return Err(StorageError::Encryption(
                "Data too short to be encrypted".to_string(),
            ));
        }

        // Verify magic byte
        if data[0] != ENCRYPTION_MAGIC {
            return Err(StorageError::Encryption(
                "Invalid encryption header (not encrypted or corrupted)".to_string(),
            ));
        }

        // Check version
        if data[1] != ENCRYPTION_VERSION {
            return Err(StorageError::Encryption(format!(
                "Unsupported encryption version: {}",
                data[1]
            )));
        }

        // Extract nonce
        let nonce = Nonce::from_slice(&data[2..HEADER_SIZE]);

        // Decrypt
        let plaintext = self
            .cipher
            .decrypt(nonce, &data[HEADER_SIZE..])
            .map_err(|e| {
                StorageError::Encryption(format!(
                    "Decryption failed (wrong key or corrupted data): {}",
                    e
                ))
            })?;

        Ok(Bytes::from(plaintext))
    }
}

impl std::fmt::Debug for EncryptedStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptedStorage")
            .field("inner", &self.inner.backend_name())
            .field("key_id", &self.key_id)
            .field("algorithm", &"AES-256-GCM")
            .finish()
    }
}

#[async_trait]
impl SegmentStorage for EncryptedStorage {
    #[instrument(skip(self, data), fields(path = %path, size = data.len(), key_id = %self.key_id))]
    async fn write(&self, path: &StoragePath, data: Bytes) -> Result<()> {
        debug!("EncryptedStorage write: {} ({} bytes)", path, data.len());

        let encrypted = self.encrypt(&data)?;

        debug!(
            "Encrypted {} bytes -> {} bytes (overhead: {} bytes)",
            data.len(),
            encrypted.len(),
            encrypted.len() - data.len()
        );

        self.inner.write(path, encrypted).await
    }

    #[instrument(skip(self), fields(path = %path, key_id = %self.key_id))]
    async fn read(&self, path: &StoragePath) -> Result<Bytes> {
        debug!("EncryptedStorage read: {}", path);

        let data = self.inner.read(path).await?;
        let decrypted = self.decrypt(&data)?;

        debug!(
            "Decrypted {} bytes -> {} bytes",
            data.len(),
            decrypted.len()
        );

        Ok(decrypted)
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
        // For encrypted storage, copy needs to preserve the ciphertext as-is
        self.inner.copy(from, to).await
    }

    async fn head(&self, path: &StoragePath) -> Result<ObjectMeta> {
        // Note: size will be the encrypted size (includes header + tag overhead)
        self.inner.head(path).await
    }

    fn backend_name(&self) -> &'static str {
        "encrypted"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LocalStorage;
    use tempfile::TempDir;

    fn create_test_config() -> EncryptionConfig {
        EncryptionConfig::generate("test-key")
    }

    async fn create_test_storage() -> (EncryptedStorage, TempDir) {
        let dir = TempDir::new().unwrap();
        let inner = Arc::new(LocalStorage::new(dir.path()));
        let config = create_test_config();
        let storage = EncryptedStorage::new(inner, config).unwrap();
        (storage, dir)
    }

    #[tokio::test]
    async fn test_roundtrip() {
        let (storage, _dir) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "data.bin");
        let data = Bytes::from("Hello, World! This is sensitive data.");

        storage.write(&path, data.clone()).await.unwrap();
        let read = storage.read(&path).await.unwrap();

        assert_eq!(read, data);
    }

    #[tokio::test]
    async fn test_large_data() {
        let (storage, _dir) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "large.bin");
        // 1MB of data
        let data = Bytes::from(vec![0x42u8; 1024 * 1024]);

        storage.write(&path, data.clone()).await.unwrap();
        let read = storage.read(&path).await.unwrap();

        assert_eq!(read, data);
    }

    #[tokio::test]
    async fn test_binary_data() {
        let (storage, _dir) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "binary.bin");
        // All possible byte values
        let data: Vec<u8> = (0..=255).collect();
        let data = Bytes::from(data);

        storage.write(&path, data.clone()).await.unwrap();
        let read = storage.read(&path).await.unwrap();

        assert_eq!(read, data);
    }

    #[tokio::test]
    async fn test_data_actually_encrypted() {
        let (storage, dir) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "secret.bin");
        let plaintext = b"This is a secret message that should not appear in ciphertext";
        let data = Bytes::copy_from_slice(plaintext);

        storage.write(&path, data).await.unwrap();

        // Read raw file from disk
        let inner = LocalStorage::new(dir.path());
        let raw = inner.read(&path).await.unwrap();

        // Verify magic byte is present
        assert_eq!(raw[0], ENCRYPTION_MAGIC);

        // Verify plaintext does not appear in ciphertext
        let raw_str = String::from_utf8_lossy(&raw);
        assert!(
            !raw_str.contains("secret"),
            "Plaintext should not appear in encrypted data"
        );
    }

    #[tokio::test]
    async fn test_wrong_key_fails_decryption() {
        let dir = TempDir::new().unwrap();
        let inner = Arc::new(LocalStorage::new(dir.path()));

        // Write with one key
        let config1 = EncryptionConfig::generate("key1");
        let storage1 = EncryptedStorage::new(inner.clone(), config1).unwrap();

        let path = StoragePath::vector("test", "shard_0", "data.bin");
        let data = Bytes::from("Secret data");
        storage1.write(&path, data).await.unwrap();

        // Try to read with different key
        let config2 = EncryptionConfig::generate("key2");
        let storage2 = EncryptedStorage::new(inner.clone(), config2).unwrap();

        let result = storage2.read(&path).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Decryption failed"));
    }

    #[tokio::test]
    async fn test_exists_and_delete() {
        let (storage, _dir) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "data.bin");
        let data = Bytes::from("test data");

        assert!(!storage.exists(&path).await.unwrap());

        storage.write(&path, data).await.unwrap();
        assert!(storage.exists(&path).await.unwrap());

        storage.delete(&path).await.unwrap();
        assert!(!storage.exists(&path).await.unwrap());
    }

    #[tokio::test]
    async fn test_unique_nonces() {
        let (storage, dir) = create_test_storage().await;

        let path1 = StoragePath::vector("test", "shard_0", "data1.bin");
        let path2 = StoragePath::vector("test", "shard_0", "data2.bin");
        let data = Bytes::from("Same data");

        // Write same data twice
        storage.write(&path1, data.clone()).await.unwrap();
        storage.write(&path2, data.clone()).await.unwrap();

        // Read raw files
        let inner = LocalStorage::new(dir.path());
        let raw1 = inner.read(&path1).await.unwrap();
        let raw2 = inner.read(&path2).await.unwrap();

        // Nonces should be different (bytes 2-14)
        assert_ne!(
            &raw1[2..HEADER_SIZE],
            &raw2[2..HEADER_SIZE],
            "Nonces should be unique per write"
        );

        // And therefore ciphertexts should be different
        assert_ne!(
            &raw1[HEADER_SIZE..],
            &raw2[HEADER_SIZE..],
            "Ciphertexts should be different due to unique nonces"
        );
    }

    #[test]
    fn test_config_from_hex() {
        let hex_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let config = EncryptionConfig::from_hex(hex_key, "test").unwrap();
        assert_eq!(config.key_id(), "test");

        // Verify round-trip
        assert_eq!(config.to_hex(), hex_key);
    }

    #[test]
    fn test_config_from_hex_invalid_length() {
        let short_key = "0123456789abcdef";
        let result = EncryptionConfig::from_hex(short_key, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_config_generate_unique() {
        let config1 = EncryptionConfig::generate("key1");
        let config2 = EncryptionConfig::generate("key2");

        assert_ne!(config1.to_hex(), config2.to_hex());
    }

    #[test]
    fn test_config_debug_redacts_key() {
        let config = EncryptionConfig::generate("test");
        let debug_output = format!("{:?}", config);

        assert!(debug_output.contains("REDACTED"));
        assert!(!debug_output.contains(&config.to_hex()));
    }
}
