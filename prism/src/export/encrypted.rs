//! Encrypted export/import for emergency data offloading.
//!
//! Provides encrypted snapshot exports that can be safely stored in
//! untrusted cloud storage during disk space emergencies.
//!
//! # Usage
//!
//! ```ignore
//! use prism::export::encrypted::{export_encrypted, EncryptedExportConfig};
//!
//! let config = EncryptedExportConfig::from_hex(hex_key)?;
//! export_encrypted(data_dir, "my_collection", output_path, config, None).await?;
//! ```
//!
//! # File Format
//!
//! The encrypted export wraps a tar.zst archive:
//! ```text
//! [Magic: 4 bytes "PENC"] [Version: 1 byte] [Nonce: 12 bytes] [Ciphertext + Tag]
//! ```

use crate::error::{Error, Result};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use super::snapshot::{export_snapshot, import_snapshot, SnapshotImportResult};
use super::types::{ExportMetadata, ExportProgress};

/// Magic bytes for encrypted export files
const MAGIC: &[u8; 4] = b"PENC";

/// Current format version
const VERSION: u8 = 0x01;

/// Nonce size (96 bits for AES-GCM)
const NONCE_SIZE: usize = 12;

/// Key size (256 bits for AES-256)
const KEY_SIZE: usize = 32;

/// Header size: magic (4) + version (1) + nonce (12)
const HEADER_SIZE: usize = 17;

/// Configuration for encrypted export.
#[derive(Clone)]
pub struct EncryptedExportConfig {
    key: [u8; KEY_SIZE],
    key_id: String,
}

impl std::fmt::Debug for EncryptedExportConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptedExportConfig")
            .field("key_id", &self.key_id)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl EncryptedExportConfig {
    /// Create config from hex-encoded key.
    pub fn from_hex(hex_key: &str) -> Result<Self> {
        let bytes = hex::decode(hex_key)
            .map_err(|e| Error::Export(format!("Invalid hex key: {}", e)))?;

        if bytes.len() != KEY_SIZE {
            return Err(Error::Export(format!(
                "Key must be {} bytes, got {}",
                KEY_SIZE,
                bytes.len()
            )));
        }

        let mut key = [0u8; KEY_SIZE];
        key.copy_from_slice(&bytes);

        Ok(Self {
            key,
            key_id: "api-provided".to_string(),
        })
    }

    /// Create config from base64-encoded key.
    pub fn from_base64(b64_key: &str) -> Result<Self> {
        use base64::{engine::general_purpose::STANDARD, Engine};

        let bytes = STANDARD
            .decode(b64_key)
            .map_err(|e| Error::Export(format!("Invalid base64 key: {}", e)))?;

        if bytes.len() != KEY_SIZE {
            return Err(Error::Export(format!(
                "Key must be {} bytes, got {}",
                KEY_SIZE,
                bytes.len()
            )));
        }

        let mut key = [0u8; KEY_SIZE];
        key.copy_from_slice(&bytes);

        Ok(Self {
            key,
            key_id: "api-provided".to_string(),
        })
    }

    /// Generate a new random key.
    pub fn generate() -> Self {
        use aes_gcm::aead::rand_core::RngCore;
        use aes_gcm::aead::OsRng;

        let mut key = [0u8; KEY_SIZE];
        OsRng.fill_bytes(&mut key);

        Self {
            key,
            key_id: "generated".to_string(),
        }
    }

    /// Export key as hex string (for saving).
    pub fn to_hex(&self) -> String {
        hex::encode(&self.key)
    }
}

/// Export a collection as an encrypted snapshot.
///
/// Creates an AES-256-GCM encrypted tar.zst archive that can be
/// safely stored in untrusted cloud storage.
///
/// # Arguments
/// * `data_dir` - Base data directory containing collection data
/// * `collection` - Name of the collection to export
/// * `output_path` - Output file path for the encrypted archive
/// * `config` - Encryption configuration with key
/// * `progress` - Progress callback (optional)
pub fn export_encrypted(
    data_dir: &Path,
    collection: &str,
    output_path: &Path,
    config: EncryptedExportConfig,
    progress: Option<&dyn ExportProgress>,
) -> Result<ExportMetadata> {
    use aes_gcm::{
        aead::{rand_core::RngCore, Aead, KeyInit, OsRng},
        Aes256Gcm, Nonce,
    };

    // Create temp file for unencrypted snapshot
    let temp_path = output_path.with_extension("tmp");

    // Export snapshot to temp file
    let metadata = export_snapshot(data_dir, collection, &temp_path, progress)?;

    // Read the unencrypted snapshot
    let plaintext = std::fs::read(&temp_path)
        .map_err(|e| Error::Export(format!("Failed to read temp file: {}", e)))?;

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_path);

    // Generate random nonce
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Encrypt
    let cipher = Aes256Gcm::new_from_slice(&config.key)
        .map_err(|e| Error::Export(format!("Cipher init failed: {}", e)))?;

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_slice())
        .map_err(|e| Error::Export(format!("Encryption failed: {}", e)))?;

    // Write encrypted file: magic + version + nonce + ciphertext
    let mut output = File::create(output_path)
        .map_err(|e| Error::Export(format!("Failed to create output: {}", e)))?;

    output
        .write_all(MAGIC)
        .map_err(|e| Error::Export(format!("Write failed: {}", e)))?;
    output
        .write_all(&[VERSION])
        .map_err(|e| Error::Export(format!("Write failed: {}", e)))?;
    output
        .write_all(&nonce_bytes)
        .map_err(|e| Error::Export(format!("Write failed: {}", e)))?;
    output
        .write_all(&ciphertext)
        .map_err(|e| Error::Export(format!("Write failed: {}", e)))?;

    Ok(metadata)
}

/// Import a collection from an encrypted snapshot.
///
/// Decrypts an AES-256-GCM encrypted archive and restores the collection.
///
/// # Arguments
/// * `data_dir` - Base data directory for extraction
/// * `input_path` - Input encrypted archive file path
/// * `config` - Encryption configuration with key
/// * `target_collection` - Optional name for the imported collection
/// * `progress` - Progress callback (optional)
pub fn import_encrypted(
    data_dir: &Path,
    input_path: &Path,
    config: EncryptedExportConfig,
    target_collection: Option<&str>,
    progress: Option<&dyn ExportProgress>,
) -> Result<SnapshotImportResult> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };

    // Read encrypted file
    let mut file = File::open(input_path)
        .map_err(|e| Error::Import(format!("Failed to open input: {}", e)))?;

    let file_size = file
        .metadata()
        .map(|m| m.len() as usize)
        .unwrap_or(0);

    if file_size < HEADER_SIZE {
        return Err(Error::Import("File too small to be encrypted".to_string()));
    }

    // Read and verify header
    let mut header = [0u8; HEADER_SIZE];
    file.read_exact(&mut header)
        .map_err(|e| Error::Import(format!("Failed to read header: {}", e)))?;

    // Check magic
    if &header[0..4] != MAGIC {
        return Err(Error::Import(
            "Invalid file format (not a Prism encrypted export)".to_string(),
        ));
    }

    // Check version
    if header[4] != VERSION {
        return Err(Error::Import(format!(
            "Unsupported format version: {}",
            header[4]
        )));
    }

    // Extract nonce
    let nonce = Nonce::from_slice(&header[5..HEADER_SIZE]);

    // Read ciphertext
    let mut ciphertext = Vec::with_capacity(file_size - HEADER_SIZE);
    file.read_to_end(&mut ciphertext)
        .map_err(|e| Error::Import(format!("Failed to read ciphertext: {}", e)))?;

    // Decrypt
    let cipher = Aes256Gcm::new_from_slice(&config.key)
        .map_err(|e| Error::Import(format!("Cipher init failed: {}", e)))?;

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_slice())
        .map_err(|_| Error::Import("Decryption failed (wrong key or corrupted data)".to_string()))?;

    // Write to temp file
    let temp_path = input_path.with_extension("dec.tmp");
    std::fs::write(&temp_path, &plaintext)
        .map_err(|e| Error::Import(format!("Failed to write temp file: {}", e)))?;

    // Import from temp file
    let result = import_snapshot(data_dir, &temp_path, target_collection, progress);

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_path);

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_collection(data_dir: &Path, name: &str) {
        let collection_dir = data_dir.join("collections").join(name);
        fs::create_dir_all(&collection_dir).unwrap();

        // Create minimal collection files
        fs::write(
            collection_dir.join("schema.yaml"),
            "collection: test\nbackends:\n  text:\n    fields: []\n",
        )
        .unwrap();

        // Create text backend dir with a dummy file
        let text_dir = collection_dir.join("text");
        fs::create_dir_all(&text_dir).unwrap();
        fs::write(text_dir.join("meta.json"), r#"{"version": 1}"#).unwrap();
    }

    #[test]
    fn test_encrypted_roundtrip() {
        let data_dir = TempDir::new().unwrap();
        let output_dir = TempDir::new().unwrap();

        setup_test_collection(data_dir.path(), "test_collection");

        let output_path = output_dir.path().join("export.enc");
        let config = EncryptedExportConfig::generate();

        // Export
        let metadata = export_encrypted(
            data_dir.path(),
            "test_collection",
            &output_path,
            config.clone(),
            None,
        )
        .unwrap();

        assert_eq!(metadata.collection, "test_collection");
        assert!(output_path.exists());

        // Verify file starts with magic bytes
        let file_contents = fs::read(&output_path).unwrap();
        assert_eq!(&file_contents[0..4], MAGIC);
        assert_eq!(file_contents[4], VERSION);

        // Import to new data dir
        let import_dir = TempDir::new().unwrap();
        let result = import_encrypted(
            import_dir.path(),
            &output_path,
            config,
            Some("restored_collection"),
            None,
        )
        .unwrap();

        assert_eq!(result.collection, "restored_collection");
        assert!(import_dir
            .path()
            .join("collections/restored_collection")
            .exists());
    }

    #[test]
    fn test_wrong_key_fails() {
        let data_dir = TempDir::new().unwrap();
        let output_dir = TempDir::new().unwrap();

        setup_test_collection(data_dir.path(), "test");

        let output_path = output_dir.path().join("export.enc");
        let config1 = EncryptedExportConfig::generate();
        let config2 = EncryptedExportConfig::generate();

        // Export with key1
        export_encrypted(data_dir.path(), "test", &output_path, config1, None).unwrap();

        // Try to import with key2
        let import_dir = TempDir::new().unwrap();
        let result = import_encrypted(import_dir.path(), &output_path, config2, None, None);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Decryption failed"));
    }

    #[test]
    fn test_config_from_hex() {
        let hex_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let config = EncryptedExportConfig::from_hex(hex_key).unwrap();
        assert_eq!(config.to_hex(), hex_key);
    }

    #[test]
    fn test_config_debug_redacts_key() {
        let config = EncryptedExportConfig::generate();
        let debug = format!("{:?}", config);
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains(&config.to_hex()));
    }
}
