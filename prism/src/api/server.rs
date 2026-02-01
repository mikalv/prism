use crate::collection::CollectionManager;
use crate::config::CorsConfig;
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
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::mcp::handler::{JsonRpcRequest, JsonRpcResponse, McpHandler};
use crate::mcp::session::{SessionManager, SseEvent};
use crate::mcp::tools::register_basic_tools;
use crate::mcp::ToolRegistry;

#[derive(Clone)]
pub struct AppState {
    pub manager: Arc<CollectionManager>,
    pub session_manager: Arc<SessionManager>,
    pub mcp_handler: Arc<McpHandler>,
}

pub struct ApiServer {
    manager: Arc<CollectionManager>,
    session_manager: Arc<SessionManager>,
    mcp_handler: Arc<McpHandler>,
    cors_config: CorsConfig,
}

impl ApiServer {
    pub fn new(manager: Arc<CollectionManager>) -> Self {
        Self::with_cors(manager, CorsConfig::default())
    }

    pub fn with_cors(manager: Arc<CollectionManager>, cors_config: CorsConfig) -> Self {
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
        }
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

    pub fn router(&self) -> Router {
        let app_state = AppState {
            manager: self.manager.clone(),
            session_manager: self.session_manager.clone(),
            mcp_handler: self.mcp_handler.clone(),
        };

        // Routes that use Arc<CollectionManager>
        let legacy_routes = Router::new()
            .route("/api/search", post(crate::api::routes::simple_search))
            .route(
                "/collections/:collection/search",
                post(crate::api::routes::search),
            )
            .route(
                "/collections/:collection/documents",
                post(crate::api::routes::index_documents),
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
            .route("/api/search", post(crate::api::mnemos_compat::search))
            .with_state(self.manager.clone());

        // MCP SSE routes that use AppState
        let mcp_routes = Router::new()
            .route("/sse", get(Self::sse_handler).post(Self::sse_post_handler))
            .with_state(app_state);

        // CORS configuration from config file
        // Dashboard runs on different port (e.g., localhost:5173)
        let cors = self.build_cors_layer();

        // Merge routers
        Router::new()
            .merge(legacy_routes)
            .merge(mcp_routes)
            .layer(cors)
            .layer(TraceLayer::new_for_http())
    }

    pub async fn serve(self, addr: &str) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("Server listening on {}", addr);

        axum::serve(listener, self.router())
            .await
            .map_err(|e| crate::Error::Backend(e.to_string()))?;

        Ok(())
    }
}
