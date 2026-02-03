use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;
use crate::error::Result;
use crate::schema::SourceSchema;
use super::traits::{ImportSource, SourceDocument};

pub struct ElasticsearchSource {
    pub base_url: url::Url,
    pub index: String,
    pub batch_size: usize,
}

impl ElasticsearchSource {
    pub fn new(base_url: url::Url, index: String) -> Self {
        Self {
            base_url,
            index,
            batch_size: 1000,
        }
    }
}

#[async_trait]
impl ImportSource for ElasticsearchSource {
    async fn fetch_schema(&self) -> Result<SourceSchema> {
        todo!("Implement in Task 5")
    }

    async fn count_documents(&self) -> Result<u64> {
        todo!("Implement in Task 6")
    }

    fn stream_documents(&self) -> Pin<Box<dyn Stream<Item = Result<SourceDocument>> + Send + '_>> {
        todo!("Implement in Task 7")
    }

    fn source_name(&self) -> &str {
        "elasticsearch"
    }
}
