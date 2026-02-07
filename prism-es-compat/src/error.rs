//! Error types for ES compatibility layer

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// ES compatibility layer errors
#[derive(Debug, thiserror::Error)]
pub enum EsCompatError {
    #[error("Index not found: {0}")]
    IndexNotFound(String),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Unsupported query type: {0}")]
    UnsupportedQueryType(String),

    #[error("Unsupported aggregation: {0}")]
    UnsupportedAggregation(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid request body: {0}")]
    InvalidRequestBody(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Prism error: {0}")]
    PrismError(#[from] prism::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Elasticsearch-style error response
#[derive(Debug, Serialize)]
struct EsErrorResponse {
    error: EsErrorDetail,
    status: u16,
}

#[derive(Debug, Serialize)]
struct EsErrorDetail {
    root_cause: Vec<RootCause>,
    #[serde(rename = "type")]
    error_type: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct RootCause {
    #[serde(rename = "type")]
    error_type: String,
    reason: String,
}

impl EsCompatError {
    fn error_type(&self) -> &'static str {
        match self {
            Self::IndexNotFound(_) => "index_not_found_exception",
            Self::InvalidQuery(_) => "query_shard_exception",
            Self::UnsupportedQueryType(_) => "parsing_exception",
            Self::UnsupportedAggregation(_) => "parsing_exception",
            Self::MissingField(_) => "parsing_exception",
            Self::InvalidRequestBody(_) => "parse_exception",
            Self::ParseError(_) => "parse_exception",
            Self::PrismError(_) => "search_phase_execution_exception",
            Self::Internal(_) => "internal_server_error",
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Self::IndexNotFound(_) => StatusCode::NOT_FOUND,
            Self::InvalidQuery(_)
            | Self::UnsupportedQueryType(_)
            | Self::UnsupportedAggregation(_)
            | Self::MissingField(_)
            | Self::InvalidRequestBody(_)
            | Self::ParseError(_) => StatusCode::BAD_REQUEST,
            Self::PrismError(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for EsCompatError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let error_type = self.error_type().to_string();
        let reason = self.to_string();

        let body = EsErrorResponse {
            error: EsErrorDetail {
                root_cause: vec![RootCause {
                    error_type: error_type.clone(),
                    reason: reason.clone(),
                }],
                error_type,
                reason,
            },
            status: status.as_u16(),
        };

        (status, axum::Json(body)).into_response()
    }
}
