//! Single graph shard — in-memory nodes/edges with optional SegmentStorage persistence.
//!
//! This is the original GraphBackend renamed to GraphShard. Each shard holds a
//! subset of the graph; edges may only reference nodes within the same shard.

use crate::error::{Error, Result};
use crate::schema::types::EdgeTypeConfig;
use parking_lot::RwLock;
use prism_storage::{SegmentStorage, StoragePath};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// A node in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// Unique node identifier
    pub id: String,
    /// Node type/category
    pub node_type: String,
    /// Human-readable title
    pub title: String,
    /// Additional payload data
    #[serde(default)]
    pub payload: serde_json::Value,
}

/// An edge in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Source node ID
    pub from: String,
    /// Target node ID
    pub to: String,
    /// Edge type/relationship
    pub edge_type: String,
    /// Edge weight (for weighted algorithms)
    #[serde(default = "default_weight")]
    pub weight: f32,
}

fn default_weight() -> f32 {
    1.0
}

/// Internal edge storage (adjacency list entry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EdgeEntry {
    to: String,
    edge_type: String,
    weight: f32,
}

/// Statistics about a graph shard (or aggregated across shards).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    /// Number of nodes
    pub node_count: usize,
    /// Number of edges
    pub edge_count: usize,
}

/// A single graph shard with SegmentStorage support.
pub struct GraphShard {
    /// Shard numeric ID
    pub shard_id: u32,
    /// Collection name
    collection: String,
    /// Shard identifier (e.g. "default", "shard_0")
    shard_name: String,
    /// Node storage: id -> node
    nodes: Arc<RwLock<HashMap<String, GraphNode>>>,
    /// Edge storage: from_id -> edges
    edges: Arc<RwLock<HashMap<String, Vec<EdgeEntry>>>>,
    /// Edge type configurations from schema
    edge_configs: Vec<EdgeTypeConfig>,
    /// Storage backend for persistence
    storage: Option<Arc<dyn SegmentStorage>>,
}

impl GraphShard {
    /// Create a new graph shard.
    pub fn new(
        shard_id: u32,
        collection: &str,
        shard_name: &str,
        edge_configs: &[EdgeTypeConfig],
        storage: Option<Arc<dyn SegmentStorage>>,
    ) -> Self {
        Self {
            shard_id,
            collection: collection.to_string(),
            shard_name: shard_name.to_string(),
            nodes: Arc::new(RwLock::new(HashMap::new())),
            edges: Arc::new(RwLock::new(HashMap::new())),
            edge_configs: edge_configs.to_vec(),
            storage,
        }
    }

    /// Initialize the shard, loading persisted data if available.
    pub async fn initialize(&self) -> Result<()> {
        if let Some(ref storage) = self.storage {
            let nodes_path = self.nodes_path();

            if storage.exists(&nodes_path).await.unwrap_or(false) {
                match self.load_from_storage(storage.as_ref()).await {
                    Ok(()) => {
                        tracing::info!(
                            collection = %self.collection,
                            shard = %self.shard_name,
                            "Loaded graph shard from storage"
                        );
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Failed to load graph shard from storage, starting fresh"
                        );
                    }
                }
            }

            self.persist().await?;
        }
        Ok(())
    }

    /// Add a node to the graph shard.
    pub async fn add_node(&self, node: GraphNode) -> Result<()> {
        {
            let mut nodes = self.nodes.write();
            nodes.insert(node.id.clone(), node);
        }
        self.persist().await
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: &str) -> Option<GraphNode> {
        let nodes = self.nodes.read();
        nodes.get(id).cloned()
    }

    /// Remove a node and all its edges.
    pub async fn remove_node(&self, id: &str) -> Result<bool> {
        let removed = {
            let mut nodes = self.nodes.write();
            let mut edges = self.edges.write();

            let existed = nodes.remove(id).is_some();
            if existed {
                edges.remove(id);
                for edge_list in edges.values_mut() {
                    edge_list.retain(|e| e.to != id);
                }
            }
            existed
        };

        if removed {
            self.persist().await?;
        }
        Ok(removed)
    }

    /// Add an edge to the graph shard.
    pub async fn add_edge(&self, edge: GraphEdge) -> Result<()> {
        // Validate edge type if configs are provided
        if !self.edge_configs.is_empty() {
            let valid = self
                .edge_configs
                .iter()
                .any(|c| c.edge_type == edge.edge_type);
            if !valid {
                return Err(Error::Schema(format!(
                    "Unknown edge type: {}. Valid types: {:?}",
                    edge.edge_type,
                    self.edge_configs
                        .iter()
                        .map(|c| &c.edge_type)
                        .collect::<Vec<_>>()
                )));
            }
        }

        {
            let mut edges = self.edges.write();
            let entry = EdgeEntry {
                to: edge.to,
                edge_type: edge.edge_type,
                weight: edge.weight,
            };
            edges.entry(edge.from).or_default().push(entry);
        }
        self.persist().await
    }

    /// Remove an edge.
    pub async fn remove_edge(&self, from: &str, to: &str, edge_type: Option<&str>) -> Result<bool> {
        let removed = {
            let mut edges = self.edges.write();
            if let Some(edge_list) = edges.get_mut(from) {
                let before_len = edge_list.len();
                edge_list.retain(|e| {
                    if e.to != to {
                        return true;
                    }
                    if let Some(et) = edge_type {
                        e.edge_type != et
                    } else {
                        false
                    }
                });
                edge_list.len() < before_len
            } else {
                false
            }
        };

        if removed {
            self.persist().await?;
        }
        Ok(removed)
    }

    /// Get all outgoing edges from a node.
    pub fn get_edges(&self, from: &str) -> Vec<GraphEdge> {
        let edges = self.edges.read();
        edges
            .get(from)
            .map(|entries| {
                entries
                    .iter()
                    .map(|e| GraphEdge {
                        from: from.to_string(),
                        to: e.to.clone(),
                        edge_type: e.edge_type.clone(),
                        weight: e.weight,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get edges of a specific type from a node.
    pub fn get_edges_by_type(&self, from: &str, edge_type: &str) -> Vec<GraphEdge> {
        let edges = self.edges.read();
        edges
            .get(from)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|e| e.edge_type == edge_type)
                    .map(|e| GraphEdge {
                        from: from.to_string(),
                        to: e.to.clone(),
                        edge_type: e.edge_type.clone(),
                        weight: e.weight,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Breadth-first search from a starting node.
    pub fn bfs(&self, start: &str, edge_type: &str, max_depth: usize) -> Vec<String> {
        let edges = self.edges.read();
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();

        queue.push_back((start.to_string(), 0));
        visited.insert(start.to_string());

        while let Some((node_id, depth)) = queue.pop_front() {
            if depth > 0 {
                result.push(node_id.clone());
            }

            if depth >= max_depth {
                continue;
            }

            if let Some(edge_list) = edges.get(&node_id) {
                for edge in edge_list {
                    if edge.edge_type == edge_type && !visited.contains(&edge.to) {
                        visited.insert(edge.to.clone());
                        queue.push_back((edge.to.clone(), depth + 1));
                    }
                }
            }
        }

        result
    }

    /// Find shortest path between two nodes using Dijkstra's algorithm.
    pub fn shortest_path(
        &self,
        start: &str,
        target: &str,
        edge_types: Option<&[String]>,
    ) -> Option<Vec<String>> {
        let edges = self.edges.read();
        let mut distances: HashMap<String, f32> = HashMap::new();
        let mut previous: HashMap<String, String> = HashMap::new();
        let mut heap = BinaryHeap::new();

        distances.insert(start.to_string(), 0.0);
        heap.push(DijkstraState {
            cost: 0.0,
            node: start.to_string(),
        });

        while let Some(DijkstraState { cost, node }) = heap.pop() {
            if node == target {
                let mut path = vec![target.to_string()];
                let mut current = target.to_string();
                while let Some(prev) = previous.get(&current) {
                    path.push(prev.clone());
                    current = prev.clone();
                }
                path.reverse();
                return Some(path);
            }

            if cost > *distances.get(&node).unwrap_or(&f32::MAX) {
                continue;
            }

            if let Some(edge_list) = edges.get(&node) {
                for edge in edge_list {
                    if let Some(types) = edge_types {
                        if !types.contains(&edge.edge_type) {
                            continue;
                        }
                    }

                    let new_cost = cost + edge.weight;
                    let current_cost = *distances.get(&edge.to).unwrap_or(&f32::MAX);

                    if new_cost < current_cost {
                        distances.insert(edge.to.clone(), new_cost);
                        previous.insert(edge.to.clone(), node.clone());
                        heap.push(DijkstraState {
                            cost: new_cost,
                            node: edge.to.clone(),
                        });
                    }
                }
            }
        }

        None
    }

    /// Get statistics about this shard.
    pub fn stats(&self) -> GraphStats {
        let nodes = self.nodes.read();
        let edges = self.edges.read();

        let edge_count: usize = edges.values().map(|v| v.len()).sum();

        GraphStats {
            node_count: nodes.len(),
            edge_count,
        }
    }

    /// List all nodes in this shard.
    pub fn list_nodes(&self) -> Vec<GraphNode> {
        let nodes = self.nodes.read();
        nodes.values().cloned().collect()
    }

    /// Merge nodes and edges from another source into this shard.
    /// Duplicate node IDs are overwritten. Edges are appended.
    pub(crate) async fn merge_from(
        &self,
        other_nodes: HashMap<String, GraphNode>,
        other_edges: HashMap<String, Vec<EdgeEntry>>,
    ) -> Result<()> {
        {
            let mut nodes = self.nodes.write();
            for (id, node) in other_nodes {
                nodes.insert(id, node);
            }
        }
        {
            let mut edges = self.edges.write();
            for (from, entries) in other_edges {
                edges.entry(from).or_default().extend(entries);
            }
        }
        self.persist().await
    }

    /// Export raw nodes and edges (for merge source).
    pub(crate) fn export_raw(
        &self,
    ) -> (HashMap<String, GraphNode>, HashMap<String, Vec<EdgeEntry>>) {
        let nodes = self.nodes.read().clone();
        let edges = self.edges.read().clone();
        (nodes, edges)
    }

    /// Clear all nodes and edges from this shard.
    pub async fn clear(&self) -> Result<()> {
        self.nodes.write().clear();
        self.edges.write().clear();
        self.persist().await
    }

    /// List all edges in this shard.
    pub fn list_edges(&self) -> Vec<GraphEdge> {
        let edges = self.edges.read();
        edges
            .iter()
            .flat_map(|(from, entries)| {
                entries.iter().map(move |e| GraphEdge {
                    from: from.clone(),
                    to: e.to.clone(),
                    edge_type: e.edge_type.clone(),
                    weight: e.weight,
                })
            })
            .collect()
    }

    // --- Storage helpers ---

    fn nodes_path(&self) -> StoragePath {
        StoragePath::graph(&self.collection, &self.shard_name, "nodes.json")
    }

    fn edges_path(&self) -> StoragePath {
        StoragePath::graph(&self.collection, &self.shard_name, "edges.json")
    }

    async fn persist(&self) -> Result<()> {
        let Some(ref storage) = self.storage else {
            return Ok(());
        };

        let (nodes_data, edges_data) = {
            let nodes = self.nodes.read();
            let edges = self.edges.read();

            let nodes_json = serde_json::to_vec_pretty(&*nodes)
                .map_err(|e| Error::Storage(format!("Failed to serialize nodes: {}", e)))?;
            let edges_json = serde_json::to_vec_pretty(&*edges)
                .map_err(|e| Error::Storage(format!("Failed to serialize edges: {}", e)))?;

            (nodes_json, edges_json)
        };

        storage
            .write_bytes(&self.nodes_path(), &nodes_data)
            .await
            .map_err(|e| Error::Storage(format!("Failed to write nodes: {}", e)))?;
        storage
            .write_bytes(&self.edges_path(), &edges_data)
            .await
            .map_err(|e| Error::Storage(format!("Failed to write edges: {}", e)))?;

        Ok(())
    }

    async fn load_from_storage(&self, storage: &dyn SegmentStorage) -> Result<()> {
        let nodes_data = storage
            .read_vec(&self.nodes_path())
            .await
            .map_err(|e| Error::Storage(format!("Failed to read nodes: {}", e)))?;
        let edges_data = storage
            .read_vec(&self.edges_path())
            .await
            .map_err(|e| Error::Storage(format!("Failed to read edges: {}", e)))?;

        let loaded_nodes: HashMap<String, GraphNode> = serde_json::from_slice(&nodes_data)
            .map_err(|e| Error::Storage(format!("Failed to deserialize nodes: {}", e)))?;
        let loaded_edges: HashMap<String, Vec<EdgeEntry>> = serde_json::from_slice(&edges_data)
            .map_err(|e| Error::Storage(format!("Failed to deserialize edges: {}", e)))?;

        {
            let mut nodes = self.nodes.write();
            let mut edges = self.edges.write();
            *nodes = loaded_nodes;
            *edges = loaded_edges;
        }

        Ok(())
    }
}

/// State for Dijkstra's algorithm.
#[derive(Debug, Clone)]
struct DijkstraState {
    cost: f32,
    node: String,
}

impl PartialEq for DijkstraState {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost && self.node == other.node
    }
}

impl Eq for DijkstraState {}

impl Ord for DijkstraState {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for DijkstraState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_storage::LocalStorage;
    use tempfile::tempdir;

    fn test_edge_configs() -> Vec<EdgeTypeConfig> {
        vec![
            EdgeTypeConfig {
                edge_type: "related".to_string(),
                from_field: "id".to_string(),
                to_field: "related_id".to_string(),
            },
            EdgeTypeConfig {
                edge_type: "parent".to_string(),
                from_field: "id".to_string(),
                to_field: "parent_id".to_string(),
            },
        ]
    }

    #[tokio::test]
    async fn test_add_and_get_node() {
        let shard = GraphShard::new(0, "test", "default", &test_edge_configs(), None);

        let node = GraphNode {
            id: "n1".to_string(),
            node_type: "document".to_string(),
            title: "Test Node".to_string(),
            payload: serde_json::json!({"key": "value"}),
        };

        shard.add_node(node.clone()).await.unwrap();

        let retrieved = shard.get_node("n1").unwrap();
        assert_eq!(retrieved.id, "n1");
        assert_eq!(retrieved.title, "Test Node");
    }

    #[tokio::test]
    async fn test_add_and_get_edges() {
        let shard = GraphShard::new(0, "test", "default", &test_edge_configs(), None);

        shard
            .add_node(GraphNode {
                id: "a".to_string(),
                node_type: "doc".to_string(),
                title: "A".to_string(),
                payload: serde_json::Value::Null,
            })
            .await
            .unwrap();
        shard
            .add_node(GraphNode {
                id: "b".to_string(),
                node_type: "doc".to_string(),
                title: "B".to_string(),
                payload: serde_json::Value::Null,
            })
            .await
            .unwrap();

        shard
            .add_edge(GraphEdge {
                from: "a".to_string(),
                to: "b".to_string(),
                edge_type: "related".to_string(),
                weight: 1.0,
            })
            .await
            .unwrap();

        let edges = shard.get_edges("a");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].to, "b");
    }

    #[tokio::test]
    async fn test_bfs() {
        let shard = GraphShard::new(0, "test", "default", &test_edge_configs(), None);

        for id in &["a", "b", "c", "d"] {
            shard
                .add_node(GraphNode {
                    id: id.to_string(),
                    node_type: "doc".to_string(),
                    title: id.to_string(),
                    payload: serde_json::Value::Null,
                })
                .await
                .unwrap();
        }

        for (from, to) in &[("a", "b"), ("b", "c"), ("c", "d")] {
            shard
                .add_edge(GraphEdge {
                    from: from.to_string(),
                    to: to.to_string(),
                    edge_type: "related".to_string(),
                    weight: 1.0,
                })
                .await
                .unwrap();
        }

        let result = shard.bfs("a", "related", 2);
        assert!(result.contains(&"b".to_string()));
        assert!(result.contains(&"c".to_string()));
        assert!(!result.contains(&"d".to_string()));
    }

    #[tokio::test]
    async fn test_shortest_path() {
        let shard = GraphShard::new(0, "test", "default", &test_edge_configs(), None);

        for id in &["a", "b", "c", "d"] {
            shard
                .add_node(GraphNode {
                    id: id.to_string(),
                    node_type: "doc".to_string(),
                    title: id.to_string(),
                    payload: serde_json::Value::Null,
                })
                .await
                .unwrap();
        }

        shard
            .add_edge(GraphEdge {
                from: "a".to_string(),
                to: "b".to_string(),
                edge_type: "related".to_string(),
                weight: 1.0,
            })
            .await
            .unwrap();
        shard
            .add_edge(GraphEdge {
                from: "b".to_string(),
                to: "d".to_string(),
                edge_type: "related".to_string(),
                weight: 1.0,
            })
            .await
            .unwrap();
        shard
            .add_edge(GraphEdge {
                from: "a".to_string(),
                to: "c".to_string(),
                edge_type: "related".to_string(),
                weight: 0.5,
            })
            .await
            .unwrap();
        shard
            .add_edge(GraphEdge {
                from: "c".to_string(),
                to: "d".to_string(),
                edge_type: "related".to_string(),
                weight: 1.0,
            })
            .await
            .unwrap();

        let path = shard.shortest_path("a", "d", None).unwrap();
        assert_eq!(path, vec!["a", "c", "d"]);
    }

    #[tokio::test]
    async fn test_persistence_with_segment_storage() {
        let dir = tempdir().unwrap();
        let storage: Arc<dyn SegmentStorage> = Arc::new(LocalStorage::new(dir.path()));

        {
            let shard = GraphShard::new(
                0,
                "test",
                "default",
                &test_edge_configs(),
                Some(storage.clone()),
            );
            shard.initialize().await.unwrap();

            shard
                .add_node(GraphNode {
                    id: "n1".to_string(),
                    node_type: "doc".to_string(),
                    title: "Node 1".to_string(),
                    payload: serde_json::Value::Null,
                })
                .await
                .unwrap();

            shard
                .add_edge(GraphEdge {
                    from: "n1".to_string(),
                    to: "n2".to_string(),
                    edge_type: "related".to_string(),
                    weight: 1.5,
                })
                .await
                .unwrap();
        }

        {
            let shard = GraphShard::new(
                0,
                "test",
                "default",
                &test_edge_configs(),
                Some(storage.clone()),
            );
            shard.initialize().await.unwrap();

            let node = shard.get_node("n1").unwrap();
            assert_eq!(node.title, "Node 1");

            let edges = shard.get_edges("n1");
            assert_eq!(edges.len(), 1);
            assert_eq!(edges[0].weight, 1.5);
        }
    }

    #[tokio::test]
    async fn test_stats() {
        let shard = GraphShard::new(0, "test", "default", &test_edge_configs(), None);

        shard
            .add_node(GraphNode {
                id: "a".to_string(),
                node_type: "doc".to_string(),
                title: "A".to_string(),
                payload: serde_json::Value::Null,
            })
            .await
            .unwrap();
        shard
            .add_node(GraphNode {
                id: "b".to_string(),
                node_type: "doc".to_string(),
                title: "B".to_string(),
                payload: serde_json::Value::Null,
            })
            .await
            .unwrap();

        shard
            .add_edge(GraphEdge {
                from: "a".to_string(),
                to: "b".to_string(),
                edge_type: "related".to_string(),
                weight: 1.0,
            })
            .await
            .unwrap();

        let stats = shard.stats();
        assert_eq!(stats.node_count, 2);
        assert_eq!(stats.edge_count, 1);
    }

    #[tokio::test]
    async fn test_invalid_edge_type() {
        let shard = GraphShard::new(0, "test", "default", &test_edge_configs(), None);

        let result = shard
            .add_edge(GraphEdge {
                from: "a".to_string(),
                to: "b".to_string(),
                edge_type: "unknown_type".to_string(),
                weight: 1.0,
            })
            .await;

        assert!(result.is_err());
    }
}
