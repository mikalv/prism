//! Cluster RPC client with connection pooling
//!
//! Provides a client for connecting to remote Prism nodes.

use crate::config::ClusterConfig;
use crate::error::{ClusterError, Result};
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
use tracing::{debug, info};

/// Connection pool entry
struct PooledConnection {
    client: PrismClusterClient,
    #[allow(dead_code)]
    created_at: std::time::Instant,
}

/// Cluster RPC client with connection pooling
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

    /// Get or create a connection to the specified node
    async fn get_client(&self, addr: SocketAddr) -> Result<PrismClusterClient> {
        // Check for existing connection
        {
            let connections = self.connections.read();
            if let Some(conn) = connections.get(&addr) {
                return Ok(conn.client.clone());
            }
        }

        // Create new connection
        let client = self.create_connection(addr).await?;

        // Store in pool
        {
            let mut connections = self.connections.write();
            connections.insert(
                addr,
                PooledConnection {
                    client: client.clone(),
                    created_at: std::time::Instant::now(),
                },
            );
        }

        Ok(client)
    }

    /// Create a new connection to the specified node
    async fn create_connection(&self, addr: SocketAddr) -> Result<PrismClusterClient> {
        debug!("Connecting to cluster node at {}", addr);

        // Connect - endpoint.connect returns Result<Connecting, ConnectError>
        let connecting = self
            .endpoint
            .connect(addr, "prism-cluster")
            .map_err(|e| ClusterError::Connection(format!("Failed to connect to {}: {}", addr, e)))?;

        // Wait for connection with timeout
        let connection = tokio::time::timeout(self.config.connect_timeout(), connecting)
            .await
            .map_err(|_| ClusterError::Timeout(format!("Connection to {} timed out", addr)))?
            .map_err(|e| {
                ClusterError::Connection(format!("Connection handshake failed with {}: {}", addr, e))
            })?;

        // Open bidirectional stream
        let (send, recv) = connection.open_bi().await.map_err(|e| {
            ClusterError::Transport(format!("Failed to open stream to {}: {}", addr, e))
        })?;

        // Create tarpc transport
        let transport = tarpc::serde_transport::new(
            tokio_util::codec::Framed::new(
                QuicBiStream { send, recv },
                tarpc::tokio_util::codec::LengthDelimitedCodec::new(),
            ),
            tarpc::tokio_serde::formats::Bincode::default(),
        );

        // Create tarpc client
        let client = PrismClusterClient::new(TarpcConfig::default(), transport).spawn();

        info!("Connected to cluster node at {}", addr);
        Ok(client)
    }

    /// Create a context with the configured request timeout
    fn context(&self) -> context::Context {
        let mut ctx = context::current();
        ctx.deadline = std::time::Instant::now() + self.config.request_timeout();
        ctx
    }

    /// Parse address string to SocketAddr
    fn parse_addr(addr: &str) -> Result<SocketAddr> {
        addr.parse()
            .map_err(|e| ClusterError::Config(format!("Invalid address '{}': {}", addr, e)))
    }

    // ========================================
    // Public API
    // ========================================

    /// Index documents on a remote node
    pub async fn index(
        &self,
        addr: &str,
        collection: &str,
        docs: Vec<RpcDocument>,
    ) -> Result<()> {
        let addr = Self::parse_addr(addr)?;
        let client = self.get_client(addr).await?;
        client
            .index(self.context(), collection.to_string(), docs)
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
    }

    /// Search on a remote node
    pub async fn search(
        &self,
        addr: &str,
        collection: &str,
        query: RpcQuery,
    ) -> Result<RpcSearchResults> {
        let addr = Self::parse_addr(addr)?;
        let client = self.get_client(addr).await?;
        client
            .search(self.context(), collection.to_string(), query)
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
    }

    /// Get document from a remote node
    pub async fn get(
        &self,
        addr: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<RpcDocument>> {
        let addr = Self::parse_addr(addr)?;
        let client = self.get_client(addr).await?;
        client
            .get(self.context(), collection.to_string(), id.to_string())
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
    }

    /// Delete documents from a remote node
    pub async fn delete(&self, addr: &str, collection: &str, ids: Vec<String>) -> Result<()> {
        let addr = Self::parse_addr(addr)?;
        let client = self.get_client(addr).await?;
        client
            .delete(self.context(), collection.to_string(), ids)
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
    }

    /// Get stats from a remote node
    pub async fn stats(&self, addr: &str, collection: &str) -> Result<RpcBackendStats> {
        let addr = Self::parse_addr(addr)?;
        let client = self.get_client(addr).await?;
        client
            .stats(self.context(), collection.to_string())
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
    }

    /// List collections on a remote node
    pub async fn list_collections(&self, addr: &str) -> Result<Vec<String>> {
        let addr = Self::parse_addr(addr)?;
        let client = self.get_client(addr).await?;
        Ok(client
            .list_collections(self.context())
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?)
    }

    /// Delete documents by query on a remote node
    pub async fn delete_by_query(
        &self,
        addr: &str,
        request: DeleteByQueryRequest,
    ) -> Result<DeleteByQueryResponse> {
        let addr = Self::parse_addr(addr)?;
        let client = self.get_client(addr).await?;
        client
            .delete_by_query(self.context(), request)
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
    }

    /// Import documents by query on a remote node
    pub async fn import_by_query(
        &self,
        addr: &str,
        request: ImportByQueryRequest,
    ) -> Result<ImportByQueryResponse> {
        let addr = Self::parse_addr(addr)?;
        let client = self.get_client(addr).await?;
        client
            .import_by_query(self.context(), request)
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?
    }

    /// Get node info from a remote node
    pub async fn node_info(&self, addr: &str) -> Result<NodeInfo> {
        let addr = Self::parse_addr(addr)?;
        let client = self.get_client(addr).await?;
        Ok(client
            .node_info(self.context())
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?)
    }

    /// Ping a remote node
    pub async fn ping(&self, addr: &str) -> Result<String> {
        let addr = Self::parse_addr(addr)?;
        let client = self.get_client(addr).await?;
        Ok(client
            .ping(self.context())
            .await
            .map_err(|e| ClusterError::Transport(e.to_string()))?)
    }

    /// Remove a connection from the pool
    pub fn remove_connection(&self, addr: &str) {
        if let Ok(addr) = Self::parse_addr(addr) {
            let mut connections = self.connections.write();
            connections.remove(&addr);
        }
    }

    /// Clear all connections from the pool
    pub fn clear_connections(&self) {
        let mut connections = self.connections.write();
        connections.clear();
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
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
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
