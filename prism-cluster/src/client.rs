//! Cluster RPC client with QUIC connection pooling
//!
//! Pools QUIC connections to remote nodes. Each RPC call opens a fresh
//! bidirectional stream on the pooled connection, creates a one-shot
//! tarpc client, executes the call, and tears down the stream.
//! This matches QUIC's design: connections are expensive (TLS handshake),
//! streams are cheap (single frame to open).

use crate::config::ClusterConfig;
use crate::error::{ClusterError, Result};
use crate::metrics::{
    record_connection_established, record_connection_failed, record_connection_pool_size, RpcTimer,
};
use crate::service::PrismClusterClient;
use crate::transport::make_client_endpoint;
use crate::types::*;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use tarpc::client::Config as TarpcConfig;
use tarpc::context;
use tracing::{debug, info, warn};

/// Pooled QUIC connection (long-lived, multiplexed)
struct PooledConnection {
    connection: quinn::Connection,
    #[allow(dead_code)]
    created_at: std::time::Instant,
}

/// Cluster RPC client with QUIC connection pooling
pub struct ClusterClient {
    config: ClusterConfig,
    endpoint: quinn::Endpoint,
    connections: Arc<RwLock<HashMap<SocketAddr, PooledConnection>>>,
}

impl ClusterClient {
    /// Create a new cluster client
    pub async fn new(config: ClusterConfig) -> Result<Self> {
        let endpoint = make_client_endpoint(&config).await?;

        Ok(Self {
            config,
            endpoint,
            connections: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get a live pooled QUIC connection, or create a new one
    async fn get_connection(
        &self,
        addr: SocketAddr,
        server_name: &str,
    ) -> Result<quinn::Connection> {
        // Check pool for a live connection
        {
            let connections = self.connections.read();
            if let Some(pooled) = connections.get(&addr) {
                if pooled.connection.close_reason().is_none() {
                    return Ok(pooled.connection.clone());
                }
            }
        }

        // Create new QUIC connection (TLS handshake)
        let connection = self.create_quic_connection(addr, server_name).await?;

        // Store in pool
        {
            let mut connections = self.connections.write();
            connections.insert(
                addr,
                PooledConnection {
                    connection: connection.clone(),
                    created_at: std::time::Instant::now(),
                },
            );
            record_connection_pool_size(connections.len());
        }

        Ok(connection)
    }

    /// Establish a new QUIC connection (TLS handshake)
    async fn create_quic_connection(
        &self,
        addr: SocketAddr,
        server_name: &str,
    ) -> Result<quinn::Connection> {
        let addr_str = addr.to_string();
        debug!(
            "Connecting to cluster node at {} (SNI: {})",
            addr, server_name
        );

        let connecting = self.endpoint.connect(addr, server_name).map_err(|e| {
            record_connection_failed(&addr_str, "connect_error");
            ClusterError::Connection(format!("Failed to connect to {}: {}", addr, e))
        })?;

        let connection = tokio::time::timeout(self.config.connect_timeout(), connecting)
            .await
            .map_err(|_| {
                record_connection_failed(&addr_str, "timeout");
                ClusterError::Timeout(format!("Connection to {} timed out", addr))
            })?
            .map_err(|e| {
                record_connection_failed(&addr_str, "handshake_error");
                ClusterError::Connection(format!(
                    "Connection handshake failed with {}: {}",
                    addr, e
                ))
            })?;

        info!("QUIC connection established to {}", addr);
        record_connection_established(&addr_str);
        Ok(connection)
    }

    /// Open a fresh bidirectional stream and create a one-shot tarpc client.
    /// Retries once with a new connection if the pooled one is stale.
    async fn new_rpc_client(
        &self,
        addr: SocketAddr,
        server_name: &str,
    ) -> Result<PrismClusterClient> {
        let connection = self.get_connection(addr, server_name).await?;

        match connection.open_bi().await {
            Ok((send, recv)) => {
                return Self::make_tarpc_client(send, recv);
            }
            Err(e) => {
                warn!(
                    "Stream open failed on pooled connection to {}: {}, reconnecting",
                    addr, e
                );
                self.evict_connection(addr);
            }
        }

        // Retry with a fresh connection
        let connection = self.get_connection(addr, server_name).await?;
        let (send, recv) = connection.open_bi().await.map_err(|e| {
            self.evict_connection(addr);
            ClusterError::Transport(format!("Failed to open stream to {}: {}", addr, e))
        })?;

        Self::make_tarpc_client(send, recv)
    }

    /// Build a tarpc client from raw QUIC send/recv streams
    fn make_tarpc_client(
        send: quinn::SendStream,
        recv: quinn::RecvStream,
    ) -> Result<PrismClusterClient> {
        let transport = tarpc::serde_transport::new(
            tokio_util::codec::Framed::new(
                QuicBiStream { send, recv },
                tarpc::tokio_util::codec::LengthDelimitedCodec::new(),
            ),
            tarpc::tokio_serde::formats::Json::default(),
        );

        Ok(PrismClusterClient::new(TarpcConfig::default(), transport).spawn())
    }

    /// Evict a dead connection from the pool
    fn evict_connection(&self, addr: SocketAddr) {
        let mut connections = self.connections.write();
        connections.remove(&addr);
        record_connection_pool_size(connections.len());
    }

    /// Create a context with the configured request timeout
    fn context(&self) -> context::Context {
        let mut ctx = context::current();
        ctx.deadline = std::time::Instant::now() + self.config.request_timeout();
        ctx
    }

    /// Resolve address string to SocketAddr and extract hostname for TLS SNI
    async fn resolve_addr(addr: &str) -> Result<(SocketAddr, String)> {
        // Extract hostname for SNI (everything before the last ':port')
        let server_name = addr
            .rsplit_once(':')
            .map(|(host, _)| host.to_string())
            .unwrap_or_else(|| addr.to_string());

        // Try direct parse first (e.g. "127.0.0.1:9080")
        if let Ok(sa) = addr.parse::<SocketAddr>() {
            return Ok((sa, server_name));
        }
        // DNS resolution for hostname:port (e.g. "prism-node1:9080")
        let mut addrs = tokio::net::lookup_host(addr).await.map_err(|e| {
            ClusterError::Config(format!("DNS resolution failed for '{}': {}", addr, e))
        })?;
        let socket_addr = addrs
            .next()
            .ok_or_else(|| ClusterError::Config(format!("No addresses resolved for '{}'", addr)))?;
        Ok((socket_addr, server_name))
    }

    // ========================================
    // Public API
    // ========================================

    /// Index documents on a remote node
    pub async fn index(&self, addr: &str, collection: &str, docs: Vec<RpcDocument>) -> Result<()> {
        let timer = RpcTimer::new("index", addr);
        let (sock_addr, server_name) = Self::resolve_addr(addr).await?;
        let client = self.new_rpc_client(sock_addr, &server_name).await?;
        match client
            .index(self.context(), collection.to_string(), docs)
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
        {
            Ok(()) => {
                timer.success();
                Ok(())
            }
            Err(e) => {
                timer.error(e.error_type());
                Err(e)
            }
        }
    }

    /// Search on a remote node
    pub async fn search(
        &self,
        addr: &str,
        collection: &str,
        query: RpcQuery,
    ) -> Result<RpcSearchResults> {
        let timer = RpcTimer::new("search", addr);
        let (sock_addr, server_name) = Self::resolve_addr(addr).await?;
        let client = self.new_rpc_client(sock_addr, &server_name).await?;
        match client
            .search(self.context(), collection.to_string(), query)
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
        {
            Ok(results) => {
                timer.success();
                Ok(results)
            }
            Err(e) => {
                timer.error(e.error_type());
                Err(e)
            }
        }
    }

    /// Get document from a remote node
    pub async fn get(&self, addr: &str, collection: &str, id: &str) -> Result<Option<RpcDocument>> {
        let timer = RpcTimer::new("get", addr);
        let (sock_addr, server_name) = Self::resolve_addr(addr).await?;
        let client = self.new_rpc_client(sock_addr, &server_name).await?;
        match client
            .get(self.context(), collection.to_string(), id.to_string())
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
        {
            Ok(doc) => {
                timer.success();
                Ok(doc)
            }
            Err(e) => {
                timer.error(e.error_type());
                Err(e)
            }
        }
    }

    /// Delete documents from a remote node
    pub async fn delete(&self, addr: &str, collection: &str, ids: Vec<String>) -> Result<()> {
        let timer = RpcTimer::new("delete", addr);
        let (sock_addr, server_name) = Self::resolve_addr(addr).await?;
        let client = self.new_rpc_client(sock_addr, &server_name).await?;
        match client
            .delete(self.context(), collection.to_string(), ids)
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
        {
            Ok(()) => {
                timer.success();
                Ok(())
            }
            Err(e) => {
                timer.error(e.error_type());
                Err(e)
            }
        }
    }

    /// Get stats from a remote node
    pub async fn stats(&self, addr: &str, collection: &str) -> Result<RpcBackendStats> {
        let timer = RpcTimer::new("stats", addr);
        let (sock_addr, server_name) = Self::resolve_addr(addr).await?;
        let client = self.new_rpc_client(sock_addr, &server_name).await?;
        match client
            .stats(self.context(), collection.to_string())
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
        {
            Ok(stats) => {
                timer.success();
                Ok(stats)
            }
            Err(e) => {
                timer.error(e.error_type());
                Err(e)
            }
        }
    }

    /// List collections on a remote node
    pub async fn list_collections(&self, addr: &str) -> Result<Vec<String>> {
        let timer = RpcTimer::new("list_collections", addr);
        let (sock_addr, server_name) = Self::resolve_addr(addr).await?;
        let client = self.new_rpc_client(sock_addr, &server_name).await?;
        let result = client
            .list_collections(self.context())
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?;
        timer.success();
        Ok(result)
    }

    /// Delete documents by query on a remote node
    pub async fn delete_by_query(
        &self,
        addr: &str,
        request: DeleteByQueryRequest,
    ) -> Result<DeleteByQueryResponse> {
        let timer = RpcTimer::new("delete_by_query", addr);
        let (sock_addr, server_name) = Self::resolve_addr(addr).await?;
        let client = self.new_rpc_client(sock_addr, &server_name).await?;
        match client
            .delete_by_query(self.context(), request)
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
        {
            Ok(response) => {
                timer.success();
                Ok(response)
            }
            Err(e) => {
                timer.error(e.error_type());
                Err(e)
            }
        }
    }

    /// Import documents by query on a remote node
    pub async fn import_by_query(
        &self,
        addr: &str,
        request: ImportByQueryRequest,
    ) -> Result<ImportByQueryResponse> {
        let timer = RpcTimer::new("import_by_query", addr);
        let (sock_addr, server_name) = Self::resolve_addr(addr).await?;
        let client = self.new_rpc_client(sock_addr, &server_name).await?;
        match client
            .import_by_query(self.context(), request)
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
        {
            Ok(response) => {
                timer.success();
                Ok(response)
            }
            Err(e) => {
                timer.error(e.error_type());
                Err(e)
            }
        }
    }

    /// Get node info from a remote node
    pub async fn node_info(&self, addr: &str) -> Result<NodeInfo> {
        let timer = RpcTimer::new("node_info", addr);
        let (sock_addr, server_name) = Self::resolve_addr(addr).await?;
        let client = self.new_rpc_client(sock_addr, &server_name).await?;
        let result = client
            .node_info(self.context())
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?;
        timer.success();
        Ok(result)
    }

    /// Ping a remote node
    pub async fn ping(&self, addr: &str) -> Result<String> {
        let timer = RpcTimer::new("ping", addr);
        let (sock_addr, server_name) = Self::resolve_addr(addr).await?;
        let client = self.new_rpc_client(sock_addr, &server_name).await?;
        let result = client
            .ping(self.context())
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?;
        timer.success();
        Ok(result)
    }

    /// Apply a schema to a remote node (for schema propagation)
    pub async fn apply_schema(&self, addr: &str, versioned: crate::VersionedSchema) -> Result<()> {
        let timer = RpcTimer::new("apply_schema", addr);
        let (sock_addr, server_name) = Self::resolve_addr(addr).await?;
        let client = self.new_rpc_client(sock_addr, &server_name).await?;

        // Convert to RPC type
        let request = RpcApplySchemaRequest {
            collection: versioned.collection,
            version: versioned.version.version(),
            schema: versioned.schema,
            created_at: versioned.created_at,
            created_by: versioned.created_by,
            changes: versioned
                .changes
                .into_iter()
                .map(|c| RpcSchemaChange {
                    change_type: format!("{:?}", c.change_type).to_lowercase(),
                    path: c.path,
                    old_value: c.old_value,
                    new_value: c.new_value,
                    description: c.description,
                })
                .collect(),
            metadata: versioned.metadata,
        };

        match client
            .apply_schema(self.context(), request)
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
        {
            Ok(response) => {
                if response.applied {
                    timer.success();
                    Ok(())
                } else {
                    timer.error("not_applied");
                    Err(ClusterError::Internal(
                        response
                            .error
                            .unwrap_or_else(|| "Schema not applied".into()),
                    ))
                }
            }
            Err(e) => {
                timer.error(e.error_type());
                Err(e)
            }
        }
    }

    /// Get schema version from a remote node
    pub async fn get_schema_version(&self, addr: &str, collection: &str) -> Result<Option<u64>> {
        let timer = RpcTimer::new("get_schema_version", addr);
        let (sock_addr, server_name) = Self::resolve_addr(addr).await?;
        let client = self.new_rpc_client(sock_addr, &server_name).await?;
        match client
            .get_schema_version(self.context(), collection.to_string())
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
        {
            Ok(version) => {
                timer.success();
                Ok(version)
            }
            Err(e) => {
                timer.error(e.error_type());
                Err(e)
            }
        }
    }

    /// Remove a connection from the pool
    pub fn remove_connection(&self, addr: &str) {
        if let Ok(addr) = addr.parse::<SocketAddr>() {
            self.evict_connection(addr);
        }
    }

    /// Clear all connections from the pool
    pub fn clear_connections(&self) {
        let mut connections = self.connections.write();
        connections.clear();
        record_connection_pool_size(0);
    }
}

/// Wrapper around QUIC bidirectional streams for tokio I/O
struct QuicBiStream {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

impl tokio::io::AsyncRead for QuicBiStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for QuicBiStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        std::pin::Pin::new(&mut self.send)
            .poll_write(cx, buf)
            .map_err(io::Error::other)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut self.send).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut self.send).poll_shutdown(cx)
    }
}
