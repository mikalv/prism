//! ES-compatible _bulk endpoint

use crate::endpoints::search::EsCompatState;
use crate::error::EsCompatError;
use crate::query::{BulkAction, BulkActionMeta};
use crate::response::{BulkItemResponse, BulkItemResult, EsBulkResponse, EsError, ShardStats};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::Json;
use prism::backends::Document;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;
use tracing::warn;
use uuid::Uuid;

/// POST /_elastic/_bulk - Bulk indexing
/// POST /_elastic/{index}/_bulk - Bulk indexing with default index
///
/// Rejects wildcard patterns in index names and limits total actions
/// to prevent resource exhaustion.
const MAX_BULK_ACTIONS: usize = 10_000;

pub async fn bulk_handler(
    State(state): State<EsCompatState>,
    default_index: Option<Path<String>>,
    body: Bytes,
) -> Result<Json<EsBulkResponse>, EsCompatError> {
    let start = Instant::now();
    let default_index = default_index.map(|p| p.0);

    // Parse NDJSON bulk body
    let actions = parse_bulk_body(&body, default_index.as_deref())?;

    if actions.len() > MAX_BULK_ACTIONS {
        return Err(EsCompatError::InvalidRequestBody(format!(
            "Bulk request contains {} actions, max allowed is {}",
            actions.len(),
            MAX_BULK_ACTIONS
        )));
    }

    let mut items = Vec::with_capacity(actions.len());
    let mut has_errors = false;

    // Group by index for batch processing
    let mut by_index: HashMap<String, Vec<(String, Document)>> = HashMap::new();
    let mut delete_by_index: HashMap<String, Vec<String>> = HashMap::new();
    // Update actions are handled individually (read-modify-write), so we keep
    // the partial doc Value rather than a flattened Document.
    let mut update_by_index: HashMap<String, Vec<(String, Value)>> = HashMap::new();

    for action in actions {
        match action {
            BulkAction::Index { index, id, doc } | BulkAction::Create { index, id, doc } => {
                let doc_id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
                let fields = match doc {
                    Value::Object(obj) => obj.into_iter().collect(),
                    _ => {
                        items.push(BulkItemResponse { index: Some(BulkItemResult { index: index.clone(),
                        id: doc_id,
                        version: 1,
                        result: "error".to_string(),
                        shards: ShardStats::default(),
                        status: 400,
                        error: Some(EsError {
                            error_type: "mapper_parsing_exception".to_string(),
                            reason: "Document must be an object".to_string(),
                        }), seq_no: 0, primary_term: 1 }), create: None, delete: None, update: None });
                        has_errors = true;
                        continue;
                    }
                };

                by_index.entry(index).or_default().push((
                    doc_id,
                    Document {
                        id: String::new(),
                        fields,
                    },
                ));
            }
            BulkAction::Delete { index, id } => {
                delete_by_index.entry(index).or_default().push(id);
            }
            BulkAction::Update { index, id, doc } => {
                update_by_index.entry(index).or_default().push((id, doc));
            }
        }
    }

    // Process index/create actions
    for (index, docs) in by_index {
        // Reject wildcard patterns in index names
        if index.contains('*') || index.contains('?') {
            for (doc_id, _) in docs {
                items.push(BulkItemResponse { index: Some(BulkItemResult { index: index.clone(),
                id: doc_id,
                version: 1,
                result: "error".to_string(),
                shards: ShardStats::default(),
                status: 400,
                error: Some(EsError {
                    error_type: "invalid_index_name_exception".to_string(),
                    reason: format!(
                        "Wildcard patterns not allowed in bulk index name: [{}]",
                        index
                    ),
                }), seq_no: 0, primary_term: 1 }), create: None, delete: None, update: None });
                has_errors = true;
            }
            continue;
        }

        // Check if collection exists (use exact name, not pattern expansion)
        let mut collections = state
            .manager
            .expand_collection_patterns(std::slice::from_ref(&index));

        // ES auto_create_index: lazily create the collection on first write
        // rather than failing the whole bulk batch with 404s.
        if collections.is_empty()
            && crate::endpoints::document::ensure_collection(
                &state,
                &index,
                docs.first().map(|(_, d)| &d.fields),
            )
            .await
            .is_ok()
            {
                collections = state
                    .manager
                    .expand_collection_patterns(std::slice::from_ref(&index));
            }

        if collections.is_empty() {
            // Collection still doesn't exist - report errors
            for (doc_id, _) in docs {
                items.push(BulkItemResponse { index: Some(BulkItemResult { index: index.clone(),
                id: doc_id,
                version: 1,
                result: "error".to_string(),
                shards: ShardStats::default(),
                status: 404,
                error: Some(EsError {
                    error_type: "index_not_found_exception".to_string(),
                    reason: format!("no such index [{}]", index),
                }), seq_no: 0, primary_term: 1 }), create: None, delete: None, update: None });
                has_errors = true;
            }
            continue;
        }

        let target_index = &collections[0];

        // Prepare documents with IDs
        let prism_docs: Vec<Document> = docs
            .iter()
            .map(|(id, doc)| Document {
                id: id.clone(),
                fields: doc.fields.clone(),
            })
            .collect();

        let doc_ids: Vec<String> = docs.iter().map(|(id, _)| id.clone()).collect();

        // Index documents
        match state.manager.index(target_index, prism_docs).await {
            Ok(_) => {
                for doc_id in doc_ids {
                    items.push(BulkItemResponse { index: Some(BulkItemResult { index: target_index.clone(),
                    id: doc_id,
                    version: 1,
                    result: "created".to_string(),
                    shards: ShardStats::default(),
                    status: 201,
                    error: None, seq_no: 0, primary_term: 1 }), create: None, delete: None, update: None });
                }
            }
            Err(e) => {
                warn!("bulk index error: {}", e);
                for doc_id in doc_ids {
                    items.push(BulkItemResponse { index: Some(BulkItemResult { index: target_index.clone(),
                    id: doc_id,
                    version: 1,
                    result: "error".to_string(),
                    shards: ShardStats::default(),
                    status: 500,
                    error: Some(EsError {
                        error_type: "mapper_exception".to_string(),
                        reason: e.to_string(),
                    }), seq_no: 0, primary_term: 1 }), create: None, delete: None, update: None });
                    has_errors = true;
                }
            }
        }
    }

    // Process delete actions
    for (index, ids) in delete_by_index {
        let collections = state
            .manager
            .expand_collection_patterns(std::slice::from_ref(&index));

        if collections.is_empty() {
            for id in ids {
                items.push(BulkItemResponse { index: None, create: None, delete: Some(BulkItemResult { index: index.clone(),
                id,
                version: 1,
                result: "not_found".to_string(),
                shards: ShardStats::default(),
                status: 404,
                error: None, seq_no: 0, primary_term: 1 }), update: None });
            }
            continue;
        }

        let target_index = &collections[0];

        match state.manager.delete(target_index, ids.clone()).await {
            Ok(_) => {
                for id in ids {
                    items.push(BulkItemResponse { index: None, create: None, delete: Some(BulkItemResult { index: target_index.clone(),
                    id,
                    version: 1,
                    result: "deleted".to_string(),
                    shards: ShardStats::default(),
                    status: 200,
                    error: None, seq_no: 0, primary_term: 1 }), update: None });
                }
            }
            Err(e) => {
                warn!("bulk delete error: {}", e);
                for id in ids {
                    items.push(BulkItemResponse { index: None, create: None, delete: Some(BulkItemResult { index: target_index.clone(),
                    id,
                    version: 1,
                    result: "error".to_string(),
                    shards: ShardStats::default(),
                    status: 500,
                    error: Some(EsError {
                        error_type: "exception".to_string(),
                        reason: e.to_string(),
                    }), seq_no: 0, primary_term: 1 }), update: None });
                    has_errors = true;
                }
            }
        }
    }

    // Process update actions (read-modify-write per document). Kibana's
    // saved-objects repository updates objects almost exclusively via bulk
    // `update`; the source line is `{"doc": {...}}` (optionally with
    // `doc_as_upsert`). We extract `doc`, merge over the existing fields
    // (upsert when absent), and re-index.
    for (index, updates) in update_by_index {
        if index.contains('*') || index.contains('?') {
            for (id, _) in updates {
                items.push(BulkItemResponse {
                    index: None, create: None, delete: None,
                    update: Some(BulkItemResult {
                        index: index.clone(), id, version: 1, seq_no: 0, primary_term: 1,
                        result: "error".to_string(), shards: ShardStats::default(),
                        status: 400,
                        error: Some(EsError {
                            error_type: "invalid_index_name_exception".to_string(),
                            reason: format!("Wildcard patterns not allowed in bulk index name: [{}]", index),
                        }),
                    }),
                });
                has_errors = true;
            }
            continue;
        }

        let collections = state.manager.expand_collection_patterns(std::slice::from_ref(&index));
        if collections.is_empty() {
            for (id, _) in updates {
                items.push(BulkItemResponse {
                    index: None, create: None, delete: None,
                    update: Some(BulkItemResult {
                        index: index.clone(), id, version: 1, seq_no: 0, primary_term: 1,
                        result: "error".to_string(), shards: ShardStats::default(),
                        status: 404,
                        error: Some(EsError {
                            error_type: "index_not_found_exception".to_string(),
                            reason: format!("no such index [{}]", index),
                        }),
                    }),
                });
                has_errors = true;
            }
            continue;
        }

        let target_index = collections.into_iter().next().unwrap();

        for (id, doc_val) in updates {
            // Source line is {"doc": {...}} (optionally doc_as_upsert).
            let partial = match doc_val.get("doc") {
                Some(d) => d.clone(),
                None => doc_val.clone(),
            };
            let partial_fields: HashMap<String, Value> = match partial.as_object() {
                Some(obj) => obj.clone().into_iter().collect(),
                None => {
                    items.push(BulkItemResponse {
                        index: None, create: None, delete: None,
                        update: Some(BulkItemResult {
                            index: target_index.clone(), id, version: 1, seq_no: 0, primary_term: 1,
                            result: "error".to_string(), shards: ShardStats::default(),
                            status: 400,
                            error: Some(EsError {
                                error_type: "mapper_parsing_exception".to_string(),
                                reason: "Update doc must be an object".to_string(),
                            }),
                        }),
                    });
                    has_errors = true;
                    continue;
                }
            };

            // read-modify-write: merge partial over existing fields (upsert).
            let mut merged = match state.manager.get(&target_index, &id).await {
                Ok(Some(existing)) => existing.fields,
                _ => HashMap::new(),
            };
            for (k, v) in partial_fields {
                merged.insert(k, v);
            }

            match state.manager
                .index(&target_index, vec![Document { id: id.clone(), fields: merged }])
                .await
            {
                Ok(_) => {
                    items.push(BulkItemResponse {
                        index: None, create: None, delete: None,
                        update: Some(BulkItemResult {
                            index: target_index.clone(), id, version: 1, seq_no: 0, primary_term: 1,
                            result: "updated".to_string(), shards: ShardStats::default(),
                            status: 200, error: None,
                        }),
                    });
                }
                Err(e) => {
                    warn!("bulk update error: {}", e);
                    items.push(BulkItemResponse {
                        index: None, create: None, delete: None,
                        update: Some(BulkItemResult {
                            index: target_index.clone(), id, version: 1, seq_no: 0, primary_term: 1,
                            result: "error".to_string(), shards: ShardStats::default(),
                            status: 500,
                            error: Some(EsError {
                                error_type: "mapper_exception".to_string(),
                                reason: e.to_string(),
                            }),
                        }),
                    });
                    has_errors = true;
                }
            }
        }
    }

    let took_ms = start.elapsed().as_millis() as u64;

    Ok(Json(EsBulkResponse {
        took: took_ms,
        errors: has_errors,
        items,
    }))
}

/// Parse NDJSON bulk request body
fn parse_bulk_body(
    body: &Bytes,
    default_index: Option<&str>,
) -> Result<Vec<BulkAction>, EsCompatError> {
    let text =
        std::str::from_utf8(body).map_err(|e| EsCompatError::InvalidRequestBody(e.to_string()))?;

    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();

    let mut actions = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let meta: BulkActionMeta = serde_json::from_str(lines[i])
            .map_err(|e| EsCompatError::InvalidRequestBody(format!("Invalid action: {}", e)))?;

        if let Some(index_meta) = meta.index {
            let index = index_meta
                .index
                .or_else(|| default_index.map(String::from))
                .ok_or_else(|| EsCompatError::MissingField("_index".to_string()))?;

            i += 1;
            if i >= lines.len() {
                return Err(EsCompatError::InvalidRequestBody(
                    "Missing document body".to_string(),
                ));
            }

            let doc: Value = serde_json::from_str(lines[i])
                .map_err(|e| EsCompatError::InvalidRequestBody(format!("Invalid doc: {}", e)))?;

            actions.push(BulkAction::Index {
                index,
                id: index_meta.id,
                doc,
            });
        } else if let Some(create_meta) = meta.create {
            let index = create_meta
                .index
                .or_else(|| default_index.map(String::from))
                .ok_or_else(|| EsCompatError::MissingField("_index".to_string()))?;

            i += 1;
            if i >= lines.len() {
                return Err(EsCompatError::InvalidRequestBody(
                    "Missing document body".to_string(),
                ));
            }

            let doc: Value = serde_json::from_str(lines[i])
                .map_err(|e| EsCompatError::InvalidRequestBody(format!("Invalid doc: {}", e)))?;

            actions.push(BulkAction::Create {
                index,
                id: create_meta.id,
                doc,
            });
        } else if let Some(delete_meta) = meta.delete {
            let index = delete_meta
                .index
                .or_else(|| default_index.map(String::from))
                .ok_or_else(|| EsCompatError::MissingField("_index".to_string()))?;

            let id = delete_meta
                .id
                .ok_or_else(|| EsCompatError::MissingField("_id".to_string()))?;

            actions.push(BulkAction::Delete { index, id });
        } else if let Some(update_meta) = meta.update {
            let index = update_meta
                .index
                .or_else(|| default_index.map(String::from))
                .ok_or_else(|| EsCompatError::MissingField("_index".to_string()))?;
            let id = update_meta
                .id
                .ok_or_else(|| EsCompatError::MissingField("_id".to_string()))?;

            i += 1;
            if i >= lines.len() {
                return Err(EsCompatError::InvalidRequestBody(
                    "Missing document body".to_string(),
                ));
            }

            let doc: Value = serde_json::from_str(lines[i])
                .map_err(|e| EsCompatError::InvalidRequestBody(format!("Invalid doc: {}", e)))?;

            actions.push(BulkAction::Update { index, id, doc });
        }

        i += 1;
    }

    Ok(actions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::BulkAction;
    use axum::body::Bytes;

    fn make_bytes(s: &str) -> Bytes {
        Bytes::from(s.to_string())
    }

    // ===================================================================
    // parse_bulk_body — index action
    // ===================================================================

    #[test]
    fn test_parse_bulk_index_action() {
        let body = make_bytes(
            r#"{"index":{"_index":"products","_id":"1"}}
{"title":"Widget","price":9.99}
"#,
        );
        let actions = parse_bulk_body(&body, None).unwrap();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            BulkAction::Index { index, id, doc } => {
                assert_eq!(index, "products");
                assert_eq!(id.as_deref(), Some("1"));
                assert_eq!(doc["title"], "Widget");
            }
            _ => panic!("Expected Index action"),
        }
    }

    #[test]
    fn test_parse_bulk_index_no_id() {
        let body = make_bytes(
            r#"{"index":{"_index":"logs"}}
{"message":"hello"}
"#,
        );
        let actions = parse_bulk_body(&body, None).unwrap();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            BulkAction::Index { index, id, .. } => {
                assert_eq!(index, "logs");
                assert!(id.is_none());
            }
            _ => panic!("Expected Index action"),
        }
    }

    #[test]
    fn test_parse_bulk_index_default_index() {
        let body = make_bytes(
            r#"{"index":{}}
{"message":"hello"}
"#,
        );
        let actions = parse_bulk_body(&body, Some("default_idx")).unwrap();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            BulkAction::Index { index, .. } => {
                assert_eq!(index, "default_idx");
            }
            _ => panic!("Expected Index"),
        }
    }

    #[test]
    fn test_parse_bulk_index_missing_index_error() {
        let body = make_bytes(
            r#"{"index":{}}
{"message":"hello"}
"#,
        );
        let result = parse_bulk_body(&body, None);
        assert!(result.is_err());
    }

    // ===================================================================
    // parse_bulk_body — create action
    // ===================================================================

    #[test]
    fn test_parse_bulk_create_action() {
        let body = make_bytes(
            r#"{"create":{"_index":"products","_id":"2"}}
{"title":"Gadget"}
"#,
        );
        let actions = parse_bulk_body(&body, None).unwrap();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            BulkAction::Create { index, id, doc } => {
                assert_eq!(index, "products");
                assert_eq!(id.as_deref(), Some("2"));
                assert_eq!(doc["title"], "Gadget");
            }
            _ => panic!("Expected Create action"),
        }
    }

    #[test]
    fn test_parse_bulk_create_default_index() {
        let body = make_bytes(
            r#"{"create":{}}
{"title":"Thing"}
"#,
        );
        let actions = parse_bulk_body(&body, Some("my_index")).unwrap();
        match &actions[0] {
            BulkAction::Create { index, .. } => {
                assert_eq!(index, "my_index");
            }
            _ => panic!("Expected Create"),
        }
    }

    // ===================================================================
    // parse_bulk_body — delete action
    // ===================================================================

    #[test]
    fn test_parse_bulk_delete_action() {
        let body = make_bytes(r#"{"delete":{"_index":"products","_id":"3"}}"#);
        let actions = parse_bulk_body(&body, None).unwrap();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            BulkAction::Delete { index, id } => {
                assert_eq!(index, "products");
                assert_eq!(id, "3");
            }
            _ => panic!("Expected Delete action"),
        }
    }

    #[test]
    fn test_parse_bulk_delete_no_id_error() {
        let body = make_bytes(r#"{"delete":{"_index":"products"}}"#);
        let result = parse_bulk_body(&body, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_bulk_delete_default_index() {
        let body = make_bytes(r#"{"delete":{"_id":"5"}}"#);
        let actions = parse_bulk_body(&body, Some("logs")).unwrap();
        match &actions[0] {
            BulkAction::Delete { index, id } => {
                assert_eq!(index, "logs");
                assert_eq!(id, "5");
            }
            _ => panic!("Expected Delete"),
        }
    }

    // ===================================================================
    // parse_bulk_body — missing doc body
    // ===================================================================

    #[test]
    fn test_parse_bulk_missing_doc_body() {
        // Index action requires a body on the next line
        let body = make_bytes(r#"{"index":{"_index":"products","_id":"1"}}"#);
        let result = parse_bulk_body(&body, None);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Missing document body"));
    }

    #[test]
    fn test_parse_bulk_create_missing_body() {
        let body = make_bytes(r#"{"create":{"_index":"products","_id":"1"}}"#);
        let result = parse_bulk_body(&body, None);
        assert!(result.is_err());
    }

    // ===================================================================
    // parse_bulk_body — invalid JSON
    // ===================================================================

    #[test]
    fn test_parse_bulk_invalid_action_json() {
        let body = make_bytes("not valid json\n");
        let result = parse_bulk_body(&body, None);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid action"));
    }

    #[test]
    fn test_parse_bulk_invalid_doc_json() {
        let body = make_bytes(
            r#"{"index":{"_index":"test","_id":"1"}}
not valid json
"#,
        );
        let result = parse_bulk_body(&body, None);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid doc"));
    }

    // ===================================================================
    // parse_bulk_body — update action (skipped)
    // ===================================================================

    #[test]
    fn test_parse_bulk_update() {
        let body = make_bytes(
            r#"{"update":{"_index":"products","_id":"1"}}
{"doc":{"price":19.99}}
"#,
        );
        let actions = parse_bulk_body(&body, None).unwrap();
        assert_eq!(actions.len(), 1, "Update should be parsed as one action");
        match &actions[0] {
            BulkAction::Update { index, id, doc } => {
                assert_eq!(index, "products");
                assert_eq!(id, "1");
                assert_eq!(doc, &serde_json::json!({"doc": {"price": 19.99}}));
            }
            other => panic!("expected Update, got {:?}", other),
        }
    }

    // ===================================================================
    // parse_bulk_body — multiple actions
    // ===================================================================

    #[test]
    fn test_parse_bulk_multiple_actions() {
        let body = make_bytes(
            r#"{"index":{"_index":"logs","_id":"1"}}
{"message":"first"}
{"index":{"_index":"logs","_id":"2"}}
{"message":"second"}
{"delete":{"_index":"logs","_id":"3"}}
"#,
        );
        let actions = parse_bulk_body(&body, None).unwrap();
        assert_eq!(actions.len(), 3);
        assert!(matches!(&actions[0], BulkAction::Index { .. }));
        assert!(matches!(&actions[1], BulkAction::Index { .. }));
        assert!(matches!(&actions[2], BulkAction::Delete { .. }));
    }

    #[test]
    fn test_parse_bulk_mixed_actions_with_default_index() {
        let body = make_bytes(
            r#"{"index":{"_id":"1"}}
{"title":"A"}
{"create":{"_id":"2"}}
{"title":"B"}
{"delete":{"_id":"3"}}
"#,
        );
        let actions = parse_bulk_body(&body, Some("myidx")).unwrap();
        assert_eq!(actions.len(), 3);
        match &actions[0] {
            BulkAction::Index { index, .. } => assert_eq!(index, "myidx"),
            _ => panic!("Expected Index"),
        }
        match &actions[1] {
            BulkAction::Create { index, .. } => assert_eq!(index, "myidx"),
            _ => panic!("Expected Create"),
        }
        match &actions[2] {
            BulkAction::Delete { index, .. } => assert_eq!(index, "myidx"),
            _ => panic!("Expected Delete"),
        }
    }

    #[test]
    fn test_parse_bulk_empty_body() {
        let body = make_bytes("");
        let actions = parse_bulk_body(&body, None).unwrap();
        assert!(actions.is_empty());
    }

    #[test]
    fn test_parse_bulk_blank_lines_ignored() {
        let body = make_bytes(
            r#"
{"index":{"_index":"test","_id":"1"}}

{"msg":"hello"}

"#,
        );
        let actions = parse_bulk_body(&body, None).unwrap();
        assert_eq!(actions.len(), 1);
    }
}
