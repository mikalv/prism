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
