# Ingest Pipelines Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a pipeline system that loads processor chains from YAML files and applies them to documents before indexing, referenced via `?pipeline=name` query parameter.

**Architecture:** A new `pipeline` module provides a `Processor` trait, five built-in processors (lowercase, html_strip, set, remove, rename), a `PipelineRegistry` that loads YAML definitions from disk, and integration into the existing index_documents route. Pipelines are standalone YAML files in `conf/pipelines/`.

**Tech Stack:** serde_yaml for pipeline parsing, regex for html_strip, chrono for `{{_now}}` expansion

---

### Task 1: Add Processor trait and basic processors

**Files:**
- Create: `/home/meeh/prism/prism/src/pipeline/mod.rs`
- Create: `/home/meeh/prism/prism/src/pipeline/processors.rs`
- Create: `/home/meeh/prism/prism/tests/pipeline_test.rs`
- Modify: `/home/meeh/prism/prism/src/lib.rs` (add `pub mod pipeline;` after line 11)

**Step 1: Write the failing tests**

Create `/home/meeh/prism/prism/tests/pipeline_test.rs`:

```rust
use prism::backends::Document;
use prism::pipeline::processors::*;
use prism::pipeline::Processor;
use std::collections::HashMap;
use serde_json::Value;

fn make_doc(fields: Vec<(&str, Value)>) -> Document {
    Document {
        id: "test-1".to_string(),
        fields: fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
    }
}

#[test]
fn test_lowercase_processor() {
    let proc = LowercaseProcessor { field: "title".to_string() };
    let mut doc = make_doc(vec![("title", Value::String("Hello WORLD".to_string()))]);
    proc.process(&mut doc).unwrap();
    assert_eq!(doc.fields["title"], Value::String("hello world".to_string()));
}

#[test]
fn test_lowercase_missing_field_errors() {
    let proc = LowercaseProcessor { field: "missing".to_string() };
    let mut doc = make_doc(vec![("title", Value::String("Hello".to_string()))]);
    assert!(proc.process(&mut doc).is_err());
}

#[test]
fn test_lowercase_non_string_errors() {
    let proc = LowercaseProcessor { field: "count".to_string() };
    let mut doc = make_doc(vec![("count", Value::Number(42.into()))]);
    assert!(proc.process(&mut doc).is_err());
}

#[test]
fn test_html_strip_processor() {
    let proc = HtmlStripProcessor { field: "content".to_string() };
    let mut doc = make_doc(vec![
        ("content", Value::String("<p>Hello <b>world</b></p>".to_string())),
    ]);
    proc.process(&mut doc).unwrap();
    assert_eq!(doc.fields["content"], Value::String("Hello world".to_string()));
}

#[test]
fn test_set_processor_static_value() {
    let proc = SetProcessor { field: "status".to_string(), value: "indexed".to_string() };
    let mut doc = make_doc(vec![]);
    proc.process(&mut doc).unwrap();
    assert_eq!(doc.fields["status"], Value::String("indexed".to_string()));
}

#[test]
fn test_set_processor_now_template() {
    let proc = SetProcessor { field: "ts".to_string(), value: "{{_now}}".to_string() };
    let mut doc = make_doc(vec![]);
    proc.process(&mut doc).unwrap();
    // Should be an ISO8601 timestamp string
    let val = doc.fields["ts"].as_str().unwrap();
    assert!(val.contains("T"), "Expected ISO8601 timestamp, got: {}", val);
}

#[test]
fn test_remove_processor() {
    let proc = RemoveProcessor { field: "secret".to_string() };
    let mut doc = make_doc(vec![
        ("title", Value::String("hi".to_string())),
        ("secret", Value::String("password".to_string())),
    ]);
    proc.process(&mut doc).unwrap();
    assert!(!doc.fields.contains_key("secret"));
    assert!(doc.fields.contains_key("title"));
}

#[test]
fn test_remove_missing_field_is_ok() {
    let proc = RemoveProcessor { field: "nonexistent".to_string() };
    let mut doc = make_doc(vec![]);
    // Removing a missing field should not error
    assert!(proc.process(&mut doc).is_ok());
}

#[test]
fn test_rename_processor() {
    let proc = RenameProcessor { from: "old".to_string(), to: "new".to_string() };
    let mut doc = make_doc(vec![("old", Value::String("value".to_string()))]);
    proc.process(&mut doc).unwrap();
    assert!(!doc.fields.contains_key("old"));
    assert_eq!(doc.fields["new"], Value::String("value".to_string()));
}

#[test]
fn test_rename_missing_field_errors() {
    let proc = RenameProcessor { from: "missing".to_string(), to: "new".to_string() };
    let mut doc = make_doc(vec![]);
    assert!(proc.process(&mut doc).is_err());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p prism --test pipeline_test 2>&1 | tail -5`
Expected: FAIL — module `pipeline` not found

**Step 3: Create the pipeline module**

Create `/home/meeh/prism/prism/src/pipeline/mod.rs`:

```rust
pub mod processors;

use crate::backends::Document;
use crate::Result;

/// A processor transforms a document in-place before indexing.
pub trait Processor: Send + Sync {
    fn name(&self) -> &str;
    fn process(&self, doc: &mut Document) -> Result<()>;
}
```

Create `/home/meeh/prism/prism/src/pipeline/processors.rs`:

```rust
use crate::backends::Document;
use crate::error::Error;
use crate::Result;
use super::Processor;

/// Convert a string field to lowercase.
pub struct LowercaseProcessor {
    pub field: String,
}

impl Processor for LowercaseProcessor {
    fn name(&self) -> &str { "lowercase" }

    fn process(&self, doc: &mut Document) -> Result<()> {
        let val = doc.fields.get(&self.field)
            .ok_or_else(|| Error::Backend(format!("lowercase: field '{}' not found", self.field)))?;
        let s = val.as_str()
            .ok_or_else(|| Error::Backend(format!("lowercase: field '{}' is not a string", self.field)))?;
        let lowered = s.to_lowercase();
        doc.fields.insert(self.field.clone(), serde_json::Value::String(lowered));
        Ok(())
    }
}

/// Strip HTML tags from a string field.
pub struct HtmlStripProcessor {
    pub field: String,
}

impl Processor for HtmlStripProcessor {
    fn name(&self) -> &str { "html_strip" }

    fn process(&self, doc: &mut Document) -> Result<()> {
        let val = doc.fields.get(&self.field)
            .ok_or_else(|| Error::Backend(format!("html_strip: field '{}' not found", self.field)))?;
        let s = val.as_str()
            .ok_or_else(|| Error::Backend(format!("html_strip: field '{}' is not a string", self.field)))?;
        let stripped = strip_html(s);
        doc.fields.insert(self.field.clone(), serde_json::Value::String(stripped));
        Ok(())
    }
}

/// Set a field to a static value. Supports `{{_now}}` for current ISO8601 timestamp.
pub struct SetProcessor {
    pub field: String,
    pub value: String,
}

impl Processor for SetProcessor {
    fn name(&self) -> &str { "set" }

    fn process(&self, doc: &mut Document) -> Result<()> {
        let resolved = if self.value == "{{_now}}" {
            chrono::Utc::now().to_rfc3339()
        } else {
            self.value.clone()
        };
        doc.fields.insert(self.field.clone(), serde_json::Value::String(resolved));
        Ok(())
    }
}

/// Remove a field from the document. No-op if field doesn't exist.
pub struct RemoveProcessor {
    pub field: String,
}

impl Processor for RemoveProcessor {
    fn name(&self) -> &str { "remove" }

    fn process(&self, doc: &mut Document) -> Result<()> {
        doc.fields.remove(&self.field);
        Ok(())
    }
}

/// Rename a field.
pub struct RenameProcessor {
    pub from: String,
    pub to: String,
}

impl Processor for RenameProcessor {
    fn name(&self) -> &str { "rename" }

    fn process(&self, doc: &mut Document) -> Result<()> {
        let val = doc.fields.remove(&self.from)
            .ok_or_else(|| Error::Backend(format!("rename: field '{}' not found", self.from)))?;
        doc.fields.insert(self.to.clone(), val);
        Ok(())
    }
}

/// Simple HTML tag stripping using a state machine (no regex dependency needed).
fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}
```

Add to `/home/meeh/prism/prism/src/lib.rs` after `pub mod mcp;` (line 9):

```rust
pub mod pipeline;
```

**Step 4: Run all pipeline tests**

Run: `cargo test -p prism --test pipeline_test 2>&1 | tail -15`
Expected: All 11 tests PASS

**Step 5: Commit**

```bash
git add prism/src/pipeline/ prism/src/lib.rs prism/tests/pipeline_test.rs
git commit -m "feat(pipeline): add Processor trait and 5 built-in processors"
```

---

### Task 2: Add Pipeline struct and YAML loading

**Files:**
- Create: `/home/meeh/prism/prism/src/pipeline/registry.rs`
- Modify: `/home/meeh/prism/prism/src/pipeline/mod.rs` (add `pub mod registry;`)
- Modify: `/home/meeh/prism/prism/tests/pipeline_test.rs` (add registry tests)

**Step 1: Write the failing tests**

Append to `/home/meeh/prism/prism/tests/pipeline_test.rs`:

```rust
use prism::pipeline::registry::PipelineRegistry;
use tempfile::TempDir;

#[test]
fn test_load_pipeline_from_yaml() {
    let tmp = TempDir::new().unwrap();
    let yaml = r#"
name: normalize
description: Normalize text fields
processors:
  - lowercase:
      field: title
  - html_strip:
      field: content
  - set:
      field: indexed_at
      value: "{{_now}}"
  - remove:
      field: _internal
  - rename:
      from: old
      to: new
"#;
    std::fs::write(tmp.path().join("normalize.yaml"), yaml).unwrap();

    let registry = PipelineRegistry::load(tmp.path()).unwrap();
    assert!(registry.get("normalize").is_some());
    assert!(registry.get("nonexistent").is_none());

    let pipeline = registry.get("normalize").unwrap();
    assert_eq!(pipeline.name, "normalize");
    assert_eq!(pipeline.processors.len(), 5);
}

#[test]
fn test_pipeline_processes_document() {
    let tmp = TempDir::new().unwrap();
    let yaml = r#"
name: test
description: Test pipeline
processors:
  - lowercase:
      field: title
  - remove:
      field: secret
"#;
    std::fs::write(tmp.path().join("test.yaml"), yaml).unwrap();

    let registry = PipelineRegistry::load(tmp.path()).unwrap();
    let pipeline = registry.get("test").unwrap();

    let mut doc = make_doc(vec![
        ("title", Value::String("HELLO".to_string())),
        ("secret", Value::String("password".to_string())),
    ]);

    pipeline.process(&mut doc).unwrap();
    assert_eq!(doc.fields["title"], Value::String("hello".to_string()));
    assert!(!doc.fields.contains_key("secret"));
}

#[test]
fn test_empty_pipeline_dir() {
    let tmp = TempDir::new().unwrap();
    let registry = PipelineRegistry::load(tmp.path()).unwrap();
    assert!(registry.get("anything").is_none());
}

#[test]
fn test_load_nonexistent_dir() {
    let registry = PipelineRegistry::load(std::path::Path::new("/nonexistent/path"));
    // Should succeed with empty registry, not crash
    assert!(registry.is_ok());
    assert!(registry.unwrap().get("anything").is_none());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p prism --test pipeline_test test_load_pipeline 2>&1 | tail -5`
Expected: FAIL — module `registry` not found

**Step 3: Create the registry**

Create `/home/meeh/prism/prism/src/pipeline/registry.rs`:

```rust
use crate::backends::Document;
use crate::error::Error;
use crate::Result;
use super::Processor;
use super::processors::*;
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

        let entries = std::fs::read_dir(dir)
            .map_err(|e| Error::Config(format!("Cannot read pipeline dir '{}': {}", dir.display(), e)))?;

        for entry in entries {
            let entry = entry.map_err(|e| Error::Config(e.to_string()))?;
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "yaml" || e == "yml") {
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| Error::Config(format!("Cannot read '{}': {}", path.display(), e)))?;
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

    /// Create an empty registry.
    pub fn empty() -> Self {
        Self { pipelines: HashMap::new() }
    }
}

// -- YAML deserialization types -----------------------------------------------

#[derive(Deserialize)]
struct PipelineDef {
    name: String,
    #[serde(default)]
    description: String,
    processors: Vec<ProcessorDef>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProcessorDef {
    Lowercase { field: String },
    HtmlStrip { field: String },
    Set { field: String, value: String },
    Remove { field: String },
    Rename { from: String, to: String },
}

impl PipelineDef {
    fn into_pipeline(self) -> Result<Pipeline> {
        let processors: Vec<Box<dyn Processor>> = self.processors
            .into_iter()
            .map(|def| -> Box<dyn Processor> {
                match def {
                    ProcessorDef::Lowercase { field } => Box::new(LowercaseProcessor { field }),
                    ProcessorDef::HtmlStrip { field } => Box::new(HtmlStripProcessor { field }),
                    ProcessorDef::Set { field, value } => Box::new(SetProcessor { field, value }),
                    ProcessorDef::Remove { field } => Box::new(RemoveProcessor { field }),
                    ProcessorDef::Rename { from, to } => Box::new(RenameProcessor { from, to }),
                }
            })
            .collect();

        Ok(Pipeline {
            name: self.name,
            description: self.description,
            processors,
        })
    }
}
```

Update `/home/meeh/prism/prism/src/pipeline/mod.rs`:

```rust
pub mod processors;
pub mod registry;

use crate::backends::Document;
use crate::Result;

/// A processor transforms a document in-place before indexing.
pub trait Processor: Send + Sync {
    fn name(&self) -> &str;
    fn process(&self, doc: &mut Document) -> Result<()>;
}
```

**Step 4: Run all pipeline tests**

Run: `cargo test -p prism --test pipeline_test 2>&1 | tail -20`
Expected: All 15 tests PASS

**Step 5: Commit**

```bash
git add prism/src/pipeline/registry.rs prism/src/pipeline/mod.rs prism/tests/pipeline_test.rs
git commit -m "feat(pipeline): add Pipeline struct and YAML registry loading"
```

---

### Task 3: Wire PipelineRegistry into ApiServer

**Files:**
- Modify: `/home/meeh/prism/prism/src/api/server.rs` (add pipeline_registry field, update constructors)
- Modify: `/home/meeh/prism/prism/src/api/routes.rs` (update index_documents to accept ?pipeline param)
- Modify: `/home/meeh/prism/prism-server/src/main.rs` (load pipelines at startup)

**Step 1: Update AppState and ApiServer**

In `/home/meeh/prism/prism/src/api/server.rs`:

Add import at top (after line 4):

```rust
use crate::pipeline::registry::PipelineRegistry;
```

Add `pipeline_registry` to `AppState` (line 28-32):

```rust
#[derive(Clone)]
pub struct AppState {
    pub manager: Arc<CollectionManager>,
    pub session_manager: Arc<SessionManager>,
    pub mcp_handler: Arc<McpHandler>,
    pub pipeline_registry: Arc<PipelineRegistry>,
}
```

Add `pipeline_registry` field to `ApiServer` struct (after line 39):

```rust
pipeline_registry: Arc<PipelineRegistry>,
```

Update `with_security` to accept pipeline_registry (line 51-70). Add a new constructor:

```rust
pub fn with_pipelines(
    manager: Arc<CollectionManager>,
    cors_config: CorsConfig,
    security_config: SecurityConfig,
    pipeline_registry: PipelineRegistry,
) -> Self {
    let session_manager = Arc::new(SessionManager::new());
    let mut tool_registry = ToolRegistry::new();
    register_basic_tools(&mut tool_registry);
    let tool_registry = Arc::new(tool_registry);
    let mcp_handler = Arc::new(McpHandler::new(tool_registry, manager.clone()));

    Self {
        manager,
        session_manager,
        mcp_handler,
        cors_config,
        security_config,
        pipeline_registry: Arc::new(pipeline_registry),
    }
}
```

Update existing `with_security` to call `with_pipelines` with `PipelineRegistry::empty()`.

In `router()`, update `app_state` construction (line 183-187) to include `pipeline_registry`:

```rust
let app_state = AppState {
    manager: self.manager.clone(),
    session_manager: self.session_manager.clone(),
    mcp_handler: self.mcp_handler.clone(),
    pipeline_registry: self.pipeline_registry.clone(),
};
```

**Important:** The index_documents route currently uses `.with_state(self.manager.clone())`. To pass the full AppState, change the route to use AppState. This means updating the route's handler signature in routes.rs (Step 2).

Change the legacy_routes `.with_state(self.manager.clone())` to `.with_state(app_state.clone())`. This requires updating ALL route handlers that currently extract `State(manager): State<Arc<CollectionManager>>` to instead extract from `AppState`. However, to minimize changes, we can create a separate sub-router for the pipeline-aware index route:

Instead, add a new pipeline-aware route that shadows the existing one. Replace the `/collections/:collection/documents` route in legacy_routes with a new version that takes `State<AppState>`:

Actually, the cleanest approach: keep legacy_routes with `Arc<CollectionManager>` state, but add the documents route in a separate router with `AppState`:

```rust
// Pipeline-aware routes that need AppState
let pipeline_routes = Router::new()
    .route(
        "/collections/:collection/documents",
        post(crate::api::routes::index_documents),
    )
    .with_state(app_state.clone());
```

Remove the `/collections/:collection/documents` route from `legacy_routes`.

Then merge: `.merge(legacy_routes).merge(pipeline_routes).merge(mcp_routes)`

**Step 2: Update index_documents handler**

In `/home/meeh/prism/prism/src/api/routes.rs`, update imports (line 1-10):

```rust
use crate::api::server::AppState;
```

Add query parameter struct and response types (after IndexRequest, around line 159):

```rust
#[derive(Deserialize)]
pub struct IndexQuery {
    pub pipeline: Option<String>,
}

#[derive(Serialize)]
pub struct IndexResponse {
    pub indexed: usize,
    pub failed: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<IndexError>,
}

#[derive(Serialize)]
pub struct IndexError {
    pub doc_id: String,
    pub error: String,
}
```

Replace `index_documents` handler (lines 161-180):

```rust
pub async fn index_documents(
    Path(collection): Path<String>,
    axum::extract::Query(query): axum::extract::Query<IndexQuery>,
    State(state): State<AppState>,
    Json(request): Json<IndexRequest>,
) -> Result<(StatusCode, Json<IndexResponse>), StatusCode> {
    let mut documents = request.documents;
    let total = documents.len();
    tracing::info!("Indexing {} documents to collection '{}'", total, collection);

    // Apply pipeline if specified
    let mut errors = Vec::new();
    if let Some(ref pipeline_name) = query.pipeline {
        let pipeline = state.pipeline_registry.get(pipeline_name)
            .ok_or_else(|| {
                tracing::warn!("Unknown pipeline: {}", pipeline_name);
                StatusCode::BAD_REQUEST
            })?;

        let mut processed = Vec::with_capacity(documents.len());
        for mut doc in documents {
            match pipeline.process(&mut doc) {
                Ok(()) => processed.push(doc),
                Err(e) => {
                    errors.push(IndexError {
                        doc_id: doc.id.clone(),
                        error: e.to_string(),
                    });
                }
            }
        }
        documents = processed;
    }

    let indexed = documents.len();
    let failed = errors.len();

    if !documents.is_empty() {
        state.manager
            .index(&collection, documents)
            .await
            .map_err(|e| {
                tracing::error!("Failed to index documents to '{}': {:?}", collection, e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }

    tracing::info!("Indexed {}/{} documents to '{}' ({} failed)", indexed, total, collection, failed);
    Ok((StatusCode::CREATED, Json(IndexResponse { indexed, failed, errors })))
}
```

**Step 3: Update prism-server main.rs**

In `/home/meeh/prism/prism-server/src/main.rs`, after `manager.initialize().await?;` (line 60), add:

```rust
// Load ingest pipelines
let config_dir = std::path::Path::new(&args.config).parent().unwrap_or(std::path::Path::new("."));
let pipelines_dir = config_dir.join("conf/pipelines");
let pipeline_registry = prism::pipeline::registry::PipelineRegistry::load(&pipelines_dir)?;
tracing::info!("Loaded ingest pipelines from {}", pipelines_dir.display());
```

Change the ApiServer construction (lines 63-67):

```rust
let server = prism::api::ApiServer::with_pipelines(
    manager,
    config.server.cors.clone(),
    config.security.clone(),
    pipeline_registry,
);
```

**Step 4: Verify compilation**

Run: `cargo check -p prism-server 2>&1 | tail -5`
Expected: compiles clean

**Step 5: Commit**

```bash
git add prism/src/api/server.rs prism/src/api/routes.rs prism-server/src/main.rs
git commit -m "feat(pipeline): wire PipelineRegistry into ApiServer and index route"
```

---

### Task 4: Add pipeline admin endpoint

**Files:**
- Modify: `/home/meeh/prism/prism/src/api/routes.rs` (add list_pipelines handler)
- Modify: `/home/meeh/prism/prism/src/api/server.rs` (add route)

**Step 1: Add list_pipelines endpoint**

In `/home/meeh/prism/prism/src/api/routes.rs`, add:

```rust
#[derive(Serialize)]
pub struct PipelineInfo {
    pub name: String,
    pub description: String,
    pub processor_count: usize,
}

#[derive(Serialize)]
pub struct PipelineListResponse {
    pub pipelines: Vec<PipelineInfo>,
}

pub async fn list_pipelines(
    State(state): State<AppState>,
) -> Json<PipelineListResponse> {
    let pipelines = state.pipeline_registry.list()
        .into_iter()
        .map(|(name, desc, count)| PipelineInfo {
            name,
            description: desc,
            processor_count: count,
        })
        .collect();
    Json(PipelineListResponse { pipelines })
}
```

Add a `list()` method to `PipelineRegistry` in `/home/meeh/prism/prism/src/pipeline/registry.rs`:

```rust
/// List all pipelines as (name, description, processor_count).
pub fn list(&self) -> Vec<(String, String, usize)> {
    self.pipelines
        .iter()
        .map(|(_, p)| (p.name.clone(), p.description.clone(), p.processors.len()))
        .collect()
}
```

In `/home/meeh/prism/prism/src/api/server.rs`, add the route in the pipeline_routes router:

```rust
let pipeline_routes = Router::new()
    .route(
        "/collections/:collection/documents",
        post(crate::api::routes::index_documents),
    )
    .route(
        "/admin/pipelines",
        get(crate::api::routes::list_pipelines),
    )
    .with_state(app_state.clone());
```

**Step 2: Verify compilation**

Run: `cargo check -p prism 2>&1 | tail -5`
Expected: compiles clean

**Step 3: Commit**

```bash
git add prism/src/api/routes.rs prism/src/api/server.rs prism/src/pipeline/registry.rs
git commit -m "feat(pipeline): add GET /admin/pipelines endpoint"
```

---

### Task 5: Integration tests

**Files:**
- Create: `/home/meeh/prism/prism/tests/pipeline_integration_test.rs`

**Step 1: Write integration tests**

Create `/home/meeh/prism/prism/tests/pipeline_integration_test.rs`:

```rust
//! Integration tests for ingest pipelines

use prism::api::ApiServer;
use prism::backends::text::TextBackend;
use prism::backends::VectorBackend;
use prism::collection::CollectionManager;
use prism::config::{SecurityConfig, CorsConfig};
use prism::pipeline::registry::PipelineRegistry;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{sleep, Duration};

async fn setup_server(pipelines_yaml: &[(&str, &str)]) -> (TempDir, String) {
    let temp = TempDir::new().unwrap();
    let schemas_dir = temp.path().join("schemas");
    let pipelines_dir = temp.path().join("pipelines");
    std::fs::create_dir_all(&schemas_dir).unwrap();
    std::fs::create_dir_all(&pipelines_dir).unwrap();

    for (name, content) in pipelines_yaml {
        std::fs::write(pipelines_dir.join(name), content).unwrap();
    }

    let text_backend = Arc::new(TextBackend::new(temp.path()).unwrap());
    let vector_backend = Arc::new(VectorBackend::new(temp.path()).unwrap());
    let manager = Arc::new(
        CollectionManager::new(&schemas_dir, text_backend, vector_backend).unwrap(),
    );

    let registry = PipelineRegistry::load(&pipelines_dir).unwrap();
    let server = ApiServer::with_pipelines(
        manager,
        CorsConfig::default(),
        SecurityConfig::default(),
        registry,
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, server.router()).await.unwrap();
    });

    sleep(Duration::from_millis(50)).await;
    (temp, url)
}

#[tokio::test]
async fn test_list_pipelines_empty() {
    let (_temp, url) = setup_server(&[]).await;
    let client = reqwest::Client::new();
    let resp = client.get(format!("{}/admin/pipelines", url)).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["pipelines"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_list_pipelines_with_one() {
    let yaml = r#"
name: normalize
description: Test pipeline
processors:
  - lowercase:
      field: title
"#;
    let (_temp, url) = setup_server(&[("normalize.yaml", yaml)]).await;
    let client = reqwest::Client::new();
    let resp = client.get(format!("{}/admin/pipelines", url)).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let pipelines = body["pipelines"].as_array().unwrap();
    assert_eq!(pipelines.len(), 1);
    assert_eq!(pipelines[0]["name"], "normalize");
    assert_eq!(pipelines[0]["processor_count"], 1);
}

#[tokio::test]
async fn test_unknown_pipeline_returns_400() {
    let (_temp, url) = setup_server(&[]).await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/collections/test/documents?pipeline=nonexistent", url))
        .json(&serde_json::json!({
            "documents": [{"id": "1", "fields": {"title": "hello"}}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}
```

**Step 2: Run integration tests**

Run: `cargo test -p prism --test pipeline_integration_test 2>&1 | tail -15`
Expected: All 3 tests PASS

**Step 3: Run full test suite**

Run: `cargo test -p prism 2>&1 | tail -20`
Expected: All tests pass, no regressions

**Step 4: Commit**

```bash
git add prism/tests/pipeline_integration_test.rs
git commit -m "feat(pipeline): add integration tests for pipeline API"
```

---

### Task 6: Update xtask dist with example pipeline

**Files:**
- Modify: `/home/meeh/prism/xtask/src/main.rs` (add pipelines dir and example)

**Step 1: Update the stage function**

In `/home/meeh/prism/xtask/src/main.rs`, in the `stage()` function (line 161), add `"conf/pipelines"` to the directory skeleton:

```rust
for dir in &["bin", "conf/schemas", "conf/tls", "conf/pipelines", "models", "data", "logs"] {
```

Add after the schemas section (around line 207), before `-- models/`:

```rust
// Example pipeline
fs::write(
    base.join("conf/pipelines/example.yaml"),
    generate_example_pipeline(),
)?;
```

Add the generator function:

```rust
fn generate_example_pipeline() -> &'static str {
    r#"# Example ingest pipeline
# Reference via: POST /collections/{name}/documents?pipeline=normalize

name: normalize
description: Normalize text fields before indexing
processors:
  - lowercase:
      field: title
  - lowercase:
      field: content
  - set:
      field: indexed_at
      value: "{{_now}}"
"#
}
```

**Step 2: Verify xtask compiles**

Run: `cargo check -p xtask 2>&1 | tail -5`
Expected: compiles clean

**Step 3: Commit**

```bash
git add xtask/src/main.rs
git commit -m "feat(pipeline): add example pipeline to dist bundle"
```

---

### Task 7: Final test suite run

**Step 1: Run full test suite**

Run: `cargo test -p prism 2>&1 | tail -20`
Expected: All tests pass

**Step 2: Verify prism-server compiles**

Run: `cargo check -p prism-server 2>&1 | tail -5`
Expected: compiles clean
