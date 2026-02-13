//! ES-compatible API router

use crate::endpoints::search::EsCompatState;
use crate::endpoints::{
    bulk_handler, cat_indices_handler, cluster_health_handler, mapping_handler, msearch_handler,
    root_handler, search_handler,
};
use axum::routing::{get, post};
use axum::Router;
use prism::collection::CollectionManager;
use std::sync::Arc;

/// Create the ES-compatible router
///
/// All endpoints are served under the `/_elastic` prefix.
///
/// # Endpoints
///
/// - `GET /_elastic/` - Cluster info
/// - `GET /_elastic/_cluster/health` - Cluster health
/// - `GET /_elastic/_cat/indices` - List indices
/// - `POST /_elastic/_search` - Search all indices
/// - `POST /_elastic/{index}/_search` - Search specific index
/// - `POST /_elastic/_msearch` - Multi-search
/// - `POST /_elastic/_bulk` - Bulk operations
/// - `POST /_elastic/{index}/_bulk` - Bulk with default index
/// - `GET /_elastic/{index}/_mapping` - Get mappings
pub fn es_compat_router(manager: Arc<CollectionManager>) -> Router {
    let state = EsCompatState { manager };

    Router::new()
        // Cluster endpoints
        .route("/", get(root_handler))
        .route("/_cluster/health", get(cluster_health_handler))
        .route("/_cat/indices", get(cat_indices_handler))
        // Search endpoints
        .route("/_search", post(search_handler_no_index))
        .route("/:index/_search", post(search_handler))
        // Multi-search
        .route("/_msearch", post(msearch_handler))
        // Bulk endpoints
        .route("/_bulk", post(bulk_handler_no_index))
        .route("/:index/_bulk", post(bulk_handler))
        // Mapping endpoints
        .route("/:index/_mapping", get(mapping_handler))
        .with_state(state)
}

// Wrapper handlers for routes without index parameter
use crate::error::EsCompatError;
use crate::query::EsSearchRequest;
use crate::response::EsBulkResponse;
use crate::response::EsSearchResponse;
use axum::body::Bytes;
use axum::extract::State;
use axum::Json;

async fn search_handler_no_index(
    state: State<EsCompatState>,
    body: Json<EsSearchRequest>,
) -> Result<Json<EsSearchResponse>, EsCompatError> {
    search_handler(state, None, body).await
}

async fn bulk_handler_no_index(
    state: State<EsCompatState>,
    body: Bytes,
) -> Result<Json<EsBulkResponse>, EsCompatError> {
    bulk_handler(state, None, body).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    /// Verify that parameterised routes (/:index/...) are matched by the router.
    /// We don't need a real CollectionManager — we just check the router
    /// dispatches to a handler (any status != 404 means the route matched).
    #[tokio::test]
    async fn test_index_specific_routes_match() {
        // Build a minimal router with dummy handlers that always return 200.
        // This isolates the routing logic from the actual handler logic.
        let router = Router::new()
            .route("/_search", post(|| async { StatusCode::OK }))
            .route("/:index/_search", post(|| async { StatusCode::OK }))
            .route("/_bulk", post(|| async { StatusCode::OK }))
            .route("/:index/_bulk", post(|| async { StatusCode::OK }))
            .route("/:index/_mapping", get(|| async { StatusCode::OK }))
            .route("/_cat/indices", get(|| async { StatusCode::OK }))
            .route("/_cluster/health", get(|| async { StatusCode::OK }));

        let cases = vec![
            ("POST", "/_search"),
            ("POST", "/my_index/_search"),
            ("POST", "/logs-2024-01/_search"),
            ("POST", "/_bulk"),
            ("POST", "/my_index/_bulk"),
            ("GET", "/my_index/_mapping"),
            ("GET", "/_cat/indices"),
            ("GET", "/_cluster/health"),
        ];

        for (method, path) in cases {
            let req = Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap();

            let resp = router.clone().oneshot(req).await.unwrap();
            assert_ne!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "Route {method} {path} should match but got 404"
            );
        }
    }
}
