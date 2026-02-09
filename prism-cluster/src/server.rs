//! Cluster RPC server implementation
//!
//! Wraps CollectionManager to serve cluster RPC requests.

use crate::config::ClusterConfig;
use crate::error::ClusterError;
use crate::metrics::{
    record_rebalance_operation, update_cluster_state_metrics, update_rebalance_status_metrics,
    RpcHandlerTimer,
};
use crate::placement::{ClusterState, PlacementStrategy, ShardAssignment, ShardState};
use crate::rebalance::{RebalanceEngine, RebalanceTrigger};
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
    cluster_state: Arc<ClusterState>,
    rebalance_engine: Arc<RebalanceEngine>,
}

impl ClusterServer {
    /// Create a new cluster server
    pub fn new(config: ClusterConfig, manager: Arc<CollectionManager>) -> Self {
        let cluster_state = Arc::new(ClusterState::new());
        let strategy = PlacementStrategy::default();
        let rebalance_engine = Arc::new(RebalanceEngine::new(
            config.rebalancing.clone(),
            Arc::clone(&cluster_state),
            strategy,
        ));

        Self {
            config,
            manager,
            start_time: Instant::now(),
            cluster_state,
            rebalance_engine,
        }
    }

    /// Create a new cluster server with existing state
    pub fn with_state(
        config: ClusterConfig,
        manager: Arc<CollectionManager>,
        cluster_state: Arc<ClusterState>,
    ) -> Self {
        let strategy = PlacementStrategy::default();
        let rebalance_engine = Arc::new(RebalanceEngine::new(
            config.rebalancing.clone(),
            Arc::clone(&cluster_state),
            strategy,
        ));

        Self {
            config,
            manager,
            start_time: Instant::now(),
            cluster_state,
            rebalance_engine,
        }
    }

    /// Get a reference to the cluster state
    pub fn cluster_state(&self) -> Arc<ClusterState> {
        Arc::clone(&self.cluster_state)
    }

    /// Get a reference to the rebalance engine
    pub fn rebalance_engine(&self) -> Arc<RebalanceEngine> {
        Arc::clone(&self.rebalance_engine)
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
                        debug!("Accepted connection from {}", connection.remote_address());

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
        let timer = RpcHandlerTimer::new("index");
        let server = self.server.read().await;
        let docs: Vec<prism::backends::Document> = docs.into_iter().map(Into::into).collect();
        match server.manager.index(&collection, docs).await {
            Ok(()) => {
                timer.success();
                Ok(())
            }
            Err(e) => {
                let err = ClusterError::from(e);
                timer.error(err.error_type());
                Err(err)
            }
        }
    }

    async fn search(
        self,
        _ctx: Context,
        collection: String,
        query: RpcQuery,
    ) -> Result<RpcSearchResults, ClusterError> {
        let timer = RpcHandlerTimer::new("search");
        let server = self.server.read().await;
        let query: prism::backends::Query = query.into();
        match server.manager.search(&collection, query).await {
            Ok(results) => {
                timer.success();
                Ok(RpcSearchResults::from(results))
            }
            Err(e) => {
                let err = ClusterError::from(e);
                timer.error(err.error_type());
                Err(err)
            }
        }
    }

    async fn get(
        self,
        _ctx: Context,
        collection: String,
        id: String,
    ) -> Result<Option<RpcDocument>, ClusterError> {
        let timer = RpcHandlerTimer::new("get");
        let server = self.server.read().await;
        match server.manager.get(&collection, &id).await {
            Ok(doc) => {
                timer.success();
                Ok(doc.map(RpcDocument::from))
            }
            Err(e) => {
                let err = ClusterError::from(e);
                timer.error(err.error_type());
                Err(err)
            }
        }
    }

    async fn delete(
        self,
        _ctx: Context,
        collection: String,
        ids: Vec<String>,
    ) -> Result<(), ClusterError> {
        let timer = RpcHandlerTimer::new("delete");
        let server = self.server.read().await;
        match server.manager.delete(&collection, ids).await {
            Ok(()) => {
                timer.success();
                Ok(())
            }
            Err(e) => {
                let err = ClusterError::from(e);
                timer.error(err.error_type());
                Err(err)
            }
        }
    }

    async fn stats(
        self,
        _ctx: Context,
        collection: String,
    ) -> Result<RpcBackendStats, ClusterError> {
        let timer = RpcHandlerTimer::new("stats");
        let server = self.server.read().await;
        match server.manager.stats(&collection).await {
            Ok(stats) => {
                timer.success();
                Ok(RpcBackendStats::from(stats))
            }
            Err(e) => {
                let err = ClusterError::from(e);
                timer.error(err.error_type());
                Err(err)
            }
        }
    }

    async fn list_collections(self, _ctx: Context) -> Vec<String> {
        let timer = RpcHandlerTimer::new("list_collections");
        let server = self.server.read().await;
        let result = server.manager.list_collections();
        timer.success();
        result
    }

    async fn delete_by_query(
        self,
        _ctx: Context,
        request: DeleteByQueryRequest,
    ) -> Result<DeleteByQueryResponse, ClusterError> {
        let timer = RpcHandlerTimer::new("delete_by_query");
        let start = Instant::now();
        let server = self.server.read().await;

        // Execute search to find matching documents
        let query: prism::backends::Query = request.query.into();
        let results = match server.manager.search(&request.collection, query).await {
            Ok(r) => r,
            Err(e) => {
                let err = ClusterError::from(e);
                timer.error(err.error_type());
                return Err(err);
            }
        };

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
            if let Err(e) = server
                .manager
                .delete(&request.collection, ids_to_delete.clone())
                .await
            {
                let err = ClusterError::from(e);
                timer.error(err.error_type());
                return Err(err);
            }
        }

        timer.success();
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
        let timer = RpcHandlerTimer::new("import_by_query");
        let start = Instant::now();
        let server = self.server.read().await;

        // If source_node is specified, this would need to connect to remote
        // For now, we implement local collection-to-collection copy
        if request.source_node.is_some() {
            timer.error("not_implemented");
            return Err(ClusterError::NotImplemented(
                "Remote import not yet implemented".to_string(),
            ));
        }

        // Search source collection
        let query: prism::backends::Query = request.query.into();
        let results = match server
            .manager
            .search(&request.source_collection, query)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let err = ClusterError::from(e);
                timer.error(err.error_type());
                return Err(err);
            }
        };

        let mut imported_count = 0;
        let mut failed_count = 0;
        let mut errors = Vec::new();

        // Fetch full documents and index into target
        for chunk in results.results.chunks(request.batch_size.max(1)) {
            let mut docs = Vec::new();

            for result in chunk {
                match server
                    .manager
                    .get(&request.source_collection, &result.id)
                    .await
                {
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
                match server
                    .manager
                    .index(&request.target_collection, docs.clone())
                    .await
                {
                    Ok(_) => imported_count += docs.len(),
                    Err(e) => {
                        failed_count += docs.len();
                        errors.push(format!("Failed to index batch: {}", e));
                    }
                }
            }
        }

        timer.success();
        Ok(ImportByQueryResponse {
            imported_count,
            failed_count,
            took_ms: start.elapsed().as_millis() as u64,
            errors,
        })
    }

    async fn node_info(self, _ctx: Context) -> NodeInfo {
        let timer = RpcHandlerTimer::new("node_info");
        let server = self.server.read().await;
        timer.success();
        NodeInfo {
            node_id: server.config.node_id.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            collections: server.manager.list_collections(),
            uptime_secs: server.start_time.elapsed().as_secs(),
            healthy: true,
        }
    }

    async fn ping(self, _ctx: Context) -> String {
        let timer = RpcHandlerTimer::new("ping");
        timer.success();
        "pong".to_string()
    }

    async fn cluster_health(self, _ctx: Context) -> RpcClusterHealth {
        let timer = RpcHandlerTimer::new("cluster_health");
        let server = self.server.read().await;

        // Get all nodes from cluster state
        let nodes = server.cluster_state.get_nodes();
        let heartbeat_timeout = 30; // Default timeout

        let mut alive_count = 0;
        let mut suspect_count = 0;
        let mut dead_count = 0;

        let rpc_nodes: Vec<RpcNodeHealth> = nodes
            .iter()
            .map(|n| {
                let healthy = n.is_healthy(heartbeat_timeout);
                let state = if !n.reachable {
                    dead_count += 1;
                    "dead"
                } else if !healthy {
                    suspect_count += 1;
                    "suspect"
                } else {
                    alive_count += 1;
                    "alive"
                };

                RpcNodeHealth {
                    node_id: n.info.node_id.clone(),
                    state: state.to_string(),
                    last_heartbeat: Some(n.last_heartbeat),
                    missed_heartbeats: 0, // Would need to track this
                    last_latency_ms: None,
                }
            })
            .collect();

        let total_count = rpc_nodes.len();
        let quorum_available = alive_count > total_count / 2;

        timer.success();
        RpcClusterHealth {
            nodes: rpc_nodes,
            alive_count,
            suspect_count,
            dead_count,
            total_count,
            quorum_available,
        }
    }

    async fn heartbeat(self, _ctx: Context) -> RpcHeartbeatResponse {
        let timer = RpcHandlerTimer::new("heartbeat");
        let server = self.server.read().await;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        timer.success();
        RpcHeartbeatResponse {
            node_id: server.config.node_id.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_secs: server.start_time.elapsed().as_secs(),
            timestamp,
        }
    }

    // ========================================
    // Shard Management
    // ========================================

    async fn assign_shard(
        self,
        _ctx: Context,
        request: ShardAssignmentRequest,
    ) -> Result<ShardAssignmentResponse, ClusterError> {
        let timer = RpcHandlerTimer::new("assign_shard");
        let server = self.server.read().await;

        // Clone values for logging before moving
        let shard_id = request.shard_id.clone();
        let primary_node = request.primary_node.clone();
        let replica_nodes_str = format!("{:?}", request.replica_nodes);

        // Create assignment
        let mut assignment = ShardAssignment::new(
            &request.collection,
            request.shard_number,
            &request.primary_node,
        );
        assignment.shard_id = request.shard_id;
        assignment.replica_nodes = request.replica_nodes;
        assignment.state = ShardState::Initializing;

        // Store in cluster state
        server.cluster_state.assign_shard(assignment);
        let epoch = server.cluster_state.next_epoch();

        // Update cluster state metrics
        update_cluster_state_metrics(&server.cluster_state);

        info!(
            "Shard {} assigned to primary={}, replicas={}",
            shard_id, primary_node, replica_nodes_str
        );

        timer.success();
        Ok(ShardAssignmentResponse {
            success: true,
            epoch,
            error: None,
        })
    }

    async fn get_shard_assignments(
        self,
        _ctx: Context,
        request: GetShardAssignmentsRequest,
    ) -> Result<Vec<RpcShardInfo>, ClusterError> {
        let timer = RpcHandlerTimer::new("get_shard_assignments");
        let server = self.server.read().await;

        let assignments = if let Some(ref collection) = request.collection {
            server.cluster_state.get_collection_shards(collection)
        } else {
            server.cluster_state.get_all_shards()
        };

        let result = assignments
            .into_iter()
            .map(|a| RpcShardInfo {
                shard_id: a.shard_id,
                collection: a.collection,
                primary_node: a.primary_node,
                replica_nodes: a.replica_nodes,
                state: format!("{:?}", a.state),
                shard_number: a.shard_number,
                size_bytes: a.size_bytes,
                document_count: a.document_count,
            })
            .collect();

        timer.success();
        Ok(result)
    }

    async fn transfer_shard(
        self,
        _ctx: Context,
        request: ShardTransferRequest,
    ) -> Result<ShardTransferResponse, ClusterError> {
        let timer = RpcHandlerTimer::new("transfer_shard");
        let server = self.server.read().await;

        // Find the shard
        let shard = match server.cluster_state.get_shard(&request.shard_id) {
            Some(s) => s,
            None => {
                timer.error("collection_not_found");
                return Err(ClusterError::CollectionNotFound(request.shard_id.clone()));
            }
        };

        // Verify source node has the shard
        if !shard.is_on_node(&request.from_node) {
            timer.error("invalid_query");
            return Err(ClusterError::InvalidQuery(format!(
                "Shard {} not found on node {}",
                request.shard_id, request.from_node
            )));
        }

        // Mark shard as relocating
        server
            .cluster_state
            .update_shard_state(&request.shard_id, ShardState::Relocating);

        let transfer_id = uuid::Uuid::new_v4().to_string();

        // Record shard transfer metric
        crate::metrics::record_shard_transfer(
            &request.shard_id,
            &request.from_node,
            &request.to_node,
        );

        // Update cluster state metrics
        update_cluster_state_metrics(&server.cluster_state);

        info!(
            "Initiated shard transfer {} from {} to {}, transfer_id={}",
            request.shard_id, request.from_node, request.to_node, transfer_id
        );

        // The actual transfer would be handled by a background task
        // For now, we just return the transfer ID
        timer.success();
        Ok(ShardTransferResponse {
            success: true,
            transfer_id: Some(transfer_id),
            error: None,
        })
    }

    // ========================================
    // Rebalancing
    // ========================================

    async fn trigger_rebalance(
        self,
        _ctx: Context,
        request: TriggerRebalanceRequest,
    ) -> Result<RpcRebalanceStatus, ClusterError> {
        let timer = RpcHandlerTimer::new("trigger_rebalance");
        let server = self.server.read().await;

        let trigger = match request.trigger.as_str() {
            "manual" => RebalanceTrigger::Manual,
            "node_joined" => RebalanceTrigger::NodeJoined,
            "node_left" => RebalanceTrigger::NodeLeft,
            "imbalance" => RebalanceTrigger::ImbalanceThreshold,
            "scheduled" => RebalanceTrigger::Scheduled,
            _ => RebalanceTrigger::Manual,
        };

        // Record rebalance operation
        record_rebalance_operation(&request.trigger);

        let status = match server.rebalance_engine.trigger(trigger) {
            Ok(s) => s,
            Err(e) => {
                timer.error("internal");
                return Err(ClusterError::Internal(e));
            }
        };

        // Update rebalance status metrics
        update_rebalance_status_metrics(&status);

        timer.success();
        Ok(RpcRebalanceStatus {
            in_progress: status.in_progress,
            phase: format!("{:?}", status.phase),
            shards_in_transit: status.shards_in_transit,
            total_shards_to_move: status.total_shards_to_move,
            completed_moves: status.completed_moves,
            failed_moves: status.failed_moves,
            started_at: status.started_at,
            last_error: status.last_error,
        })
    }

    async fn get_rebalance_status(self, _ctx: Context) -> Result<RpcRebalanceStatus, ClusterError> {
        let timer = RpcHandlerTimer::new("get_rebalance_status");
        let server = self.server.read().await;
        let status = server.rebalance_engine.status();

        // Update rebalance status metrics
        update_rebalance_status_metrics(&status);

        timer.success();
        Ok(RpcRebalanceStatus {
            in_progress: status.in_progress,
            phase: format!("{:?}", status.phase),
            shards_in_transit: status.shards_in_transit,
            total_shards_to_move: status.total_shards_to_move,
            completed_moves: status.completed_moves,
            failed_moves: status.failed_moves,
            started_at: status.started_at,
            last_error: status.last_error,
        })
    }

    // ========================================
    // Schema Management
    // ========================================

    async fn apply_schema(
        self,
        _ctx: Context,
        request: RpcApplySchemaRequest,
    ) -> Result<RpcApplySchemaResponse, ClusterError> {
        let timer = RpcHandlerTimer::new("apply_schema");

        // For now, just acknowledge the schema
        // A full implementation would:
        // 1. Validate the schema
        // 2. Store it in a local schema registry
        // 3. Possibly trigger collection re-configuration

        info!(
            "Received schema version {} for collection {} from {}",
            request.version, request.collection, request.created_by
        );

        timer.success();
        Ok(RpcApplySchemaResponse {
            applied: true,
            current_version: request.version,
            error: None,
        })
    }

    async fn get_schema_version(
        self,
        _ctx: Context,
        collection: String,
    ) -> Result<Option<u64>, ClusterError> {
        let timer = RpcHandlerTimer::new("get_schema_version");

        // For now, return None indicating no version is tracked locally
        // A full implementation would check the local schema registry
        debug!("Getting schema version for collection {}", collection);

        timer.success();
        Ok(None)
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
            .map_err(|e| std::io::Error::other(e))
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
