//! Prism Cluster - Inter-node communication for distributed Prism deployments
//!
//! This crate provides RPC-based communication between Prism nodes using
//! tarpc over QUIC transport with TLS encryption.
//!
//! # Architecture
//!
//! - **Service**: tarpc-based RPC service definition mirroring CollectionManager operations
//! - **Transport**: Quinn QUIC with TLS for secure, multiplexed connections
//! - **Server**: Wraps CollectionManager to serve cluster RPC requests
//! - **Client**: Connection-pooled client for calling remote nodes
//! - **Discovery**: Pluggable node discovery (static, DNS)
//! - **Placement**: Zone-aware shard placement with configurable strategies
//! - **Rebalancing**: Automatic and manual shard rebalancing
//!
//! # Key Operations
//!
//! - Core CRUD: index, search, get, delete, stats
//! - delete_by_query: Bulk deletion using search criteria
//! - import_by_query: Cross-cluster data migration
//! - Shard management: assign, transfer, get assignments
//! - Rebalancing: trigger rebalance, get status
//! - Discovery: static config, DNS-based node discovery

pub mod config;
pub mod discovery;
pub mod error;
pub mod health;
pub mod metrics;
pub mod placement;
pub mod rebalance;
pub mod service;
pub mod transport;
pub mod types;

mod client;
mod server;

pub use client::ClusterClient;
pub use config::{ClusterConfig, ClusterTlsConfig, FailureAction, HealthConfig, NodeTopology, RebalancingConfig};
pub use discovery::{ClusterEvent, DiscoveredNode, DiscoveryConfig, DnsDiscovery, NodeDiscovery, StaticDiscovery};
pub use error::ClusterError;
pub use health::{ClusterHealth, HealthChecker, HealthEvent, HealthState, NodeHealthInfo};
pub use placement::{
    BalanceFactor, ClusterState, ClusterStateSnapshot, NodeInfo, NodeState, PlacementDecision,
    PlacementError, PlacementStrategy, ReplicaRole, ShardAssignment, ShardState, SpreadLevel,
};
pub use rebalance::{
    OperationStatus, RebalanceEngine, RebalanceOperation, RebalanceOperationStatus,
    RebalancePhase, RebalancePlan, RebalanceStatus, RebalanceTrigger,
};
pub use server::ClusterServer;
pub use service::PrismClusterClient;
pub use types::*;
