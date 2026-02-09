use crate::collection::CollectionManager;
use crate::config::{CorsConfig, SecurityConfig, TlsConfig};
use crate::pipeline::registry::PipelineRegistry;
use crate::security::permissions::PermissionChecker;
use crate::Result;
use axum::http::StatusCode;
use axum::{
    extract::State,
    http::{HeaderValue, Method},
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
    routing::post,
    Router,
};
use futures::stream::Stream;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, watch, RwLock};
use tokio_rustls::TlsAcceptor;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::mcp::handler::{JsonRpcRequest, JsonRpcResponse, McpHandler};
use crate::mcp::session::{SessionManager, SseEvent};
use crate::mcp::tools::register_basic_tools;
use crate::mcp::ToolRegistry;

async fn metrics_middleware(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let start = std::time::Instant::now();

    let response = next.run(req).await;

    let duration = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    metrics::counter!("prism_http_requests_total",
        "method" => method.clone(),
        "path" => path.clone(),
        "status_code" => status,
    )
    .increment(1);

    metrics::histogram!("prism_http_request_duration_seconds",
        "method" => method,
        "path" => path,
    )
    .record(duration);

    response
}

#[derive(Clone)]
pub struct AppState {
    pub manager: Arc<CollectionManager>,
    pub session_manager: Arc<SessionManager>,
    pub mcp_handler: Arc<McpHandler>,
    pub pipeline_registry: Arc<PipelineRegistry>,
    pub metrics_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
    pub security_config: Arc<RwLock<SecurityConfig>>,
    pub ilm_manager: Option<Arc<crate::ilm::IlmManager>>,
}

/// Handle for reloading server configuration at runtime (via SIGHUP)
#[derive(Clone)]
pub struct ConfigReloader {
    security_tx: watch::Sender<SecurityConfig>,
    security_config: Arc<RwLock<SecurityConfig>>,
}

impl ConfigReloader {
    /// Reload security configuration from a new SecurityConfig
    pub async fn reload_security(&self, new_config: SecurityConfig) {
        tracing::info!("Reloading security configuration...");
        tracing::info!(
            "  API keys: {} -> {}",
            self.security_tx.borrow().api_keys.len(),
            new_config.api_keys.len()
        );
        tracing::info!(
            "  Roles: {} -> {}",
            self.security_tx.borrow().roles.len(),
            new_config.roles.len()
        );

        // Update the shared config
        {
            let mut config = self.security_config.write().await;
            *config = new_config.clone();
        }

        // Notify watchers
        let _ = self.security_tx.send(new_config);
        tracing::info!("Security configuration reloaded successfully");
    }
}

pub struct ApiServer {
    manager: Arc<CollectionManager>,
    session_manager: Arc<SessionManager>,
    mcp_handler: Arc<McpHandler>,
    cors_config: CorsConfig,
    security_config: Arc<RwLock<SecurityConfig>>,
    config_reloader: ConfigReloader,
    pipeline_registry: Arc<PipelineRegistry>,
    metrics_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
    ilm_manager: Option<Arc<crate::ilm::IlmManager>>,
    template_manager: Option<Arc<crate::templates::TemplateManager>>,
    data_dir: Option<std::path::PathBuf>,
}

impl ApiServer {
    pub fn new(manager: Arc<CollectionManager>) -> Self {
        Self::with_cors(manager, CorsConfig::default())
    }

    pub fn with_cors(manager: Arc<CollectionManager>, cors_config: CorsConfig) -> Self {
        Self::with_security(manager, cors_config, SecurityConfig::default())
    }

    pub fn with_security(
        manager: Arc<CollectionManager>,
        cors_config: CorsConfig,
        security_config: SecurityConfig,
    ) -> Self {
        Self::with_pipelines(
            manager,
            cors_config,
            security_config,
            PipelineRegistry::empty(),
        )
    }

    pub fn with_pipelines(
        manager: Arc<CollectionManager>,
        cors_config: CorsConfig,
        security_config: SecurityConfig,
        pipeline_registry: PipelineRegistry,
    ) -> Self {
        // Initialize MCP components
        let session_manager = Arc::new(SessionManager::new());
        let mut tool_registry = ToolRegistry::new();
        register_basic_tools(&mut tool_registry);
        let tool_registry = Arc::new(tool_registry);
        let mcp_handler = Arc::new(McpHandler::new(tool_registry, manager.clone()));

        // Create watch channel for config reload notifications
        let (security_tx, _security_watch_rx) = watch::channel(security_config.clone());
        let security_config = Arc::new(RwLock::new(security_config));

        let config_reloader = ConfigReloader {
            security_tx,
            security_config: security_config.clone(),
        };

        Self {
            manager,
            session_manager,
            mcp_handler,
            cors_config,
            security_config,
            config_reloader,
            pipeline_registry: Arc::new(pipeline_registry),
            metrics_handle: None,
            ilm_manager: None,
            template_manager: None,
            data_dir: None,
        }
    }

    /// Set the ILM manager for lifecycle management
    pub fn with_ilm(mut self, ilm_manager: Arc<crate::ilm::IlmManager>) -> Self {
        self.ilm_manager = Some(ilm_manager);
        self
    }

    /// Set the template manager for index templates
    pub fn with_templates(
        mut self,
        template_manager: Arc<crate::templates::TemplateManager>,
    ) -> Self {
        self.template_manager = Some(template_manager);
        self
    }

    /// Set the data directory for export/import operations
    pub fn with_data_dir(mut self, data_dir: impl Into<std::path::PathBuf>) -> Self {
        self.data_dir = Some(data_dir.into());
        self
    }

    /// Get a handle for reloading configuration at runtime
    pub fn config_reloader(&self) -> ConfigReloader {
        self.config_reloader.clone()
    }

    /// Get a reference to the collection manager for cluster integration
    pub fn manager(&self) -> Arc<CollectionManager> {
        self.manager.clone()
    }

    /// Build router with additional routes merged in
    /// Used for integrating optional features like ES-compat
    pub async fn router_with_extension(&self, extension: Router) -> Router {
        self.router().await.merge(extension)
    }

    /// Serve with additional routes merged in
    pub async fn serve_with_extension(
        self,
        addr: &str,
        tls_config: Option<&TlsConfig>,
        extension: Router,
    ) -> Result<()> {
        let router = self.router().await.merge(extension);
        self.serve_router(router, addr, tls_config).await
    }

    /// Internal serve implementation that takes a pre-built router
    async fn serve_router(
        self,
        router: Router,
        addr: &str,
        tls_config: Option<&TlsConfig>,
    ) -> Result<()> {
        let tls_enabled = tls_config.is_some_and(|t| t.enabled);

        if tls_enabled {
            let tls = tls_config.unwrap();
            let rustls_config = load_rustls_config(tls)?;
            let acceptor = TlsAcceptor::from(Arc::new(rustls_config));

            // Spawn HTTP listener
            let http_router = router.clone();
            let http_addr = addr.to_string();
            let http_handle = tokio::spawn(async move {
                let listener = tokio::net::TcpListener::bind(&http_addr).await?;
                tracing::info!("HTTP server listening on {}", http_addr);
                axum::serve(listener, http_router)
                    .await
                    .map_err(|e| crate::Error::Backend(e.to_string()))
            });

            // Run TLS accept loop
            let tls_addr = tls.bind_addr.clone();
            let tls_handle = tokio::spawn(async move {
                let listener = tokio::net::TcpListener::bind(&tls_addr).await?;
                tracing::info!("HTTPS server listening on {}", tls_addr);

                loop {
                    let (tcp_stream, peer_addr) = listener.accept().await?;
                    let acceptor = acceptor.clone();
                    let svc = router.clone();

                    tokio::spawn(async move {
                        let tls_stream = match acceptor.accept(tcp_stream).await {
                            Ok(s) => s,
                            Err(e) => {
                                tracing::debug!("TLS handshake failed from {}: {}", peer_addr, e);
                                return;
                            }
                        };

                        let io = hyper_util::rt::TokioIo::new(tls_stream);
                        let hyper_svc = hyper_util::service::TowerToHyperService::new(svc);
                        let builder = hyper_util::server::conn::auto::Builder::new(
                            hyper_util::rt::TokioExecutor::new(),
                        );

                        if let Err(e) = builder.serve_connection(io, hyper_svc).await {
                            tracing::debug!("HTTPS connection error from {}: {}", peer_addr, e);
                        }
                    });
                }

                #[allow(unreachable_code)]
                Ok::<(), crate::Error>(())
            });

            tokio::select! {
                res = http_handle => {
                    res.map_err(|e| crate::Error::Backend(format!("HTTP task panicked: {}", e)))??;
                }
                res = tls_handle => {
                    res.map_err(|e| crate::Error::Backend(format!("HTTPS task panicked: {}", e)))??;
                }
            }
        } else {
            let listener = tokio::net::TcpListener::bind(addr).await?;
            tracing::info!("Server listening on {}", addr);
            axum::serve(listener, router)
                .await
                .map_err(|e| crate::Error::Backend(e.to_string()))?;
        }

        Ok(())
    }

    pub fn with_metrics(
        mut self,
        handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
    ) -> Self {
        self.metrics_handle = handle;
        self
    }

    /// GET /sse - SSE stream for MCP
    async fn sse_handler(
        State(state): State<AppState>,
        headers: axum::http::HeaderMap,
    ) -> std::result::Result<
        Sse<impl Stream<Item = std::result::Result<Event, Infallible>>>,
        axum::http::StatusCode,
    > {
        let session_id = headers
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let (id, mut rx) = state
            .session_manager
            .get_or_create(session_id)
            .await
            .map_err(|e| {
                tracing::error!("Failed to get or create MCP session: {:?}", e);
                axum::http::StatusCode::SERVICE_UNAVAILABLE
            })?;

        let stream = async_stream::stream! {
            // Send session ID as first event
            yield std::result::Result::<Event, Infallible>::Ok(Event::default().event("session").data(&id));

            // Listen for events
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        yield std::result::Result::<Event, Infallible>::Ok(Event::default()
                            .event(&event.event_type)
                            .data(&event.data));
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("SSE client lagged, dropped {} messages", n);
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        };

        Ok(Sse::new(stream).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(30))
                .text("ping"),
        ))
    }

    /// POST /sse - JSON-RPC requests
    async fn sse_post_handler(
        State(state): State<AppState>,
        headers: axum::http::HeaderMap,
        axum::Json(req): axum::Json<JsonRpcRequest>,
    ) -> axum::Json<JsonRpcResponse> {
        let response = state.mcp_handler.handle(req).await;

        // Optionally broadcast to session
        if let Some(session_id) = headers.get("Mcp-Session-Id").and_then(|v| v.to_str().ok()) {
            let event = SseEvent {
                event_type: "response".to_string(),
                data: serde_json::to_string(&response).unwrap_or_else(|e| {
                    tracing::error!("Failed to serialize MCP response: {}", e);
                    "{}".to_string()
                }),
            };
            state.session_manager.broadcast(session_id, event).await;
        }

        axum::Json(response)
    }

    /// Build CORS layer from configuration
    fn build_cors_layer(&self) -> CorsLayer {
        if !self.cors_config.enabled {
            return CorsLayer::new();
        }

        let origins: Vec<HeaderValue> = self
            .cors_config
            .origins
            .iter()
            .filter_map(|o| {
                if o == "*" {
                    // Wildcard handled separately
                    None
                } else {
                    o.parse().ok()
                }
            })
            .collect();

        // Check if wildcard is specified
        let has_wildcard = self.cors_config.origins.iter().any(|o| o == "*");

        let cors = if has_wildcard {
            CorsLayer::new().allow_origin(tower_http::cors::Any)
        } else if origins.is_empty() {
            CorsLayer::new()
        } else {
            CorsLayer::new().allow_origin(origins)
        };

        cors.allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers(tower_http::cors::Any)
    }

    async fn metrics_handler(State(state): State<AppState>) -> impl axum::response::IntoResponse {
        match &state.metrics_handle {
            Some(handle) => (
                StatusCode::OK,
                [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
                handle.render(),
            ),
            None => (
                StatusCode::NOT_FOUND,
                [("content-type", "text/plain; charset=utf-8")],
                "Metrics not enabled".to_string(),
            ),
        }
    }

    pub async fn router(&self) -> Router {
        let security_config = self.security_config.read().await.clone();

        let app_state = AppState {
            manager: self.manager.clone(),
            session_manager: self.session_manager.clone(),
            mcp_handler: self.mcp_handler.clone(),
            pipeline_registry: self.pipeline_registry.clone(),
            metrics_handle: self.metrics_handle.clone(),
            security_config: self.security_config.clone(),
            ilm_manager: self.ilm_manager.clone(),
        };

        // ILM state for ILM routes
        let ilm_state = crate::api::routes::IlmAppState {
            manager: self.manager.clone(),
            ilm_manager: self.ilm_manager.clone(),
        };

        // Pipeline-aware routes that need AppState
        let pipeline_routes = Router::new()
            .route(
                "/collections/:collection/documents",
                post(crate::api::routes::index_documents),
            )
            .route("/admin/pipelines", get(crate::api::routes::list_pipelines))
            .route("/metrics", get(Self::metrics_handler))
            .with_state(app_state.clone());

        // Routes that use Arc<CollectionManager>
        let legacy_routes = Router::new()
            .route("/api/search", post(crate::api::routes::simple_search))
            .route(
                "/collections/:collection/search",
                post(crate::api::routes::search),
            )
            .route(
                "/collections/:collection/documents/:id",
                get(crate::api::routes::get_document),
            )
            // Collection metadata API (Issue #21)
            .route(
                "/collections/:collection/schema",
                get(crate::api::routes::get_collection_schema),
            )
            .route(
                "/collections/:collection/stats",
                get(crate::api::routes::get_collection_stats),
            )
            .route(
                "/admin/collections",
                get(crate::api::routes::list_collections),
            )
            .route("/admin/lint-schemas", get(crate::api::routes::lint_schemas))
            .route("/health", get(crate::api::routes::health))
            // Stats API (Issue #22)
            .route("/stats/cache", get(crate::api::routes::get_cache_stats))
            .route("/stats/server", get(crate::api::routes::get_server_info))
            // Aggregations API (Issue #23)
            .route(
                "/collections/:collection/aggregate",
                post(crate::api::routes::aggregate),
            )
            // Index Inspection API (Issue #24)
            .route(
                "/collections/:collection/terms/:field",
                get(crate::api::routes::get_top_terms),
            )
            .route(
                "/collections/:collection/segments",
                get(crate::api::routes::get_segments),
            )
            .route(
                "/collections/:collection/doc/:id/reconstruct",
                get(crate::api::routes::reconstruct_document),
            )
            // Suggestions / Autocomplete API (Issue #47)
            .route(
                "/collections/:collection/_suggest",
                post(crate::api::routes::suggest),
            )
            // More Like This API (Issue #48)
            .route(
                "/collections/:collection/_mlt",
                post(crate::api::routes::more_like_this),
            )
            // Multi-Collection Search API (Issue #74)
            .route("/_msearch", post(crate::api::routes::multi_search))
            .route(
                "/:collections/_search",
                post(crate::api::routes::multi_index_search),
            )
            // Lucene-style query DSL
            .route(
                "/search/lucene",
                post(crate::api::search_lucene::search_lucene),
            )
            // Mnemos-compatible API routes
            .route(
                "/api/session/init",
                post(crate::api::mnemos_compat::session_init),
            )
            .route("/api/context", post(crate::api::mnemos_compat::context))
            .with_state(self.manager.clone());

        // MCP SSE routes that use AppState
        let mcp_routes = Router::new()
            .route("/sse", get(Self::sse_handler).post(Self::sse_post_handler))
            .with_state(app_state);

        // ILM routes (Issue #45)
        let ilm_routes = Router::new()
            // Policy management
            .route("/_ilm/policy", get(crate::api::routes::list_ilm_policies))
            .route(
                "/_ilm/policy/:name",
                get(crate::api::routes::get_ilm_policy)
                    .put(crate::api::routes::create_ilm_policy)
                    .delete(crate::api::routes::delete_ilm_policy),
            )
            // Index management
            .route("/_ilm/status", get(crate::api::routes::get_ilm_status))
            .route(
                "/:collection/_ilm/explain",
                get(crate::api::routes::ilm_explain),
            )
            .route("/:index/_rollover", post(crate::api::routes::ilm_rollover))
            .route(
                "/:collection/_ilm/move/:phase",
                post(crate::api::routes::ilm_move_phase),
            )
            .route(
                "/:collection/_ilm/attach",
                post(crate::api::routes::ilm_attach_policy),
            )
            // Alias management
            .route(
                "/_aliases",
                get(crate::api::routes::list_aliases).put(crate::api::routes::update_aliases),
            )
            .with_state(ilm_state);

        // Template state for template routes (Issue #51)
        let template_state = crate::api::routes::TemplateAppState {
            manager: self.manager.clone(),
            template_manager: self.template_manager.clone(),
        };

        // Template routes (Issue #51)
        let template_routes = Router::new()
            .route("/_template", get(crate::api::routes::list_templates))
            .route(
                "/_template/:name",
                get(crate::api::routes::get_template)
                    .put(crate::api::routes::put_template)
                    .delete(crate::api::routes::delete_template),
            )
            .route(
                "/_template/_simulate/:index",
                get(crate::api::routes::simulate_template),
            )
            .with_state(template_state);

        // Export routes (Issue #75 - encrypted export API)
        let export_routes = if let Some(ref data_dir) = self.data_dir {
            let export_state = crate::api::routes::ExportAppState {
                manager: self.manager.clone(),
                data_dir: data_dir.clone(),
            };
            Router::new()
                .route(
                    "/_admin/export/encrypted",
                    post(crate::api::routes::encrypted_export),
                )
                .route(
                    "/_admin/import/encrypted",
                    post(crate::api::routes::encrypted_import),
                )
                .route(
                    "/_admin/encryption/generate-key",
                    post(crate::api::routes::generate_encryption_key),
                )
                .with_state(export_state)
        } else {
            Router::new()
        };

        // CORS configuration from config file
        // Dashboard runs on different port (e.g., localhost:5173)
        let cors = self.build_cors_layer();

        // Merge routers
        let mut app = Router::new()
            .merge(legacy_routes)
            .merge(pipeline_routes)
            .merge(mcp_routes)
            .merge(ilm_routes)
            .merge(template_routes)
            .merge(export_routes)
            .layer(cors)
            .layer(axum::middleware::from_fn(metrics_middleware))
            .layer(TraceLayer::new_for_http());

        // Add audit middleware (independent of security.enabled)
        if security_config.audit.enabled {
            let mgr = self.manager.clone();
            let index = security_config.audit.index_to_collection;
            app = app.layer(axum::middleware::from_fn(move |req, next| {
                crate::security::audit::audit_middleware(mgr.clone(), index, req, next)
            }));
        }

        // Add auth middleware (only when security.enabled)
        // Added last so it runs first (tower layers are LIFO)
        // Uses dynamic config from AppState for hot-reload support
        if security_config.enabled {
            let security_config_arc = self.security_config.clone();
            app = app.layer(axum::middleware::from_fn(move |req, next| {
                let config = security_config_arc.clone();
                async move {
                    let security_config = config.read().await;
                    let checker = PermissionChecker::new(&security_config);
                    crate::security::middleware::auth_middleware_dynamic(checker, req, next).await
                }
            }));
        }

        app
    }

    pub async fn serve(self, addr: &str, tls_config: Option<&TlsConfig>) -> Result<()> {
        let router = self.router().await;
        self.serve_router(router, addr, tls_config).await
    }
}

fn load_rustls_config(tls: &TlsConfig) -> crate::Result<rustls::ServerConfig> {
    let cert_file = std::fs::File::open(&tls.cert_path).map_err(|e| {
        crate::Error::Config(format!(
            "Cannot open TLS cert '{}': {}. Run bin/generate-cert.sh to create a self-signed certificate.",
            tls.cert_path.display(), e
        ))
    })?;
    let key_file = std::fs::File::open(&tls.key_path).map_err(|e| {
        crate::Error::Config(format!(
            "Cannot open TLS key '{}': {}. Run bin/generate-cert.sh to create a self-signed certificate.",
            tls.key_path.display(), e
        ))
    })?;

    let certs: Vec<_> = rustls_pemfile::certs(&mut std::io::BufReader::new(cert_file))
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| crate::Error::Config(format!("Failed to parse TLS certs: {}", e)))?;

    let key = rustls_pemfile::private_key(&mut std::io::BufReader::new(key_file))
        .map_err(|e| crate::Error::Config(format!("Failed to parse TLS key: {}", e)))?
        .ok_or_else(|| crate::Error::Config("No private key found in key file".into()))?;

    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| crate::Error::Config(format!("Invalid TLS protocol config: {}", e)))?
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .map_err(|e| crate::Error::Config(format!("Invalid TLS config: {}", e)))?;

    Ok(config)
}
