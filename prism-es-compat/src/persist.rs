//! Tiny JSON persistence helpers for ES-compat state (aliases, index
//! templates). State is kept in memory for fast reads and snapshotted to
//! `{data_dir}/es-compat/*.json` so it survives prism restarts.
//!
//! Prism's collection *schemas* persist separately (schema yaml files); this
//! module covers the ES-compat metadata that lives above the core manager.

use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;

pub fn es_compat_dir(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("es-compat")
}

pub fn ensure_dir(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
}

/// Serialize `value` to `<dir>/<name>.json` atomically (tmp + rename).
pub fn save_json<T: Serialize>(dir: &Path, name: &str, value: &T) {
    ensure_dir(dir);
    let path = dir.join(format!("{name}.json"));
    let tmp = dir.join(format!("{name}.json.tmp"));
    match serde_json::to_vec_pretty(value) {
        Ok(bytes) => {
            if std::fs::write(&tmp, &bytes).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
        Err(e) => tracing::warn!(%name, "failed to serialize es-compat state: {e}"),
    }
}

/// Load `<dir>/<name>.json` and deserialize, or `None` if absent/invalid.
pub fn load_json<T: DeserializeOwned>(dir: &Path, name: &str) -> Option<T> {
    let path = dir.join(format!("{name}.json"));
    let bytes = std::fs::read(&path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!(%name, "failed to parse es-compat state: {e}");
            None
        }
    }
}
