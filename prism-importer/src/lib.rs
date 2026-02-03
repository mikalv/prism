//! prism-importer: Import data from external search engines into Prism
//!
//! Supported sources:
//! - Elasticsearch 7.x/8.x

pub mod error;
pub mod progress;
pub mod schema;
pub mod sources;

pub use error::{ImportError, Result};
pub use progress::ImportProgress;
pub use schema::{SourceField, SourceFieldType, SourceSchema};
pub use sources::{AuthMethod, ElasticsearchSource, ImportSource};
