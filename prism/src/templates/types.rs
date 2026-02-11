//! Index Template types
//!
//! Templates automatically apply settings, schema fields, and aliases
//! to new collections matching a pattern.

use crate::schema::types::{
    FieldType, IndexingConfig, QuotaConfig, SystemFieldsConfig, TextField, TokenizerType,
    TreeSitterOptions,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// An index template definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexTemplate {
    /// Template name (unique identifier)
    pub name: String,

    /// Index patterns to match (supports wildcards like "logs-*")
    pub index_patterns: Vec<String>,

    /// Priority for template selection (higher wins)
    #[serde(default)]
    pub priority: u32,

    /// Template settings to apply
    #[serde(default)]
    pub settings: TemplateSettings,

    /// Schema fields to add
    #[serde(default)]
    pub schema: TemplateSchema,

    /// Aliases to create for matching indices
    #[serde(default)]
    pub aliases: HashMap<String, AliasDefinition>,
}

/// Template settings
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TemplateSettings {
    /// ILM policy to attach
    #[serde(default)]
    pub ilm_policy: Option<String>,

    /// Indexing configuration
    #[serde(default)]
    pub indexing: Option<IndexingConfig>,

    /// Quota configuration
    #[serde(default)]
    pub quota: Option<QuotaConfig>,

    /// System fields configuration
    #[serde(default)]
    pub system_fields: Option<SystemFieldsConfig>,
}

/// Template schema definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TemplateSchema {
    /// Text fields to add
    #[serde(default)]
    pub text_fields: Vec<TemplateTextField>,

    /// Vector configuration (if any)
    #[serde(default)]
    pub vector: Option<TemplateVectorConfig>,
}

/// Template text field definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateTextField {
    pub name: String,
    #[serde(rename = "type", default = "default_field_type")]
    pub field_type: FieldType,
    #[serde(default = "default_true")]
    pub indexed: bool,
    #[serde(default)]
    pub stored: bool,
    #[serde(default)]
    pub tokenizer: Option<TokenizerType>,
    #[serde(default)]
    pub tokenizer_options: Option<TreeSitterOptions>,
}

fn default_field_type() -> FieldType {
    FieldType::Text
}

fn default_true() -> bool {
    true
}

impl From<TemplateTextField> for TextField {
    fn from(t: TemplateTextField) -> Self {
        TextField {
            name: t.name,
            field_type: t.field_type,
            indexed: t.indexed,
            stored: t.stored,
            tokenizer: t.tokenizer,
            tokenizer_options: t.tokenizer_options,
        }
    }
}

/// Template vector configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateVectorConfig {
    pub embedding_field: String,
    pub dimension: usize,
    #[serde(default = "default_distance")]
    pub distance: VectorDistance,
}

fn default_distance() -> VectorDistance {
    VectorDistance::Cosine
}

/// Vector distance metric
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum VectorDistance {
    #[default]
    Cosine,
    Euclidean,
    DotProduct,
}

/// Alias definition within a template
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AliasDefinition {
    /// Whether this is the write index for the alias
    #[serde(default)]
    pub is_write_index: bool,

    /// Optional filter (for filtered aliases)
    #[serde(default)]
    pub filter: Option<serde_json::Value>,

    /// Optional routing value
    #[serde(default)]
    pub routing: Option<String>,
}

/// Result of template matching
#[derive(Debug, Clone)]
pub struct TemplateMatch {
    /// The matched template
    pub template: IndexTemplate,

    /// The pattern that matched
    pub matched_pattern: String,
}

/// Collection of templates with metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TemplateRegistry {
    /// All registered templates
    pub templates: HashMap<String, IndexTemplate>,

    /// Version for optimistic concurrency
    #[serde(default)]
    pub version: u64,
}

impl TemplateRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update a template
    pub fn upsert(&mut self, template: IndexTemplate) {
        self.templates.insert(template.name.clone(), template);
        self.version += 1;
    }

    /// Remove a template by name
    pub fn remove(&mut self, name: &str) -> Option<IndexTemplate> {
        let result = self.templates.remove(name);
        if result.is_some() {
            self.version += 1;
        }
        result
    }

    /// Get a template by name
    pub fn get(&self, name: &str) -> Option<&IndexTemplate> {
        self.templates.get(name)
    }

    /// List all templates
    pub fn list(&self) -> Vec<&IndexTemplate> {
        self.templates.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_serialization() {
        let template = IndexTemplate {
            name: "logs".to_string(),
            index_patterns: vec!["logs-*".to_string()],
            priority: 100,
            settings: TemplateSettings {
                ilm_policy: Some("logs_policy".to_string()),
                ..Default::default()
            },
            schema: TemplateSchema {
                text_fields: vec![TemplateTextField {
                    name: "message".to_string(),
                    field_type: FieldType::Text,
                    indexed: true,
                    stored: true,
                    tokenizer: None,
                    tokenizer_options: None,
                }],
                vector: None,
            },
            aliases: HashMap::from([("logs-read".to_string(), AliasDefinition::default())]),
        };

        let json = serde_json::to_string_pretty(&template).unwrap();
        let parsed: IndexTemplate = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "logs");
        assert_eq!(parsed.index_patterns, vec!["logs-*"]);
    }
}
