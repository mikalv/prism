//! DNS-based node discovery
//!
//! Discovers nodes using DNS SRV and A/AAAA records.
//! Ideal for Kubernetes headless services and other DNS-based service discovery.
//!
//! # Kubernetes Example
//!
//! For a Kubernetes headless service named `prism` in namespace `default`:
//! ```yaml
//! apiVersion: v1
//! kind: Service
//! metadata:
//!   name: prism-headless
//! spec:
//!   clusterIP: None  # Headless service
//!   selector:
//!     app: prism
//!   ports:
//!     - port: 9080
//!       name: cluster
//! ```
//!
//! Configure discovery with:
//! ```toml
//! [cluster.discovery]
//! backend = "dns"
//! name = "prism-headless.default.svc.cluster.local"
//! ```

use super::{ClusterEvent, DiscoveredNode, NodeDiscovery};
use crate::error::ClusterError;
use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashSet;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// DNS-based node discovery
pub struct DnsDiscovery {
    /// DNS name to query
    dns_name: String,

    /// Refresh interval
    refresh_interval: Duration,

    /// Optional DNS server
    #[allow(dead_code)]
    dns_server: Option<String>,

    /// Default port if not in SRV record
    default_port: u16,

    /// Currently known nodes
    nodes: Arc<RwLock<Vec<DiscoveredNode>>>,

    /// Event broadcaster
    event_tx: broadcast::Sender<ClusterEvent>,

    /// Whether background refresh is running
    running: Arc<AtomicBool>,

    /// Background task handle
    task_handle: RwLock<Option<JoinHandle<()>>>,
}

impl DnsDiscovery {
    /// Create a new DNS discovery
    pub fn new(
        dns_name: String,
        refresh_interval_secs: u64,
        dns_server: Option<String>,
        default_port: u16,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(64);

        Self {
            dns_name,
            refresh_interval: Duration::from_secs(refresh_interval_secs),
            dns_server,
            default_port,
            nodes: Arc::new(RwLock::new(Vec::new())),
            event_tx,
            running: Arc::new(AtomicBool::new(false)),
            task_handle: RwLock::new(None),
        }
    }

    /// Resolve DNS name to addresses
    async fn resolve(&self) -> Result<Vec<DiscoveredNode>, ClusterError> {
        // First try SRV record lookup
        match self.resolve_srv().await {
            Ok(nodes) if !nodes.is_empty() => {
                debug!(
                    dns_name = %self.dns_name,
                    count = nodes.len(),
                    "Resolved SRV records"
                );
                return Ok(nodes);
            }
            Ok(_) => {
                debug!(dns_name = %self.dns_name, "No SRV records found, trying A/AAAA");
            }
            Err(e) => {
                debug!(
                    dns_name = %self.dns_name,
                    error = %e,
                    "SRV lookup failed, trying A/AAAA"
                );
            }
        }

        // Fall back to A/AAAA records
        self.resolve_a_aaaa().await
    }

    /// Resolve SRV records
    async fn resolve_srv(&self) -> Result<Vec<DiscoveredNode>, ClusterError> {
        // Use hickory-resolver for proper async DNS
        // For now, we'll use a simpler approach with the standard library
        // In production, you'd want to use hickory-dns for full SRV support

        // SRV record format: _service._proto.name
        // For Kubernetes headless services, the format is different:
        // servicename.namespace.svc.cluster.local

        // Since standard library doesn't support SRV, we fall back to A records
        // A proper implementation would use hickory-dns here
        Err(ClusterError::Discovery(
            "SRV lookup not implemented - falling back to A/AAAA".into(),
        ))
    }

    /// Resolve A/AAAA records
    async fn resolve_a_aaaa(&self) -> Result<Vec<DiscoveredNode>, ClusterError> {
        let dns_name = self.dns_name.clone();
        let default_port = self.default_port;

        // Perform DNS lookup in blocking task
        let result = tokio::task::spawn_blocking(move || {
            // Format: hostname:port or just hostname
            let lookup_addr = if dns_name.contains(':') {
                dns_name.clone()
            } else {
                format!("{}:{}", dns_name, default_port)
            };

            lookup_addr
                .to_socket_addrs()
                .map(|addrs| addrs.map(DiscoveredNode::new).collect::<Vec<_>>())
        })
        .await
        .map_err(|e| ClusterError::Discovery(format!("DNS task failed: {}", e)))?;

        result.map_err(|e| ClusterError::Discovery(format!("DNS resolution failed: {}", e)))
    }

    /// Compare current and new node lists, emit events
    fn emit_changes(&self, new_nodes: &[DiscoveredNode]) {
        let old_nodes = self.nodes.read();
        let old_addrs: HashSet<SocketAddr> = old_nodes.iter().map(|n| n.address).collect();
        let new_addrs: HashSet<SocketAddr> = new_nodes.iter().map(|n| n.address).collect();

        // Nodes that joined
        for node in new_nodes {
            if !old_addrs.contains(&node.address) {
                debug!(address = %node.address, "Node joined via DNS discovery");
                let _ = self.event_tx.send(ClusterEvent::NodeJoined(node.clone()));
            }
        }

        // Nodes that left
        for addr in &old_addrs {
            if !new_addrs.contains(addr) {
                debug!(address = %addr, "Node left (no longer in DNS)");
                let _ = self.event_tx.send(ClusterEvent::NodeLeft(*addr));
            }
        }
    }
}

#[async_trait]
impl NodeDiscovery for DnsDiscovery {
    async fn get_nodes(&self) -> Result<Vec<DiscoveredNode>, ClusterError> {
        Ok(self.nodes.read().clone())
    }

    fn subscribe(&self) -> broadcast::Receiver<ClusterEvent> {
        self.event_tx.subscribe()
    }

    async fn refresh(&self) -> Result<(), ClusterError> {
        match self.resolve().await {
            Ok(new_nodes) => {
                // Emit change events
                self.emit_changes(&new_nodes);

                // Update nodes
                *self.nodes.write() = new_nodes;

                let node_count = self.nodes.read().len();
                let _ = self
                    .event_tx
                    .send(ClusterEvent::RefreshComplete { node_count });

                Ok(())
            }
            Err(e) => {
                warn!(
                    dns_name = %self.dns_name,
                    error = %e,
                    "DNS refresh failed"
                );
                Err(e)
            }
        }
    }

    async fn start(&self) -> Result<(), ClusterError> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(()); // Already running
        }

        // Do initial refresh
        if let Err(e) = self.refresh().await {
            warn!(error = %e, "Initial DNS refresh failed");
        }

        // Start background refresh task
        let nodes = self.nodes.clone();
        let running = self.running.clone();
        let event_tx = self.event_tx.clone();
        let dns_name = self.dns_name.clone();
        let default_port = self.default_port;
        let refresh_interval = self.refresh_interval;

        let handle = tokio::spawn(async move {
            info!(
                dns_name = %dns_name,
                interval_secs = refresh_interval.as_secs(),
                "Starting DNS discovery background refresh"
            );

            let mut interval = tokio::time::interval(refresh_interval);
            interval.tick().await; // Skip immediate tick

            while running.load(Ordering::SeqCst) {
                interval.tick().await;

                if !running.load(Ordering::SeqCst) {
                    break;
                }

                // Perform DNS lookup
                let lookup_addr = format!("{}:{}", dns_name, default_port);

                match tokio::task::spawn_blocking(move || {
                    lookup_addr
                        .to_socket_addrs()
                        .map(|addrs| addrs.collect::<Vec<_>>())
                })
                .await
                {
                    Ok(Ok(addrs)) => {
                        let new_nodes: Vec<DiscoveredNode> =
                            addrs.into_iter().map(DiscoveredNode::new).collect();

                        // Calculate changes
                        let old_addrs: HashSet<SocketAddr> =
                            nodes.read().iter().map(|n| n.address).collect();
                        let new_addrs: HashSet<SocketAddr> =
                            new_nodes.iter().map(|n| n.address).collect();

                        // Emit events
                        for node in &new_nodes {
                            if !old_addrs.contains(&node.address) {
                                let _ = event_tx.send(ClusterEvent::NodeJoined(node.clone()));
                            }
                        }
                        for addr in &old_addrs {
                            if !new_addrs.contains(addr) {
                                let _ = event_tx.send(ClusterEvent::NodeLeft(*addr));
                            }
                        }

                        // Update state
                        *nodes.write() = new_nodes;

                        let node_count = nodes.read().len();
                        let _ = event_tx.send(ClusterEvent::RefreshComplete { node_count });
                    }
                    Ok(Err(e)) => {
                        warn!(error = %e, "DNS refresh failed");
                    }
                    Err(e) => {
                        error!(error = %e, "DNS task panicked");
                    }
                }
            }

            info!("DNS discovery background refresh stopped");
        });

        *self.task_handle.write() = Some(handle);

        Ok(())
    }

    async fn stop(&self) -> Result<(), ClusterError> {
        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.task_handle.write().take() {
            handle.abort();
        }

        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "dns"
    }
}

impl Drop for DnsDiscovery {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.task_handle.write().take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dns_discovery_localhost() {
        let discovery = DnsDiscovery::new("localhost".into(), 30, None, 9080);

        // Start discovery
        discovery.start().await.unwrap();

        // Should resolve localhost
        let _nodes = discovery.get_nodes().await.unwrap();
        // Note: might be empty if DNS resolution is blocked in test env
        // but shouldn't error

        discovery.stop().await.unwrap();
    }

    #[test]
    fn test_backend_name() {
        let discovery = DnsDiscovery::new("test.local".into(), 30, None, 9080);
        assert_eq!(discovery.backend_name(), "dns");
    }

    #[tokio::test]
    async fn test_subscribe() {
        let discovery = DnsDiscovery::new("localhost".into(), 30, None, 9080);
        let _rx = discovery.subscribe();
        // Just verify we can subscribe
    }
}
