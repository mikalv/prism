use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;
use crate::Result;
use crate::schema::SourceSchema;

/// A document from an external source
#[derive(Debug, Clone)]
pub struct SourceDocument {
    pub id: String,
    pub fields: serde_json::Value,
}

/// Trait for import sources (Elasticsearch, Solr, etc.)
#[async_trait]
pub trait ImportSource: Send + Sync {
    /// Fetch the schema/mapping from the source
    async fn fetch_schema(&self) -> Result<SourceSchema>;

    /// Get total document count (for progress bar)
    async fn count_documents(&self) -> Result<u64>;

    /// Stream documents from the source
    fn stream_documents(&self) -> Pin<Box<dyn Stream<Item = Result<SourceDocument>> + Send + '_>>;

    /// Human-readable source name
    fn source_name(&self) -> &str;
}
