//! Configuration management for engraph-core
//!
//! Default config location: ~/.engraph/config.toml

mod storage;

pub use storage::{
    CacheStorageConfig, CompressionStorageConfig, S3StorageConfig, UnifiedStorageConfig,
};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Main configuration
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    /// Unified storage configuration (S3, cached, etc.)
    /// When present, overrides the basic storage config for backends.
    #[serde(default)]
    pub unified_storage: Option<UnifiedStorageConfig>,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub observability: ObservabilityConfig,
    /// Cluster configuration for inter-node communication
    #[serde(default)]
    pub cluster: ClusterConfig,

    /// Index Lifecycle Management configuration
    #[serde(default)]
    pub ilm: crate::ilm::IlmConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind_addr")]
    pub bind_addr: String,
    pub unix_socket: Option<PathBuf>,
    #[serde(default)]
    pub cors: CorsConfig,
    #[serde(default)]
    pub tls: TlsConfig,
    /// Maximum request body size in bytes (default: 100MB)
    #[serde(default = "default_max_body_size")]
    pub max_body_size: usize,
}

fn default_bind_addr() -> String {
    "127.0.0.1:8080".to_string()
}

fn default_max_body_size() -> usize {
    100 * 1024 * 1024 // 100MB
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: default_bind_addr(),
            unix_socket: None,
            cors: CorsConfig::default(),
            tls: TlsConfig::default(),
            max_body_size: default_max_body_size(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CorsConfig {
    /// Enable CORS (default: true for development)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Allowed origins. Use "*" for any origin, or list specific origins.
    /// Default: common development ports
    #[serde(default = "default_cors_origins")]
    pub origins: Vec<String>,
}

fn default_cors_origins() -> Vec<String> {
    vec![
        "http://localhost:5173".to_string(),
        "http://localhost:3000".to_string(),
        "http://127.0.0.1:5173".to_string(),
        "http://127.0.0.1:3000".to_string(),
    ]
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            origins: default_cors_origins(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TlsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_tls_bind_addr")]
    pub bind_addr: String,
    #[serde(default = "default_tls_cert_path")]
    pub cert_path: PathBuf,
    #[serde(default = "default_tls_key_path")]
    pub key_path: PathBuf,
}

fn default_tls_bind_addr() -> String {
    "127.0.0.1:3443".to_string()
}

fn default_tls_cert_path() -> PathBuf {
    PathBuf::from("./conf/tls/cert.pem")
}

fn default_tls_key_path() -> PathBuf {
    PathBuf::from("./conf/tls/key.pem")
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: default_tls_bind_addr(),
            cert_path: default_tls_cert_path(),
            key_path: default_tls_key_path(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SecurityConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub api_keys: Vec<ApiKeyConfig>,
    #[serde(default)]
    pub roles: HashMap<String, RoleConfig>,
    #[serde(default)]
    pub audit: AuditConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiKeyConfig {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RoleConfig {
    #[serde(default)]
    pub collections: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuditConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub index_to_collection: bool,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            index_to_collection: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorageConfig {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default = "default_max_gb")]
    pub max_local_gb: f64,
}

fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".engraph")
}

fn default_max_gb() -> f64 {
    5.0
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            max_local_gb: default_max_gb(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbeddingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Provider configuration (ollama, openai, etc.)
    #[serde(default)]
    pub provider: crate::embedding::ProviderConfig,
    /// Cache directory for embeddings (optional)
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
}

fn default_true() -> bool {
    true
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            provider: crate::embedding::ProviderConfig::default(),
            cache_dir: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingConfig {
    #[serde(default = "default_level")]
    pub level: String,
    pub file: Option<PathBuf>,
}

fn default_level() -> String {
    "info".to_string()
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_level(),
            file: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObservabilityConfig {
    /// Log output format: "pretty" or "json"
    /// Override with LOG_FORMAT env var
    #[serde(default = "default_log_format")]
    pub log_format: String,

    /// Log level filter string
    /// Override with RUST_LOG env var
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Enable Prometheus metrics at GET /metrics
    #[serde(default = "default_true")]
    pub metrics_enabled: bool,
}

fn default_log_format() -> String {
    "pretty".to_string()
}

fn default_log_level() -> String {
    "info,prism=debug".to_string()
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            log_format: default_log_format(),
            log_level: default_log_level(),
            metrics_enabled: true,
        }
    }
}

/// Cluster configuration for inter-node RPC communication
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClusterConfig {
    /// Enable cluster mode
    #[serde(default)]
    pub enabled: bool,

    /// Unique identifier for this node
    #[serde(default = "default_node_id")]
    pub node_id: String,

    /// Address to bind the cluster RPC server
    #[serde(default = "default_cluster_bind_addr")]
    pub bind_addr: String,

    /// Address to advertise to other nodes (defaults to bind_addr)
    /// Set this to a reachable hostname:port when bind_addr is 0.0.0.0
    #[serde(default)]
    pub advertise_addr: Option<String>,

    /// Seed nodes for cluster discovery
    #[serde(default)]
    pub seed_nodes: Vec<String>,

    /// Connection timeout in milliseconds
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_ms: u64,

    /// Request timeout in milliseconds
    #[serde(default = "default_request_timeout")]
    pub request_timeout_ms: u64,

    /// TLS configuration for cluster communication
    #[serde(default)]
    pub tls: ClusterTlsConfig,
}

fn default_node_id() -> String {
    format!("node-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn default_cluster_bind_addr() -> String {
    "0.0.0.0:9080".to_string()
}

fn default_connect_timeout() -> u64 {
    5000
}

fn default_request_timeout() -> u64 {
    30000
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            node_id: default_node_id(),
            bind_addr: default_cluster_bind_addr(),
            advertise_addr: None,
            seed_nodes: Vec::new(),
            connect_timeout_ms: default_connect_timeout(),
            request_timeout_ms: default_request_timeout(),
            tls: ClusterTlsConfig::default(),
        }
    }
}

/// TLS configuration for cluster transport
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClusterTlsConfig {
    /// Enable TLS for cluster communication
    #[serde(default = "default_cluster_tls_enabled")]
    pub enabled: bool,

    /// Path to the certificate file (PEM format)
    #[serde(default = "default_cluster_cert_path")]
    pub cert_path: PathBuf,

    /// Path to the private key file (PEM format)
    #[serde(default = "default_cluster_key_path")]
    pub key_path: PathBuf,

    /// Path to CA certificate for verifying peer certificates
    pub ca_cert_path: Option<PathBuf>,

    /// Skip peer certificate verification (INSECURE - development only)
    #[serde(default)]
    pub skip_verify: bool,
}

fn default_cluster_tls_enabled() -> bool {
    true
}

fn default_cluster_cert_path() -> PathBuf {
    PathBuf::from("./conf/tls/cluster-cert.pem")
}

fn default_cluster_key_path() -> PathBuf {
    PathBuf::from("./conf/tls/cluster-key.pem")
}

impl Default for ClusterTlsConfig {
    fn default() -> Self {
        Self {
            enabled: default_cluster_tls_enabled(),
            cert_path: default_cluster_cert_path(),
            key_path: default_cluster_key_path(),
            ca_cert_path: None,
            skip_verify: false,
        }
    }
}

/// Expand ~ to home directory in path
pub fn expand_tilde(path: &Path) -> Result<PathBuf> {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("Cannot determine home directory"))?;
        Ok(home.join(rest))
    } else if s == "~" {
        dirs::home_dir().ok_or_else(|| anyhow!("Cannot determine home directory"))
    } else {
        Ok(path.to_path_buf())
    }
}

impl Config {
    /// Load config from default location (~/.engraph/config.toml)
    pub fn load() -> Result<Self> {
        let data_dir = default_data_dir();
        Self::load_from(&data_dir)
    }

    /// Load config from specific data directory
    pub fn load_from(data_dir: &Path) -> Result<Self> {
        let config_path = data_dir.join("config.toml");

        let mut config = if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            toml::from_str(&content)?
        } else {
            Config::default()
        };

        // Override data_dir to the one we loaded from
        config.storage.data_dir = data_dir.to_path_buf();
        config.expand_paths()?;
        Ok(config)
    }

    /// Load config from file path, or create default
    pub fn load_or_create(config_path: &Path) -> Result<Self> {
        if config_path.exists() {
            let content = fs::read_to_string(config_path)?;
            let mut config: Config = toml::from_str(&content)?;
            config.expand_paths()?;
            Ok(config)
        } else {
            let config = Config::default();
            // Try to save default config
            if let Some(parent) = config_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = config.save(config_path);
            Ok(config)
        }
    }

    /// Save config to file
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
        Ok(())
    }

    /// Expand ~ in all paths
    fn expand_paths(&mut self) -> Result<()> {
        self.storage.data_dir = expand_tilde(&self.storage.data_dir)?;
        if let Some(ref sock) = self.server.unix_socket {
            self.server.unix_socket = Some(expand_tilde(sock)?);
        }
        if self.server.tls.enabled {
            self.server.tls.cert_path = expand_tilde(&self.server.tls.cert_path)?;
            self.server.tls.key_path = expand_tilde(&self.server.tls.key_path)?;
        }
        if let Some(ref f) = self.logging.file {
            self.logging.file = Some(expand_tilde(f)?);
        }
        if let Some(ref mut unified) = self.unified_storage {
            unified.expand_paths().map_err(|e| anyhow!("{}", e))?;
        }
        // Expand cluster TLS paths
        if self.cluster.tls.enabled {
            self.cluster.tls.cert_path = expand_tilde(&self.cluster.tls.cert_path)?;
            self.cluster.tls.key_path = expand_tilde(&self.cluster.tls.key_path)?;
            if let Some(ref ca) = self.cluster.tls.ca_cert_path {
                self.cluster.tls.ca_cert_path = Some(expand_tilde(ca)?);
            }
        }
        Ok(())
    }

    /// Ensure all required directories exist
    pub fn ensure_dirs(&self) -> Result<()> {
        let base = &self.storage.data_dir;
        fs::create_dir_all(base.join("data/text"))?;
        fs::create_dir_all(base.join("data/vector"))?;
        fs::create_dir_all(base.join("schemas"))?;
        fs::create_dir_all(base.join("cache/models"))?;
        fs::create_dir_all(base.join("logs"))?;
        Ok(())
    }

    /// Get path to text index directory
    pub fn text_data_dir(&self) -> PathBuf {
        self.storage.data_dir.join("data/text")
    }

    /// Get path to vector index directory
    pub fn vector_data_dir(&self) -> PathBuf {
        self.storage.data_dir.join("data/vector")
    }

    /// Get path to schemas directory
    pub fn schemas_dir(&self) -> PathBuf {
        self.storage.data_dir.join("schemas")
    }

    /// Get path to model cache directory
    pub fn model_cache_dir(&self) -> PathBuf {
        self.storage.data_dir.join("cache/models")
    }

    /// Get path to logs directory
    pub fn logs_dir(&self) -> PathBuf {
        self.storage.data_dir.join("logs")
    }

    /// Create a SegmentStorage based on unified_storage config.
    /// Falls back to LocalStorage using storage.data_dir if unified_storage is not configured.
    #[cfg(feature = "storage-s3")]
    pub fn create_segment_storage(
        &self,
    ) -> Result<std::sync::Arc<dyn prism_storage::SegmentStorage>> {
        if let Some(ref unified) = self.unified_storage {
            unified.create_storage().map_err(|e| anyhow!("{}", e))
        } else {
            Ok(std::sync::Arc::new(prism_storage::LocalStorage::new(
                &self.storage.data_dir,
            )))
        }
    }

    /// Create a SegmentStorage based on unified_storage config (non-S3 version).
    #[cfg(not(feature = "storage-s3"))]
    pub fn create_segment_storage(
        &self,
    ) -> Result<std::sync::Arc<dyn prism_storage::SegmentStorage>> {
        if let Some(ref unified) = self.unified_storage {
            if unified.backend != "local" {
                return Err(anyhow!("S3/cached storage requires 'storage-s3' feature"));
            }
        }
        Ok(std::sync::Arc::new(prism_storage::LocalStorage::new(
            &self.storage.data_dir,
        )))
    }

    /// Check if unified storage is configured (S3, cached, etc.)
    pub fn has_unified_storage(&self) -> bool {
        self.unified_storage.is_some()
    }

    /// Check if remote storage is configured (S3 or cached)
    pub fn is_remote_storage(&self) -> bool {
        self.unified_storage.as_ref().is_some_and(|u| u.is_remote())
    }
}
