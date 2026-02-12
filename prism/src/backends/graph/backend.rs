//! Sharded graph backend — distributes nodes across multiple GraphShards.
//!
//! Uses the same hash function as the vector backend (`shard_for_doc`) so graph
//! nodes land in the same shard as their vector embeddings.

use crate::backends::vector::shard::shard_for_doc;
use crate::error::{Error, Result};
use crate::schema::types::{GraphBackendConfig, GraphScope};
use prism_storage::SegmentStorage;
use std::sync::Arc;

use super::shard::{GraphEdge, GraphNode, GraphShard, GraphStats};

/// Multi-shard graph backend.
///
/// When `num_shards == 1` (default) this wraps a single `GraphShard` and
/// behaves identically to the old `GraphBackend`.
pub struct ShardedGraphBackend {
    collection: String,
    shards: Vec<GraphShard>,
    num_shards: usize,
    scope: GraphScope,
}

impl ShardedGraphBackend {
    /// Create a new sharded graph backend.
    pub fn new(
        collection: &str,
        config: &GraphBackendConfig,
        storage: Option<Arc<dyn SegmentStorage>>,
    ) -> Self {
        let num_shards = config.num_shards.max(1);
        let shards: Vec<GraphShard> = (0..num_shards)
            .map(|i| {
                let shard_name = if num_shards == 1 {
                    "default".to_string()
                } else {
                    format!("shard_{}", i)
                };
                GraphShard::new(
                    i as u32,
                    collection,
                    &shard_name,
                    &config.edges,
                    storage.clone(),
                )
            })
            .collect();

        Self {
            collection: collection.to_string(),
            shards,
            num_shards,
            scope: config.scope.clone(),
        }
    }

    /// Initialize all shards (load persisted data).
    pub async fn initialize(&self) -> Result<()> {
        for shard in &self.shards {
            shard.initialize().await?;
        }
        Ok(())
    }

    /// Route a node ID to its shard index.
    fn shard_idx(&self, node_id: &str) -> usize {
        shard_for_doc(node_id, self.num_shards) as usize
    }

    /// Add a node — routed to the appropriate shard.
    pub async fn add_node(&self, node: GraphNode) -> Result<()> {
        let idx = self.shard_idx(&node.id);
        self.shards[idx].add_node(node).await
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: &str) -> Option<GraphNode> {
        let idx = self.shard_idx(id);
        self.shards[idx].get_node(id)
    }

    /// Remove a node and all its edges.
    pub async fn remove_node(&self, id: &str) -> Result<bool> {
        let idx = self.shard_idx(id);
        self.shards[idx].remove_node(id).await
    }

    /// Add an edge. When scope == Shard, both endpoints must hash to the same shard.
    pub async fn add_edge(&self, edge: GraphEdge) -> Result<()> {
        let from_idx = self.shard_idx(&edge.from);

        if self.scope == GraphScope::Shard {
            let to_idx = self.shard_idx(&edge.to);
            if from_idx != to_idx {
                return Err(Error::Schema(format!(
                    "Cross-shard edge not allowed (scope=shard): '{}' (shard {}) -> '{}' (shard {})",
                    edge.from, from_idx, edge.to, to_idx
                )));
            }
        }

        self.shards[from_idx].add_edge(edge).await
    }

    /// Remove an edge — routed by `from` node.
    pub async fn remove_edge(&self, from: &str, to: &str, edge_type: Option<&str>) -> Result<bool> {
        let idx = self.shard_idx(from);
        self.shards[idx].remove_edge(from, to, edge_type).await
    }

    /// Get all outgoing edges from a node.
    pub fn get_edges(&self, from: &str) -> Vec<GraphEdge> {
        let idx = self.shard_idx(from);
        self.shards[idx].get_edges(from)
    }

    /// Get edges of a specific type from a node.
    pub fn get_edges_by_type(&self, from: &str, edge_type: &str) -> Vec<GraphEdge> {
        let idx = self.shard_idx(from);
        self.shards[idx].get_edges_by_type(from, edge_type)
    }

    /// BFS traversal — entirely local within the start node's shard.
    pub fn bfs(&self, start: &str, edge_type: &str, max_depth: usize) -> Vec<String> {
        let idx = self.shard_idx(start);
        self.shards[idx].bfs(start, edge_type, max_depth)
    }

    /// Shortest path. Returns None if start and target are on different shards.
    pub fn shortest_path(
        &self,
        start: &str,
        target: &str,
        edge_types: Option<&[String]>,
    ) -> Option<Vec<String>> {
        let start_idx = self.shard_idx(start);
        let target_idx = self.shard_idx(target);

        if start_idx != target_idx {
            return None;
        }

        self.shards[start_idx].shortest_path(start, target, edge_types)
    }

    /// Aggregate stats across all shards.
    pub fn stats(&self) -> GraphStats {
        let mut total = GraphStats {
            node_count: 0,
            edge_count: 0,
        };
        for shard in &self.shards {
            let s = shard.stats();
            total.node_count += s.node_count;
            total.edge_count += s.edge_count;
        }
        total
    }

    /// List all nodes across all shards.
    pub fn list_nodes(&self) -> Vec<GraphNode> {
        let mut all = Vec::new();
        for shard in &self.shards {
            all.extend(shard.list_nodes());
        }
        all
    }

    /// List all edges across all shards.
    pub fn list_edges(&self) -> Vec<GraphEdge> {
        let mut all = Vec::new();
        for shard in &self.shards {
            all.extend(shard.list_edges());
        }
        all
    }

    /// Merge all shards into shard 0, clearing other shards.
    /// After merge, the graph behaves as a single shard.
    /// Returns (nodes_merged, edges_merged) counts.
    pub async fn merge_all_shards(&self) -> Result<(usize, usize)> {
        if self.num_shards <= 1 {
            let s = self.shards[0].stats();
            return Ok((s.node_count, s.edge_count));
        }

        // Collect data from shards 1..N
        let mut all_nodes = std::collections::HashMap::new();
        let mut all_edges: std::collections::HashMap<String, Vec<_>> = std::collections::HashMap::new();
        for shard in &self.shards[1..] {
            let (nodes, edges) = shard.export_raw();
            all_nodes.extend(nodes);
            for (from, entries) in edges {
                all_edges.entry(from).or_default().extend(entries);
            }
        }

        // Merge into shard 0
        self.shards[0].merge_from(all_nodes, all_edges).await?;

        // Clear shards 1..N
        for shard in &self.shards[1..] {
            shard.clear().await?;
        }

        let s = self.shards[0].stats();
        Ok((s.node_count, s.edge_count))
    }

    /// Collection name.
    pub fn collection(&self) -> &str {
        &self.collection
    }

    /// Number of shards.
    pub fn num_shards(&self) -> usize {
        self.num_shards
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::types::EdgeTypeConfig;

    fn test_config(num_shards: usize) -> GraphBackendConfig {
        GraphBackendConfig {
            path: String::new(),
            edges: vec![EdgeTypeConfig {
                edge_type: "related".to_string(),
                from_field: "id".to_string(),
                to_field: "related_id".to_string(),
            }],
            num_shards,
            scope: GraphScope::Shard,
        }
    }

    #[tokio::test]
    async fn test_single_shard_roundtrip() {
        let backend = ShardedGraphBackend::new("test", &test_config(1), None);
        backend.initialize().await.unwrap();

        backend
            .add_node(GraphNode {
                id: "n1".to_string(),
                node_type: "doc".to_string(),
                title: "Node 1".to_string(),
                payload: serde_json::Value::Null,
            })
            .await
            .unwrap();

        assert!(backend.get_node("n1").is_some());
        assert_eq!(backend.stats().node_count, 1);
    }

    #[tokio::test]
    async fn test_multi_shard_distribution() {
        let backend = ShardedGraphBackend::new("test", &test_config(4), None);
        backend.initialize().await.unwrap();

        // Add many nodes — they should distribute across shards
        for i in 0..20 {
            backend
                .add_node(GraphNode {
                    id: format!("node_{}", i),
                    node_type: "doc".to_string(),
                    title: format!("Node {}", i),
                    payload: serde_json::Value::Null,
                })
                .await
                .unwrap();
        }

        assert_eq!(backend.stats().node_count, 20);
        assert_eq!(backend.list_nodes().len(), 20);

        // Each node should be retrievable
        for i in 0..20 {
            assert!(backend.get_node(&format!("node_{}", i)).is_some());
        }
    }

    #[tokio::test]
    async fn test_cross_shard_edge_rejected() {
        let backend = ShardedGraphBackend::new("test", &test_config(4), None);
        backend.initialize().await.unwrap();

        // Find two node IDs that hash to different shards
        let mut shard_0_id = None;
        let mut shard_1_id = None;
        for i in 0..100 {
            let id = format!("node_{}", i);
            let idx = shard_for_doc(&id, 4);
            if idx == 0 && shard_0_id.is_none() {
                shard_0_id = Some(id);
            } else if idx == 1 && shard_1_id.is_none() {
                shard_1_id = Some(id);
            }
            if shard_0_id.is_some() && shard_1_id.is_some() {
                break;
            }
        }

        let from = shard_0_id.unwrap();
        let to = shard_1_id.unwrap();

        backend
            .add_node(GraphNode {
                id: from.clone(),
                node_type: "doc".to_string(),
                title: "A".to_string(),
                payload: serde_json::Value::Null,
            })
            .await
            .unwrap();
        backend
            .add_node(GraphNode {
                id: to.clone(),
                node_type: "doc".to_string(),
                title: "B".to_string(),
                payload: serde_json::Value::Null,
            })
            .await
            .unwrap();

        // Cross-shard edge should fail
        let result = backend
            .add_edge(GraphEdge {
                from,
                to,
                edge_type: "related".to_string(),
                weight: 1.0,
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_shortest_path_cross_shard_returns_none() {
        let backend = ShardedGraphBackend::new("test", &test_config(4), None);
        backend.initialize().await.unwrap();

        // Find two IDs on different shards
        let mut id_a = None;
        let mut id_b = None;
        for i in 0..100 {
            let id = format!("node_{}", i);
            let idx = shard_for_doc(&id, 4);
            if idx == 0 && id_a.is_none() {
                id_a = Some(id);
            } else if idx == 1 && id_b.is_none() {
                id_b = Some(id);
            }
            if id_a.is_some() && id_b.is_some() {
                break;
            }
        }

        let a = id_a.unwrap();
        let b = id_b.unwrap();

        // Shortest path between nodes on different shards => None
        assert!(backend.shortest_path(&a, &b, None).is_none());
    }
}
