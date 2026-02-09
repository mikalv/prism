//! Export/import types and metadata.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Export format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// JSON-based, cross-version compatible, human-readable
    Portable,
    /// Binary archive (tar.zst), fast backup/restore, same-version only
    Snapshot,
}

impl std::fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportFormat::Portable => write!(f, "portable"),
            ExportFormat::Snapshot => write!(f, "snapshot"),
        }
    }
}

impl std::str::FromStr for ExportFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "portable" | "json" => Ok(ExportFormat::Portable),
            "snapshot" | "binary" | "tar" => Ok(ExportFormat::Snapshot),
            _ => Err(format!(
                "Invalid export format '{}'. Use 'portable' or 'snapshot'",
                s
            )),
        }
    }
}

/// Metadata included in exports for verification and compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportMetadata {
    /// Export format version
    pub version: String,
    /// Collection name
    pub collection: String,
    /// Prism version that created this export
    pub prism_version: String,
    /// Export timestamp (RFC 3339)
    pub exported_at: String,
    /// Number of documents
    pub document_count: u64,
    /// Total size in bytes (approximate)
    pub size_bytes: u64,
    /// SHA-256 checksum of the content (for verification)
    pub checksum: Option<String>,
    /// Backend types enabled for this collection
    pub backends: ExportBackendInfo,
}

/// Information about which backends are included in the export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportBackendInfo {
    pub text: bool,
    pub vector: bool,
    pub graph: bool,
}

/// A single exported document with all its data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedDocument {
    /// Document ID
    pub id: String,
    /// Document fields (stored values)
    pub fields: HashMap<String, serde_json::Value>,
    /// Vector embedding if present
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector: Option<Vec<f32>>,
}

/// Progress callback for long-running export/import operations.
pub trait ExportProgress: Send + Sync {
    /// Called when progress is made.
    fn on_progress(&self, current: u64, total: u64, message: &str);

    /// Called when the operation completes.
    fn on_complete(&self, total: u64);

    /// Called when an error occurs.
    fn on_error(&self, error: &str);
}

/// No-op progress reporter for when progress isn't needed.
pub struct NoopProgress;

impl ExportProgress for NoopProgress {
    fn on_progress(&self, _current: u64, _total: u64, _message: &str) {}
    fn on_complete(&self, _total: u64) {}
    fn on_error(&self, _error: &str) {}
}

/// Simple console progress reporter.
pub struct ConsoleProgress {
    prefix: String,
}

impl ConsoleProgress {
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
        }
    }
}

impl ExportProgress for ConsoleProgress {
    fn on_progress(&self, current: u64, total: u64, message: &str) {
        let pct = if total > 0 {
            (current as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        eprint!(
            "\r{} [{:.1}%] {} ({}/{})",
            self.prefix, pct, message, current, total
        );
    }

    fn on_complete(&self, total: u64) {
        eprintln!("\r{} Complete. {} items processed.", self.prefix, total);
    }

    fn on_error(&self, error: &str) {
        eprintln!("\r{} Error: {}", self.prefix, error);
    }
}
