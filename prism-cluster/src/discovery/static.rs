//! Static node discovery from configuration
//!
//! Simple discovery backend that uses a fixed list of node addresses
//! from configuration. Best for development and small fixed deployments.

use super::{ClusterEvent, DiscoveredNode, NodeDiscovery};
use crate::error::ClusterError;
use async_trait::async_trait;
use parking_lot::RwLock;
use std::net::{SocketAddr, ToSocketAddrs};
use tokio::sync::broadcast;
use tracing::{debug, warn};

/// Static discovery using a configured list of nodes
pub struct StaticDiscovery {
    /// Resolved node addresses
    nodes: RwLock<Vec<DiscoveredNode>>,

    /// Original hostnames (for re-resolution)
    hostnames: Vec<String>,

    /// Event broadcaster
    event_tx: broadcast::Sender<ClusterEvent>,
}

impl StaticDiscovery {
    /// Create a new static discovery from a list of addresses
    ///
    /// Addresses can be in "host:port" format. If port is omitted, 9080 is used.
    pub fn new(addresses: Vec<String>) -> Result<Self, ClusterError> {
        let (event_tx, _) = broadcast::channel(64);

        let mut nodes = Vec::new();
        let mut hostnames = Vec::new();

        for addr_str in addresses {
            // Add default port if not specified
            let addr_with_port = if addr_str.contains(':') {
                addr_str.clone()
            } else {
                format!("{}:9080", addr_str)
            };

            hostnames.push(addr_with_port.clone());

            // Try to resolve the address
            match addr_with_port.to_socket_addrs() {
                Ok(mut addrs) => {
                    if let Some(addr) = addrs.next() {
                        debug!(address = %addr, hostname = %addr_with_port, "Resolved node address");
                        nodes.push(DiscoveredNode::new(addr));
                    }
                }
                Err(e) => {
                    warn!(
                        hostname = %addr_with_port,
                        error = %e,
                        "Failed to resolve node address, will retry on refresh"
                    );
                }
            }
        }

        Ok(Self {
            nodes: RwLock::new(nodes),
            hostnames,
            event_tx,
        })
    }

    /// Add a node dynamically
    pub fn add_node(&self, address: SocketAddr) {
        let node = DiscoveredNode::new(address);
        let mut nodes = self.nodes.write();

        // Check for duplicates
        if nodes.iter().any(|n| n.address == address) {
            return;
        }

        nodes.push(node.clone());
        let _ = self.event_tx.send(ClusterEvent::NodeJoined(node));
    }

    /// Remove a node dynamically
    pub fn remove_node(&self, address: SocketAddr) {
        let mut nodes = self.nodes.write();
        if let Some(pos) = nodes.iter().position(|n| n.address == address) {
            nodes.remove(pos);
            let _ = self.event_tx.send(ClusterEvent::NodeLeft(address));
        }
    }
}

#[async_trait]
impl NodeDiscovery for StaticDiscovery {
    async fn get_nodes(&self) -> Result<Vec<DiscoveredNode>, ClusterError> {
        Ok(self.nodes.read().clone())
    }

    fn subscribe(&self) -> broadcast::Receiver<ClusterEvent> {
        self.event_tx.subscribe()
    }

    async fn refresh(&self) -> Result<(), ClusterError> {
        let mut new_nodes = Vec::new();
        let old_nodes: Vec<SocketAddr> = self.nodes.read().iter().map(|n| n.address).collect();

        for hostname in &self.hostnames {
            match hostname.to_socket_addrs() {
                Ok(mut addrs) => {
                    if let Some(addr) = addrs.next() {
                        new_nodes.push(DiscoveredNode::new(addr));
                    }
                }
                Err(e) => {
                    warn!(
                        hostname = %hostname,
                        error = %e,
                        "Failed to resolve node address during refresh"
                    );
                }
            }
        }

        // Find new nodes
        for node in &new_nodes {
            if !old_nodes.contains(&node.address) {
                let _ = self.event_tx.send(ClusterEvent::NodeJoined(node.clone()));
            }
        }

        // Find removed nodes
        let new_addrs: Vec<SocketAddr> = new_nodes.iter().map(|n| n.address).collect();
        for addr in &old_nodes {
            if !new_addrs.contains(addr) {
                let _ = self.event_tx.send(ClusterEvent::NodeLeft(*addr));
            }
        }

        *self.nodes.write() = new_nodes;

        let node_count = self.nodes.read().len();
        let _ = self
            .event_tx
            .send(ClusterEvent::RefreshComplete { node_count });

        Ok(())
    }

    async fn start(&self) -> Result<(), ClusterError> {
        // Static discovery doesn't need background tasks
        // but we can do an initial refresh
        self.refresh().await
    }

    async fn stop(&self) -> Result<(), ClusterError> {
        // Nothing to stop for static discovery
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "static"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_static_discovery_creation() {
        let discovery =
            StaticDiscovery::new(vec!["127.0.0.1:9080".into(), "127.0.0.1:9081".into()]).unwrap();

        let nodes = discovery.get_nodes().await.unwrap();
        assert_eq!(nodes.len(), 2);
    }

    #[tokio::test]
    async fn test_static_discovery_default_port() {
        let discovery = StaticDiscovery::new(vec!["127.0.0.1".into()]).unwrap();

        let nodes = discovery.get_nodes().await.unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].address.port(), 9080);
    }

    #[tokio::test]
    async fn test_add_remove_node() {
        let discovery = StaticDiscovery::new(vec!["127.0.0.1:9080".into()]).unwrap();

        let addr: SocketAddr = "127.0.0.1:9081".parse().unwrap();
        discovery.add_node(addr);

        let nodes = discovery.get_nodes().await.unwrap();
        assert_eq!(nodes.len(), 2);

        discovery.remove_node(addr);

        let nodes = discovery.get_nodes().await.unwrap();
        assert_eq!(nodes.len(), 1);
    }

    #[tokio::test]
    async fn test_subscribe_events() {
        let discovery = StaticDiscovery::new(vec!["127.0.0.1:9080".into()]).unwrap();
        let mut rx = discovery.subscribe();

        let addr: SocketAddr = "127.0.0.1:9081".parse().unwrap();
        discovery.add_node(addr);

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, ClusterEvent::NodeJoined(_)));
    }

    #[test]
    fn test_backend_name() {
        let discovery = StaticDiscovery::new(vec![]).unwrap();
        assert_eq!(discovery.backend_name(), "static");
    }
}
