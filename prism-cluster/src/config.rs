//! Cluster configuration

use crate::discovery::DiscoveryConfig;
use crate::federation::FederationConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Main cluster configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClusterConfig {
    /// Enable cluster mode
    #[serde(default)]
    pub enabled: bool,

    /// Unique identifier for this node
    #[serde(default = "default_node_id")]
    pub node_id: String,

    /// Address to bind the cluster RPC server
    #[serde(default = "default_bind_addr")]
    pub bind_addr: String,

    /// Seed nodes for cluster discovery
    #[serde(default)]
    pub seed_nodes: Vec<String>,

    /// Connection timeout in milliseconds
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_ms: u64,

    /// Request timeout in milliseconds
    #[serde(default = "default_request_timeout")]
    pub request_timeout_ms: u64,

    /// Maximum concurrent connections per node
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,

    /// TLS configuration for cluster communication
    #[serde(default)]
    pub tls: ClusterTlsConfig,

    /// Node topology for zone-aware placement
    #[serde(default)]
    pub topology: NodeTopology,

    /// Rebalancing configuration
    #[serde(default)]
    pub rebalancing: RebalancingConfig,

    /// Health check configuration
    #[serde(default)]
    pub health: HealthConfig,

    /// Node discovery configuration
    #[serde(default)]
    pub discovery: DiscoveryConfig,

    /// Consistency and partition handling configuration
    #[serde(default)]
    pub consistency: ConsistencyConfig,

    /// Federation (distributed query) configuration
    #[serde(default)]
    pub federation: FederationConfig,
}

fn default_node_id() -> String {
    format!("node-{}", uuid::Uuid::new_v4().to_string()[..8].to_string())
}

fn default_bind_addr() -> String {
    "0.0.0.0:9080".to_string()
}

fn default_connect_timeout() -> u64 {
    5000
}

fn default_request_timeout() -> u64 {
    30000
}

fn default_max_connections() -> usize {
    10
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            node_id: default_node_id(),
            bind_addr: default_bind_addr(),
            seed_nodes: Vec::new(),
            connect_timeout_ms: default_connect_timeout(),
            request_timeout_ms: default_request_timeout(),
            max_connections: default_max_connections(),
            tls: ClusterTlsConfig::default(),
            topology: NodeTopology::default(),
            rebalancing: RebalancingConfig::default(),
            health: HealthConfig::default(),
            discovery: DiscoveryConfig::default(),
            consistency: ConsistencyConfig::default(),
            federation: FederationConfig::default(),
        }
    }
}

/// TLS configuration for cluster transport
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClusterTlsConfig {
    /// Enable TLS for cluster communication (strongly recommended)
    #[serde(default = "default_tls_enabled")]
    pub enabled: bool,

    /// Path to the certificate file (PEM format)
    #[serde(default = "default_cert_path")]
    pub cert_path: PathBuf,

    /// Path to the private key file (PEM format)
    #[serde(default = "default_key_path")]
    pub key_path: PathBuf,

    /// Path to CA certificate for verifying peer certificates
    pub ca_cert_path: Option<PathBuf>,

    /// Skip peer certificate verification (INSECURE - for development only)
    #[serde(default)]
    pub skip_verify: bool,
}

fn default_tls_enabled() -> bool {
    true
}

fn default_cert_path() -> PathBuf {
    PathBuf::from("./conf/tls/cluster-cert.pem")
}

fn default_key_path() -> PathBuf {
    PathBuf::from("./conf/tls/cluster-key.pem")
}

impl Default for ClusterTlsConfig {
    fn default() -> Self {
        Self {
            enabled: default_tls_enabled(),
            cert_path: default_cert_path(),
            key_path: default_key_path(),
            ca_cert_path: None,
            skip_verify: false,
        }
    }
}

impl ClusterConfig {
    /// Parse bind address into socket address
    pub fn parse_bind_addr(&self) -> Result<std::net::SocketAddr, std::net::AddrParseError> {
        self.bind_addr.parse()
    }

    /// Get connection timeout as Duration
    pub fn connect_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.connect_timeout_ms)
    }

    /// Get request timeout as Duration
    pub fn request_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.request_timeout_ms)
    }
}

/// Node topology for zone-aware shard placement
///
/// Topology information enables the placement algorithm to spread replicas
/// across failure domains (zones, racks, regions) for high availability.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NodeTopology {
    /// Availability zone (e.g., "us-east-1a", "eu-west-1b")
    #[serde(default)]
    pub zone: String,

    /// Rack within the zone (optional, for finer-grained placement)
    #[serde(default)]
    pub rack: Option<String>,

    /// Region containing multiple zones (e.g., "us-east-1", "eu-west-1")
    #[serde(default)]
    pub region: Option<String>,

    /// Custom attributes for placement decisions
    /// Examples: disk_type=ssd, storage_gb=500, memory_gb=64
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

impl Default for NodeTopology {
    fn default() -> Self {
        Self {
            zone: "default".to_string(),
            rack: None,
            region: None,
            attributes: HashMap::new(),
        }
    }
}

impl NodeTopology {
    /// Check if this node matches the required attributes
    pub fn matches_attributes(&self, required: &HashMap<String, String>) -> bool {
        required.iter().all(|(k, v)| {
            self.attributes.get(k).map(|av| av == v).unwrap_or(false)
        })
    }

    /// Get storage capacity in GB from attributes (if set)
    pub fn storage_gb(&self) -> Option<u64> {
        self.attributes.get("storage_gb").and_then(|v| v.parse().ok())
    }

    /// Get disk type from attributes (if set)
    pub fn disk_type(&self) -> Option<&str> {
        self.attributes.get("disk_type").map(|s| s.as_str())
    }
}

/// Configuration for automatic shard rebalancing
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RebalancingConfig {
    /// Enable automatic rebalancing
    #[serde(default = "default_rebalancing_enabled")]
    pub enabled: bool,

    /// Imbalance threshold as percentage before triggering rebalance
    /// For example, 15 means rebalance when any node has 15% more shards than average
    #[serde(default = "default_imbalance_threshold")]
    pub imbalance_threshold_percent: u8,

    /// Maximum number of concurrent shard moves
    #[serde(default = "default_max_concurrent_moves")]
    pub max_concurrent_moves: usize,

    /// Maximum bytes per second for shard transfers (bandwidth throttling)
    #[serde(default = "default_max_bytes_per_sec")]
    pub max_bytes_per_sec: u64,

    /// Minimum time between rebalance operations (in seconds)
    #[serde(default = "default_rebalance_cooldown")]
    pub cooldown_secs: u64,
}

fn default_rebalancing_enabled() -> bool {
    false
}

fn default_imbalance_threshold() -> u8 {
    15
}

fn default_max_concurrent_moves() -> usize {
    2
}

fn default_max_bytes_per_sec() -> u64 {
    104_857_600 // 100 MB/s
}

fn default_rebalance_cooldown() -> u64 {
    300 // 5 minutes
}

impl Default for RebalancingConfig {
    fn default() -> Self {
        Self {
            enabled: default_rebalancing_enabled(),
            imbalance_threshold_percent: default_imbalance_threshold(),
            max_concurrent_moves: default_max_concurrent_moves(),
            max_bytes_per_sec: default_max_bytes_per_sec(),
            cooldown_secs: default_rebalance_cooldown(),
        }
    }
}

/// Action to take when a node fails
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureAction {
    /// Trigger shard rebalancing
    Rebalance,
    /// Only emit alerts/metrics
    AlertOnly,
    /// Require manual intervention
    Manual,
}

impl Default for FailureAction {
    fn default() -> Self {
        FailureAction::Rebalance
    }
}

/// Health check configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HealthConfig {
    /// Heartbeat interval in milliseconds
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_ms: u64,

    /// Number of missed heartbeats before marking node as suspect
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,

    /// Time in milliseconds a node can stay in suspect state before being marked dead
    #[serde(default = "default_suspect_timeout")]
    pub suspect_timeout_ms: u64,

    /// Action to take when a node is confirmed dead
    #[serde(default)]
    pub on_failure: FailureAction,
}

fn default_heartbeat_interval() -> u64 {
    1000 // 1 second
}

fn default_failure_threshold() -> u32 {
    3 // 3 missed heartbeats
}

fn default_suspect_timeout() -> u64 {
    5000 // 5 seconds
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval_ms: default_heartbeat_interval(),
            failure_threshold: default_failure_threshold(),
            suspect_timeout_ms: default_suspect_timeout(),
            on_failure: FailureAction::default(),
        }
    }
}

/// Minimum nodes required for write operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WriteQuorum {
    /// Majority of nodes (n/2 + 1)
    Quorum,
    /// All nodes must be available
    All,
    /// Specific number of nodes
    Count(usize),
    /// Single node (no replication requirement)
    One,
}

impl Default for WriteQuorum {
    fn default() -> Self {
        WriteQuorum::Quorum
    }
}

impl WriteQuorum {
    /// Check if quorum is satisfied given alive and total node counts
    pub fn is_satisfied(&self, alive_count: usize, total_count: usize) -> bool {
        match self {
            WriteQuorum::Quorum => alive_count > total_count / 2,
            WriteQuorum::All => alive_count == total_count,
            WriteQuorum::Count(n) => alive_count >= *n,
            WriteQuorum::One => alive_count >= 1,
        }
    }

    /// Get minimum nodes required for this quorum level
    pub fn min_nodes(&self, total_count: usize) -> usize {
        match self {
            WriteQuorum::Quorum => total_count / 2 + 1,
            WriteQuorum::All => total_count,
            WriteQuorum::Count(n) => *n,
            WriteQuorum::One => 1,
        }
    }
}

/// Behavior when the cluster is partitioned
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartitionBehavior {
    /// Accept reads but reject writes (safe default)
    ReadOnly,
    /// Reject all requests
    RejectAll,
    /// Continue serving requests with potentially stale data
    ServeStale,
}

impl Default for PartitionBehavior {
    fn default() -> Self {
        PartitionBehavior::ReadOnly
    }
}

/// Consistency and partition handling configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConsistencyConfig {
    /// Minimum nodes required for write operations
    #[serde(default)]
    pub min_nodes_for_write: WriteQuorum,

    /// Behavior when the cluster is partitioned (no quorum)
    #[serde(default)]
    pub partition_behavior: PartitionBehavior,

    /// Allow reading stale data during partition
    #[serde(default = "default_allow_stale_reads")]
    pub allow_stale_reads: bool,

    /// Maximum age of stale data to serve (in seconds)
    #[serde(default = "default_stale_read_max_age")]
    pub stale_read_max_age_secs: u64,

    /// Enable automatic partition healing
    #[serde(default = "default_auto_healing")]
    pub auto_healing: bool,

    /// Conflict resolution strategy
    #[serde(default)]
    pub conflict_resolution: ConflictResolution,
}

fn default_allow_stale_reads() -> bool {
    true
}

fn default_stale_read_max_age() -> u64 {
    30 // 30 seconds
}

fn default_auto_healing() -> bool {
    true
}

impl Default for ConsistencyConfig {
    fn default() -> Self {
        Self {
            min_nodes_for_write: WriteQuorum::default(),
            partition_behavior: PartitionBehavior::default(),
            allow_stale_reads: default_allow_stale_reads(),
            stale_read_max_age_secs: default_stale_read_max_age(),
            auto_healing: default_auto_healing(),
            conflict_resolution: ConflictResolution::default(),
        }
    }
}

/// Strategy for resolving conflicting writes during partition healing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictResolution {
    /// Use the write with the latest timestamp (simple but may lose data)
    LastWriteWins,
    /// Flag conflicts for manual resolution
    Manual,
    /// Merge conflicting values (application-specific)
    Merge,
}

impl Default for ConflictResolution {
    fn default() -> Self {
        ConflictResolution::LastWriteWins
    }
}
