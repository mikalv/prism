//! Node discovery module for cluster formation
//!
//! Provides pluggable discovery mechanisms for finding other nodes in the cluster.
//!
//! # Discovery Backends
//!
//! - **Static**: Manual list of node addresses in configuration
//! - **DNS**: SRV record-based discovery (ideal for Kubernetes)
//!
//! # Example
//!
//! ```ignore
//! use prism_cluster::discovery::{DiscoveryConfig, NodeDiscovery};
//!
//! let config = DiscoveryConfig::static_nodes(vec!["node1:9080", "node2:9080"]);
//! let discovery = config.create_discovery()?;
//!
//! // Get all known nodes
//! let nodes = discovery.get_nodes().await?;
//!
//! // Watch for cluster changes
//! let mut events = discovery.watch();
//! while let Some(event) = events.recv().await {
//!     match event {
//!         ClusterEvent::NodeJoined(info) => println!("Node joined: {}", info.node_id),
//!         ClusterEvent::NodeLeft(id) => println!("Node left: {}", id),
//!         ClusterEvent::NodeUpdated(info) => println!("Node updated: {}", info.node_id),
//!     }
//! }
//! ```

mod dns;
mod r#static;

pub use dns::DnsDiscovery;
pub use r#static::StaticDiscovery;

use crate::error::ClusterError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::sync::broadcast;

/// Information about a discovered node
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredNode {
    /// Node address (host:port)
    pub address: SocketAddr,

    /// Optional node ID (may be unknown until connection)
    pub node_id: Option<String>,

    /// Zone hint from DNS TXT records or SRV metadata
    pub zone: Option<String>,

    /// Priority from SRV record (lower = higher priority)
    pub priority: u16,

    /// Weight from SRV record
    pub weight: u16,
}

impl DiscoveredNode {
    /// Create a new discovered node with just an address
    pub fn new(address: SocketAddr) -> Self {
        Self {
            address,
            node_id: None,
            zone: None,
            priority: 0,
            weight: 0,
        }
    }

    /// Create with SRV record metadata
    pub fn with_srv_metadata(address: SocketAddr, priority: u16, weight: u16) -> Self {
        Self {
            address,
            node_id: None,
            zone: None,
            priority,
            weight,
        }
    }
}

/// Events emitted by the discovery service
#[derive(Debug, Clone)]
pub enum ClusterEvent {
    /// A new node was discovered
    NodeJoined(DiscoveredNode),

    /// A node left the cluster (address)
    NodeLeft(SocketAddr),

    /// A node's information was updated
    NodeUpdated(DiscoveredNode),

    /// Discovery refresh completed
    RefreshComplete { node_count: usize },
}

/// Trait for node discovery implementations
#[async_trait]
pub trait NodeDiscovery: Send + Sync {
    /// Get all currently known nodes
    async fn get_nodes(&self) -> Result<Vec<DiscoveredNode>, ClusterError>;

    /// Subscribe to cluster events
    fn subscribe(&self) -> broadcast::Receiver<ClusterEvent>;

    /// Force a refresh of the node list
    async fn refresh(&self) -> Result<(), ClusterError>;

    /// Start background discovery (if applicable)
    async fn start(&self) -> Result<(), ClusterError>;

    /// Stop background discovery
    async fn stop(&self) -> Result<(), ClusterError>;

    /// Get the discovery backend name
    fn backend_name(&self) -> &'static str;
}

/// Discovery configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "lowercase")]
pub enum DiscoveryConfig {
    /// Static list of nodes
    Static {
        /// List of node addresses (host:port)
        nodes: Vec<String>,
    },

    /// DNS-based discovery
    Dns {
        /// DNS name to query (e.g., _prism._tcp.cluster.local)
        name: String,

        /// Refresh interval in seconds
        #[serde(default = "default_refresh_interval")]
        refresh_interval_secs: u64,

        /// DNS server to query (optional, uses system default)
        server: Option<String>,

        /// Port to use if not specified in SRV record
        #[serde(default = "default_port")]
        default_port: u16,
    },
}

fn default_refresh_interval() -> u64 {
    30
}

fn default_port() -> u16 {
    9080
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        DiscoveryConfig::Static { nodes: Vec::new() }
    }
}

impl DiscoveryConfig {
    /// Create static discovery configuration
    pub fn static_nodes<S: Into<String>>(nodes: Vec<S>) -> Self {
        DiscoveryConfig::Static {
            nodes: nodes.into_iter().map(|s| s.into()).collect(),
        }
    }

    /// Create DNS discovery configuration
    pub fn dns(name: impl Into<String>) -> Self {
        DiscoveryConfig::Dns {
            name: name.into(),
            refresh_interval_secs: default_refresh_interval(),
            server: None,
            default_port: default_port(),
        }
    }

    /// Create DNS discovery with custom refresh interval
    pub fn dns_with_interval(name: impl Into<String>, interval_secs: u64) -> Self {
        DiscoveryConfig::Dns {
            name: name.into(),
            refresh_interval_secs: interval_secs,
            server: None,
            default_port: default_port(),
        }
    }

    /// Create the appropriate discovery implementation
    pub fn create_discovery(&self) -> Result<Box<dyn NodeDiscovery>, ClusterError> {
        match self {
            DiscoveryConfig::Static { nodes } => {
                Ok(Box::new(StaticDiscovery::new(nodes.clone())?))
            }
            DiscoveryConfig::Dns {
                name,
                refresh_interval_secs,
                server,
                default_port,
            } => Ok(Box::new(DnsDiscovery::new(
                name.clone(),
                *refresh_interval_secs,
                server.clone(),
                *default_port,
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_config() {
        let config = DiscoveryConfig::static_nodes(vec!["node1:9080", "node2:9080"]);
        assert!(matches!(config, DiscoveryConfig::Static { nodes } if nodes.len() == 2));
    }

    #[test]
    fn test_dns_config() {
        let config = DiscoveryConfig::dns("prism.cluster.local");
        assert!(matches!(config, DiscoveryConfig::Dns { name, .. } if name == "prism.cluster.local"));
    }

    #[test]
    fn test_discovered_node() {
        let addr: SocketAddr = "127.0.0.1:9080".parse().unwrap();
        let node = DiscoveredNode::new(addr);
        assert_eq!(node.address, addr);
        assert!(node.node_id.is_none());
    }

    #[test]
    fn test_config_serde() {
        let config = DiscoveryConfig::Static {
            nodes: vec!["localhost:9080".into()],
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("static"));

        let config = DiscoveryConfig::Dns {
            name: "prism.local".into(),
            refresh_interval_secs: 60,
            server: None,
            default_port: 9080,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("dns"));
    }
}
