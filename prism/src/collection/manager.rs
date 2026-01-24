use crate::backends::{BackendStats, Document, Query, SearchBackend, SearchResults, TextBackend, VectorBackend, HybridSearchCoordinator};
use crate::schema::{CollectionSchema, SchemaLoader};
use crate::{Error, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub struct CollectionManager {
    schemas: HashMap<String, CollectionSchema>,
    per_collection_backends: HashMap<String, Arc<dyn SearchBackend>>,
    text_backend: Arc<TextBackend>,
    vector_backend: Arc<VectorBackend>,
}

impl CollectionManager {
    pub fn new(schemas_dir: impl AsRef<Path>, text_backend: Arc<TextBackend>, vector_backend: Arc<VectorBackend>) -> Result<Self> {
        let loader = SchemaLoader::new(schemas_dir);
        let schemas = loader.load_all()?;

        // Lint schemas at runtime and fail fast if critical issues found
        let lint_issues = SchemaLoader::lint_all(&schemas);
        if !lint_issues.is_empty() {
            // Aggregate messages
            let mut msgs = Vec::new();
            for (col, issues) in &lint_issues {
                for issue in issues {
                    msgs.push(format!("{}: {}", col, issue));
                }
            }
            return Err(Error::Schema(format!("Schema lint errors:\n{}", msgs.join("\n"))));
        }

        let mut per_collection_backends = HashMap::new();
        for (name, schema) in &schemas {
            // Decide which backend to use per collection
            let use_text = schema.backends.text.is_some();
            let use_vector = schema.backends.vector.is_some();

            if use_text && use_vector {
                let vw = schema.backends.vector.as_ref().map(|v| v.vector_weight).unwrap_or(0.5);
                if !(0.0..=1.0).contains(&vw) {
                    return Err(Error::Schema(format!("vector_weight must be between 0.0 and 1.0 for collection {}", name)));
                }
                let hybrid = HybridSearchCoordinator::new(text_backend.clone(), vector_backend.clone(), vw);
                per_collection_backends.insert(name.clone(), Arc::new(hybrid) as Arc<dyn SearchBackend>);
            } else if use_text {
                per_collection_backends.insert(name.clone(), text_backend.clone() as Arc<dyn SearchBackend>);
            } else if use_vector {
                per_collection_backends.insert(name.clone(), vector_backend.clone() as Arc<dyn SearchBackend>);
            }
        }

        Ok(Self {
            schemas,
            per_collection_backends,
            text_backend: text_backend.clone(),
            vector_backend: vector_backend.clone(),
        })
    }

    pub async fn initialize(&self) -> Result<()> {
        for (name, schema) in &self.schemas {
            if schema.backends.text.is_some() {
                self.text_backend.initialize(name, schema).await?;
            }
            if schema.backends.vector.is_some() {
                self.vector_backend.initialize(name, schema).await?;
            }
        }
        Ok(())
    }

    pub async fn index(&self, collection: &str, docs: Vec<Document>) -> Result<()> {
        let schema = self
            .schemas
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        if let Some(backend) = self.per_collection_backends.get(collection) {
            backend.index(collection, docs).await?;
            return Ok(());
        }

        // Fallback: try text backend
        if schema.backends.text.is_some() {
            self.text_backend.index(collection, docs).await?;
        }

        Ok(())
    }

    pub async fn search(&self, collection: &str, query: Query) -> Result<SearchResults> {
        let schema = self
            .schemas
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        if let Some(backend) = self.per_collection_backends.get(collection) {
            return backend.search(collection, query).await;
        }

        if schema.backends.text.is_some() {
            return self.text_backend.search(collection, query).await;
        }

        Err(Error::Backend(
            "No backend available for collection".to_string(),
        ))
    }

    pub async fn get(&self, collection: &str, id: &str) -> Result<Option<Document>> {
        let schema = self
            .schemas
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        if let Some(backend) = self.per_collection_backends.get(collection) {
            return backend.get(collection, id).await;
        }

        if schema.backends.text.is_some() {
            return self.text_backend.get(collection, id).await;
        }

        Ok(None)
    }

    pub async fn delete(&self, collection: &str, ids: Vec<String>) -> Result<()> {
        let schema = self
            .schemas
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        if let Some(backend) = self.per_collection_backends.get(collection) {
            backend.delete(collection, ids).await?;
            return Ok(());
        }

        if schema.backends.text.is_some() {
            self.text_backend.delete(collection, ids).await?;
        }

        Ok(())
    }

    pub async fn stats(&self, collection: &str) -> Result<BackendStats> {
        let schema = self
            .schemas
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        if let Some(backend) = self.per_collection_backends.get(collection) {
            return backend.stats(collection).await;
        }

        if schema.backends.text.is_some() {
            return self.text_backend.stats(collection).await;
        }

        Err(Error::Backend(
            "No backend available for collection".to_string(),
        ))
    }

    pub fn list_collections(&self) -> Vec<String> {
        self.schemas.keys().cloned().collect()
    }

    pub fn get_schema(&self, collection: &str) -> Option<&CollectionSchema> {
        self.schemas.get(collection)
    }

    /// Run schema linting at runtime and return map collection -> issues
    pub fn lint_schemas(&self) -> std::collections::HashMap<String, Vec<String>> {
        let mut map = std::collections::HashMap::new();
        for (name, schema) in &self.schemas {
            let issues = crate::schema::loader::SchemaLoader::lint_schema(schema);
            if !issues.is_empty() {
                map.insert(name.clone(), issues);
            }
        }
        map
    }

    /// Generate an embedding for query text using the collection's configured embedder.
    /// Requires embedding-gen feature and embedding_generation config in schema.
    pub fn embed_query(&self, _collection: &str, text: &str) -> Result<Vec<f32>> {
        self.vector_backend.embed_query(text)
    }

    /// Perform hybrid search combining text and vector search results.
    ///
    /// # Arguments
    /// * `collection` - Collection name
    /// * `text_query` - Text query string for full-text search
    /// * `vector` - Optional vector for semantic search
    /// * `limit` - Maximum number of results
    /// * `merge_strategy` - "rrf" (default) or "weighted"
    /// * `text_weight` - Weight for text results in weighted merge (default 0.5)
    /// * `vector_weight` - Weight for vector results in weighted merge (default 0.5)
    pub async fn hybrid_search(
        &self,
        collection: &str,
        text_query: &str,
        vector: Option<Vec<f32>>,
        limit: usize,
        merge_strategy: Option<&str>,
        text_weight: Option<f32>,
        vector_weight: Option<f32>,
    ) -> Result<SearchResults> {
        let schema = self
            .schemas
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let has_text = schema.backends.text.is_some();
        let has_vector = schema.backends.vector.is_some();

        // Text-only search
        if !has_vector || vector.is_none() {
            let query = Query {
                query_string: text_query.to_string(),
                fields: vec![],
                limit,
                offset: 0,
                merge_strategy: None,
                text_weight: None,
                vector_weight: None,
            };
            return self.text_backend.search(collection, query).await;
        }

        // Vector-only search (no text backend configured)
        if !has_text {
            let vec = vector.unwrap();
            let query = Query {
                query_string: serde_json::to_string(&vec).unwrap_or_default(),
                fields: vec![],
                limit,
                offset: 0,
                merge_strategy: None,
                text_weight: None,
                vector_weight: None,
            };
            return self.vector_backend.search(collection, query).await;
        }

        // Hybrid search: run both and merge
        let vec = vector.unwrap();

        let text_query_obj = Query {
            query_string: text_query.to_string(),
            fields: vec![],
            limit,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
        };

        let vec_query_obj = Query {
            query_string: serde_json::to_string(&vec).unwrap_or_default(),
            fields: vec![],
            limit,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
        };

        // Run searches in parallel
        let (text_results, vec_results) = tokio::join!(
            self.text_backend.search(collection, text_query_obj),
            self.vector_backend.search(collection, vec_query_obj)
        );

        let text_results = text_results?;
        let vec_results = vec_results?;

        // Merge results
        let merged = match merge_strategy {
            Some("weighted") => {
                let tw = text_weight.unwrap_or(0.5);
                let vw = vector_weight.unwrap_or(0.5);
                HybridSearchCoordinator::merge_weighted_public(text_results, vec_results, tw, vw, limit)
            }
            _ => {
                // Default to RRF
                HybridSearchCoordinator::merge_rrf_public(text_results, vec_results, 60, limit)
            }
        };

        Ok(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::TextBackend;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_collection_manager_search() -> Result<()> {
        let temp = TempDir::new()?;
        let schemas_dir = temp.path().join("schemas");
        let data_dir = temp.path().join("data");

        std::fs::create_dir_all(&schemas_dir)?;

        // Write schema
        fs::write(
            schemas_dir.join("articles.yaml"),
            r#"
collection: articles
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
        stored: true
      - name: content
        type: text
        indexed: true
"#,
        )?;

        let text_backend = Arc::new(TextBackend::new(&data_dir)?);
        let vector_backend = Arc::new(VectorBackend::new(&data_dir)?);
        let manager = CollectionManager::new(&schemas_dir, text_backend, vector_backend)?;
        manager.initialize().await?;

        // Index document
        let doc = Document {
            id: "article1".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Rust Programming")),
                ("content".to_string(), json!("Learn Rust today")),
            ]),
        };

        manager.index("articles", vec![doc]).await?;

        // Search
        let query = Query {
            query_string: "rust".to_string(),
            fields: vec!["title".to_string(), "content".to_string()],
            limit: 10,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
        };

        let results = manager.search("articles", query).await?;
        assert!(results.total > 0);

        Ok(())
    }
}
