//! Elasticsearch API compatibility layer for Prism
//!
//! This crate provides a translation layer that allows Elasticsearch clients
//! (like Kibana) to communicate with Prism using familiar ES APIs.
//!
//! # Endpoints
//!
//! All ES-compatible endpoints are served under the `/_elastic` prefix:
//!
//! - `/_elastic/_search` - Search with Query DSL
//! - `/_elastic/_msearch` - Multi-search
//! - `/_elastic/_bulk` - Bulk indexing
//! - `/_elastic/{index}/_mapping` - Field mappings
//! - `/_elastic/_cluster/health` - Cluster health
//! - `/_elastic/_cat/indices` - List indices
//!
//! # Query DSL Support
//!
//! Supported query types:
//! - `bool` (must, should, must_not, filter)
//! - `match` / `match_phrase`
//! - `term` / `terms`
//! - `range`
//! - `exists`
//! - `query_string`
//!
//! Supported aggregations:
//! - `terms`
//! - `date_histogram`
//! - `stats` / `avg` / `sum` / `min` / `max`

pub mod error;
pub mod query;
pub mod response;
pub mod router;

mod endpoints;

pub use error::EsCompatError;
pub use router::es_compat_router;

/// Result type for ES compat operations
pub type Result<T> = std::result::Result<T, EsCompatError>;
