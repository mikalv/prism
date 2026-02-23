//! ES-compatible _msearch endpoint

use crate::endpoints::search::EsCompatState;
use crate::error::EsCompatError;
use crate::query::{EsSearchRequest, MSearchHeader, QueryTranslator};
use crate::response::{EsError, EsMSearchItem, EsMSearchResponse, ResponseMapper};
use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use prism::backends::SearchResult;
use prism::collection::CollectionManager;
use std::time::Instant;
use tracing::warn;

/// POST /_elastic/_msearch - Multi-search
pub async fn msearch_handler(
    State(state): State<EsCompatState>,
    body: Bytes,
) -> Result<Json<EsMSearchResponse>, EsCompatError> {
    let start = Instant::now();

    // Parse NDJSON body
    let searches = parse_msearch_body(&body)?;

    let mut responses = Vec::with_capacity(searches.len());

    for (header, request) in searches {
        let result = execute_single_search(&state.manager, header, request).await;
        responses.push(result);
    }

    let took_ms = start.elapsed().as_millis() as u64;

    Ok(Json(EsMSearchResponse {
        took: took_ms,
        responses,
    }))
}

/// Parse NDJSON multi-search body
fn parse_msearch_body(
    body: &Bytes,
) -> Result<Vec<(MSearchHeader, EsSearchRequest)>, EsCompatError> {
    let text =
        std::str::from_utf8(body).map_err(|e| EsCompatError::InvalidRequestBody(e.to_string()))?;

    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();

    if !lines.len().is_multiple_of(2) {
        return Err(EsCompatError::InvalidRequestBody(
            "msearch body must have header/body pairs".to_string(),
        ));
    }

    let mut searches = Vec::new();

    for chunk in lines.chunks(2) {
        let header: MSearchHeader = serde_json::from_str(chunk[0])
            .map_err(|e| EsCompatError::InvalidRequestBody(format!("Invalid header: {}", e)))?;

        let body: EsSearchRequest = serde_json::from_str(chunk[1])
            .map_err(|e| EsCompatError::InvalidRequestBody(format!("Invalid body: {}", e)))?;

        searches.push((header, body));
    }

    Ok(searches)
}

/// Execute a single search from msearch batch
async fn execute_single_search(
    manager: &CollectionManager,
    header: MSearchHeader,
    request: EsSearchRequest,
) -> EsMSearchItem {
    let start = Instant::now();

    let index_name = header.index.unwrap_or_else(|| "*".to_string());

    // Expand index pattern (sync)
    let collections = manager.expand_collection_patterns(std::slice::from_ref(&index_name));

    if collections.is_empty() {
        return EsMSearchItem::Error {
            error: EsError {
                error_type: "index_not_found_exception".to_string(),
                reason: format!("no such index [{}]", index_name),
            },
            status: 404,
        };
    }

    // Get default fields (sync)
    let default_fields = get_text_fields(manager, &collections[0]);

    // Translate query
    let (query, aggregations) = match QueryTranslator::translate(&request, &default_fields) {
        Ok(r) => r,
        Err(e) => {
            return EsMSearchItem::Error {
                error: EsError {
                    error_type: "parsing_exception".to_string(),
                    reason: e.to_string(),
                },
                status: 400,
            };
        }
    };

    // Execute search
    let results = if collections.len() == 1 {
        match manager
            .search_with_aggs(&collections[0], &query, aggregations)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!("msearch query error: {}", e);
                return EsMSearchItem::Error {
                    error: EsError {
                        error_type: "search_phase_execution_exception".to_string(),
                        reason: e.to_string(),
                    },
                    status: 500,
                };
            }
        }
    } else {
        match manager.multi_search(&collections, query, None).await {
            Ok(multi) => prism::backends::SearchResultsWithAggs {
                results: multi
                    .results
                    .into_iter()
                    .map(|r| SearchResult {
                        id: r.id,
                        score: r.score,
                        fields: r.fields,
                        highlight: r.highlight,
                    })
                    .collect(),
                total: multi.total as u64,
                aggregations: std::collections::HashMap::new(),
            },
            Err(e) => {
                warn!("msearch multi query error: {}", e);
                return EsMSearchItem::Error {
                    error: EsError {
                        error_type: "search_phase_execution_exception".to_string(),
                        reason: e.to_string(),
                    },
                    status: 500,
                };
            }
        }
    };

    let took_ms = start.elapsed().as_millis() as u64;
    let response = ResponseMapper::map_search_results(&index_name, results, took_ms);
    EsMSearchItem::Success(response)
}

fn get_text_fields(manager: &CollectionManager, collection: &str) -> Vec<String> {
    manager
        .get_schema(collection)
        .map(|schema| {
            schema
                .backends
                .text
                .as_ref()
                .map(|t| t.fields.iter().map(|f| f.name.clone()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;

    fn make_bytes(s: &str) -> Bytes {
        Bytes::from(s.to_string())
    }

    // ===================================================================
    // parse_msearch_body — valid header/body pairs
    // ===================================================================

    #[test]
    fn test_parse_msearch_single_pair() {
        let body = make_bytes(
            r#"{"index":"my_index"}
{"query":{"match_all":{}}}
"#,
        );
        let searches = parse_msearch_body(&body).unwrap();
        assert_eq!(searches.len(), 1);
        assert_eq!(searches[0].0.index, Some("my_index".to_string()));
        assert!(searches[0].1.query.is_some());
    }

    #[test]
    fn test_parse_msearch_multiple_pairs() {
        let body = make_bytes(
            r#"{"index":"index_a"}
{"query":{"match_all":{}}}
{"index":"index_b"}
{"query":{"term":{"status":"active"}},"size":5}
"#,
        );
        let searches = parse_msearch_body(&body).unwrap();
        assert_eq!(searches.len(), 2);
        assert_eq!(searches[0].0.index, Some("index_a".to_string()));
        assert_eq!(searches[1].0.index, Some("index_b".to_string()));
        assert_eq!(searches[1].1.size, Some(5));
    }

    #[test]
    fn test_parse_msearch_empty_header() {
        let body = make_bytes(
            r#"{}
{"query":{"match_all":{}}}
"#,
        );
        let searches = parse_msearch_body(&body).unwrap();
        assert_eq!(searches.len(), 1);
        assert!(searches[0].0.index.is_none());
    }

    #[test]
    fn test_parse_msearch_with_preference_routing() {
        let body = make_bytes(
            r#"{"index":"logs","preference":"_local","routing":"user123"}
{"query":{"match_all":{}}}
"#,
        );
        let searches = parse_msearch_body(&body).unwrap();
        assert_eq!(searches[0].0.preference, Some("_local".to_string()));
        assert_eq!(searches[0].0.routing, Some("user123".to_string()));
    }

    // ===================================================================
    // parse_msearch_body — odd line count error
    // ===================================================================

    #[test]
    fn test_parse_msearch_odd_lines_error() {
        let body = make_bytes(
            r#"{"index":"my_index"}
{"query":{"match_all":{}}}
{"index":"another"}
"#,
        );
        let result = parse_msearch_body(&body);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("header/body pairs"));
    }

    #[test]
    fn test_parse_msearch_single_line_error() {
        let body = make_bytes(r#"{"index":"my_index"}"#);
        let result = parse_msearch_body(&body);
        assert!(result.is_err());
    }

    // ===================================================================
    // parse_msearch_body — invalid JSON
    // ===================================================================

    #[test]
    fn test_parse_msearch_invalid_header() {
        let body = make_bytes(
            r#"not valid json
{"query":{"match_all":{}}}
"#,
        );
        let result = parse_msearch_body(&body);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid header"));
    }

    #[test]
    fn test_parse_msearch_invalid_body() {
        let body = make_bytes(
            r#"{"index":"test"}
not valid json
"#,
        );
        let result = parse_msearch_body(&body);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid body"));
    }

    // ===================================================================
    // parse_msearch_body — empty body
    // ===================================================================

    #[test]
    fn test_parse_msearch_empty() {
        let body = make_bytes("");
        let searches = parse_msearch_body(&body).unwrap();
        assert!(searches.is_empty());
    }

    #[test]
    fn test_parse_msearch_blank_lines_filtered() {
        // Blank lines are filtered out; remaining must be even count
        let body = make_bytes(
            r#"
{"index":"test"}

{"query":{"match_all":{}}}

"#,
        );
        let searches = parse_msearch_body(&body).unwrap();
        assert_eq!(searches.len(), 1);
    }

    #[test]
    fn test_parse_msearch_no_query_in_body() {
        let body = make_bytes(
            r#"{"index":"test"}
{}
"#,
        );
        let searches = parse_msearch_body(&body).unwrap();
        assert_eq!(searches.len(), 1);
        assert!(searches[0].1.query.is_none());
    }
}
