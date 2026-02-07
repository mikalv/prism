//! Cluster configuration

use serde::{Deserialize, Serialize};
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
