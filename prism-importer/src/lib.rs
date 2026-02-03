//! prism-importer: Import data from external search engines into Prism
//!
//! Supported sources:
//! - Elasticsearch 7.x/8.x

pub mod error;
pub mod sources;
pub mod schema;
pub mod progress;

pub use error::{ImportError, Result};
