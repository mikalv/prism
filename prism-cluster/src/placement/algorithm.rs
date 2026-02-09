//! Zone-aware shard placement algorithm
//!
//! Implements a two-phase placement algorithm:
//! 1. Filter nodes by hard constraints (zone spread, resource availability)
//! 2. Score remaining nodes by soft constraints (load balance, preferences)

use super::{
    BalanceFactor, NodeInfo, PlacementDecision, PlacementStrategy, ShardAssignment, SpreadLevel,
};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

/// Errors that can occur during placement
#[derive(Error, Debug, Clone)]
pub enum PlacementError {
    #[error("Not enough nodes available: need {needed}, have {available}")]
    InsufficientNodes { needed: usize, available: usize },

    #[error("Cannot satisfy zone spread constraint: need {needed} zones, have {available}")]
    InsufficientZones { needed: usize, available: usize },

    #[error("Cannot satisfy rack spread constraint: need {needed} racks, have {available}")]
    InsufficientRacks { needed: usize, available: usize },

    #[error("No healthy nodes available")]
    NoHealthyNodes,

    #[error("Node {0} not found")]
    NodeNotFound(String),

    #[error("Required attribute not satisfied: {0}={1}")]
    RequiredAttributeMissing(String, String),
}

/// Place replicas for a shard across available nodes
///
/// # Arguments
/// * `shard_id` - Identifier for the shard being placed
/// * `replication_factor` - Total number of copies (including primary)
/// * `nodes` - Available nodes in the cluster
/// * `existing_assignments` - Current shard assignments (for load balancing)
/// * `strategy` - Placement strategy to use
///
/// # Returns
/// A `PlacementDecision` containing the selected nodes
pub fn place_replicas(
    shard_id: &str,
    replication_factor: usize,
    nodes: &[NodeInfo],
    existing_assignments: &[ShardAssignment],
    strategy: &PlacementStrategy,
) -> Result<PlacementDecision, PlacementError> {
    if replication_factor == 0 {
        return Err(PlacementError::InsufficientNodes {
            needed: 1,
            available: nodes.len(),
        });
    }

    // Phase 1: Filter by hard constraints
    let eligible_nodes = filter_by_hard_constraints(nodes, strategy)?;

    if eligible_nodes.len() < replication_factor {
        return Err(PlacementError::InsufficientNodes {
            needed: replication_factor,
            available: eligible_nodes.len(),
        });
    }

    // Check spread constraints
    validate_spread_constraint(&eligible_nodes, replication_factor, strategy)?;

    // Phase 2: Score and select nodes
    let selected = select_nodes_with_spread(
        &eligible_nodes,
        replication_factor,
        existing_assignments,
        strategy,
    )?;

    let (primary, replicas) = selected.split_first().unwrap();

    Ok(PlacementDecision {
        shard_id: shard_id.to_string(),
        primary_node: primary.to_string(),
        replica_nodes: replicas.iter().map(|s| s.to_string()).collect(),
        score: 1.0,
        reason: format!(
            "Placed with {:?} spread across {} nodes",
            strategy.spread_across, replication_factor
        ),
    })
}

/// Filter nodes by hard constraints
fn filter_by_hard_constraints<'a>(
    nodes: &'a [NodeInfo],
    strategy: &PlacementStrategy,
) -> Result<Vec<&'a NodeInfo>, PlacementError> {
    let eligible: Vec<&NodeInfo> = nodes
        .iter()
        .filter(|node| {
            // Must be healthy
            if !node.healthy {
                return false;
            }

            // Must have required attributes
            if !strategy.required_attributes.is_empty() {
                for (key, value) in &strategy.required_attributes {
                    if !node
                        .topology
                        .attributes
                        .get(key)
                        .map(|v| v == value)
                        .unwrap_or(false)
                    {
                        return false;
                    }
                }
            }

            true
        })
        .collect();

    if eligible.is_empty() {
        return Err(PlacementError::NoHealthyNodes);
    }

    Ok(eligible)
}

/// Validate that spread constraints can be satisfied
fn validate_spread_constraint(
    nodes: &[&NodeInfo],
    replication_factor: usize,
    strategy: &PlacementStrategy,
) -> Result<(), PlacementError> {
    match strategy.spread_across {
        SpreadLevel::Zone => {
            let zones: HashSet<_> = nodes.iter().map(|n| &n.topology.zone).collect();
            if zones.len() < replication_factor {
                return Err(PlacementError::InsufficientZones {
                    needed: replication_factor,
                    available: zones.len(),
                });
            }
        }
        SpreadLevel::Rack => {
            let racks: HashSet<_> = nodes
                .iter()
                .filter_map(|n| n.topology.rack.as_ref())
                .collect();
            // If no rack info, fall back to zone
            if !racks.is_empty() && racks.len() < replication_factor {
                return Err(PlacementError::InsufficientRacks {
                    needed: replication_factor,
                    available: racks.len(),
                });
            }
        }
        SpreadLevel::Region => {
            let regions: HashSet<_> = nodes
                .iter()
                .filter_map(|n| n.topology.region.as_ref())
                .collect();
            if !regions.is_empty() && regions.len() < replication_factor {
                return Err(PlacementError::InsufficientZones {
                    needed: replication_factor,
                    available: regions.len(),
                });
            }
        }
        SpreadLevel::None => {}
    }

    Ok(())
}

/// Select nodes respecting spread constraints and optimizing for balance
fn select_nodes_with_spread(
    nodes: &[&NodeInfo],
    replication_factor: usize,
    existing_assignments: &[ShardAssignment],
    strategy: &PlacementStrategy,
) -> Result<Vec<String>, PlacementError> {
    // Build a map of domain -> nodes for spread constraint
    let domain_nodes = group_nodes_by_domain(nodes, strategy.spread_across);

    // Score all nodes
    let mut node_scores: Vec<(&NodeInfo, f64)> = nodes
        .iter()
        .map(|node| {
            let score = score_node(node, existing_assignments, strategy);
            (*node, score)
        })
        .collect();

    // Sort by score (highest first)
    node_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Select nodes ensuring spread constraint
    let mut selected: Vec<String> = Vec::new();
    let mut used_domains: HashSet<String> = HashSet::new();

    // First pass: one node per domain
    if strategy.spread_across != SpreadLevel::None {
        for (node, _score) in &node_scores {
            let domain = get_node_domain(node, strategy.spread_across);
            if !used_domains.contains(&domain) {
                selected.push(node.node_id.clone());
                used_domains.insert(domain);
                if selected.len() >= replication_factor {
                    break;
                }
            }
        }
    }

    // Second pass: fill remaining slots with best available
    if selected.len() < replication_factor {
        for (node, _score) in &node_scores {
            if !selected.contains(&node.node_id) {
                selected.push(node.node_id.clone());
                if selected.len() >= replication_factor {
                    break;
                }
            }
        }
    }

    if selected.len() < replication_factor {
        return Err(PlacementError::InsufficientNodes {
            needed: replication_factor,
            available: selected.len(),
        });
    }

    Ok(selected)
}

/// Group nodes by their domain (zone, rack, region)
fn group_nodes_by_domain<'a>(
    nodes: &[&'a NodeInfo],
    spread_level: SpreadLevel,
) -> HashMap<String, Vec<&'a NodeInfo>> {
    let mut groups: HashMap<String, Vec<&NodeInfo>> = HashMap::new();

    for node in nodes {
        let domain = get_node_domain(node, spread_level);
        groups.entry(domain).or_default().push(*node);
    }

    groups
}

/// Get the domain identifier for a node based on spread level
fn get_node_domain(node: &NodeInfo, spread_level: SpreadLevel) -> String {
    match spread_level {
        SpreadLevel::Zone => node.topology.zone.clone(),
        SpreadLevel::Rack => node
            .topology
            .rack
            .clone()
            .unwrap_or_else(|| node.topology.zone.clone()),
        SpreadLevel::Region => node
            .topology
            .region
            .clone()
            .unwrap_or_else(|| node.topology.zone.clone()),
        SpreadLevel::None => "default".to_string(),
    }
}

/// Score a node for placement (higher is better)
pub fn score_node(
    node: &NodeInfo,
    existing_assignments: &[ShardAssignment],
    strategy: &PlacementStrategy,
) -> f64 {
    let mut score = 100.0;

    for factor in &strategy.balance_by {
        match factor {
            BalanceFactor::ShardCount => {
                // Lower shard count is better
                let shard_count = count_shards_on_node(&node.node_id, existing_assignments);
                score -= shard_count as f64 * 5.0;
            }
            BalanceFactor::DiskUsage => {
                // Lower disk usage is better
                let usage_percent = node.disk_usage_percent();
                score -= usage_percent * 0.5;
            }
            BalanceFactor::IndexSize => {
                // Lower index size is better (for balance)
                let size_gb = node.index_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                score -= size_gb * 2.0;
            }
            BalanceFactor::PreferSsd => {
                // Bonus for SSD
                if node.has_ssd() {
                    score += 20.0;
                }
            }
        }
    }

    // Bonus for preferred attributes
    for (key, value) in &strategy.preferred_attributes {
        if node.topology.attributes.get(key) == Some(value) {
            score += 10.0;
        }
    }

    score.max(0.0)
}

/// Count shards assigned to a node
fn count_shards_on_node(node_id: &str, assignments: &[ShardAssignment]) -> usize {
    assignments.iter().filter(|a| a.is_on_node(node_id)).count()
}

/// Find the best node to transfer a shard to for rebalancing
pub fn find_rebalance_target(
    shard: &ShardAssignment,
    nodes: &[NodeInfo],
    existing_assignments: &[ShardAssignment],
    strategy: &PlacementStrategy,
) -> Result<String, PlacementError> {
    // Filter out nodes that already have this shard
    let candidates: Vec<&NodeInfo> = nodes
        .iter()
        .filter(|n| n.healthy && !shard.is_on_node(&n.node_id))
        .collect();

    if candidates.is_empty() {
        return Err(PlacementError::NoHealthyNodes);
    }

    // Check spread constraint for candidates
    let current_domains: HashSet<String> = shard
        .all_nodes()
        .iter()
        .filter_map(|node_id| nodes.iter().find(|n| n.node_id == *node_id))
        .map(|n| get_node_domain(n, strategy.spread_across))
        .collect();

    // Filter candidates that would maintain spread constraint
    let valid_candidates: Vec<&NodeInfo> = if strategy.spread_across != SpreadLevel::None {
        candidates
            .iter()
            .filter(|n| {
                let domain = get_node_domain(n, strategy.spread_across);
                !current_domains.contains(&domain)
            })
            .copied()
            .collect()
    } else {
        candidates.clone()
    };

    // Score and select best candidate
    let final_candidates: &[&NodeInfo] = if valid_candidates.is_empty() {
        &candidates
    } else {
        &valid_candidates
    };

    let best = final_candidates
        .iter()
        .map(|n| (n, score_node(n, existing_assignments, strategy)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(n, _)| n.node_id.clone())
        .ok_or(PlacementError::NoHealthyNodes)?;

    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NodeTopology;

    fn make_node(id: &str, zone: &str, shard_count: usize) -> NodeInfo {
        NodeInfo {
            node_id: id.to_string(),
            address: format!("{}:9080", id),
            topology: NodeTopology {
                zone: zone.to_string(),
                rack: None,
                region: None,
                attributes: HashMap::new(),
            },
            healthy: true,
            shard_count,
            disk_used_bytes: 0,
            disk_total_bytes: 100_000_000_000,
            index_size_bytes: 0,
        }
    }

    #[test]
    fn test_place_replicas_single() {
        let nodes = vec![
            make_node("node-1", "zone-a", 0),
            make_node("node-2", "zone-b", 0),
            make_node("node-3", "zone-c", 0),
        ];

        let strategy = PlacementStrategy::default();
        let result = place_replicas("shard-1", 1, &nodes, &[], &strategy);

        assert!(result.is_ok());
        let decision = result.unwrap();
        assert!(["node-1", "node-2", "node-3"].contains(&decision.primary_node.as_str()));
        assert!(decision.replica_nodes.is_empty());
    }

    #[test]
    fn test_place_replicas_zone_spread() {
        let nodes = vec![
            make_node("node-1", "zone-a", 0),
            make_node("node-2", "zone-b", 0),
            make_node("node-3", "zone-c", 0),
        ];

        let strategy = PlacementStrategy {
            spread_across: SpreadLevel::Zone,
            ..Default::default()
        };

        let result = place_replicas("shard-1", 3, &nodes, &[], &strategy);
        assert!(result.is_ok());

        let decision = result.unwrap();
        let all_nodes = std::iter::once(&decision.primary_node)
            .chain(&decision.replica_nodes)
            .collect::<Vec<_>>();

        // Verify all nodes are different
        let unique: HashSet<_> = all_nodes.iter().collect();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn test_place_replicas_insufficient_zones() {
        let nodes = vec![
            make_node("node-1", "zone-a", 0),
            make_node("node-2", "zone-a", 0), // Same zone
            make_node("node-3", "zone-b", 0),
        ];

        let strategy = PlacementStrategy {
            spread_across: SpreadLevel::Zone,
            ..Default::default()
        };

        let result = place_replicas("shard-1", 3, &nodes, &[], &strategy);
        assert!(matches!(
            result,
            Err(PlacementError::InsufficientZones { .. })
        ));
    }

    #[test]
    fn test_place_replicas_balance() {
        let nodes = vec![
            make_node("node-1", "zone-a", 5), // Has many shards
            make_node("node-2", "zone-b", 0), // Empty
            make_node("node-3", "zone-c", 2), // Some shards
        ];

        // Create existing assignments for the balance check
        let existing: Vec<ShardAssignment> = (0..5)
            .map(|i| {
                let mut a = ShardAssignment::new("test", i, "node-1");
                a
            })
            .chain((0..2).map(|i| ShardAssignment::new("test", i + 5, "node-3")))
            .collect();

        let strategy = PlacementStrategy {
            spread_across: SpreadLevel::Zone,
            balance_by: vec![BalanceFactor::ShardCount],
            ..Default::default()
        };

        let result = place_replicas("shard-1", 1, &nodes, &existing, &strategy);
        assert!(result.is_ok());

        // Should prefer node-2 (least loaded)
        let decision = result.unwrap();
        assert_eq!(decision.primary_node, "node-2");
    }

    #[test]
    fn test_score_node() {
        let node = make_node("node-1", "zone-a", 0);
        let strategy = PlacementStrategy::default();

        let score = score_node(&node, &[], &strategy);
        assert!(score > 0.0);
    }

    #[test]
    fn test_score_node_with_shards() {
        let node = make_node("node-1", "zone-a", 5);
        let assignments: Vec<ShardAssignment> = (0..5)
            .map(|i| ShardAssignment::new("test", i, "node-1"))
            .collect();

        let strategy = PlacementStrategy {
            balance_by: vec![BalanceFactor::ShardCount],
            ..Default::default()
        };

        let score_with_shards = score_node(&node, &assignments, &strategy);
        let empty_node = make_node("node-2", "zone-b", 0);
        let score_empty = score_node(&empty_node, &[], &strategy);

        // Empty node should have higher score
        assert!(score_empty > score_with_shards);
    }
}
