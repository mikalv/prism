//! Optional search-query logging to a dedicated file (JSON lines).
//!
//! Toggled via `[observability] query_log = true` plus an optional
//! `query_log_path` (defaults to `<data_dir>/logs/queries.log`). When disabled,
//! every call is a single relaxed atomic-bool load followed by an early return —
//! near-zero overhead, safe to leave the call sites in permanently.
//!
//! Two layers are instrumented:
//!   * `text-parse` — the single convergence point in [`crate::backends::text`]
//!     where *every* search (native API + ES-compat, text + hybrid backend) is
//!     parsed. Captures the post-rewrite query string and the parse outcome,
//!     so a failing query is logged in full, untruncated, regardless of how it
//!     arrived.
//!   * `es-compat` / `native` — the handler layer, which additionally captures
//!     the raw request body (ground truth) for ES-compat searches.
//!
//! The logger is initialized once at startup via [`init`]; call sites use the
//! free function [`log`].

use chrono::Utc;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static ENABLED: AtomicBool = AtomicBool::new(false);
static WRITER: OnceLock<Mutex<BufWriter<File>>> = OnceLock::new();

/// Initialize query logging. Call exactly once at startup.
///
/// When `enabled` is false this is a no-op (the static stays disabled and every
/// [`log`] call short-circuits). When enabled, the file is opened in append
/// mode (created if missing) and its parent directory is ensured.
pub fn init(enabled: bool, path: &Path) {
    if !enabled {
        return;
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!("Query log: cannot create dir {}: {}", parent.display(), e);
            }
        }
    }
    match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => {
            let _ = WRITER.set(Mutex::new(BufWriter::new(file)));
            ENABLED.store(true, Ordering::Relaxed);
            tracing::info!("Query logging enabled → {}", path.display());
        }
        Err(e) => tracing::warn!("Query log: cannot open {}: {}", path.display(), e),
    }
}

/// Whether query logging is active.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Resolve the effective query-log path from config + a default base dir.
pub fn resolve_path(configured: Option<&Path>, default_dir: &Path) -> PathBuf {
    configured
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| default_dir.join("logs").join("queries.log"))
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// A single query-log line. Fields not applicable at a given layer are `None`
/// and serialized as `null`.
#[derive(Serialize)]
pub struct QueryLogEntry<'a> {
    pub ts: String,
    /// Capture point: `"text-parse"`, `"es-compat"`, or `"native"`.
    pub layer: &'a str,
    pub op: Option<&'a str>,
    pub collection: Option<&'a str>,
    pub index: Option<&'a str>,
    /// Post-rewrite query string (text-parse) or translated query (handler).
    pub query_string: Option<&'a str>,
    /// Raw request body, serialized (es-compat only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_body: Option<serde_json::Value>,
    /// `"ok"` or `"error"`.
    pub status: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub took_ms: Option<u64>,
}

impl<'a> QueryLogEntry<'a> {
    /// Build an entry with a fresh timestamp. Only `layer` is required; chain
    /// the field setters you need.
    pub fn new(layer: &'a str) -> Self {
        Self {
            ts: now_iso(),
            layer,
            op: None,
            collection: None,
            index: None,
            query_string: None,
            raw_body: None,
            status: "ok",
            error: None,
            took_ms: None,
        }
    }
    pub fn op(mut self, v: &'a str) -> Self {
        self.op = Some(v);
        self
    }
    pub fn collection(mut self, v: &'a str) -> Self {
        self.collection = Some(v);
        self
    }
    pub fn index(mut self, v: &'a str) -> Self {
        self.index = Some(v);
        self
    }
    pub fn query(mut self, v: &'a str) -> Self {
        self.query_string = Some(v);
        self
    }
    pub fn raw(mut self, v: serde_json::Value) -> Self {
        self.raw_body = Some(v);
        self
    }
    pub fn status(mut self, v: &'a str) -> Self {
        self.status = v;
        self
    }
    pub fn error(mut self, v: impl Into<String>) -> Self {
        self.error = Some(v.into());
        self
    }
    pub fn took(mut self, v: u64) -> Self {
        self.took_ms = Some(v);
        self
    }
}

/// Append a query-log line if logging is enabled; otherwise no-op.
pub fn log(entry: &QueryLogEntry<'_>) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(mx) = WRITER.get() {
        if let Ok(mut w) = mx.lock() {
            let _ = serde_json::to_writer(&mut *w, entry);
            let _ = writeln!(w);
            let _ = w.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_is_noop() {
        // Default state is disabled; ensure init(false,...) keeps it off and
        // log() does not panic without a writer.
        assert!(!is_enabled());
        init(false, Path::new("/nonexistent/should/not/touch"));
        assert!(!is_enabled());
        log(&QueryLogEntry::new("test").query("*"));
    }
}
