use crate::collection::CollectionManager;
use crate::config::{CorsConfig, SecurityConfig, TlsConfig};
use crate::pipeline::registry::PipelineRegistry;
use crate::security::permissions::PermissionChecker;
use crate::Result;
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
use tokio::sync::broadcast;
use tokio_rustls::TlsAcceptor;
use axum::http::StatusCode;
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
}

pub struct ApiServer {
    manager: Arc<CollectionManager>,
    session_manager: Arc<SessionManager>,
    mcp_handler: Arc<McpHandler>,
    cors_config: CorsConfig,
    security_config: SecurityConfig,
    pipeline_registry: Arc<PipelineRegistry>,
    metrics_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
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
        Self::with_pipelines(manager, cors_config, security_config, PipelineRegistry::empty())
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

        Self {
            manager,
            session_manager,
            mcp_handler,
            cors_config,
            security_config,
            pipeline_registry: Arc::new(pipeline_registry),
            metrics_handle: None,
        }
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

    async fn metrics_handler(
        State(state): State<AppState>,
    ) -> impl axum::response::IntoResponse {
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

    pub fn router(&self) -> Router {
        let app_state = AppState {
            manager: self.manager.clone(),
            session_manager: self.session_manager.clone(),
            mcp_handler: self.mcp_handler.clone(),
            pipeline_registry: self.pipeline_registry.clone(),
            metrics_handle: self.metrics_handle.clone(),
        };

        // Pipeline-aware routes that need AppState
        let pipeline_routes = Router::new()
            .route(
                "/collections/:collection/documents",
                post(crate::api::routes::index_documents),
            )
            .route(
                "/admin/pipelines",
                get(crate::api::routes::list_pipelines),
            )
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

        // CORS configuration from config file
        // Dashboard runs on different port (e.g., localhost:5173)
        let cors = self.build_cors_layer();

        // Merge routers
        let mut app = Router::new()
            .merge(legacy_routes)
            .merge(pipeline_routes)
            .merge(mcp_routes)
            .layer(cors)
            .layer(axum::middleware::from_fn(metrics_middleware))
            .layer(TraceLayer::new_for_http());

        // Add audit middleware (independent of security.enabled)
        if self.security_config.audit.enabled {
            let mgr = self.manager.clone();
            let index = self.security_config.audit.index_to_collection;
            app = app.layer(axum::middleware::from_fn(move |req, next| {
                crate::security::audit::audit_middleware(mgr.clone(), index, req, next)
            }));
        }

        // Add auth middleware (only when security.enabled)
        // Added last so it runs first (tower layers are LIFO)
        if self.security_config.enabled {
            let checker = Arc::new(PermissionChecker::new(&self.security_config));
            app = app.layer(axum::middleware::from_fn(move |req, next| {
                crate::security::middleware::auth_middleware(checker.clone(), req, next)
            }));
        }

        app
    }

    pub async fn serve(self, addr: &str, tls_config: Option<&TlsConfig>) -> Result<()> {
        let router = self.router();

        let tls_enabled = tls_config.map_or(false, |t| t.enabled);

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
                        let hyper_svc =
                            hyper_util::service::TowerToHyperService::new(svc);
                        let builder = hyper_util::server::conn::auto::Builder::new(
                            hyper_util::rt::TokioExecutor::new(),
                        );

                        if let Err(e) = builder
                            .serve_connection(io, hyper_svc)
                            .await
                        {
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
