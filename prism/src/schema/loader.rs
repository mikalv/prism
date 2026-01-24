use crate::schema::CollectionSchema;
use crate::{Error, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct SchemaLoader {
    schemas_dir: PathBuf,
}

impl SchemaLoader {
    pub fn new(schemas_dir: impl AsRef<Path>) -> Self {
        Self {
            schemas_dir: schemas_dir.as_ref().to_path_buf(),
        }
    }

    pub fn load_all(&self) -> Result<HashMap<String, CollectionSchema>> {
        let mut schemas = HashMap::new();

        if !self.schemas_dir.exists() {
            return Err(Error::Schema(format!(
                "Schemas directory does not exist: {}",
                self.schemas_dir.display()
            )));
        }

        for entry in fs::read_dir(&self.schemas_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }

            let schema = self.load_schema(&path)?;
            schemas.insert(schema.collection.clone(), schema);
        }

        Ok(schemas)
    }

    pub fn load_schema(&self, path: &Path) -> Result<CollectionSchema> {
        let content = fs::read_to_string(path)?;
        let schema: CollectionSchema = serde_yaml::from_str(&content)?;
        Ok(schema)
    }

    /// Lint a single schema and return a list of human-readable issues (empty = ok)
    pub fn lint_schema(schema: &CollectionSchema) -> Vec<String> {
        let mut issues = Vec::new();
        if let Some(v) = &schema.backends.vector {
            if v.dimension == 0 {
                issues.push("vector.dimension must be > 0".to_string());
            }
            if v.embedding_field.trim().is_empty() {
                issues.push("vector.embedding_field must be set".to_string());
            }
            if !(0.0..=1.0).contains(&v.vector_weight) {
                issues.push(format!("vector.vector_weight must be between 0.0 and 1.0 (got {})", v.vector_weight));
            }
        }
        if let Some(t) = &schema.backends.text {
            if t.fields.is_empty() {
                issues.push("text.fields should have at least one field defined".to_string());
            }
        }
        issues
    }

    /// Lint all loaded schemas and return map collection -> issues
    pub fn lint_all(schemas: &HashMap<String, CollectionSchema>) -> HashMap<String, Vec<String>> {
        let mut map = HashMap::new();
        for (name, schema) in schemas {
            let issues = SchemaLoader::lint_schema(schema);
            if !issues.is_empty() {
                map.insert(name.clone(), issues);
            }
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_load_schemas_from_directory() -> Result<()> {
        let temp = TempDir::new()?;
        let schema_dir = temp.path();

        // Write test schema
        fs::write(
            schema_dir.join("test.yaml"),
            r#"
collection: test_collection
backends:
  text:
    fields:
      - name: content
        type: text
        indexed: true
"#,
        )?;

        let loader = SchemaLoader::new(schema_dir);
        let schemas = loader.load_all()?;

        assert_eq!(schemas.len(), 1);
        assert!(schemas.contains_key("test_collection"));

        Ok(())
    }
}
