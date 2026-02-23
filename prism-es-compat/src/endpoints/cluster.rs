//! ES-compatible cluster endpoints

use crate::endpoints::search::EsCompatState;
use crate::error::EsCompatError;
use crate::response::{EsCatIndex, EsClusterHealth, EsRootInfo};
use axum::extract::State;
use axum::Json;

/// GET /_elastic/ - Root info (ES version info)
pub async fn root_handler() -> Json<EsRootInfo> {
    Json(EsRootInfo::default())
}

/// GET /_elastic/_cluster/health - Cluster health
pub async fn cluster_health_handler(
    State(state): State<EsCompatState>,
) -> Result<Json<EsClusterHealth>, EsCompatError> {
    // Get basic stats from manager (sync method)
    let collections = state.manager.list_collections();

    let health = EsClusterHealth {
        active_primary_shards: collections.len() as u32,
        active_shards: collections.len() as u32,
        ..Default::default()
    };

    Ok(Json(health))
}

/// GET /_elastic/_cat/indices - List indices
pub async fn cat_indices_handler(
    State(state): State<EsCompatState>,
) -> Result<Json<Vec<EsCatIndex>>, EsCompatError> {
    // Sync method
    let collections = state.manager.list_collections();

    let mut indices = Vec::with_capacity(collections.len());

    for collection in collections {
        // Get stats for collection (async)
        let stats = state.manager.stats(&collection).await.ok();

        let (doc_count, store_size) = stats
            .map(|s| (s.document_count.to_string(), format_bytes(s.size_bytes)))
            .unwrap_or_else(|| ("0".to_string(), "0b".to_string()));

        indices.push(EsCatIndex {
            health: "green".to_string(),
            status: "open".to_string(),
            index: collection.clone(),
            uuid: format!("{:x}", md5_hash(&collection)),
            pri: "1".to_string(),
            rep: "0".to_string(),
            docs_count: doc_count.clone(),
            docs_deleted: "0".to_string(),
            store_size: store_size.clone(),
            pri_store_size: store_size,
        });
    }

    Ok(Json(indices))
}

fn format_bytes(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = KB * 1024;
    const GB: usize = MB * 1024;

    if bytes >= GB {
        format!("{:.1}gb", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}mb", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}kb", bytes as f64 / KB as f64)
    } else {
        format!("{}b", bytes)
    }
}

fn md5_hash(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===================================================================
    // format_bytes
    // ===================================================================

    #[test]
    fn test_format_bytes_zero() {
        assert_eq!(format_bytes(0), "0b");
    }

    #[test]
    fn test_format_bytes_small() {
        assert_eq!(format_bytes(512), "512b");
    }

    #[test]
    fn test_format_bytes_exactly_1kb() {
        assert_eq!(format_bytes(1024), "1.0kb");
    }

    #[test]
    fn test_format_bytes_kilobytes() {
        assert_eq!(format_bytes(1536), "1.5kb");
    }

    #[test]
    fn test_format_bytes_megabytes() {
        assert_eq!(format_bytes(1048576), "1.0mb");
    }

    #[test]
    fn test_format_bytes_megabytes_fractional() {
        // 1.5 MB = 1572864
        assert_eq!(format_bytes(1572864), "1.5mb");
    }

    #[test]
    fn test_format_bytes_gigabytes() {
        assert_eq!(format_bytes(1073741824), "1.0gb");
    }

    #[test]
    fn test_format_bytes_gigabytes_fractional() {
        // 2.5 GB
        assert_eq!(format_bytes(2684354560), "2.5gb");
    }

    #[test]
    fn test_format_bytes_just_under_kb() {
        assert_eq!(format_bytes(1023), "1023b");
    }

    // ===================================================================
    // md5_hash — deterministic
    // ===================================================================

    #[test]
    fn test_md5_hash_deterministic() {
        let h1 = md5_hash("test_collection");
        let h2 = md5_hash("test_collection");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_md5_hash_different_inputs() {
        let h1 = md5_hash("collection_a");
        let h2 = md5_hash("collection_b");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_md5_hash_empty() {
        // Should not panic
        let _ = md5_hash("");
    }

    // ===================================================================
    // root_handler
    // ===================================================================

    #[tokio::test]
    async fn test_root_handler_returns_info() {
        let Json(info) = root_handler().await;
        assert_eq!(info.name, "prism");
        assert_eq!(info.version.number, "7.17.0");
        assert!(info.tagline.contains("Prism"));
    }
}
