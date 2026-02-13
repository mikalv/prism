use anyhow::Result;
use clap::Parser;
use std::path::Path;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug, Clone)]
#[command(name = "prism-server")]
#[command(about = "Prism hybrid search server")]
#[command(version)]
struct Args {
    /// Configuration file path (env: PRISM_CONFIG_PATH)
    #[arg(short, long, default_value = "prism.toml", env = "PRISM_CONFIG_PATH")]
    config: String,

    /// Host to bind to (env: PRISM_HOST)
    #[arg(long, default_value = "127.0.0.1", env = "PRISM_HOST")]
    host: String,

    /// Port to listen on (env: PRISM_PORT)
    #[arg(short, long, default_value = "3080", env = "PRISM_PORT")]
    port: u16,

    /// Schemas directory path (env: PRISM_SCHEMAS_DIR)
    #[arg(long, default_value = "schemas", env = "PRISM_SCHEMAS_DIR")]
    schemas_dir: String,

    /// Data directory path (env: PRISM_DATA_DIR)
    #[arg(long, default_value = "data", env = "PRISM_DATA_DIR")]
    data_dir: String,

    /// Logs directory path - overrides config (env: PRISM_LOG_DIR)
    #[arg(long, env = "PRISM_LOG_DIR")]
    log_dir: Option<String>,

    /// Embedding cache directory - overrides config (env: PRISM_CACHE_DIR)
    #[arg(long, env = "PRISM_CACHE_DIR")]
    cache_dir: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Load config first (tracing init depends on it)
    let config = prism::config::Config::load_or_create(std::path::Path::new(&args.config))?;
    let config_path = args.config.clone();

    // Determine log format: env var overrides config
    let log_format =
        std::env::var("LOG_FORMAT").unwrap_or_else(|_| config.observability.log_format.clone());

    // Determine log level: RUST_LOG overrides config
    let log_level =
        std::env::var("RUST_LOG").unwrap_or_else(|_| config.observability.log_level.clone());

    // Initialize tracing with configured format
    let env_filter = tracing_subscriber::EnvFilter::new(&log_level);
    let registry = tracing_subscriber::registry().with(env_filter);

    if log_format == "json" {
        registry
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        registry.with(tracing_subscriber::fmt::layer()).init();
    }

    // Initialize Prometheus metrics recorder
    let metrics_handle = if config.observability.metrics_enabled {
        let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
        let handle = builder
            .install_recorder()
            .expect("Failed to install Prometheus metrics recorder");
        tracing::info!("Prometheus metrics enabled at /metrics");
        Some(handle)
    } else {
        None
    };

    tracing::info!("Starting Prism server on {}:{}", args.host, args.port);
    tracing::info!("Schemas dir: {}", args.schemas_dir);
    tracing::info!("Data dir: {}", args.data_dir);
    if let Some(ref log_dir) = args.log_dir {
        tracing::info!("Log dir: {}", log_dir);
        std::fs::create_dir_all(log_dir)?;
    }
    if let Some(ref cache_dir) = args.cache_dir {
        tracing::info!("Cache dir: {}", cache_dir);
        std::fs::create_dir_all(cache_dir)?;
    }

    config.ensure_dirs()?;
    let data_path = Path::new(&args.data_dir);
    std::fs::create_dir_all(data_path)?;
    let addr = format!("{}:{}", args.host, args.port);

    // Create backends
    let text_backend = Arc::new(prism::backends::text::TextBackend::new(
        &config.storage.data_dir,
    )?);
    let vector_backend = Arc::new(prism::backends::VectorBackend::new(
        &config.storage.data_dir,
    )?);

    // Set up embedding provider if enabled
    if config.embedding.enabled {
        tracing::info!("Setting up embedding provider...");
        match prism::embedding::create_provider(&config.embedding.provider).await {
            Ok(provider) => {
                // Priority: CLI arg > env var > config > default
                let cache_path = args
                    .cache_dir
                    .as_ref()
                    .map(std::path::PathBuf::from)
                    .or_else(|| config.embedding.cache_dir.clone())
                    .unwrap_or_else(|| config.storage.data_dir.join("embedding_cache.db"));
                let cache = Arc::new(
                    prism::cache::SqliteCache::new(
                        cache_path.to_str().unwrap_or("embedding_cache.db"),
                    )
                    .expect("Failed to create embedding cache"),
                );
                let cached_provider =
                    Arc::new(prism::embedding::CachedEmbeddingProvider::new(
                        provider,
                        cache,
                        prism::cache::KeyStrategy::ModelText,
                    ));
                vector_backend.set_embedding_provider(cached_provider);
                tracing::info!(
                    "Embedding provider configured (cache: {})",
                    cache_path.display()
                );
            }
            Err(e) => {
                tracing::warn!("Failed to create embedding provider: {}. Vector search with auto-embedding will not work.", e);
            }
        }
    }

    // Create collection manager
    // Use CLI arg for schemas_dir if provided, otherwise fall back to config
    let schemas_path = if args.schemas_dir != "schemas" {
        std::path::PathBuf::from(&args.schemas_dir)
    } else {
        config.schemas_dir()
    };
    // Create graph storage (uses same data_dir as other backends)
    let graph_storage: Option<Arc<dyn prism_storage::SegmentStorage>> = Some(
        Arc::new(prism_storage::LocalStorage::new(&config.storage.data_dir)),
    );

    let manager = Arc::new(prism::collection::CollectionManager::new(
        &schemas_path,
        text_backend,
        vector_backend,
        graph_storage,
    )?);
    manager.initialize().await?;

    // Load ingest pipelines
    let config_dir = std::path::Path::new(&args.config)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let pipelines_dir = config_dir.join("conf/pipelines");
    let pipeline_registry = prism::pipeline::registry::PipelineRegistry::load(&pipelines_dir)?;
    tracing::info!("Loaded ingest pipelines from {}", pipelines_dir.display());

    // Create ILM manager if enabled
    let ilm_manager = if config.ilm.enabled {
        tracing::info!("ILM enabled, initializing manager...");
        match prism::ilm::IlmManager::new(manager.clone(), &config.ilm, &config.storage.data_dir)
            .await
        {
            Ok(ilm) => {
                let ilm = Arc::new(ilm);
                tracing::info!(
                    "ILM manager initialized ({} policies)",
                    config.ilm.policies.len()
                );
                Some(ilm)
            }
            Err(e) => {
                tracing::error!("Failed to initialize ILM manager: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Create server
    let mut server = prism::api::ApiServer::with_pipelines(
        manager,
        config.server.cors.clone(),
        config.security.clone(),
        pipeline_registry,
    )
    .with_metrics(metrics_handle)
    .with_data_dir(&config.storage.data_dir);

    // Add ILM manager if available
    if let Some(ref ilm) = ilm_manager {
        server = server.with_ilm(ilm.clone());
    }

    // Get config reloader for SIGHUP handling
    let config_reloader = server.config_reloader();

    let tls = if config.server.tls.enabled {
        Some(&config.server.tls)
    } else {
        None
    };

    tracing::info!("Listening on {}", addr);
    if tls.is_some() {
        tracing::info!("TLS enabled");
    }
    if config.security.enabled {
        tracing::info!(
            "Security enabled ({} API keys, {} roles)",
            config.security.api_keys.len(),
            config.security.roles.len()
        );
    }
    if config.security.audit.enabled {
        tracing::info!(
            "Audit logging enabled (index_to_collection: {})",
            config.security.audit.index_to_collection
        );
    }

    // Spawn SIGHUP handler for config reload
    #[cfg(unix)]
    {
        let reloader = config_reloader.clone();
        let cfg_path = config_path.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};

            let mut sighup =
                signal(SignalKind::hangup()).expect("Failed to register SIGHUP handler");

            loop {
                sighup.recv().await;
                tracing::info!("Received SIGHUP, reloading configuration...");

                match prism::config::Config::load_or_create(std::path::Path::new(&cfg_path)) {
                    Ok(new_config) => {
                        reloader.reload_security(new_config.security).await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to reload config: {}", e);
                    }
                }
            }
        });
        tracing::info!("SIGHUP handler registered - send SIGHUP to reload security config");
    }

    // Start ILM background service if enabled
    if let Some(ref ilm) = ilm_manager {
        let ilm_clone = ilm.clone();
        tokio::spawn(async move {
            if let Err(e) = ilm_clone.start().await {
                tracing::error!("ILM manager error: {}", e);
            }
        });
        tracing::info!(
            "ILM background service started (check interval: {}s)",
            config.ilm.check_interval_secs
        );
    }

    // Build extension router with optional features
    #[allow(unused_mut)]
    let mut extension_router: axum::Router<()> = axum::Router::new();

    // Start cluster RPC server and federated search if enabled
    #[cfg(feature = "cluster")]
    if config.cluster.enabled {
        let cluster_config = prism_cluster::ClusterConfig {
            enabled: config.cluster.enabled,
            node_id: config.cluster.node_id.clone(),
            bind_addr: config.cluster.bind_addr.clone(),
            advertise_addr: config.cluster.advertise_addr.clone(),
            seed_nodes: config.cluster.seed_nodes.clone(),
            connect_timeout_ms: config.cluster.connect_timeout_ms,
            request_timeout_ms: config.cluster.request_timeout_ms,
            tls: prism_cluster::ClusterTlsConfig {
                enabled: config.cluster.tls.enabled,
                cert_path: config.cluster.tls.cert_path.clone(),
                key_path: config.cluster.tls.key_path.clone(),
                ca_cert_path: config.cluster.tls.ca_cert_path.clone(),
                skip_verify: config.cluster.tls.skip_verify,
            },
            ..Default::default()
        };

        // 1. Create shared cluster state
        let cluster_state = Arc::new(prism_cluster::ClusterState::new());

        // 2. Register self node
        let self_node_id = cluster_config.node_id.clone();
        let self_addr = cluster_config.advertise_address().to_string();
        cluster_state.register_node(prism_cluster::NodeInfo {
            node_id: self_node_id.clone(),
            address: self_addr.clone(),
            topology: prism_cluster::NodeTopology::default(),
            healthy: true,
            shard_count: 0,
            disk_used_bytes: 0,
            disk_total_bytes: 0,
            index_size_bytes: 0,
            draining: false,
        });

        // 3. Register seed nodes (use address as node_id until we discover their real ID)
        let mut all_node_ids = vec![self_node_id.clone()];
        for seed in &cluster_config.seed_nodes {
            let seed_id = seed.clone();
            cluster_state.register_node(prism_cluster::NodeInfo {
                node_id: seed_id.clone(),
                address: seed.clone(),
                topology: prism_cluster::NodeTopology::default(),
                healthy: true,
                shard_count: 0,
                disk_used_bytes: 0,
                disk_total_bytes: 0,
                index_size_bytes: 0,
                draining: false,
            });
            all_node_ids.push(seed_id);
        }

        // 4. Auto-assign 1 shard per node per collection
        let collections = server.manager().list_collections();
        for collection in &collections {
            for (i, node_id) in all_node_ids.iter().enumerate() {
                let mut assignment =
                    prism_cluster::ShardAssignment::new(collection, i as u32, node_id);
                assignment.state = prism_cluster::ShardState::Active;
                cluster_state.assign_shard(assignment);
            }
        }

        tracing::info!(
            "Cluster initialized: {} nodes, {} collections, {} total shards",
            all_node_ids.len(),
            collections.len(),
            all_node_ids.len() * collections.len()
        );

        // 5. Create ClusterClient
        let cluster_client = match prism_cluster::ClusterClient::new(cluster_config.clone()).await {
            Ok(c) => Arc::new(c),
            Err(e) => {
                tracing::error!("Failed to create cluster client: {}", e);
                return Err(anyhow::anyhow!("Cluster client init failed: {}", e));
            }
        };

        // 6. Create FederatedSearch
        let federation = Arc::new(prism_cluster::FederatedSearch::new(
            cluster_client,
            Arc::clone(&cluster_state),
            prism_cluster::FederationConfig::default(),
        ));

        // 7. Build cluster routes
        extension_router =
            extension_router.merge(cluster_routes(federation, Arc::clone(&cluster_state)));

        // 8. Start ClusterServer with shared state
        let cluster_manager = server.manager();
        tracing::info!(
            "Starting cluster RPC server on {} (node_id: {})",
            cluster_config.bind_addr,
            cluster_config.node_id
        );

        tokio::spawn(async move {
            let cluster_server = prism_cluster::ClusterServer::with_state(
                cluster_config,
                cluster_manager,
                cluster_state,
            );
            if let Err(e) = cluster_server.serve().await {
                tracing::error!("Cluster server error: {}", e);
            }
        });
    }

    // Add UI routes if enabled
    #[cfg(feature = "ui")]
    {
        extension_router = extension_router.nest("/ui", prism_ui::ui_router());
        if std::path::Path::new("webui").is_dir() {
            tracing::info!("Web UI enabled at /ui (dev mode: serving from ./webui/)");
        } else {
            tracing::info!("Web UI enabled at /ui (embedded assets)");
        }
    }

    // Add ES-compat routes if enabled
    #[cfg(feature = "es-compat")]
    {
        extension_router = extension_router.nest(
            "/_elastic",
            prism_es_compat::es_compat_router(server.manager()),
        );
        tracing::info!("Elasticsearch compatibility enabled at /_elastic/*");
    }

    // Serve with extensions if any are enabled
    #[cfg(any(feature = "ui", feature = "es-compat", feature = "cluster"))]
    {
        server
            .serve_with_extension(&addr, tls, extension_router)
            .await?;
    }

    #[cfg(not(any(feature = "ui", feature = "es-compat", feature = "cluster")))]
    {
        server.serve(&addr, tls).await?;
    }

    Ok(())
}

/// Build cluster federation routes
#[cfg(feature = "cluster")]
fn cluster_routes(
    federation: Arc<prism_cluster::FederatedSearch>,
    cluster_state: Arc<prism_cluster::ClusterState>,
) -> axum::Router<()> {
    use axum::extract::{Path, State};
    use axum::routing::{get, post};
    use axum::Json;

    async fn federated_search(
        Path(collection): Path<String>,
        State(fed): State<Arc<prism_cluster::FederatedSearch>>,
        Json(body): Json<serde_json::Value>,
    ) -> axum::response::Response {
        use axum::http::StatusCode;
        use axum::response::IntoResponse;

        let query_string = body
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string();
        let limit = body.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        let rpc_query = prism_cluster::RpcQuery {
            query_string,
            fields: vec![],
            limit,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
            highlight: None,
            rrf_k: None,
            min_score: None,
            score_function: None,
            skip_ranking: false,
        };

        match fed.search(&collection, rpc_query).await {
            Ok(results) => {
                let response = serde_json::json!({
                    "results": results.results,
                    "total": results.total,
                    "latency_ms": results.latency_ms,
                    "is_partial": results.is_partial,
                    "shard_status": {
                        "total": results.shard_status.total,
                        "successful": results.shard_status.successful,
                        "failed": results.shard_status.failed,
                    }
                });
                (StatusCode::OK, Json(response)).into_response()
            }
            Err(e) => {
                let response = serde_json::json!({
                    "error": e.to_string()
                });
                (StatusCode::INTERNAL_SERVER_ERROR, Json(response)).into_response()
            }
        }
    }

    async fn federated_index(
        Path(collection): Path<String>,
        State(fed): State<Arc<prism_cluster::FederatedSearch>>,
        Json(body): Json<serde_json::Value>,
    ) -> axum::response::Response {
        use axum::http::StatusCode;
        use axum::response::IntoResponse;

        let docs: Vec<prism_cluster::RpcDocument> = match body.get("documents") {
            Some(docs_val) => match serde_json::from_value(docs_val.clone()) {
                Ok(d) => d,
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error": e.to_string()})),
                    )
                        .into_response()
                }
            },
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "missing 'documents' field"})),
                )
                    .into_response()
            }
        };

        match fed.index(&collection, docs).await {
            Ok(status) => {
                let response = serde_json::json!({
                    "total_docs": status.total_docs,
                    "successful_docs": status.successful_docs,
                    "failed_docs": status.failed_docs,
                    "latency_ms": status.latency_ms,
                });
                (StatusCode::CREATED, Json(response)).into_response()
            }
            Err(e) => {
                let response = serde_json::json!({"error": e.to_string()});
                (StatusCode::INTERNAL_SERVER_ERROR, Json(response)).into_response()
            }
        }
    }

    async fn cluster_health(
        State(_fed): State<Arc<prism_cluster::FederatedSearch>>,
    ) -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "status": "ok",
            "federated": true,
        }))
    }

    async fn drain_node(
        Path(node_id): Path<String>,
        State(state): State<Arc<prism_cluster::ClusterState>>,
    ) -> axum::response::Response {
        use axum::http::StatusCode;
        use axum::response::IntoResponse;

        let result = state.drain_node(&node_id);
        if result {
            (
                StatusCode::OK,
                Json(serde_json::json!({"drained": true, "node_id": node_id})),
            )
                .into_response()
        } else {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("node {} not found", node_id)})),
            )
                .into_response()
        }
    }

    async fn undrain_node(
        Path(node_id): Path<String>,
        State(state): State<Arc<prism_cluster::ClusterState>>,
    ) -> axum::response::Response {
        use axum::http::StatusCode;
        use axum::response::IntoResponse;

        let result = state.undrain_node(&node_id);
        if result {
            (
                StatusCode::OK,
                Json(serde_json::json!({"undrained": true, "node_id": node_id})),
            )
                .into_response()
        } else {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("node {} not found", node_id)})),
            )
                .into_response()
        }
    }

    async fn upgrade_status(
        State(state): State<Arc<prism_cluster::ClusterState>>,
    ) -> Json<serde_json::Value> {
        let nodes = state.get_nodes();
        let node_statuses: Vec<serde_json::Value> = nodes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "node_id": n.info.node_id,
                    "version": n.version,
                    "protocol_version": n.protocol_version,
                    "min_supported_version": n.min_supported_version,
                    "draining": n.draining,
                    "reachable": n.reachable,
                })
            })
            .collect();

        let all_same_version = nodes.windows(2).all(|w| {
            w[0].protocol_version == w[1].protocol_version
        });

        Json(serde_json::json!({
            "nodes": node_statuses,
            "total_nodes": nodes.len(),
            "draining_count": nodes.iter().filter(|n| n.draining).count(),
            "all_same_version": all_same_version,
        }))
    }

    // Federation routes (with federation state)
    let federation_routes = axum::Router::new()
        .route(
            "/cluster/collections/:collection/search",
            post(federated_search),
        )
        .route(
            "/cluster/collections/:collection/documents",
            post(federated_index),
        )
        .route("/cluster/health", get(cluster_health))
        .with_state(federation);

    // Drain/upgrade routes (with cluster_state)
    let cluster_mgmt_routes = axum::Router::new()
        .route("/cluster/nodes/:node_id/drain", post(drain_node))
        .route("/cluster/nodes/:node_id/undrain", post(undrain_node))
        .route("/cluster/upgrade/status", get(upgrade_status))
        .with_state(cluster_state);

    federation_routes.merge(cluster_mgmt_routes)
}
