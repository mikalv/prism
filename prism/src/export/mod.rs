//! Collection export/import functionality.
//!
//! Supports two formats:
//! - **Portable**: JSON-based, cross-version compatible, human-readable
//! - **Snapshot**: Binary archive (tar.zst), fast backup/restore, same-version only
//! - **Encrypted**: AES-256-GCM encrypted snapshot for secure cloud offloading

pub mod encrypted;
pub mod portable;
pub mod snapshot;
pub mod types;

pub use encrypted::{export_encrypted, import_encrypted, EncryptedExportConfig};
pub use portable::{export_portable, import_portable};
pub use snapshot::{export_snapshot, import_snapshot};
pub use types::{ExportFormat, ExportMetadata, ExportProgress};
