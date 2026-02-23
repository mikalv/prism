//! Mnemos-compatible API routes
//!
//! Provides `/api/*` routes compatible with the mnemos MCP server.

use crate::backends::Query;
use crate::collection::CollectionManager;
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// Request types (matching mnemos daemon API)

#[derive(Debug, Deserialize)]
pub struct SessionInitRequest {
    pub folder_path: String,
    #[serde(default)]
    pub context_hint: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ContextRequest {
    pub user_message: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct MnemosSearchRequest {
    pub mode: String,
    pub query: String,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub output_format: Option<String>,
}

// Response types

#[derive(Debug, Serialize)]
pub struct SessionInitResponse {
    pub workspace_id: String,
    pub project_id: String,
    pub project_name: String,
}

#[derive(Debug, Serialize)]
pub struct ContextResponse {
    pub context: Vec<ContextItem>,
    pub meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub struct ContextItem {
    pub content: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relevance: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct MnemosSearchResponse {
    pub results: Vec<MnemosSearchResult>,
    pub total: u32,
    pub meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub struct MnemosSearchResult {
    pub path: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

#[derive(Debug, Default, Serialize)]
pub struct ResponseMeta {
    #[serde(default)]
    pub tokens_used: u32,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub meta: ResponseMeta,
}

// Route handlers

pub async fn session_init(
    Json(req): Json<SessionInitRequest>,
) -> Json<ApiResponse<SessionInitResponse>> {
    // Extract project name from folder path
    let project_name = req
        .folder_path
        .split('/')
        .rfind(|s| !s.is_empty())
        .unwrap_or("default")
        .to_string();

    let response = SessionInitResponse {
        workspace_id: "default".to_string(),
        project_id: project_name.clone(),
        project_name,
    };

    Json(ApiResponse {
        success: true,
        data: Some(response),
        error: None,
        meta: ResponseMeta::default(),
    })
}

pub async fn context(
    State(manager): State<Arc<CollectionManager>>,
    Json(req): Json<ContextRequest>,
) -> Result<Json<ApiResponse<ContextResponse>>, StatusCode> {
    // Search both observations and memories for context
    let limit = req.max_tokens.unwrap_or(400) / 50; // Rough estimate: 50 tokens per result

    let query = Query {
        query_string: req.user_message.clone(),
        fields: vec![], // Use default fields
        limit: limit as usize,
        offset: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    };

    let mut context_items = Vec::new();

    // Search observations
    if let Ok(results) = manager.search("observations", query.clone(), None).await {
        for r in results.results {
            let content = r
                .fields
                .get("content")
                .and_then(|v| v.as_str())
                .or_else(|| r.fields.get("summary").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string();

            if !content.is_empty() {
                context_items.push(ContextItem {
                    content,
                    source: format!("observation:{}", r.id),
                    relevance: Some(r.score),
                });
            }
        }
    }

    // Search memories
    let query2 = Query {
        query_string: req.user_message,
        fields: vec![],
        limit: limit as usize,
        offset: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    };

    if let Ok(results) = manager.search("memories", query2, None).await {
        for r in results.results {
            let content = r
                .fields
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if !content.is_empty() {
                context_items.push(ContextItem {
                    content,
                    source: format!("memory:{}", r.id),
                    relevance: Some(r.score),
                });
            }
        }
    }

    // Sort by relevance
    context_items.sort_by(|a, b| {
        b.relevance
            .unwrap_or(0.0)
            .partial_cmp(&a.relevance.unwrap_or(0.0))
            .unwrap()
    });

    // Limit total context
    context_items.truncate(limit as usize);

    Ok(Json(ApiResponse {
        success: true,
        data: Some(ContextResponse {
            context: context_items,
            meta: ResponseMeta::default(),
        }),
        error: None,
        meta: ResponseMeta::default(),
    }))
}

pub async fn search(
    State(manager): State<Arc<CollectionManager>>,
    Json(req): Json<MnemosSearchRequest>,
) -> Result<Json<ApiResponse<MnemosSearchResponse>>, StatusCode> {
    let limit = req.limit.unwrap_or(10) as usize;

    let query = Query {
        query_string: req.query,
        fields: vec![],
        limit,
        offset: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    };

    let mut all_results = Vec::new();

    // Search both collections
    for collection in ["observations", "memories"] {
        if let Ok(results) = manager.search(collection, query.clone(), None).await {
            for r in results.results {
                let content = r
                    .fields
                    .get("content")
                    .and_then(|v| v.as_str())
                    .or_else(|| r.fields.get("summary").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string();

                all_results.push(MnemosSearchResult {
                    path: format!("{}:{}", collection, r.id),
                    content,
                    line: None,
                    score: Some(r.score),
                });
            }
        }
    }

    // Sort by score and limit
    all_results.sort_by(|a, b| {
        b.score
            .unwrap_or(0.0)
            .partial_cmp(&a.score.unwrap_or(0.0))
            .unwrap()
    });
    all_results.truncate(limit);

    let total = all_results.len() as u32;

    Ok(Json(ApiResponse {
        success: true,
        data: Some(MnemosSearchResponse {
            results: all_results,
            total,
            meta: ResponseMeta::default(),
        }),
        error: None,
        meta: ResponseMeta::default(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // session_init: project name extraction from folder_path
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_session_init_extracts_project_name() {
        let req = SessionInitRequest {
            folder_path: "/home/user/projects/my-app".to_string(),
            context_hint: None,
        };

        let Json(response) = session_init(Json(req)).await;
        assert!(response.success);

        let data = response.data.unwrap();
        assert_eq!(data.project_name, "my-app");
        assert_eq!(data.project_id, "my-app");
        assert_eq!(data.workspace_id, "default");
    }

    #[tokio::test]
    async fn test_session_init_trailing_slash() {
        let req = SessionInitRequest {
            folder_path: "/home/user/project/".to_string(),
            context_hint: None,
        };

        let Json(response) = session_init(Json(req)).await;
        let data = response.data.unwrap();
        assert_eq!(data.project_name, "project");
    }

    #[tokio::test]
    async fn test_session_init_root_path() {
        let req = SessionInitRequest {
            folder_path: "/".to_string(),
            context_hint: None,
        };

        let Json(response) = session_init(Json(req)).await;
        // With only "/" all split segments are empty, rfind returns None -> "default"
        let data = response.data.unwrap();
        assert_eq!(data.project_name, "default");
    }

    #[tokio::test]
    async fn test_session_init_single_segment() {
        let req = SessionInitRequest {
            folder_path: "my-project".to_string(),
            context_hint: None,
        };

        let Json(response) = session_init(Json(req)).await;
        let data = response.data.unwrap();
        assert_eq!(data.project_name, "my-project");
    }

    // ---------------------------------------------------------------
    // Request deserialization
    // ---------------------------------------------------------------

    #[test]
    fn test_session_init_request_deserialize() {
        let json = r#"{"folder_path": "/tmp/foo"}"#;
        let req: SessionInitRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.folder_path, "/tmp/foo");
        assert!(req.context_hint.is_none());
    }

    #[test]
    fn test_session_init_request_with_context_hint() {
        let json = r#"{"folder_path": "/tmp", "context_hint": "rust"}"#;
        let req: SessionInitRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.context_hint.unwrap(), "rust");
    }

    #[test]
    fn test_context_request_deserialize() {
        let json = r#"{"user_message": "hello"}"#;
        let req: ContextRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.user_message, "hello");
        assert!(req.format.is_none());
        assert!(req.max_tokens.is_none());
    }

    #[test]
    fn test_context_request_with_all_fields() {
        let json = r#"{"user_message": "test", "format": "json", "max_tokens": 500}"#;
        let req: ContextRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.user_message, "test");
        assert_eq!(req.format.unwrap(), "json");
        assert_eq!(req.max_tokens.unwrap(), 500);
    }

    #[test]
    fn test_mnemos_search_request_defaults() {
        let json = r#"{"mode": "search", "query": "hello"}"#;
        let req: MnemosSearchRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.mode, "search");
        assert_eq!(req.query, "hello");
        assert!(req.limit.is_none());
        assert!(req.output_format.is_none());
    }

    #[test]
    fn test_mnemos_search_request_all_fields() {
        let json = r#"{"mode": "grep", "query": "fn main", "limit": 20, "output_format": "compact"}"#;
        let req: MnemosSearchRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.mode, "grep");
        assert_eq!(req.limit.unwrap(), 20);
        assert_eq!(req.output_format.unwrap(), "compact");
    }

    // ---------------------------------------------------------------
    // Response types
    // ---------------------------------------------------------------

    #[test]
    fn test_response_meta_default() {
        let meta = ResponseMeta::default();
        assert_eq!(meta.tokens_used, 0);
    }

    #[test]
    fn test_api_response_serialization_success() {
        let resp = ApiResponse {
            success: true,
            data: Some("hello".to_string()),
            error: None,
            meta: ResponseMeta::default(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["data"], "hello");
        // error should be absent (skip_serializing_if)
        assert!(json.get("error").is_none());
    }

    #[test]
    fn test_api_response_serialization_error() {
        let resp: ApiResponse<String> = ApiResponse {
            success: false,
            data: None,
            error: Some("something went wrong".to_string()),
            meta: ResponseMeta::default(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["success"], false);
        assert!(json["data"].is_null());
        assert_eq!(json["error"], "something went wrong");
    }

    #[test]
    fn test_mnemos_search_result_skip_none_fields() {
        let result = MnemosSearchResult {
            path: "test:doc1".to_string(),
            content: "hello world".to_string(),
            line: None,
            score: None,
        };
        let json = serde_json::to_value(&result).unwrap();
        // line and score should be absent
        assert!(json.get("line").is_none());
        assert!(json.get("score").is_none());
    }

    #[test]
    fn test_context_item_skip_none_relevance() {
        let item = ContextItem {
            content: "test".to_string(),
            source: "memory:1".to_string(),
            relevance: None,
        };
        let json = serde_json::to_value(&item).unwrap();
        assert!(json.get("relevance").is_none());
    }

    #[test]
    fn test_context_item_with_relevance() {
        let item = ContextItem {
            content: "test".to_string(),
            source: "observation:2".to_string(),
            relevance: Some(0.95),
        };
        let json = serde_json::to_value(&item).unwrap();
        let rel = json["relevance"].as_f64().unwrap();
        assert!((rel - 0.95).abs() < 0.001);
    }
}
