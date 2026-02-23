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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_type_all_variants() {
        let cases: Vec<(EsCompatError, &str)> = vec![
            (
                EsCompatError::IndexNotFound("test".into()),
                "index_not_found_exception",
            ),
            (
                EsCompatError::InvalidQuery("bad".into()),
                "query_shard_exception",
            ),
            (
                EsCompatError::UnsupportedQueryType("geo".into()),
                "parsing_exception",
            ),
            (
                EsCompatError::UnsupportedAggregation("composite".into()),
                "parsing_exception",
            ),
            (
                EsCompatError::MissingField("query".into()),
                "parsing_exception",
            ),
            (
                EsCompatError::InvalidRequestBody("malformed".into()),
                "parse_exception",
            ),
            (
                EsCompatError::ParseError("bad json".into()),
                "parse_exception",
            ),
            (
                EsCompatError::Internal("panic".into()),
                "internal_server_error",
            ),
        ];

        for (err, expected) in cases {
            assert_eq!(err.error_type(), expected, "Failed for: {}", err);
        }
    }

    #[test]
    fn test_status_code_not_found() {
        let err = EsCompatError::IndexNotFound("test".into());
        assert_eq!(err.status_code(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_status_code_bad_request() {
        let bad_request_errors = vec![
            EsCompatError::InvalidQuery("x".into()),
            EsCompatError::UnsupportedQueryType("x".into()),
            EsCompatError::UnsupportedAggregation("x".into()),
            EsCompatError::MissingField("x".into()),
            EsCompatError::InvalidRequestBody("x".into()),
            EsCompatError::ParseError("x".into()),
        ];

        for err in bad_request_errors {
            assert_eq!(
                err.status_code(),
                StatusCode::BAD_REQUEST,
                "Expected BAD_REQUEST for: {}",
                err
            );
        }
    }

    #[test]
    fn test_status_code_internal() {
        assert_eq!(
            EsCompatError::Internal("x".into()).status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn test_display_impl() {
        assert_eq!(
            EsCompatError::IndexNotFound("products".into()).to_string(),
            "Index not found: products"
        );
        assert_eq!(
            EsCompatError::InvalidQuery("syntax error".into()).to_string(),
            "Invalid query: syntax error"
        );
    }

    #[test]
    fn test_into_response() {
        let err = EsCompatError::IndexNotFound("test_index".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_into_response_bad_request() {
        let err = EsCompatError::InvalidQuery("bad".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_from_prism_error() {
        let prism_err = prism::Error::CollectionNotFound("test".into());
        let es_err: EsCompatError = prism_err.into();
        assert_eq!(es_err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            es_err.error_type(),
            "search_phase_execution_exception"
        );
    }
}
