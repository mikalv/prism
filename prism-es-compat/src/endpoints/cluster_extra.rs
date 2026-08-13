//! Extra cluster / nodes / cat endpoints needed for Kibana compatibility

use std::collections::HashMap;

use crate::endpoints::search::EsCompatState;
use crate::response::EsRootInfo;
use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;

// ===================================================================
// Response types
// ===================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodesResponse {
    pub nodes: HashMap<String, EsNodeInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeInfo {
    pub name: String,
    pub transport_address: String,
    pub host: String,
    pub ip: String,
    pub version: String,
    pub build_hash: String,
    pub r#type: String,
    pub roles: Vec<String>,
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodesStatsResponse {
    pub nodes: HashMap<String, EsNodeStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeStats {
    pub name: String,
    pub transport_address: String,
    pub host: String,
    pub ip: String,
    pub version: String,
    pub build_hash: String,
    pub r#type: String,
    pub roles: Vec<String>,
    pub attributes: HashMap<String, String>,
    pub indices: EsNodeIndicesStats,
    pub os: EsNodeOsStats,
    pub process: EsNodeProcessStats,
    pub jvm: EsNodeJvmStats,
    pub thread_pool: HashMap<String, EsThreadPoolStats>,
    pub breaker: HashMap<String, EsBreakerStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EsNodeIndicesStats {
    pub docs: EsNodeDocStats,
    pub store: EsNodeStoreStats,
    pub indexing: EsNodeIndexingStats,
    pub search: EsNodeSearchStats,
    pub query_cache: EsNodeQueryCacheStats,
    pub request_cache: EsNodeRequestCacheStats,
    pub segments: EsNodeSegmentsStats,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EsNodeDocStats {
    pub count: u64,
    pub deleted: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EsNodeStoreStats {
    pub size_in_bytes: u64,
    pub reserved_in_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EsNodeIndexingStats {
    pub index_total: u64,
    pub index_time_in_millis: u64,
    pub index_current: u64,
    pub index_failed: u64,
    pub delete_total: u64,
    pub delete_time_in_millis: u64,
    pub delete_current: u64,
    pub noop_update_total: u64,
    pub is_throttled: bool,
    pub throttle_time_in_millis: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EsNodeSearchStats {
    pub query_total: u64,
    pub query_time_in_millis: u64,
    pub query_current: u64,
    pub fetch_total: u64,
    pub fetch_time_in_millis: u64,
    pub fetch_current: u64,
    pub scroll_total: u64,
    pub scroll_time_in_millis: u64,
    pub scroll_current: u64,
    pub suggest_total: u64,
    pub suggest_time_in_millis: u64,
    pub suggest_current: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EsNodeQueryCacheStats {
    pub memory_size_in_bytes: u64,
    pub total_count: u64,
    pub hit_count: u64,
    pub miss_count: u64,
    pub cache_size: u64,
    pub cache_count: u64,
    pub evictions: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EsNodeRequestCacheStats {
    pub memory_size_in_bytes: u64,
    pub evictions: u64,
    pub hit_count: u64,
    pub miss_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EsNodeSegmentsStats {
    pub count: u64,
    pub memory_in_bytes: u64,
    pub terms_memory_in_bytes: u64,
    pub stored_fields_memory_in_bytes: u64,
    pub term_vectors_memory_in_bytes: u64,
    pub norms_memory_in_bytes: u64,
    pub points_memory_in_bytes: u64,
    pub doc_values_memory_in_bytes: u64,
    pub index_writer_memory_in_bytes: u64,
    pub version_map_memory_in_bytes: u64,
    pub fixed_bit_set_memory_in_bytes: u64,
    pub max_unsafe_auto_id_timestamp: i64,
    pub segms: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EsShardStats {
    pub r#type: String,
    pub state: String,
    pub primary: bool,
    pub index: String,
    pub shard: String,
    pub node_id: String,
    pub docs: EsNodeDocStats,
    pub store: EsNodeStoreStats,
    pub segms: HashMap<String, u64>,
    pub routing_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeOsStats {
    pub timestamp: u64,
    pub cpu: EsNodeCpuStats,
    pub memory: EsNodeMemoryStats,
    pub swap: EsNodeSwapStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeCpuStats {
    pub load_average: Vec<f64>,
    pub percent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeMemoryStats {
    pub total_in_bytes: u64,
    pub free_in_bytes: u64,
    pub used_in_bytes: u64,
    pub free_percent: u32,
    pub used_percent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeSwapStats {
    pub total_in_bytes: u64,
    pub free_in_bytes: u64,
    pub used_in_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeProcessStats {
    pub timestamp: u64,
    pub cpu: EsNodeProcessCpuStats,
    pub mem: EsNodeProcessMemStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeProcessCpuStats {
    pub percent: u32,
    pub total_in_millis: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeProcessMemStats {
    pub resident_in_bytes: u64,
    pub share_in_bytes: u64,
    pub total_virtual_in_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeJvmStats {
    pub timestamp: u64,
    pub mem: EsNodeJvmMemStats,
    pub threads: EsNodeJvmThreadStats,
    pub gc: HashMap<String, EsNodeGcStats>,
    pub buffer_pools: HashMap<String, EsNodeBufferPoolStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeJvmMemStats {
    pub heap_used_in_bytes: u64,
    pub heap_used_percent: u32,
    pub heap_committed_in_bytes: u64,
    pub heap_max_in_bytes: u64,
    pub non_heap_used_in_bytes: u64,
    pub non_heap_committed_in_bytes: u64,
    pub pools: HashMap<String, EsNodeJvmPoolStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeJvmPoolStats {
    pub used_in_bytes: u64,
    pub max_in_bytes: u64,
    pub peak_used_in_bytes: u64,
    pub peak_max_in_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeJvmThreadStats {
    pub count: u32,
    pub peak_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeGcStats {
    pub collectors: HashMap<String, EsNodeGcCollectorStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeGcCollectorStats {
    pub collection_count: u64,
    pub collection_time_in_millis: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsNodeBufferPoolStats {
    pub count: u32,
    pub used_in_bytes: u64,
    pub total_capacity_in_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsThreadPoolStats {
    pub threads: u32,
    pub queue: u32,
    pub active: u32,
    pub rejected: u64,
    pub largest: u32,
    pub completed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsBreakerStats {
    pub limit_in_bytes: u64,
    pub estimated: String,
    pub estimated_size_in_bytes: u64,
    pub overhead: f64,
    pub tripped: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsLicenseResponse {
    pub status: String,
    pub license: EsLicenseInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsLicenseInfo {
    pub r#type: String,
    pub mode: String,
    pub status: String,
    pub uid: String,
    pub expiry_date_in_millis: i64,
    pub max_resource_units: i64,
    pub issued_to: String,
    pub issuer: String,
    pub issue_date_in_millis: i64,
    pub start_date_in_millis: i64,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsCatAliasResponse {
    pub alias: String,
    pub index: String,
    pub filter: String,
    pub routing_index: String,
    pub is_write_index: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsCatShardResponse {
    pub index: String,
    pub shard: String,
    pub prirep: String,
    pub state: String,
    pub docs: String,
    pub store: String,
    pub ip: String,
    pub node: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsCatNodeResponse {
    pub name: String,
    pub ip: String,
    pub id: String,
    pub r#type: String,
    pub build_type: String,
    pub version: String,
    pub max_heap: String,
    pub used_heap: String,
    pub heap_used_percent: String,
    pub ram: String,
    pub cpu: String,
    pub load_1m: String,
    pub load_5m: String,
    pub load_15m: String,
    pub uptime: String,
    pub role: String,
    pub master: String,
    pub ingest: String,
    pub data: String,
    pub coordinating: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsClusterStateResponse {
    pub cluster_name: String,
    pub cluster_uuid: String,
    pub version: i64,
    pub state: EsClusterState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsClusterState {
    pub metadata: EsClusterStateMetadata,
    pub blocks: HashMap<String, EsClusterStateBlock>,
    pub routing_nodes: EsRoutingNodes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsClusterStateMetadata {
    pub indices: HashMap<String, EsIndexMetadata>,
    pub index_template: HashMap<String, Value>,
    pub ingest_pipeline: HashMap<String, Value>,
    pub templates: HashMap<String, Value>,
    pub persistent: HashMap<String, Value>,
    pub transient: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsIndexMetadata {
    pub aliases: HashMap<String, Value>,
    pub index_provider: EsIndexProviderMetadata,
    pub settings: EsIndexSettingsMetadata,
    pub mappings: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsIndexProviderMetadata {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsIndexSettingsMetadata {
    pub index: EsIndexProviderSettingsMetadata,
    pub uuid: String,
    pub provided_name: String,
    pub creation_date: String,
    pub routing: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsIndexProviderSettingsMetadata {
    pub number_of_shards: String,
    pub number_of_replicas: String,
    pub codec: String,
    pub max_result_window: String,
    pub mapping: HashMap<String, Value>,
    pub merge: HashMap<String, Value>,
    pub translog: HashMap<String, Value>,
    pub allocation: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsClusterStateBlock {
    pub description: String,
    pub reason: String,
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsRoutingNodes {
    pub nodes: HashMap<String, EsRoutingNode>,
    pub unassigned_shards: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsRoutingNode {
    pub name: String,
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsClusterSettingsResponse {
    pub persistent: HashMap<String, Value>,
    pub transient: HashMap<String, Value>,
    pub defaults: HashMap<String, Value>,
}

// ===================================================================
// Helpers
// ===================================================================

#[derive(Debug, Default, Deserialize)]
struct FilterPathQuery {
    #[serde(rename = "filter_path")]
    filter_path: Option<String>,
}

fn build_node_info(node_id: &str, metric: &str) -> EsNodeInfo {
    let _ = (node_id, metric);
    EsNodeInfo {
        name: "prism".to_string(),
        transport_address: "127.0.0.1:9300".to_string(),
        host: "127.0.0.1".to_string(),
        ip: "127.0.0.1".to_string(),
        version: EsRootInfo::default().version.number.clone(),
        build_hash: "unknown".to_string(),
        r#type: "".to_string(),
        roles: vec![
            "data".to_string(),
            "master".to_string(),
            "ingest".to_string(),
        ],
        attributes: HashMap::new(),
    }
}

// ===================================================================
// Handlers
// ===================================================================

pub async fn nodes_handler(
    State(_state): State<EsCompatState>,
    Path((_node_id, _metric)): Path<(String, String)>,
) -> Json<EsNodesResponse> {
    let mut nodes = HashMap::new();
    nodes.insert("prism-node-1".to_string(), build_node_info(&_node_id, &_metric));
    Json(EsNodesResponse { nodes })
}

pub async fn nodes_stats_handler(
    State(state): State<EsCompatState>,
    Path((_node_id, _metric)): Path<(String, String)>,
) -> Json<EsNodesStatsResponse> {
    let collections = state.manager.list_collections();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let mut total_docs: u64 = 0;
    let mut total_store_bytes: u64 = 0;
    for c in &collections {
        if let Ok(stats) = state.manager.stats(c).await {
            total_docs += stats.document_count as u64;
            total_store_bytes += stats.size_bytes as u64;
        }
    }

    let mut nodes = HashMap::new();
    nodes.insert(
        "prism-node-1".to_string(),
        EsNodeStats {
            name: "prism".to_string(),
            transport_address: "127.0.0.1:9300".to_string(),
            host: "127.0.0.1".to_string(),
            ip: "127.0.0.1".to_string(),
            version: EsRootInfo::default().version.number.clone(),
            build_hash: "unknown".to_string(),
            r#type: "".to_string(),
            roles: vec![
                "data".to_string(),
                "master".to_string(),
                "ingest".to_string(),
            ],
            attributes: HashMap::new(),
            indices: EsNodeIndicesStats {
                docs: EsNodeDocStats {
                    count: total_docs,
                    deleted: 0,
                },
                store: EsNodeStoreStats {
                    size_in_bytes: total_store_bytes,
                    reserved_in_bytes: 0,
                },
                ..Default::default()
            },
            os: EsNodeOsStats {
                timestamp: now_ms,
                cpu: EsNodeCpuStats {
                    load_average: vec![0.0, 0.0, 0.0],
                    percent: 0,
                },
                memory: EsNodeMemoryStats {
                    total_in_bytes: 0,
                    free_in_bytes: 0,
                    used_in_bytes: 0,
                    free_percent: 0,
                    used_percent: 0,
                },
                swap: EsNodeSwapStats {
                    total_in_bytes: 0,
                    free_in_bytes: 0,
                    used_in_bytes: 0,
                },
            },
            process: EsNodeProcessStats {
                timestamp: now_ms,
                cpu: EsNodeProcessCpuStats {
                    percent: 0,
                    total_in_millis: 0,
                },
                mem: EsNodeProcessMemStats {
                    resident_in_bytes: 0,
                    share_in_bytes: 0,
                    total_virtual_in_bytes: 0,
                },
            },
            jvm: EsNodeJvmStats {
                timestamp: now_ms,
                mem: EsNodeJvmMemStats {
                    heap_used_in_bytes: 0,
                    heap_used_percent: 0,
                    heap_committed_in_bytes: 0,
                    heap_max_in_bytes: 0,
                    non_heap_used_in_bytes: 0,
                    non_heap_committed_in_bytes: 0,
                    pools: HashMap::new(),
                },
                threads: EsNodeJvmThreadStats { count: 0, peak_count: 0 },
                gc: HashMap::new(),
                buffer_pools: HashMap::new(),
            },
            thread_pool: HashMap::new(),
            breaker: HashMap::new(),
        },
    );

    Json(EsNodesStatsResponse { nodes })
}

pub async fn license_handler() -> Json<EsLicenseResponse> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    Json(EsLicenseResponse {
        status: "active".to_string(),
        license: EsLicenseInfo {
            r#type: "basic".to_string(),
            mode: "basic".to_string(),
            status: "active".to_string(),
            uid: "prism-basic".to_string(),
            expiry_date_in_millis: now_ms + 365i64 * 24 * 60 * 60 * 1000,
            max_resource_units: -1,
            issued_to: "prism".to_string(),
            issuer: "prism".to_string(),
            issue_date_in_millis: now_ms,
            start_date_in_millis: now_ms,
            signature: "".to_string(),
        },
    })
}

/// GET /_xpack
///
/// Kibana's licensing plugin reads license mode/features from here. Without
/// it the plugin reports "License information could not be obtained from
/// Elasticsearch" and `/api/status` returns 503.
pub async fn xpack_handler() -> Json<Value> {
    let info = EsRootInfo::default();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    Json(json!({
        "build": {
            "flavor": "default",
            "type": "prism",
            "hash": "prism",
            "date": "2026-01-01",
            "version": info.version.number
        },
        "license": {
            "uid": "prism-basic",
            "type": "basic",
            "mode": "basic",
            "status": "active",
            "expiry_date_in_millis": now_ms + 365 * 24 * 60 * 60 * 1000
        },
        "features": {
            "security": { "available": true, "enabled": false },
            "monitoring": { "available": true, "enabled": false },
            "rollup": { "available": true, "enabled": true },
            "transform": { "available": true, "enabled": true },
            "sql": { "available": true, "enabled": true },
            "watcher": { "available": true, "enabled": false },
            "data_streams": { "available": true, "enabled": true },
            "data_lifecycle": { "available": true, "enabled": true },
            "logsdb": { "available": true, "enabled": true },
            "enrich": { "available": true, "enabled": true },
            "frozen_indices": { "available": true, "enabled": true },
            "searchable_snapshots": { "available": true, "enabled": true },
            "spatial": { "available": true, "enabled": true },
            "aggregate_metric": { "available": true, "enabled": true }
        },
        "tagline": "You know, for X-Pack"
    }))
}

pub async fn cat_aliases_handler(
    State(state): State<EsCompatState>,
) -> Json<Vec<EsCatAliasResponse>> {
    let mut rows = Vec::new();
    for (alias, indices) in state.manager.list_aliases() {
        for index in indices {
            rows.push(EsCatAliasResponse {
                alias: alias.clone(),
                index,
                filter: "-".to_string(),
                routing_index: "-".to_string(),
                is_write_index: "-".to_string(),
            });
        }
    }
    Json(rows)
}

/// POST /_aliases
///
/// ES alias management. Parses `actions` and applies add/remove to the
/// in-memory alias map. Returns `{"acknowledged": true}`. Prism resolves
/// aliases transparently via `expand_collection_patterns`.
pub async fn update_aliases_handler(
    State(state): State<EsCompatState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    if let Some(actions) = body.get("actions").and_then(|a| a.as_array()) {
        for action in actions {
            if let Some(obj) = action.as_object() {
                if let Some(add) = obj.get("add").and_then(|v| v.as_object()) {
                    let indices = collect_indices(add);
                    if let Some(alias) = add.get("alias").and_then(|v| v.as_str()) {
                        if !indices.is_empty() {
                            state.manager.add_alias(alias, &indices);
                        }
                    }
                }
                if let Some(rem) = obj.get("remove").and_then(|v| v.as_object()) {
                    let indices = collect_indices(rem);
                    if let Some(alias) = rem.get("alias").and_then(|v| v.as_str()) {
                        if !indices.is_empty() {
                            state.manager.remove_alias(alias, &indices);
                        }
                    }
                }
            }
        }
    }
    // Persist the resulting alias map so it survives prism restarts
    // (e.g. `.kibana_task_manager` -> `.kibana_task_manager_9.5.0_001`).
    let map: std::collections::HashMap<String, Vec<String>> =
        state.manager.list_aliases().into_iter().collect();
    crate::persist::save_json(&state.data_dir, "aliases", &map);

    Json(serde_json::json!({ "acknowledged": true }))
}

/// Collect `index` (string) and/or `indices` (array) from an action body.
fn collect_indices(map: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(s) = map.get("index").and_then(|v| v.as_str()) {
        out.push(s.to_string());
    }
    if let Some(arr) = map.get("indices").and_then(|v| v.as_array()) {
        for v in arr {
            if let Some(s) = v.as_str() {
                out.push(s.to_string());
            }
        }
    }
    out
}

pub async fn cat_shards_handler(
    State(state): State<EsCompatState>,
) -> Json<Vec<EsCatShardResponse>> {
    let collections = state.manager.list_collections();

    let mut shards = Vec::with_capacity(collections.len());
    for collection in collections {
        shards.push(EsCatShardResponse {
            index: collection.clone(),
            shard: "0".to_string(),
            prirep: "p".to_string(),
            state: "STARTED".to_string(),
            docs: "0".to_string(),
            store: "0b".to_string(),
            ip: "127.0.0.1".to_string(),
            node: "prism-node-1".to_string(),
        });
    }

    Json(shards)
}

pub async fn cat_nodes_handler() -> Json<Vec<EsCatNodeResponse>> {
    Json(vec![EsCatNodeResponse {
        name: "prism".to_string(),
        ip: "127.0.0.1".to_string(),
        id: "prism-node-1".to_string(),
        r#type: "".to_string(),
        build_type: "prism".to_string(),
        version: EsRootInfo::default().version.number.clone(),
        max_heap: "".to_string(),
        used_heap: "".to_string(),
        heap_used_percent: "".to_string(),
        ram: "".to_string(),
        cpu: "".to_string(),
        load_1m: "".to_string(),
        load_5m: "".to_string(),
        load_15m: "".to_string(),
        uptime: "".to_string(),
        role: "dim".to_string(),
        master: "".to_string(),
        ingest: "".to_string(),
        data: "d".to_string(),
        coordinating: "".to_string(),
    }])
}

pub async fn cluster_state_handler(
    State(state): State<EsCompatState>,
) -> Json<EsClusterStateResponse> {
    let collections = state.manager.list_collections();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let mut indices = HashMap::new();
    for collection in collections {
        indices.insert(
            collection.clone(),
            EsIndexMetadata {
                aliases: HashMap::new(),
                index_provider: EsIndexProviderMetadata { name: "".to_string() },
                settings: EsIndexSettingsMetadata {
                    index: EsIndexProviderSettingsMetadata {
                        number_of_shards: "1".to_string(),
                        number_of_replicas: "0".to_string(),
                        codec: "default".to_string(),
                        max_result_window: "10000".to_string(),
                        mapping: HashMap::new(),
                        merge: HashMap::new(),
                        translog: HashMap::new(),
                        allocation: HashMap::new(),
                    },
                    uuid: collection.clone(),
                    provided_name: collection.clone(),
                    creation_date: now_ms.to_string(),
                    routing: HashMap::new(),
                },
                mappings: Value::Null,
            },
        );
    }

    let mut nodes = HashMap::new();
    nodes.insert(
        "prism-node-1".to_string(),
        EsRoutingNode {
            name: "prism".to_string(),
            attributes: HashMap::new(),
        },
    );

    Json(EsClusterStateResponse {
        cluster_name: "prism".to_string(),
        cluster_uuid: "prism-es-compat".to_string(),
        version: 1,
        state: EsClusterState {
            metadata: EsClusterStateMetadata {
                indices,
                index_template: HashMap::new(),
                ingest_pipeline: HashMap::new(),
                templates: HashMap::new(),
                persistent: HashMap::new(),
                transient: HashMap::new(),
            },
            blocks: HashMap::new(),
            routing_nodes: EsRoutingNodes {
                nodes,
                unassigned_shards: vec![],
            },
        },
    })
}

pub async fn cluster_settings_handler() -> Json<EsClusterSettingsResponse> {
    Json(EsClusterSettingsResponse {
        persistent: HashMap::new(),
        transient: HashMap::new(),
        defaults: HashMap::new(),
    })
}
