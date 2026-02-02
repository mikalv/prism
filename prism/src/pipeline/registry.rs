use super::processors::*;
use super::Processor;
use crate::backends::Document;
use crate::error::Error;
use crate::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// A pipeline is a named, ordered list of processors.
pub struct Pipeline {
    pub name: String,
    pub description: String,
    pub processors: Vec<Box<dyn Processor>>,
}

impl Pipeline {
    /// Run all processors on a document in order.
    pub fn process(&self, doc: &mut Document) -> Result<()> {
        for proc in &self.processors {
            proc.process(doc)?;
        }
        Ok(())
    }
}

/// Registry holding all loaded pipelines.
pub struct PipelineRegistry {
    pipelines: HashMap<String, Pipeline>,
}

impl PipelineRegistry {
    /// Load all YAML pipeline definitions from a directory.
    pub fn load(dir: &Path) -> Result<Self> {
        let mut pipelines = HashMap::new();

        if !dir.exists() {
            return Ok(Self { pipelines });
        }

        let entries = std::fs::read_dir(dir).map_err(|e| {
            Error::Config(format!(
                "Cannot read pipeline dir '{}': {}",
                dir.display(),
                e
            ))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| Error::Config(e.to_string()))?;
            let path = entry.path();
            if path
                .extension()
                .map_or(false, |e| e == "yaml" || e == "yml")
            {
                let content = std::fs::read_to_string(&path).map_err(|e| {
                    Error::Config(format!("Cannot read '{}': {}", path.display(), e))
                })?;
                let def: PipelineDef = serde_yaml::from_str(&content)?;
                let pipeline = def.into_pipeline()?;
                pipelines.insert(pipeline.name.clone(), pipeline);
            }
        }

        Ok(Self { pipelines })
    }

    /// Get a pipeline by name.
    pub fn get(&self, name: &str) -> Option<&Pipeline> {
        self.pipelines.get(name)
    }

    /// List all pipelines as (name, description, processor_count).
    pub fn list(&self) -> Vec<(String, String, usize)> {
        self.pipelines
            .iter()
            .map(|(_, p)| (p.name.clone(), p.description.clone(), p.processors.len()))
            .collect()
    }

    /// Create an empty registry.
    pub fn empty() -> Self {
        Self {
            pipelines: HashMap::new(),
        }
    }
}

// -- YAML deserialization types -----------------------------------------------

#[derive(Deserialize)]
struct PipelineDef {
    name: String,
    #[serde(default)]
    description: String,
    processors: Vec<serde_yaml::Value>,
}

/// Each processor entry in YAML is a single-key map like `lowercase: { field: title }`.
/// We deserialize each entry as a HashMap<String, serde_yaml::Value> and dispatch on the key.
impl PipelineDef {
    fn into_pipeline(self) -> Result<Pipeline> {
        let mut processors: Vec<Box<dyn Processor>> = Vec::with_capacity(self.processors.len());

        for entry in self.processors {
            let map = entry.as_mapping().ok_or_else(|| {
                Error::Config("processor entry must be a YAML mapping".to_string())
            })?;
            if map.len() != 1 {
                return Err(Error::Config(
                    "each processor entry must have exactly one key".to_string(),
                ));
            }
            let (key, params) = map.iter().next().unwrap();
            let name = key
                .as_str()
                .ok_or_else(|| Error::Config("processor name must be a string".to_string()))?;

            let proc: Box<dyn Processor> = match name {
                "lowercase" => {
                    let field = get_string_field(params, "field")?;
                    Box::new(LowercaseProcessor { field })
                }
                "html_strip" => {
                    let field = get_string_field(params, "field")?;
                    Box::new(HtmlStripProcessor { field })
                }
                "set" => {
                    let field = get_string_field(params, "field")?;
                    let value = get_string_field(params, "value")?;
                    Box::new(SetProcessor { field, value })
                }
                "remove" => {
                    let field = get_string_field(params, "field")?;
                    Box::new(RemoveProcessor { field })
                }
                "rename" => {
                    let from = get_string_field(params, "from")?;
                    let to = get_string_field(params, "to")?;
                    Box::new(RenameProcessor { from, to })
                }
                other => return Err(Error::Config(format!("unknown processor type: {}", other))),
            };
            processors.push(proc);
        }

        Ok(Pipeline {
            name: self.name,
            description: self.description,
            processors,
        })
    }
}

fn get_string_field(params: &serde_yaml::Value, field: &str) -> Result<String> {
    params
        .get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| Error::Config(format!("missing or non-string field '{}'", field)))
}
