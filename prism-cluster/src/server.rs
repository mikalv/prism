//! Cluster RPC server implementation
//!
//! Wraps CollectionManager to serve cluster RPC requests.

use crate::config::ClusterConfig;
use crate::error::ClusterError;
use crate::service::PrismCluster;
use crate::transport::make_server_endpoint;
use crate::types::*;
use futures::StreamExt;
use prism::collection::CollectionManager;
use std::sync::Arc;
use std::time::Instant;
use tarpc::context::Context;
use tarpc::server::{BaseChannel, Channel};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Cluster RPC server that wraps CollectionManager
pub struct ClusterServer {
    config: ClusterConfig,
    manager: Arc<CollectionManager>,
    start_time: Instant,
}

impl ClusterServer {
    /// Create a new cluster server
    pub fn new(config: ClusterConfig, manager: Arc<CollectionManager>) -> Self {
        Self {
            config,
            manager,
            start_time: Instant::now(),
        }
    }

    /// Start the cluster RPC server
    pub async fn serve(self) -> crate::error::Result<()> {
        let endpoint = make_server_endpoint(&self.config).await?;
        let server = Arc::new(RwLock::new(self));

        info!(
            "Cluster server started, node_id={}",
            server.read().await.config.node_id
        );

        while let Some(incoming) = endpoint.accept().await {
            let server = Arc::clone(&server);

            tokio::spawn(async move {
                match incoming.await {
                    Ok(connection) => {
                        debug!(
                            "Accepted connection from {}",
                            connection.remote_address()
                        );

                        // Accept bidirectional streams
                        loop {
                            match connection.accept_bi().await {
                                Ok((send, recv)) => {
                                    let server = Arc::clone(&server);
                                    tokio::spawn(async move {
                                        if let Err(e) =
                                            Self::handle_stream(server, send, recv).await
                                        {
                                            warn!("Stream error: {}", e);
                                        }
                                    });
                                }
                                Err(e) => {
                                    // Connection closed
                                    debug!("Connection closed: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to accept connection: {}", e);
                    }
                }
            });
        }

        Ok(())
    }

    /// Handle a single bidirectional stream
    async fn handle_stream(
        server: Arc<RwLock<ClusterServer>>,
        send: quinn::SendStream,
        recv: quinn::RecvStream,
    ) -> crate::error::Result<()> {
        // Create tarpc transport from QUIC streams
        let transport = tarpc::serde_transport::new(
            tokio_util::codec::Framed::new(
                QuicBiStream { send, recv },
                tarpc::tokio_util::codec::LengthDelimitedCodec::new(),
            ),
            tarpc::tokio_serde::formats::Bincode::default(),
        );

        let handler = ClusterHandler {
            server: Arc::clone(&server),
        };

        // Serve the connection
        BaseChannel::with_defaults(transport)
            .execute(handler.serve())
            .for_each(|response| async move {
                tokio::spawn(response);
            })
            .await;

        Ok(())
    }
}

/// Handler that implements the PrismCluster service
#[derive(Clone)]
struct ClusterHandler {
    server: Arc<RwLock<ClusterServer>>,
}

impl PrismCluster for ClusterHandler {
    async fn index(
        self,
        _ctx: Context,
        collection: String,
        docs: Vec<RpcDocument>,
    ) -> Result<(), ClusterError> {
        let server = self.server.read().await;
        let docs: Vec<prism::backends::Document> = docs.into_iter().map(Into::into).collect();
        server
            .manager
            .index(&collection, docs)
            .await
            .map_err(ClusterError::from)
    }

    async fn search(
        self,
        _ctx: Context,
        collection: String,
        query: RpcQuery,
    ) -> Result<RpcSearchResults, ClusterError> {
        let server = self.server.read().await;
        let query: prism::backends::Query = query.into();
        let results = server
            .manager
            .search(&collection, query)
            .await
            .map_err(ClusterError::from)?;
        Ok(RpcSearchResults::from(results))
    }

    async fn get(
        self,
        _ctx: Context,
        collection: String,
        id: String,
    ) -> Result<Option<RpcDocument>, ClusterError> {
        let server = self.server.read().await;
        let doc = server
            .manager
            .get(&collection, &id)
            .await
            .map_err(ClusterError::from)?;
        Ok(doc.map(RpcDocument::from))
    }

    async fn delete(
        self,
        _ctx: Context,
        collection: String,
        ids: Vec<String>,
    ) -> Result<(), ClusterError> {
        let server = self.server.read().await;
        server
            .manager
            .delete(&collection, ids)
            .await
            .map_err(ClusterError::from)
    }

    async fn stats(
        self,
        _ctx: Context,
        collection: String,
    ) -> Result<RpcBackendStats, ClusterError> {
        let server = self.server.read().await;
        let stats = server
            .manager
            .stats(&collection)
            .await
            .map_err(ClusterError::from)?;
        Ok(RpcBackendStats::from(stats))
    }

    async fn list_collections(self, _ctx: Context) -> Vec<String> {
        let server = self.server.read().await;
        server.manager.list_collections()
    }

    async fn delete_by_query(
        self,
        _ctx: Context,
        request: DeleteByQueryRequest,
    ) -> Result<DeleteByQueryResponse, ClusterError> {
        let start = Instant::now();
        let server = self.server.read().await;

        // Execute search to find matching documents
        let query: prism::backends::Query = request.query.into();
        let results = server
            .manager
            .search(&request.collection, query)
            .await
            .map_err(ClusterError::from)?;

        let ids_to_delete: Vec<String> = if request.max_docs > 0 {
            results
                .results
                .into_iter()
                .take(request.max_docs)
                .map(|r| r.id)
                .collect()
        } else {
            results.results.into_iter().map(|r| r.id).collect()
        };

        let deleted_count = ids_to_delete.len();

        if !request.dry_run && !ids_to_delete.is_empty() {
            server
                .manager
                .delete(&request.collection, ids_to_delete.clone())
                .await
                .map_err(ClusterError::from)?;
        }

        Ok(DeleteByQueryResponse {
            deleted_count,
            took_ms: start.elapsed().as_millis() as u64,
            deleted_ids: if request.max_docs > 0 {
                ids_to_delete
            } else {
                Vec::new()
            },
        })
    }

    async fn import_by_query(
        self,
        _ctx: Context,
        request: ImportByQueryRequest,
    ) -> Result<ImportByQueryResponse, ClusterError> {
        let start = Instant::now();
        let server = self.server.read().await;

        // If source_node is specified, this would need to connect to remote
        // For now, we implement local collection-to-collection copy
        if request.source_node.is_some() {
            return Err(ClusterError::NotImplemented(
                "Remote import not yet implemented".to_string(),
            ));
        }

        // Search source collection
        let query: prism::backends::Query = request.query.into();
        let results = server
            .manager
            .search(&request.source_collection, query)
            .await
            .map_err(ClusterError::from)?;

        let mut imported_count = 0;
        let mut failed_count = 0;
        let mut errors = Vec::new();

        // Fetch full documents and index into target
        for chunk in results.results.chunks(request.batch_size.max(1)) {
            let mut docs = Vec::new();

            for result in chunk {
                match server.manager.get(&request.source_collection, &result.id).await {
                    Ok(Some(doc)) => docs.push(doc),
                    Ok(None) => {
                        failed_count += 1;
                        errors.push(format!("Document {} not found", result.id));
                    }
                    Err(e) => {
                        failed_count += 1;
                        errors.push(format!("Failed to fetch {}: {}", result.id, e));
                    }
                }
            }

            if !docs.is_empty() {
                match server.manager.index(&request.target_collection, docs.clone()).await {
                    Ok(_) => imported_count += docs.len(),
                    Err(e) => {
                        failed_count += docs.len();
                        errors.push(format!("Failed to index batch: {}", e));
                    }
                }
            }
        }

        Ok(ImportByQueryResponse {
            imported_count,
            failed_count,
            took_ms: start.elapsed().as_millis() as u64,
            errors,
        })
    }

    async fn node_info(self, _ctx: Context) -> NodeInfo {
        let server = self.server.read().await;
        NodeInfo {
            node_id: server.config.node_id.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            collections: server.manager.list_collections(),
            uptime_secs: server.start_time.elapsed().as_secs(),
            healthy: true,
        }
    }

    async fn ping(self, _ctx: Context) -> String {
        "pong".to_string()
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
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for QuicBiStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.send)
            .poll_write(cx, buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.send).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.send).poll_shutdown(cx)
    }
}
