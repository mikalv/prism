use crate::schema::{
    Backends, CollectionSchema, FieldType, IndexingConfig, QuotaConfig, TextBackendConfig,
    TextField,
};
use crate::Result;
use std::path::{Path, PathBuf};
use tantivy::Index;

pub struct SchemaDiscoverer {
    old_engraph_path: PathBuf,
}

impl SchemaDiscoverer {
    pub fn new(old_engraph_path: impl AsRef<Path>) -> Self {
        Self {
            old_engraph_path: old_engraph_path.as_ref().to_path_buf(),
        }
    }

    pub fn discover_all(&self) -> Result<Vec<CollectionSchema>> {
        let mut schemas = Vec::new();

        // List all index directories
        for entry in std::fs::read_dir(&self.old_engraph_path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if let Ok(schema) = self.discover_collection(entry.path()) {
                    schemas.push(schema);
                }
            }
        }

        Ok(schemas)
    }

    fn discover_collection(&self, index_path: PathBuf) -> Result<CollectionSchema> {
        let collection_name = index_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| crate::Error::Schema("Invalid collection name".to_string()))?
            .to_string();

        // Try to open Tantivy index
        let index = Index::open_in_dir(&index_path)?;
        let schema = index.schema();

        // Convert Tantivy schema to our YAML schema
        let mut fields = Vec::new();

        for (_field, field_entry) in schema.fields() {
            if field_entry.name() == "id" {
                continue; // Skip ID field, it's implicit
            }

            let field_type = match field_entry.field_type() {
                tantivy::schema::FieldType::Str(_) => {
                    // Check if it's TEXT or STRING
                    if field_entry.is_indexed() {
                        FieldType::Text
                    } else {
                        FieldType::String
                    }
                }
                tantivy::schema::FieldType::U64(_) => FieldType::U64,
                tantivy::schema::FieldType::I64(_) => FieldType::I64,
                tantivy::schema::FieldType::F64(_) => FieldType::F64,
                tantivy::schema::FieldType::Bool(_) => FieldType::Bool,
                tantivy::schema::FieldType::Date(_) => FieldType::Date,
                tantivy::schema::FieldType::Bytes(_) => FieldType::Bytes,
                _ => continue,
            };

            fields.push(TextField {
                name: field_entry.name().to_string(),
                field_type,
                stored: field_entry.is_stored(),
                indexed: field_entry.is_indexed(),
            });
        }

        Ok(CollectionSchema {
            collection: collection_name,
            description: None,
            backends: Backends {
                text: Some(TextBackendConfig { fields }),
                vector: None,
                graph: None,
            },
            indexing: IndexingConfig::default(),
            quota: QuotaConfig::default(),
            embedding_generation: None,
            facets: None,
            boosting: None,
            storage: Default::default(),
        })
    }

    pub fn write_schemas(
        &self,
        output_dir: impl AsRef<Path>,
        schemas: &[CollectionSchema],
    ) -> Result<()> {
        std::fs::create_dir_all(&output_dir)?;

        for schema in schemas {
            let filename = format!("{}.yaml", schema.collection);
            let path = output_dir.as_ref().join(filename);
            let yaml = serde_yaml::to_string(schema)?;
            std::fs::write(path, yaml)?;
        }

        Ok(())
    }
}
