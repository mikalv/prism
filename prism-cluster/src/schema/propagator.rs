//! Schema propagation across cluster nodes
//!
//! Handles distributing schema changes to all nodes with proper
//! ordering, retry logic, and consistency guarantees.

use super::registry::SchemaRegistry;
use super::version::VersionedSchema;
use super::{PropagationStrategy, SchemaOperationResult};
use crate::client::ClusterClient;
use crate::error::ClusterError;
use crate::metrics;
use crate::placement::ClusterState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

/// Configuration for schema propagation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropagationConfig {
    /// Timeout for propagating to a single node
    #[serde(default = "default_node_timeout")]
    pub node_timeout_ms: u64,
    /// Maximum concurrent propagation requests
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
    /// Number of retries for failed propagation
    #[serde(default = "default_max_retries")]
    pub max_retries: usize,
    /// Delay between retries
    #[serde(default = "default_retry_delay")]
    pub retry_delay_ms: u64,
    /// Require all nodes to acknowledge (strict mode)
    #[serde(default)]
    pub require_all_nodes: bool,
    /// Minimum nodes that must acknowledge for success
    #[serde(default = "default_min_ack")]
    pub min_acknowledgements: usize,
}

fn default_node_timeout() -> u64 {
    5000
}
fn default_max_concurrent() -> usize {
    10
}
fn default_max_retries() -> usize {
    3
}
fn default_retry_delay() -> u64 {
    1000
}
fn default_min_ack() -> usize {
    1
}

impl Default for PropagationConfig {
    fn default() -> Self {
        Self {
            node_timeout_ms: default_node_timeout(),
            max_concurrent: default_max_concurrent(),
            max_retries: default_max_retries(),
            retry_delay_ms: default_retry_delay(),
            require_all_nodes: false,
            min_acknowledgements: default_min_ack(),
        }
    }
}

/// Status of a propagation operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropagationStatus {
    /// Collection being propagated
    pub collection: String,
    /// Schema version being propagated
    pub version: u64,
    /// Nodes that successfully received the schema
    pub succeeded: Vec<String>,
    /// Nodes that failed to receive the schema
    pub failed: Vec<String>,
    /// Nodes still pending
    pub pending: Vec<String>,
    /// Whether propagation is complete
    pub complete: bool,
    /// Overall success
    pub success: bool,
    /// Started at (unix timestamp ms)
    pub started_at: u64,
    /// Completed at (unix timestamp ms)
    pub completed_at: Option<u64>,
    /// Error message if failed
    pub error: Option<String>,
}

impl PropagationStatus {
    fn new(collection: &str, version: u64, nodes: Vec<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            collection: collection.to_string(),
            version,
            succeeded: Vec::new(),
            failed: Vec::new(),
            pending: nodes,
            complete: false,
            success: false,
            started_at: now,
            completed_at: None,
            error: None,
        }
    }

    fn mark_complete(&mut self, success: bool, error: Option<String>) {
        self.complete = true;
        self.success = success;
        self.error = error;
        self.completed_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
    }
}

/// Schema propagator for distributing changes
pub struct SchemaPropagator {
    /// Cluster client for RPC
    client: Arc<ClusterClient>,
    /// Cluster state
    cluster_state: Arc<ClusterState>,
    /// Schema registry
    registry: Arc<SchemaRegistry>,
    /// Configuration
    config: PropagationConfig,
    /// This node's ID
    node_id: String,
    /// Event sender
    event_tx: broadcast::Sender<PropagationEvent>,
}

/// Events emitted during propagation
#[derive(Debug, Clone)]
pub enum PropagationEvent {
    /// Propagation started
    Started {
        collection: String,
        version: u64,
        target_nodes: Vec<String>,
    },
    /// Node acknowledged schema
    NodeAcknowledged {
        collection: String,
        version: u64,
        node_id: String,
    },
    /// Node failed to receive schema
    NodeFailed {
        collection: String,
        version: u64,
        node_id: String,
        error: String,
    },
    /// Propagation completed
    Completed {
        collection: String,
        version: u64,
        success: bool,
    },
}

impl SchemaPropagator {
    /// Create a new schema propagator
    pub fn new(
        client: Arc<ClusterClient>,
        cluster_state: Arc<ClusterState>,
        registry: Arc<SchemaRegistry>,
        config: PropagationConfig,
        node_id: impl Into<String>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(100);
        Self {
            client,
            cluster_state,
            registry,
            config,
            node_id: node_id.into(),
            event_tx,
        }
    }

    /// Subscribe to propagation events
    pub fn subscribe(&self) -> broadcast::Receiver<PropagationEvent> {
        self.event_tx.subscribe()
    }

    /// Propagate a schema to all cluster nodes
    pub async fn propagate(&self, versioned: &VersionedSchema) -> SchemaOperationResult {
        let start = Instant::now();
        let collection = &versioned.collection;
        let version = versioned.version.version();

        // Get target nodes (all nodes except self)
        let nodes = self.cluster_state.get_nodes();
        let target_nodes: Vec<_> = nodes
            .into_iter()
            .filter(|n| n.info.node_id != self.node_id)
            .collect();

        if target_nodes.is_empty() {
            debug!(
                collection = collection,
                version = version,
                "No other nodes to propagate to"
            );
            return SchemaOperationResult::success(version, vec![self.node_id.clone()]);
        }

        let target_node_ids: Vec<String> = target_nodes
            .iter()
            .map(|n| n.info.node_id.clone())
            .collect();

        info!(
            collection = collection,
            version = version,
            node_count = target_nodes.len(),
            "Starting schema propagation"
        );

        // Emit start event
        let _ = self.event_tx.send(PropagationEvent::Started {
            collection: collection.clone(),
            version,
            target_nodes: target_node_ids.clone(),
        });

        let mut status = PropagationStatus::new(collection, version, target_node_ids);

        // Propagate to each node with concurrency limit
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.max_concurrent));
        let mut handles = Vec::new();

        for node in target_nodes {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let client = self.client.clone();
            let versioned = versioned.clone();
            let config = self.config.clone();
            let event_tx = self.event_tx.clone();
            let collection = collection.clone();
            let node_id = node.info.node_id.clone();
            let node_address = node.info.address.clone();

            let handle = tokio::spawn(async move {
                let result =
                    propagate_to_node(&client, &node_id, &node_address, &versioned, &config).await;

                match &result {
                    Ok(_) => {
                        let _ = event_tx.send(PropagationEvent::NodeAcknowledged {
                            collection: collection.clone(),
                            version,
                            node_id: node_id.clone(),
                        });
                    }
                    Err(e) => {
                        let _ = event_tx.send(PropagationEvent::NodeFailed {
                            collection: collection.clone(),
                            version,
                            node_id: node_id.clone(),
                            error: e.to_string(),
                        });
                    }
                }

                drop(permit);
                (node_id, result)
            });

            handles.push(handle);
        }

        // Collect results
        let mut succeeded = vec![self.node_id.clone()]; // Include self
        let mut failed = Vec::new();

        for handle in handles {
            match handle.await {
                Ok((node_id, result)) => {
                    status.pending.retain(|n| n != &node_id);
                    match result {
                        Ok(_) => {
                            succeeded.push(node_id);
                        }
                        Err(e) => {
                            warn!(node = %node_id, error = %e, "Failed to propagate schema");
                            failed.push(node_id);
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "Propagation task panicked");
                }
            }
        }

        status.succeeded = succeeded.clone();
        status.failed = failed.clone();

        // Determine success
        let success = if self.config.require_all_nodes {
            failed.is_empty()
        } else {
            succeeded.len() >= self.config.min_acknowledgements
        };

        // Record metrics
        metrics::record_schema_propagation(
            collection,
            version,
            succeeded.len(),
            failed.len(),
            start.elapsed(),
        );

        let result = if success {
            if failed.is_empty() {
                status.mark_complete(true, None);
                SchemaOperationResult::success(version, succeeded)
            } else {
                status.mark_complete(true, None);
                SchemaOperationResult::partial(version, succeeded, failed)
            }
        } else {
            let error = format!(
                "Failed to propagate to required nodes. Succeeded: {}, Failed: {}",
                status.succeeded.len(),
                status.failed.len()
            );
            status.mark_complete(false, Some(error.clone()));
            SchemaOperationResult::failure(error)
        };

        // Emit completion event
        let _ = self.event_tx.send(PropagationEvent::Completed {
            collection: collection.clone(),
            version,
            success,
        });

        info!(
            collection = collection,
            version = version,
            succeeded = status.succeeded.len(),
            failed = status.failed.len(),
            duration_ms = start.elapsed().as_millis() as u64,
            "Schema propagation completed"
        );

        result
    }

    /// Propagate with specific strategy
    pub async fn propagate_with_strategy(
        &self,
        versioned: &VersionedSchema,
        strategy: PropagationStrategy,
    ) -> SchemaOperationResult {
        match strategy {
            PropagationStrategy::Immediate => {
                // Apply immediately to all nodes
                self.propagate(versioned).await
            }
            PropagationStrategy::Versioned => {
                // For versioned, we still propagate but nodes will migrate gracefully
                info!(
                    collection = %versioned.collection,
                    version = versioned.version.version(),
                    "Using versioned propagation strategy"
                );
                self.propagate(versioned).await
            }
            PropagationStrategy::Manual => {
                // Don't propagate, just register locally
                info!(
                    collection = %versioned.collection,
                    version = versioned.version.version(),
                    "Manual propagation - skipping automatic distribution"
                );
                SchemaOperationResult::success(
                    versioned.version.version(),
                    vec![self.node_id.clone()],
                )
            }
        }
    }

    /// Sync schemas with a specific node
    pub async fn sync_with_node(&self, node_id: &str, address: &str) -> Result<usize, ClusterError> {
        let snapshot = self.registry.snapshot().await;
        let mut synced = 0;

        for (collection, versioned) in snapshot.schemas {
            match propagate_to_node(
                &self.client,
                node_id,
                address,
                &versioned,
                &self.config,
            )
            .await
            {
                Ok(_) => {
                    synced += 1;
                    debug!(
                        collection = %collection,
                        node = node_id,
                        "Synced schema to node"
                    );
                }
                Err(e) => {
                    warn!(
                        collection = %collection,
                        node = node_id,
                        error = %e,
                        "Failed to sync schema to node"
                    );
                }
            }
        }

        Ok(synced)
    }

    /// Get nodes that are missing a schema version
    pub async fn get_outdated_nodes(
        &self,
        collection: &str,
        version: u64,
    ) -> Vec<String> {
        // This would query each node for their schema version
        // For now, we track this via propagation status
        // In a full implementation, we'd maintain a version map
        Vec::new()
    }
}

/// Propagate schema to a single node with retries
async fn propagate_to_node(
    client: &ClusterClient,
    node_id: &str,
    address: &str,
    versioned: &VersionedSchema,
    config: &PropagationConfig,
) -> Result<(), ClusterError> {
    let mut last_error = None;
    let timeout = Duration::from_millis(config.node_timeout_ms);
    let retry_delay = Duration::from_millis(config.retry_delay_ms);

    for attempt in 0..=config.max_retries {
        if attempt > 0 {
            debug!(
                node = node_id,
                attempt = attempt,
                "Retrying schema propagation"
            );
            tokio::time::sleep(retry_delay).await;
        }

        let start = Instant::now();
        match tokio::time::timeout(
            timeout,
            client.apply_schema(address, versioned.clone()),
        )
        .await
        {
            Ok(Ok(_)) => {
                metrics::record_schema_node_propagation(
                    &versioned.collection,
                    node_id,
                    true,
                    start.elapsed(),
                );
                return Ok(());
            }
            Ok(Err(e)) => {
                metrics::record_schema_node_propagation(
                    &versioned.collection,
                    node_id,
                    false,
                    start.elapsed(),
                );
                last_error = Some(e);
            }
            Err(_) => {
                metrics::record_schema_node_propagation(
                    &versioned.collection,
                    node_id,
                    false,
                    start.elapsed(),
                );
                last_error = Some(ClusterError::Timeout(format!(
                    "Timeout propagating schema to {}",
                    node_id
                )));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| ClusterError::Internal("Unknown error".into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_propagation_config_default() {
        let config = PropagationConfig::default();
        assert_eq!(config.node_timeout_ms, 5000);
        assert_eq!(config.max_concurrent, 10);
        assert_eq!(config.max_retries, 3);
        assert!(!config.require_all_nodes);
    }

    #[test]
    fn test_propagation_status() {
        let mut status = PropagationStatus::new(
            "products",
            1,
            vec!["node-1".into(), "node-2".into()],
        );

        assert!(!status.complete);
        assert_eq!(status.pending.len(), 2);

        status.succeeded.push("node-1".into());
        status.pending.retain(|n| n != "node-1");

        status.mark_complete(true, None);

        assert!(status.complete);
        assert!(status.success);
        assert!(status.completed_at.is_some());
    }
}
