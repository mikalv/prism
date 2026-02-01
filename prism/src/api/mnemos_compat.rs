//! Mnemos-compatible API routes
//!
//! Provides `/api/*` routes compatible with the mnemos MCP server.

use crate::backends::Query;
use crate::collection::CollectionManager;
use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
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
    let project_name = req.folder_path
        .split('/')
        .filter(|s| !s.is_empty())
        .last()
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
    };

    let mut context_items = Vec::new();

    // Search observations
    if let Ok(results) = manager.search("observations", query.clone()).await {
        for r in results.results {
            let content = r.fields.get("content")
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
    };

    if let Ok(results) = manager.search("memories", query2).await {
        for r in results.results {
            let content = r.fields.get("content")
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
        b.relevance.unwrap_or(0.0).partial_cmp(&a.relevance.unwrap_or(0.0)).unwrap()
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
    };

    let mut all_results = Vec::new();

    // Search both collections
    for collection in ["observations", "memories"] {
        if let Ok(results) = manager.search(collection, query.clone()).await {
            for r in results.results {
                let content = r.fields.get("content")
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
        b.score.unwrap_or(0.0).partial_cmp(&a.score.unwrap_or(0.0)).unwrap()
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
