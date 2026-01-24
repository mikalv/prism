//! Query DSL parsing and execution for Prism
//!
//! Provides Lucene-style query syntax with facets, boosting, and filters.

pub mod aggregations;
pub mod ast;
pub mod engine;
pub mod parser;
pub mod suggestions;

pub use aggregations::{AggregationRequest, AggregationResult, AggregationType};
pub use suggestions::{suggest_corrections, suggest_query_corrections, Suggestion};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum QueryError {
    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Invalid field: {0}")]
    InvalidField(String),

    #[error("Schema error: {0}")]
    SchemaError(String),

    #[error("Tantivy error: {0}")]
    TantivyError(String),
}

pub type Result<T> = std::result::Result<T, QueryError>;

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn test_invalid_field_error() {
        let err = QueryError::InvalidField("unknown_field".to_string());
        assert!(err.to_string().contains("unknown_field"));
    }

    #[test]
    fn test_schema_error() {
        let err = QueryError::SchemaError("missing schema".to_string());
        assert!(err.to_string().contains("missing schema"));
    }

    #[test]
    fn test_tantivy_error() {
        let err = QueryError::TantivyError("index error".to_string());
        assert!(err.to_string().contains("index error"));
    }
}
