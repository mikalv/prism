//! Index Templates
//!
//! Templates automatically apply settings, schema fields, and aliases
//! to new collections matching a pattern.
//!
//! # Example
//!
//! ```yaml
//! templates:
//!   logs_template:
//!     index_patterns: ["logs-*"]
//!     priority: 100
//!     settings:
//!       ilm_policy: "logs_policy"
//!     schema:
//!       text_fields:
//!         - name: message
//!           type: text
//!           indexed: true
//!         - name: level
//!           type: string
//!     aliases:
//!       logs-read: {}
//! ```

pub mod matcher;
pub mod types;

pub use matcher::TemplateMatcher;
pub use types::{
    AliasDefinition, IndexTemplate, TemplateMatch, TemplateRegistry, TemplateSchema,
    TemplateSettings, TemplateTextField,
};

use crate::schema::types::{
    Backends, CollectionSchema, TextBackendConfig, VectorBackendConfig, VectorDistance,
};
use crate::{Error, Result};
use std::path::Path;
use tokio::sync::RwLock;

/// Template Manager handles template storage and application
pub struct TemplateManager {
    /// Template registry
    registry: RwLock<TemplateRegistry>,

    /// Path for persisting templates
    storage_path: std::path::PathBuf,
}

impl TemplateManager {
    /// Create a new TemplateManager
    pub fn new(data_dir: impl AsRef<Path>) -> Result<Self> {
        let storage_path = data_dir.as_ref().join("templates.json");

        // Load existing templates if available
        let registry = if storage_path.exists() {
            let content = std::fs::read_to_string(&storage_path).map_err(|e| {
                Error::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to read templates: {}", e),
                ))
            })?;
            serde_json::from_str(&content)
                .map_err(|e| Error::Config(format!("Failed to parse templates: {}", e)))?
        } else {
            TemplateRegistry::new()
        };

        Ok(Self {
            registry: RwLock::new(registry),
            storage_path,
        })
    }

    /// Create a new TemplateManager with an empty registry (for testing)
    pub fn empty() -> Self {
        Self {
            registry: RwLock::new(TemplateRegistry::new()),
            storage_path: std::path::PathBuf::new(),
        }
    }

    /// Persist templates to disk
    async fn persist(&self) -> Result<()> {
        if self.storage_path.as_os_str().is_empty() {
            return Ok(()); // No persistence path configured
        }

        let registry = self.registry.read().await;
        let content = serde_json::to_string_pretty(&*registry)
            .map_err(|e| Error::Config(format!("Failed to serialize templates: {}", e)))?;

        // Ensure parent directory exists
        if let Some(parent) = self.storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&self.storage_path, content)?;
        Ok(())
    }

    /// Create or update a template
    pub async fn put_template(&self, template: IndexTemplate) -> Result<()> {
        // Validate template
        if template.name.is_empty() {
            return Err(Error::Config("Template name cannot be empty".to_string()));
        }
        if template.index_patterns.is_empty() {
            return Err(Error::Config(
                "Template must have at least one index pattern".to_string(),
            ));
        }

        // Validate patterns are valid
        for pattern in &template.index_patterns {
            if pattern.is_empty() {
                return Err(Error::Config("Index pattern cannot be empty".to_string()));
            }
        }

        let mut registry = self.registry.write().await;
        registry.upsert(template);
        drop(registry);

        self.persist().await?;
        Ok(())
    }

    /// Get a template by name
    pub async fn get_template(&self, name: &str) -> Option<IndexTemplate> {
        let registry = self.registry.read().await;
        registry.get(name).cloned()
    }

    /// Delete a template by name
    pub async fn delete_template(&self, name: &str) -> Result<Option<IndexTemplate>> {
        let mut registry = self.registry.write().await;
        let removed = registry.remove(name);
        drop(registry);

        if removed.is_some() {
            self.persist().await?;
        }

        Ok(removed)
    }

    /// List all templates
    pub async fn list_templates(&self) -> Vec<IndexTemplate> {
        let registry = self.registry.read().await;
        registry.list().into_iter().cloned().collect()
    }

    /// Find matching templates for an index name
    pub async fn find_matches(&self, index_name: &str) -> Vec<TemplateMatch> {
        let registry = self.registry.read().await;
        TemplateMatcher::find_matches(&registry, index_name)
    }

    /// Find the best matching template for an index name
    pub async fn find_best_match(&self, index_name: &str) -> Option<TemplateMatch> {
        let registry = self.registry.read().await;
        TemplateMatcher::find_best_match(&registry, index_name)
    }

    /// Apply a template to create a collection schema
    pub fn apply_template(template: &IndexTemplate, collection_name: &str) -> CollectionSchema {
        // Build text backend config from template
        let text = if !template.schema.text_fields.is_empty() {
            Some(TextBackendConfig {
                fields: template
                    .schema
                    .text_fields
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .collect(),
                bm25_k1: None,
                bm25_b: None,
            })
        } else {
            None
        };

        // Build vector backend config from template
        let vector = template
            .schema
            .vector
            .as_ref()
            .map(|v| VectorBackendConfig {
                embedding_field: v.embedding_field.clone(),
                dimension: v.dimension,
                distance: match v.distance {
                    types::VectorDistance::Cosine => VectorDistance::Cosine,
                    types::VectorDistance::Euclidean => VectorDistance::Euclidean,
                    types::VectorDistance::DotProduct => VectorDistance::Dot,
                },
                hnsw_m: 16,
                hnsw_ef_construction: 200,
                hnsw_ef_search: 100,
                vector_weight: 0.5,
                num_shards: 1,
                shard_oversample: 2.5,
                compaction: Default::default(),
            });

        CollectionSchema {
            collection: collection_name.to_string(),
            description: Some(format!("Created from template '{}'", template.name)),
            backends: Backends {
                text,
                vector,
                graph: None,
            },
            indexing: template.settings.indexing.clone().unwrap_or_default(),
            quota: template.settings.quota.clone().unwrap_or_default(),
            embedding_generation: None,
            facets: None,
            boosting: None,
            storage: Default::default(),
            system_fields: template.settings.system_fields.clone().unwrap_or_default(),
            hybrid: None,
            replication: None,
            reranking: None,
            ilm_policy: template.settings.ilm_policy.clone(),
        }
    }

    /// Merge template settings with an existing schema
    /// Template fields are added if not already present in the schema
    pub fn merge_with_schema(
        template: &IndexTemplate,
        mut schema: CollectionSchema,
    ) -> CollectionSchema {
        // Apply ILM policy if not set
        if schema.ilm_policy.is_none() {
            schema.ilm_policy = template.settings.ilm_policy.clone();
        }

        // Merge text fields
        if let Some(ref mut text_config) = schema.backends.text {
            let existing_names: std::collections::HashSet<_> =
                text_config.fields.iter().map(|f| f.name.clone()).collect();

            for field in &template.schema.text_fields {
                if !existing_names.contains(&field.name) {
                    text_config.fields.push(field.clone().into());
                }
            }
        } else if !template.schema.text_fields.is_empty() {
            schema.backends.text = Some(TextBackendConfig {
                fields: template
                    .schema
                    .text_fields
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .collect(),
                bm25_k1: None,
                bm25_b: None,
            });
        }

        // Apply vector config if not set
        if schema.backends.vector.is_none() {
            if let Some(ref v) = template.schema.vector {
                schema.backends.vector = Some(VectorBackendConfig {
                    embedding_field: v.embedding_field.clone(),
                    dimension: v.dimension,
                    distance: match v.distance {
                        types::VectorDistance::Cosine => VectorDistance::Cosine,
                        types::VectorDistance::Euclidean => VectorDistance::Euclidean,
                        types::VectorDistance::DotProduct => VectorDistance::Dot,
                    },
                    hnsw_m: 16,
                    hnsw_ef_construction: 200,
                    hnsw_ef_search: 100,
                    vector_weight: 0.5,
                    num_shards: 1,
                    shard_oversample: 2.5,
                    compaction: Default::default(),
                });
            }
        }

        schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::types::FieldType;
    use std::collections::HashMap;

    fn create_logs_template() -> IndexTemplate {
        IndexTemplate {
            name: "logs".to_string(),
            index_patterns: vec!["logs-*".to_string()],
            priority: 100,
            settings: TemplateSettings {
                ilm_policy: Some("logs_policy".to_string()),
                ..Default::default()
            },
            schema: TemplateSchema {
                text_fields: vec![
                    TemplateTextField {
                        name: "message".to_string(),
                        field_type: FieldType::Text,
                        indexed: true,
                        stored: true,
                        tokenizer: None,
                    },
                    TemplateTextField {
                        name: "level".to_string(),
                        field_type: FieldType::String,
                        indexed: true,
                        stored: true,
                        tokenizer: None,
                    },
                ],
                vector: None,
            },
            aliases: HashMap::from([("logs-read".to_string(), AliasDefinition::default())]),
        }
    }

    #[test]
    fn test_apply_template() {
        let template = create_logs_template();
        let schema = TemplateManager::apply_template(&template, "logs-2026.02.07");

        assert_eq!(schema.collection, "logs-2026.02.07");
        assert_eq!(schema.ilm_policy, Some("logs_policy".to_string()));

        let text = schema.backends.text.unwrap();
        assert_eq!(text.fields.len(), 2);
        assert_eq!(text.fields[0].name, "message");
        assert_eq!(text.fields[1].name, "level");
    }

    #[test]
    fn test_merge_with_schema() {
        let template = create_logs_template();

        // Create a schema with one existing field
        let schema = CollectionSchema {
            collection: "logs-2026.02.07".to_string(),
            description: None,
            backends: Backends {
                text: Some(TextBackendConfig {
                    fields: vec![crate::schema::TextField {
                        name: "custom".to_string(),
                        field_type: FieldType::Text,
                        indexed: true,
                        stored: false,
                        tokenizer: None,
                    }],
                    bm25_k1: None,
                    bm25_b: None,
                }),
                vector: None,
                graph: None,
            },
            indexing: Default::default(),
            quota: Default::default(),
            embedding_generation: None,
            facets: None,
            boosting: None,
            storage: Default::default(),
            system_fields: Default::default(),
            hybrid: None,
            replication: None,
            reranking: None,
            ilm_policy: None,
        };

        let merged = TemplateManager::merge_with_schema(&template, schema);

        // Should have ILM policy from template
        assert_eq!(merged.ilm_policy, Some("logs_policy".to_string()));

        // Should have all 3 fields (1 existing + 2 from template)
        let text = merged.backends.text.unwrap();
        assert_eq!(text.fields.len(), 3);

        let names: Vec<_> = text.fields.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"custom"));
        assert!(names.contains(&"message"));
        assert!(names.contains(&"level"));
    }

    #[tokio::test]
    async fn test_template_manager_crud() {
        let manager = TemplateManager::empty();

        // Put template
        manager.put_template(create_logs_template()).await.unwrap();

        // Get template
        let template = manager.get_template("logs").await;
        assert!(template.is_some());
        assert_eq!(template.unwrap().name, "logs");

        // List templates
        let templates = manager.list_templates().await;
        assert_eq!(templates.len(), 1);

        // Find match
        let matches = manager.find_matches("logs-2026.02.07").await;
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].template.name, "logs");

        // No match
        let matches = manager.find_matches("metrics-2026").await;
        assert!(matches.is_empty());

        // Delete
        let deleted = manager.delete_template("logs").await.unwrap();
        assert!(deleted.is_some());

        let templates = manager.list_templates().await;
        assert!(templates.is_empty());
    }
}
