//! ES-compatible API router

use crate::endpoints::search::EsCompatState;
use crate::endpoints::{
    bulk_handler, cat_indices_handler, cluster_health_handler, count_handler, delete_doc_handler,
    get_doc_handler, get_search_handler, head_doc_handler, head_index_handler, mapping_handler,
    msearch_handler, post_doc_handler, put_doc_handler, root_handler, search_handler,
};
use axum::routing::{get, head, post};
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
/// - `GET|POST /_elastic/{index}/_search` - Search specific index
/// - `POST /_elastic/_search` - Search all indices
/// - `POST /_elastic/_msearch` - Multi-search
/// - `POST /_elastic/_bulk` - Bulk operations
/// - `POST /_elastic/{index}/_bulk` - Bulk with default index
/// - `GET /_elastic/{index}/_mapping` - Get mappings
/// - `GET /_elastic/{index}/_doc/{id}` - Get document
/// - `POST /_elastic/{index}/_doc` - Index document (auto ID)
/// - `PUT /_elastic/{index}/_doc/{id}` - Index document (explicit ID)
/// - `DELETE /_elastic/{index}/_doc/{id}` - Delete document
/// - `HEAD /_elastic/{index}/_doc/{id}` - Check document exists
/// - `HEAD /_elastic/{index}` - Check index exists
/// - `GET /_elastic/{index}/_count` - Count documents
pub fn es_compat_router(manager: Arc<CollectionManager>) -> Router {
    let state = EsCompatState { manager };

    Router::new()
        // Cluster endpoints
        .route("/", get(root_handler))
        .route("/_cluster/health", get(cluster_health_handler))
        .route("/_cat/indices", get(cat_indices_handler))
        // Search endpoints
        .route("/_search", post(search_handler_no_index))
        .route(
            "/:index/_search",
            get(get_search_handler).post(search_handler),
        )
        // Multi-search
        .route("/_msearch", post(msearch_handler))
        // Bulk endpoints
        .route("/_bulk", post(bulk_handler_no_index))
        .route("/:index/_bulk", post(bulk_handler))
        // Mapping endpoints
        .route("/:index/_mapping", get(mapping_handler))
        // Document CRUD endpoints
        .route("/:index/_doc", post(post_doc_handler))
        .route(
            "/:index/_doc/:id",
            get(get_doc_handler)
                .put(put_doc_handler)
                .delete(delete_doc_handler)
                .head(head_doc_handler),
        )
        // Index-level endpoints
        .route("/:index", head(head_index_handler))
        .route("/:index/_count", get(count_handler))
        .with_state(state)
        // Fallback so that unmatched `/_elastic/*` paths (e.g. the trailing-slash
        // root `/_elastic/` that some clients probe) still return a clean 404
        // through the layer below, and thus still carry the product header.
        .fallback(elastic_not_found)
        // Official ES clients verify this header on every response.
        .layer(axum::middleware::map_response(
            add_elastic_product_header,
        ))
}

/// 404 for unmatched `/_elastic/*` paths, kept inside the layered router so the
/// `X-Elastic-Product` header is still applied.
async fn elastic_not_found() -> axum::http::StatusCode {
    axum::http::StatusCode::NOT_FOUND
}

/// Stamp `X-Elastic-Product: Elasticsearch` on every response so official
/// Elasticsearch clients (≥7.14) accept the server as genuine.
async fn add_elastic_product_header(mut response: axum::response::Response) -> axum::response::Response {
    response.headers_mut().insert(
        "X-Elastic-Product",
        axum::http::HeaderValue::from_static("Elasticsearch"),
    );
    response
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

    /// Official Elasticsearch clients (elasticsearch-py/js/java ≥7.14) refuse to
    /// operate unless every response carries `X-Elastic-Product: Elasticsearch`.
    /// The header must be present on success AND error responses.
    #[tokio::test]
    async fn test_x_elastic_product_header_present() {
        let ok = || async { StatusCode::OK };
        let err = || async { StatusCode::BAD_REQUEST };
        let router = Router::new()
            .route("/ok", get(ok))
            .route("/err", get(err))
            .layer(axum::middleware::map_response(add_elastic_product_header));

        for path in ["/ok", "/err"] {
            let req = Request::builder()
                .method("GET")
                .uri(path)
                .body(Body::empty())
                .unwrap();
            let resp = router.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.headers()
                    .get("X-Elastic-Product")
                    .and_then(|v| v.to_str().ok()),
                Some("Elasticsearch"),
                "X-Elastic-Product header missing on {path}"
            );
        }
    }

    /// Verify that parameterised routes (/:index/...) are matched by the router.
    /// We don't need a real CollectionManager — we just check the router
    /// dispatches to a handler (any status != 404 means the route matched).
    #[tokio::test]
    async fn test_index_specific_routes_match() {
        // Build a minimal router with dummy handlers that always return 200.
        // This isolates the routing logic from the actual handler logic.
        let ok = || async { StatusCode::OK };
        let router = Router::new()
            .route("/_search", post(ok))
            .route("/:index/_search", get(ok).post(ok))
            .route("/_bulk", post(ok))
            .route("/:index/_bulk", post(ok))
            .route("/:index/_mapping", get(ok))
            .route("/_cat/indices", get(ok))
            .route("/_cluster/health", get(ok))
            .route("/:index/_doc", post(ok))
            .route("/:index/_doc/:id", get(ok).put(ok).delete(ok).head(ok))
            .route("/:index", head(ok))
            .route("/:index/_count", get(ok));

        let cases = vec![
            ("POST", "/_search"),
            ("POST", "/my_index/_search"),
            ("GET", "/my_index/_search"),
            ("POST", "/logs-2024-01/_search"),
            ("POST", "/_bulk"),
            ("POST", "/my_index/_bulk"),
            ("GET", "/my_index/_mapping"),
            ("GET", "/_cat/indices"),
            ("GET", "/_cluster/health"),
            ("POST", "/my_index/_doc"),
            ("GET", "/my_index/_doc/1"),
            ("PUT", "/my_index/_doc/1"),
            ("DELETE", "/my_index/_doc/1"),
            ("HEAD", "/my_index/_doc/1"),
            ("HEAD", "/my_index"),
            ("GET", "/my_index/_count"),
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
