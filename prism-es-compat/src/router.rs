//! ES-compatible API router

use crate::endpoints::search::EsCompatState;
use axum::response::IntoResponse;
use crate::endpoints::{
    bulk_handler, cat_aliases_handler, cat_indices_handler, cat_nodes_handler, cat_shards_handler, cluster_health_handler, cluster_health_index_handler, cluster_settings_handler, cluster_state_handler, create_pit_handler, delete_pit_handler, get_index_handler,
    update_aliases_handler, xpack_handler,
    xpack_usage_handler,
    delete_component_template_handler, delete_index_template_handler,
    get_all_index_templates_handler, get_component_template_handler, get_index_template_handler, head_index_template_handler,
    put_component_template_handler, put_index_template_handler,
    license_handler, nodes_handler, nodes_stats_handler,
    count_handler, delete_doc_handler, get_doc_handler, get_search_handler, head_doc_handler, head_index_handler, mapping_handler, put_mapping_handler,
    msearch_handler, post_doc_handler, create_doc_handler, put_doc_handler, put_index_handler, root_handler, search_handler,
    delete_by_query_handler, get_task_handler, update_by_query_handler,
};
use axum::routing::{get, post};
use axum::Router;
use prism::collection::CollectionManager;
use std::sync::Arc;
use std::path::PathBuf;
use std::collections::HashMap;

/// Create the ES-compatible router
///
/// All endpoints are served at their standard Elasticsearch paths (no prefix).
///
/// # Endpoints
///
/// - `GET /` - Cluster info
/// - `GET /_cluster/health` - Cluster health
/// - `GET /_cluster/state` - Cluster state
/// - `GET /_cluster/settings` - Cluster settings
/// - `GET /_license` - License info
/// - `GET /_nodes` - Nodes info
/// - `GET /_nodes/stats` - Nodes stats
/// - `GET /_nodes/:node_id/:metric` - Specific node metric
/// - `GET /_nodes/stats/:node_id/:metric` - Specific node stats metric
/// - `GET /_cat/indices` - List indices
/// - `GET /_cat/aliases` - List aliases
/// - `GET /_cat/shards` - List shards
/// - `GET /_cat/nodes` - List nodes
/// - `GET|POST /_search` - Search all indices
/// - `GET|POST /{index}/_search` - Search specific index
/// - `POST /_msearch` - Multi-search
/// - `POST /_bulk` - Bulk operations
/// - `POST /{index}/_bulk` - Bulk with default index
/// - `GET /{index}/_mapping` - Get mappings
/// - `GET /{index}/_doc/{id}` - Get document
/// - `POST /{index}/_doc` - Index document (auto ID)
/// - `PUT /{index}/_doc/{id}` - Index document (explicit ID)
/// - `DELETE /{index}/_doc/{id}` - Delete document
/// - `HEAD /{index}/_doc/{id}` - Check document exists
/// - `GET /{index}` - Get index info
/// - `HEAD /{index}` - Check index exists
/// - `GET /{index}/_count` - Count documents
pub fn es_compat_router(manager: Arc<CollectionManager>, data_dir: PathBuf) -> Router {
    // ES-compat metadata lives under {data_dir}/es-compat/ so aliases and
    // index templates survive prism restarts.
    let es_compat_dir = crate::persist::es_compat_dir(&data_dir);
    crate::persist::ensure_dir(&es_compat_dir);

    // Replay persisted aliases into the manager so expand_collection_patterns
    // resolves them immediately (e.g. `.kibana_task_manager` -> concrete index).
    if let Some(aliases) = crate::persist::load_json::<HashMap<String, Vec<String>>>(
        &es_compat_dir,
        "aliases",
    ) {
        let n = aliases.len();
        for (alias, indices) in &aliases {
            manager.add_alias(alias, indices);
        }
        tracing::info!("Loaded {n} persisted ES-compat aliases");
    }

    let templates = crate::endpoints::templates::TemplateStore::load_from(&es_compat_dir);
    let state = EsCompatState {
        manager,
        templates,
        ilm: crate::endpoints::ilm::IlmStore::new(),
        data_dir: es_compat_dir,
    };
    let ilm_state = state.clone();

    Router::new()
        // Root
        .route("/", get(root_handler))
        // Cluster endpoints
        .route("/_cluster/health", get(cluster_health_handler))
        .route("/_cluster/health/:index", get(cluster_health_index_handler))
        .route("/_cluster/state", get(cluster_state_handler))
        .route("/_cluster/settings", get(cluster_settings_handler))
        .route("/_license", get(license_handler))
        // X-Pack info (license mode/features) — Kibana licensing plugin
        .route("/_xpack", get(xpack_handler))
        .route("/_xpack/_usage", get(xpack_usage_handler))
        .route("/_nodes", get(nodes_handler))
        .route("/_nodes/stats", get(nodes_stats_handler))
        .route("/_nodes/:node_id/:metric", get(nodes_handler))
        .route("/_nodes/stats/:node_id/:metric", get(nodes_stats_handler))
        .route("/_cat/indices", get(cat_indices_handler))
        .route("/_cat/aliases", get(cat_aliases_handler))
        // Alias management (used by Kibana saved-objects migration final step)
        .route("/_aliases", post(update_aliases_handler))
        // Index templates (composable + component) — stored as metadata;
        // Kibana plugins register these at startup. NOTE: the legacy
        // `/_template/{name}` API is intentionally omitted: axum 0.7's matchit
        // trie cannot co-register `_template/:name` with the existing
        // `_tasks/:task_id` (shared `_t` static prefix). Kibana 9.x uses the
        // composable `/_index_template` API, so this is sufficient.
        .route(
            "/_index_template",
            get(get_all_index_templates_handler),
        )
        .route(
            "/_index_template/:name",
            get(get_index_template_handler)
                .put(put_index_template_handler)
                .head(head_index_template_handler)
                .delete(delete_index_template_handler),
        )
        .route(
            "/_component_template/:name",
            get(get_component_template_handler)
                .put(put_component_template_handler)
                .delete(delete_component_template_handler),
        )
        // ILM policies + data streams (ES-compat shims) are served via the
        // fallback handler (`es_fallback` -> `es_compat_fallback`), NOT via
        // registered routes: matchit 0.7 panics on `/_ilm/policy/:name`.
        .route("/_cat/shards", get(cat_shards_handler))
        .route("/_cat/nodes", get(cat_nodes_handler))
        // Search endpoints
        .route("/_search", post(search_handler_no_index))
        .route(
            "/:index/_search",
            get(get_search_handler).post(search_handler),
        )
        // Point-In-Time endpoints (stateless pseudo-PIT)
        .route("/:index/_pit", post(create_pit_handler))
        .route("/_pit", axum::routing::delete(delete_pit_handler))
        // Multi-search
        .route("/_msearch", post(msearch_handler))
        // Bulk endpoints
        .route("/_bulk", post(bulk_handler_no_index))
        .route("/:index/_bulk", post(bulk_handler))
        // Mapping endpoints
        .route("/:index/_mapping", get(mapping_handler).put(put_mapping_handler))
        // Bulk-by-query + async task endpoints (used by Kibana saved-objects migration)
        .route("/:index/_update_by_query", post(update_by_query_handler))
        .route("/:index/_delete_by_query", post(delete_by_query_handler))
        .route("/_tasks/:task_id", get(get_task_handler))
        // Document CRUD endpoints
        .route("/:index/_doc", post(post_doc_handler))
        // ES `client.create()` sends PUT `/{index}/_create/{id}` (not POST).
        // Accept both PUT and POST so the route never 405s on create ops.
        .route(
            "/:index/_create/:id",
            post(create_doc_handler).put(create_doc_handler),
        )
        .route(
            "/:index/_doc/:id",
            get(get_doc_handler)
                .put(put_doc_handler)
                .delete(delete_doc_handler)
                .head(head_doc_handler),
        )
        // Index-level endpoints
        .route("/:index", get(get_index_handler).head(head_index_handler).put(put_index_handler))
        .route("/:index/_count", get(count_handler))
        // Fallback so that unmatched paths still return a clean 404 (and
        // dispatch ILM / data-stream paths). Must come BEFORE `.with_state`
        // because the fallback handler takes `State<EsCompatState>`.
        .fallback(es_not_found)
        .with_state(state)
        // ILM + data-stream interception BEFORE routing. matchit 0.7 mis-routes
        // `/_ilm/policy/{name}` (the literal segment "policy" collides in the
        // trie), so intercept at the middleware layer and dispatch via
        // `es_compat_fallback`, never touching the matchit router.
        .layer(axum::middleware::from_fn_with_state(ilm_state, ilm_ds_interceptor))
        // Official ES clients verify this header on every response.
        .layer(axum::middleware::map_response(add_elastic_product_header))
        // Log every 4xx/5xx request so missing/broken ES endpoints surface
        // in the prism server log instead of being silently retried by
        // clients like Kibana.
        .layer(axum::middleware::from_fn(log_es_failures))
}

/// Fallback for unmatched paths. ILM + data-stream paths are dispatched
/// here (matchit 0.7 panics on their `/_ilm/policy/:name` route shape);
/// everything else returns a clean 404. Kept inside the layered router so
/// `X-Elastic-Product` is still applied.
/// 404 for unmatched paths, kept inside the layered router so the
/// `X-Elastic-Product` header is still applied.
async fn es_not_found() -> axum::http::StatusCode {
    axum::http::StatusCode::NOT_FOUND
}

/// Middleware that intercepts ILM + data-stream paths BEFORE routing.
/// matchit 0.7 mis-routes `/_ilm/policy/{name}` (the literal segment
/// "policy" collides with something in the trie), so we short-circuit these
/// prefixes here and dispatch via `es_compat_fallback`, bypassing matchit.
async fn ilm_ds_interceptor(
    axum::extract::State(state): axum::extract::State<EsCompatState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let path = req.uri().path();
    if path == "/_ilm"
        || path.starts_with("/_ilm/")
        || path == "/_data_stream"
        || path.starts_with("/_data_stream/")
    {
        let method = req.method().clone();
        let path = path.to_string();
        let body = axum::body::to_bytes(req.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap_or_default();
        // Stamp the product header directly here. The outer `map_response`
        // header layer does NOT reliably process responses short-circuited by
        // this interceptor (verified empirically: ILM/data-stream responses
        // reached clients without `X-Elastic-Product`, tripping the ES JS
        // client's ProductCheck → `ProductNotSupportedError`). Stamping here
        // is bulletproof regardless of axum layer-composition subtleties.
        let mut resp = crate::endpoints::ilm::es_compat_fallback(&state, method, &path, &body)
            .await
            .unwrap_or_else(|| axum::http::StatusCode::NOT_FOUND.into_response());
        resp.headers_mut().insert(
            "X-Elastic-Product",
            axum::http::HeaderValue::from_static("Elasticsearch"),
        );
        return resp;
    }
    next.run(req).await
}

/// Log ES-compat requests that fail (status >= 400). Helps surface missing
/// or broken endpoints used by clients like Kibana.
async fn log_es_failures(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let resp = next.run(req).await;
    let status = resp.status();
    if status.is_client_error() || status.is_server_error() {
        tracing::warn!(%method, %path, %status, "es-compat request failed");
    }
    resp
}

/// Stamp `X-Elastic-Product: Elasticsearch` on every response so official
/// Elasticsearch clients (≥7.14) accept the server as genuine.
async fn add_elastic_product_header(
    mut response: axum::response::Response,
) -> axum::response::Response {
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
