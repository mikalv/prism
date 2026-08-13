//! ILM policy + data-stream + xpack/usage stub endpoints.
//!
//! These are Kibana-compatibility shims. Prism has its own internal lifecycle
//! manager; this module lets clients that speak the ES ILM / data-stream APIs
//! (Kibana alerting, reporting, event-log, workflows) boot without 404s.
//! ILM policy bodies are stored verbatim so `GET` returns exactly what `PUT`
//! stored. Data-stream creation provisions a real backing collection so that
//! subsequent document writes to the stream resolve like any other index.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::extract::{Path, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::endpoints::search::EsCompatState;

/// Provision a real backing collection for a data stream so subsequent
/// `POST /{name}/_doc` writes resolve like a normal index.
async fn create_backing_collection(
    state: &EsCompatState,
    name: &str,
) -> Result<(), crate::error::EsCompatError> {
    if state.manager.get_schema(name).is_some() {
        return Ok(());
    }
    use prism::schema::{
        types::{FieldType, SystemFieldsConfig, TextField},
        Backends, CollectionSchema, IndexingConfig,
        QuotaConfig, TextBackendConfig,
    };
    use prism::storage::StorageConfig;
    // A text backend REQUIRES at least one field (add_collection rejects an
    // empty field list as a Schema error -> 400). Provide a catch-all
    // `message` field, matching put_index_handler's default.
    let fields = vec![TextField {
        name: "message".to_string(),
        field_type: FieldType::Text,
        stored: true,
        indexed: true,
        tokenizer: None,
        tokenizer_options: None,
    }];
    let schema = CollectionSchema {
        collection: name.to_string(),
        description: Some("Auto-created data stream backing index".to_string()),
        backends: Backends {
            text: Some(TextBackendConfig { fields, bm25_k1: None, bm25_b: None }),
            vector: None,
            graph: None,
        },
        indexing: IndexingConfig::default(),
        quota: QuotaConfig::default(),
        embedding_generation: None,
        facets: None,
        boosting: None,
        storage: StorageConfig::default(),
        system_fields: SystemFieldsConfig::default(),
        hybrid: None,
        replication: None,
        reranking: None,
        ilm_policy: None,
    };
    // Persist first so the backing index survives restarts (matches
    // put_index_handler), then register.
    state
        .manager
        .persist_schema(&schema)
        .map_err(|e| crate::error::EsCompatError::Internal(format!("persist_schema failed: {e}")))?;
    state.manager.add_collection(schema).await?;
    Ok(())
}

/// In-memory store for ES-compat ILM policies.
#[derive(Clone, Default)]
pub struct IlmStore {
    policies: Arc<RwLock<HashMap<String, Value>>>,
}

impl IlmStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put_policy(&self, name: &str, body: Value) {
        if let Ok(mut g) = self.policies.write() {
            g.insert(name.to_string(), body);
        }
    }

    pub fn get_policy(&self, name: &str) -> Option<Value> {
        self.policies.read().ok()?.get(name).cloned()
    }

    pub fn all_policies(&self) -> HashMap<String, Value> {
        self.policies.read().map(|g| g.clone()).unwrap_or_default()
    }
}

/// PUT /_ilm/policy/{name}
///
/// Store the policy verbatim. Acknowledge like ES.
pub async fn put_ilm_policy_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
    body: axum::body::Bytes,
) -> Json<Value> {
    // Parse leniently: keep the raw body if it's valid JSON, else store a
    // minimal placeholder. Never reject — Kibana treats ILM PUT failure as
    // a blocking error for alerting/reporting.
    let parsed: Value = serde_json::from_slice(&body).unwrap_or_else(|_| {
        json!({ "policy": { "phases": {} } })
    });
    state.ilm.put_policy(&name, parsed);
    Json(json!({ "acknowledged": true }))
}

/// GET /_ilm/policy/{name}
pub async fn get_ilm_policy_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
) -> Json<Value> {
    let policy = state.ilm.get_policy(&name).unwrap_or_else(|| {
        json!({ "version": 1, "modified_date": "2026-01-01T00:00:00.000Z", "policy": { "phases": {} } })
    });
    Json(json!({ name: policy }))
}

/// GET /_ilm/policy  (list all)
pub async fn list_ilm_policies_handler(
    State(state): State<EsCompatState>,
) -> Json<Value> {
    Json(json!(state.ilm.all_policies()))
}

/// GET /_xpack/_usage
///
/// Kibana's xpack usage collection polls this. Returning an empty object is
/// sufficient — the call just must not error. Previously this 404'd and
/// produced an unhandled promise rejection on every status poll.
pub async fn xpack_usage_handler() -> Json<Value> {
    Json(json!({}))
}

/// PUT /_data_stream/{name}
///
/// Provision a real backing collection so subsequent `POST /{name}/_doc`
/// writes resolve like a normal index. Returns `acknowledged: true`.
pub async fn create_data_stream_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, crate::error::EsCompatError> {
    // Auto-create a backing collection for the data stream.
    if state.manager.get_schema(&name).is_none() {
        use prism::schema::{
            types::SystemFieldsConfig, Backends, CollectionSchema, IndexingConfig,
            QuotaConfig, TextBackendConfig,
        };
        use prism::storage::StorageConfig;
        let schema = CollectionSchema {
            collection: name.clone(),
            description: Some("Auto-created data stream backing index".to_string()),
            backends: Backends {
                text: Some(TextBackendConfig {
                    fields: vec![],
                    bm25_k1: None,
                    bm25_b: None,
                }),
                vector: None,
                graph: None,
            },
            indexing: IndexingConfig::default(),
            quota: QuotaConfig::default(),
            embedding_generation: None,
            facets: None,
            boosting: None,
            storage: StorageConfig::default(),
            system_fields: SystemFieldsConfig::default(),
            hybrid: None,
            replication: None,
            reranking: None,
            ilm_policy: None,
        };
        state.manager.add_collection(schema).await?;
    }
    Ok(Json(json!({ "acknowledged": true })))
}

/// GET /_data_stream  (list)
pub async fn list_data_streams_handler(
    State(state): State<EsCompatState>,
) -> Json<Value> {
    let _ = state;
    Json(json!([]))
}

// ===================================================================
// Fallback dispatcher (bypasses matchit 0.7)
// ===================================================================
// matchit 0.7 panics on the `/_ilm/policy/:name` route shape (even though
// the identical `/_cluster/health/:index` works), and `/*rest` catch-alls
// match unreliably. So ILM + data-stream paths are served from the router's
// *fallback* handler instead, which never touches the matchit trie.

/// Dispatch ILM + data-stream requests. Returns `Some(response)` if the path
/// was handled, else `None` (caller returns 404).
///
/// - `GET  /_ilm/policy`           → list all policies
/// - `GET  /_ilm/policy/{name}`    → fetch one policy
/// - `PUT  /_ilm/policy/{name}`    → install a policy
/// - `PUT  /_data_stream/{name}`   → create (provisions a backing collection)
pub async fn es_compat_fallback(
    state: &EsCompatState,
    method: Method,
    path: &str,
    body: &[u8],
) -> Option<Response> {
    // ILM: /_ilm, /_ilm/policy, /_ilm/policy/{name}
    if path == "/_ilm" || path.starts_with("/_ilm/") {
        let rest = path.trim_start_matches("/_ilm").trim_start_matches('/');
        let resp: Value = if rest.is_empty() || rest == "policy" {
            json!(state.ilm.all_policies())
        } else if let Some(name) = rest.strip_prefix("policy/") {
            match method {
                Method::GET => {
                    let p = state
                        .ilm
                        .get_policy(name)
                        .unwrap_or_else(|| json!({ "version": 1, "policy": { "phases": {} } }));
                    json!({ name: p })
                }
                Method::PUT => {
                    let parsed: Value = serde_json::from_slice(body)
                        .unwrap_or_else(|_| json!({ "policy": { "phases": {} } }));
                    state.ilm.put_policy(name, parsed);
                    json!({ "acknowledged": true })
                }
                _ => json!({ "acknowledged": true }),
            }
        } else {
            json!({ "acknowledged": true })
        };
        return Some(Json(resp).into_response());
    }

    // Data streams: /_data_stream (list), /_data_stream/{name} (get/put/delete)
    if path == "/_data_stream" || path.starts_with("/_data_stream/") {
        let name = path.trim_start_matches("/_data_stream").trim_start_matches('/');

        // GET /_data_stream  → list. ES returns {"data_streams": [...]}.
        // (Kibana destructures `response.data_streams`, so a bare array crashes.)
        if name.is_empty() {
            return Some(Json(json!({ "data_streams": [] })).into_response());
        }

        match method {
            Method::GET => {
                // A data stream "exists" iff its backing collection exists.
                // If absent, return 404 — Kibana's getExistingDataStream
                // catches 404 and proceeds to create the DS + index template.
                // Returning 200-without-data_streams here is a FATAL crash.
                if state.manager.get_schema(name).is_some() {
                    return Some(Json(json!({
                        "data_streams": [ data_stream_object(name) ]
                    })).into_response());
                }
                return Some(StatusCode::NOT_FOUND.into_response());
            }
            Method::PUT => {
                if let Err(e) = create_backing_collection(state, name).await {
                    return Some(e.into_response());
                }
                return Some(Json(json!({ "acknowledged": true })).into_response());
            }
            Method::DELETE => {
                return Some(Json(json!({ "acknowledged": true })).into_response());
            }
            _ => {
                return Some(StatusCode::METHOD_NOT_ALLOWED.into_response());
            }
        }
    }

    None
}

/// Minimal ES data-stream object. Returned inside `{"data_streams": [...]}`
/// so Kibana's `getExistingDataStream` destructure succeeds.
fn data_stream_object(name: &str) -> Value {
    json!({
        "name": name,
        "timestamp_field": { "name": "@timestamp" },
        "indices": [],
        "generation": 1,
        "next_generation_id": 2,
        "status": "GREEN",
        "template": name,
        "ilm_policy": name,
        "prefer_ilm": true,
        "hidden": false,
        "system": false,
        "allow_auto_routing": true,
        "replicated": false,
        "metadata": {}
    })
}

/// GET|PUT /_ilm/*rest — dispatch ILM policy operations.
///
/// - `GET  /_ilm/policy`           → list all policies
/// - `GET  /_ilm/policy/{name}`    → fetch one policy
/// - `PUT  /_ilm/policy/{name}`    → install a policy
pub async fn ilm_dispatch(
    State(state): State<EsCompatState>,
    Path(rest): Path<String>,
    method: Method,
    body: axum::body::Bytes,
) -> Json<Value> {
    let rest = rest.trim_start_matches('/');
    if rest == "policy" || rest.is_empty() {
        return Json(json!(state.ilm.all_policies()));
    }
    if let Some(name) = rest.strip_prefix("policy/") {
        match method {
            Method::GET => {
                let policy = state.ilm.get_policy(name).unwrap_or_else(|| {
                    json!({ "version": 1, "modified_date": "2026-01-01T00:00:00.000Z", "policy": { "phases": {} } })
                });
                return Json(json!({ name: policy }));
            }
            Method::PUT => {
                let parsed: Value =
                    serde_json::from_slice(&body).unwrap_or_else(|_| json!({ "policy": { "phases": {} } }));
                state.ilm.put_policy(name, parsed);
                return Json(json!({ "acknowledged": true }));
            }
            _ => {}
        }
    }
    Json(json!({ "acknowledged": true }))
}

/// GET|PUT /_data_stream/*rest — dispatch data-stream operations.
///
/// - `PUT  /_data_stream/{name}` → create (provisions a backing collection)
/// - `GET  /_data_stream`        → list (empty)
pub async fn data_stream_dispatch(
    State(state): State<EsCompatState>,
    Path(rest): Path<String>,
    method: Method,
) -> Result<Json<Value>, crate::error::EsCompatError> {
    let name = rest.trim_start_matches('/');
    if method == Method::PUT && !name.is_empty() {
        create_backing_collection(&state, name).await?;
        return Ok(Json(json!({ "acknowledged": true })));
    }
    Ok(Json(json!([])))
}
