//! ES-compatible async-task endpoints: `_update_by_query`, `_delete_by_query`,
//! and `_tasks/{id}`.
//!
//! Kibana's saved-objects migration invokes `_update_by_query` and
//! `_delete_by_query` with `wait_for_completion=false`, which in Elasticsearch
//! returns a task id immediately. The migration then polls `_tasks/{id}` with
//! `wait_for_completion=true` until the task reports completion.
//!
//! Prism processes work synchronously and has no persistent task queue, so
//! these endpoints are implemented as instant-completion stubs: the
//! `_update_by_query` / `_delete_by_query` handlers acknowledge a task id, and
//! `_tasks/{id}` always reports a completed task with zero changes. For the
//! migration's empty/fresh target indices this is also the *correct* result
//! (there are no documents to transform).

use crate::endpoints::search::EsCompatState;
use crate::error::EsCompatError;
use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};

/// A fixed task id. Because every task completes instantly in Prism, a single
/// well-known id is sufficient; `_tasks/<id>` reports completion regardless.
const TASK_ID: &str = "prism:1";

/// Verify the index pattern resolves to at least one collection.
fn require_collection(state: &EsCompatState, index: &str) -> Result<(), EsCompatError> {
    let idx = index.to_string();
    let collections = state.manager.expand_collection_patterns(std::slice::from_ref(&idx));
    if collections.is_empty() {
        return Err(EsCompatError::IndexNotFound(index.to_string()));
    }
    Ok(())
}

/// POST /{index}/_update_by_query
///
/// With `wait_for_completion=false` Elasticsearch returns a task reference.
/// Prism performs no scripted bulk update; for a fresh index there is nothing
/// to update, so we simply hand back a task id that `_tasks` will report as
/// completed with zero changes.
pub async fn update_by_query_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
) -> Result<Json<Value>, EsCompatError> {
    require_collection(&state, &index)?;
    tracing::debug!(index = %index, "POST /_update_by_query -> task {} (no-op)", TASK_ID);
    Ok(Json(json!({ "task": TASK_ID })))
}

/// POST /{index}/_delete_by_query
///
/// Same async-task pattern as `_update_by_query`. Prism performs no bulk
/// delete here; the task is reported as completed with zero changes.
pub async fn delete_by_query_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
) -> Result<Json<Value>, EsCompatError> {
    require_collection(&state, &index)?;
    tracing::debug!(index = %index, "POST /_delete_by_query -> task {} (no-op)", TASK_ID);
    Ok(Json(json!({ "task": TASK_ID })))
}

/// GET /_tasks/{task_id}
///
/// Always reports the task as completed with an empty result set. Kibana's
/// `waitForTask` reads `completed`, `task.description`, and
/// `response.failures` from this body.
pub async fn get_task_handler(
    Path(task_id): Path<String>,
) -> Result<Json<Value>, EsCompatError> {
    tracing::debug!(task_id = %task_id, "GET /_tasks/{} -> completed (no-op)", task_id);
    Ok(Json(json!({
        "completed": true,
        "task": {
            "node": "prism",
            "id": 1,
            "type": "transport",
            "action": "indices:data/write/update_by_query",
            "description": format!("update-by-query {}", task_id),
            "start_time_in_millis": 0,
            "running_time_in_nanos": 0,
            "cancellable": true,
            "headers": {}
        },
        "response": {
            "took": 0,
            "timed_out": false,
            "total": 0,
            "updated": 0,
            "deleted": 0,
            "created": 0,
            "batches": 0,
            "version_conflicts": 0,
            "noops": 0,
            "retries": { "bulk": 0, "search": 0 },
            "failures": []
        }
    })))
}
