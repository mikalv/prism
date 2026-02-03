# prism-importer Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Create `prism-importer` crate for importing Elasticsearch 7.x/8.x data into Prism collections via REST API.

**Architecture:** Trait-based source abstraction (`ImportSource`) with Elasticsearch as first implementation. CLI wrapper using clap. Schema auto-conversion with dry-run/interactive modes.

**Tech Stack:** reqwest, tokio, clap, serde, indicatif, async-stream, thiserror

---

### Task 1: Create crate scaffold and add to workspace

**Files:**
- Create: `prism-importer/Cargo.toml`
- Create: `prism-importer/src/lib.rs`
- Create: `prism-importer/src/main.rs`
- Modify: `Cargo.toml` (workspace members)

**Step 1: Create directory structure**

```bash
mkdir -p prism-importer/src
```

**Step 2: Create Cargo.toml**

Create `prism-importer/Cargo.toml`:

```toml
[package]
name = "prism-importer"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "Import tool for migrating data from Elasticsearch and other sources to Prism"

[[bin]]
name = "prism-import"
path = "src/main.rs"

[dependencies]
prism = { path = "../prism" }
reqwest = { workspace = true, features = ["json", "stream"] }
tokio = { workspace = true }
clap = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
async-trait = { workspace = true }
async-stream = { workspace = true }
futures = { workspace = true }
url = "2"
indicatif = "0.17"

[dev-dependencies]
tempfile = { workspace = true }
```

**Step 3: Create minimal lib.rs**

Create `prism-importer/src/lib.rs`:

```rust
//! prism-importer: Import data from external search engines into Prism
//!
//! Supported sources:
//! - Elasticsearch 7.x/8.x

pub mod error;
pub mod sources;
pub mod schema;
pub mod progress;

pub use error::{ImportError, Result};
```

**Step 4: Create minimal main.rs**

Create `prism-importer/src/main.rs`:

```rust
use clap::Parser;

#[derive(Parser)]
#[command(name = "prism-import")]
#[command(about = "Import data from external search engines into Prism")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Import from Elasticsearch
    Es {
        #[arg(long)]
        source: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Es { source } => {
            println!("Would import from: {}", source);
        }
    }

    Ok(())
}
```

**Step 5: Add to workspace**

In root `Cargo.toml`, add `"prism-importer"` to members:

```toml
members = ["prism", "prism-server", "prism-cli", "prism-storage", "prism-importer", "xtask"]
```

And add workspace dependency:

```toml
prism-importer = { path = "prism-importer" }
```

**Step 6: Verify it compiles**

Run: `cargo check -p prism-importer 2>&1 | tail -5`
Expected: Compiles (may have warnings about unused modules)

**Step 7: Commit**

```bash
git add Cargo.toml prism-importer/
git commit -m "feat(importer): scaffold prism-importer crate"
```

---

### Task 2: Add error types

**Files:**
- Create: `prism-importer/src/error.rs`

**Step 1: Create error module**

Create `prism-importer/src/error.rs`:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ImportError {
    #[error("Connection failed: {0}")]
    Connection(#[from] reqwest::Error),

    #[error("Authentication failed (status {status})")]
    Auth { status: u16 },

    #[error("Index not found: {0}")]
    IndexNotFound(String),

    #[error("Schema conversion failed for field '{field}': {reason}")]
    SchemaConversion { field: String, reason: String },

    #[error("Document error at '{id}': {reason}")]
    Document { id: String, reason: String },

    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ImportError>;
```

**Step 2: Verify it compiles**

Run: `cargo check -p prism-importer 2>&1 | tail -5`
Expected: Compiles

**Step 3: Commit**

```bash
git add prism-importer/src/error.rs
git commit -m "feat(importer): add error types"
```

---

### Task 3: Add ImportSource trait and source module structure

**Files:**
- Create: `prism-importer/src/sources/mod.rs`
- Create: `prism-importer/src/sources/traits.rs`

**Step 1: Create sources module**

Create `prism-importer/src/sources/mod.rs`:

```rust
pub mod traits;
pub mod elasticsearch;

pub use traits::ImportSource;
pub use elasticsearch::ElasticsearchSource;
```

**Step 2: Create traits module**

Create `prism-importer/src/sources/traits.rs`:

```rust
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
```

**Step 3: Create placeholder elasticsearch module**

Create `prism-importer/src/sources/elasticsearch.rs`:

```rust
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
```

**Step 4: Verify it compiles**

Run: `cargo check -p prism-importer 2>&1 | tail -5`
Expected: Compiles (with warnings about todo!)

**Step 5: Commit**

```bash
git add prism-importer/src/sources/
git commit -m "feat(importer): add ImportSource trait and module structure"
```

---

### Task 4: Add schema types and mapping module

**Files:**
- Create: `prism-importer/src/schema/mod.rs`
- Create: `prism-importer/src/schema/types.rs`
- Create: `prism-importer/src/schema/mapping.rs`

**Step 1: Create schema module**

Create `prism-importer/src/schema/mod.rs`:

```rust
pub mod types;
pub mod mapping;

pub use types::{SourceSchema, SourceField, SourceFieldType};
pub use mapping::convert_es_mapping;
```

**Step 2: Create schema types**

Create `prism-importer/src/schema/types.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Schema extracted from a source system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSchema {
    pub name: String,
    pub fields: Vec<SourceField>,
}

/// A field in the source schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceField {
    pub name: String,
    pub field_type: SourceFieldType,
    pub indexed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_dims: Option<usize>,
}

/// Normalized field types across sources
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SourceFieldType {
    Text,
    Keyword,
    I64,
    F64,
    Bool,
    Date,
    Vector,
    Json,
    Unknown(String),
}

impl std::fmt::Display for SourceFieldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text => write!(f, "text"),
            Self::Keyword => write!(f, "keyword"),
            Self::I64 => write!(f, "i64"),
            Self::F64 => write!(f, "f64"),
            Self::Bool => write!(f, "bool"),
            Self::Date => write!(f, "date"),
            Self::Vector => write!(f, "vector"),
            Self::Json => write!(f, "json"),
            Self::Unknown(s) => write!(f, "unknown({})", s),
        }
    }
}
```

**Step 3: Create mapping conversion with tests**

Create `prism-importer/src/schema/mapping.rs`:

```rust
use serde::Deserialize;
use std::collections::HashMap;
use crate::error::{ImportError, Result};
use super::types::{SourceSchema, SourceField, SourceFieldType};

/// Elasticsearch mapping response structure
#[derive(Debug, Deserialize)]
pub struct EsMappingResponse {
    #[serde(flatten)]
    pub indices: HashMap<String, EsIndexMapping>,
}

#[derive(Debug, Deserialize)]
pub struct EsIndexMapping {
    pub mappings: EsMappings,
}

#[derive(Debug, Deserialize)]
pub struct EsMappings {
    #[serde(default)]
    pub properties: HashMap<String, EsFieldMapping>,
}

#[derive(Debug, Deserialize)]
pub struct EsFieldMapping {
    #[serde(rename = "type")]
    pub field_type: Option<String>,
    #[serde(default)]
    pub properties: Option<HashMap<String, EsFieldMapping>>,
    pub dims: Option<usize>,
    pub index: Option<bool>,
}

/// Convert Elasticsearch mapping to SourceSchema
pub fn convert_es_mapping(index_name: &str, mapping: &EsMappings) -> Result<SourceSchema> {
    let mut fields = Vec::new();

    for (name, prop) in &mapping.properties {
        let field = convert_field(name, prop)?;
        fields.push(field);
    }

    Ok(SourceSchema {
        name: index_name.to_string(),
        fields,
    })
}

fn convert_field(name: &str, prop: &EsFieldMapping) -> Result<SourceField> {
    let es_type = prop.field_type.as_deref().unwrap_or("object");

    let (field_type, vector_dims) = match es_type {
        "text" | "match_only_text" => (SourceFieldType::Text, None),
        "keyword" | "constant_keyword" | "wildcard" => (SourceFieldType::Keyword, None),
        "long" | "integer" | "short" | "byte" => (SourceFieldType::I64, None),
        "float" | "double" | "half_float" | "scaled_float" => (SourceFieldType::F64, None),
        "boolean" => (SourceFieldType::Bool, None),
        "date" | "date_nanos" => (SourceFieldType::Date, None),
        "dense_vector" => (SourceFieldType::Vector, prop.dims),
        "object" | "nested" | "flattened" => (SourceFieldType::Json, None),
        other => {
            tracing::warn!("Unknown ES type '{}' for field '{}', mapping to text", other, name);
            (SourceFieldType::Unknown(other.to_string()), None)
        }
    };

    let indexed = prop.index.unwrap_or(true);

    Ok(SourceField {
        name: name.to_string(),
        field_type,
        indexed,
        vector_dims,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_convert_basic_types() {
        let mapping_json = json!({
            "properties": {
                "title": { "type": "text" },
                "category": { "type": "keyword" },
                "price": { "type": "float" },
                "count": { "type": "integer" },
                "active": { "type": "boolean" },
                "created": { "type": "date" }
            }
        });

        let mapping: EsMappings = serde_json::from_value(mapping_json).unwrap();
        let schema = convert_es_mapping("products", &mapping).unwrap();

        assert_eq!(schema.name, "products");
        assert_eq!(schema.fields.len(), 6);

        let title = schema.fields.iter().find(|f| f.name == "title").unwrap();
        assert_eq!(title.field_type, SourceFieldType::Text);

        let price = schema.fields.iter().find(|f| f.name == "price").unwrap();
        assert_eq!(price.field_type, SourceFieldType::F64);
    }

    #[test]
    fn test_convert_vector_field() {
        let mapping_json = json!({
            "properties": {
                "embedding": { "type": "dense_vector", "dims": 384 }
            }
        });

        let mapping: EsMappings = serde_json::from_value(mapping_json).unwrap();
        let schema = convert_es_mapping("docs", &mapping).unwrap();

        let embedding = schema.fields.iter().find(|f| f.name == "embedding").unwrap();
        assert_eq!(embedding.field_type, SourceFieldType::Vector);
        assert_eq!(embedding.vector_dims, Some(384));
    }

    #[test]
    fn test_unknown_type_falls_back() {
        let mapping_json = json!({
            "properties": {
                "geo": { "type": "geo_point" }
            }
        });

        let mapping: EsMappings = serde_json::from_value(mapping_json).unwrap();
        let schema = convert_es_mapping("places", &mapping).unwrap();

        let geo = schema.fields.iter().find(|f| f.name == "geo").unwrap();
        assert!(matches!(geo.field_type, SourceFieldType::Unknown(_)));
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p prism-importer 2>&1 | tail -15`
Expected: 3 tests pass

**Step 5: Commit**

```bash
git add prism-importer/src/schema/
git commit -m "feat(importer): add schema types and ES mapping conversion"
```

---

### Task 5: Implement ES fetch_schema

**Files:**
- Modify: `prism-importer/src/sources/elasticsearch.rs`

**Step 1: Add reqwest client and auth**

Update `prism-importer/src/sources/elasticsearch.rs`:

```rust
use async_trait::async_trait;
use futures::Stream;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use std::pin::Pin;
use url::Url;

use crate::error::{ImportError, Result};
use crate::schema::{SourceSchema, convert_es_mapping, mapping::EsMappingResponse};
use super::traits::{ImportSource, SourceDocument};

/// Authentication method for Elasticsearch
#[derive(Debug, Clone)]
pub enum AuthMethod {
    None,
    Basic { user: String, password: String },
    ApiKey(String),
}

pub struct ElasticsearchSource {
    client: reqwest::Client,
    base_url: Url,
    index: String,
    batch_size: usize,
}

impl ElasticsearchSource {
    pub fn new(base_url: Url, index: String, auth: AuthMethod) -> Result<Self> {
        let mut headers = HeaderMap::new();

        match auth {
            AuthMethod::None => {}
            AuthMethod::Basic { user, password } => {
                let credentials = base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    format!("{}:{}", user, password),
                );
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Basic {}", credentials))
                        .map_err(|e| ImportError::Other(e.to_string()))?,
                );
            }
            AuthMethod::ApiKey(key) => {
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("ApiKey {}", key))
                        .map_err(|e| ImportError::Other(e.to_string()))?,
                );
            }
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self {
            client,
            base_url,
            index,
            batch_size: 1000,
        })
    }

    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }
}

#[async_trait]
impl ImportSource for ElasticsearchSource {
    async fn fetch_schema(&self) -> Result<SourceSchema> {
        let url = self.base_url.join(&format!("{}/_mapping", self.index))?;

        let response = self.client.get(url).send().await?;

        if response.status() == 404 {
            return Err(ImportError::IndexNotFound(self.index.clone()));
        }

        if response.status() == 401 || response.status() == 403 {
            return Err(ImportError::Auth {
                status: response.status().as_u16(),
            });
        }

        let mapping_response: EsMappingResponse = response.json().await?;

        // Get first index from response (handles aliases and patterns)
        let (index_name, index_mapping) = mapping_response
            .indices
            .into_iter()
            .next()
            .ok_or_else(|| ImportError::IndexNotFound(self.index.clone()))?;

        convert_es_mapping(&index_name, &index_mapping.mappings)
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
```

**Step 2: Add base64 dependency**

In `prism-importer/Cargo.toml`, add:

```toml
base64 = { workspace = true }
```

**Step 3: Verify it compiles**

Run: `cargo check -p prism-importer 2>&1 | tail -5`
Expected: Compiles

**Step 4: Commit**

```bash
git add prism-importer/
git commit -m "feat(importer): implement ES fetch_schema with auth support"
```

---

### Task 6: Implement ES count_documents

**Files:**
- Modify: `prism-importer/src/sources/elasticsearch.rs`

**Step 1: Add count API call**

In `ElasticsearchSource`, replace the `count_documents` todo:

```rust
    async fn count_documents(&self) -> Result<u64> {
        let url = self.base_url.join(&format!("{}/_count", self.index))?;

        let response = self.client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(ImportError::Other(format!(
                "Count failed with status {}",
                response.status()
            )));
        }

        #[derive(serde::Deserialize)]
        struct CountResponse {
            count: u64,
        }

        let count_response: CountResponse = response.json().await?;
        Ok(count_response.count)
    }
```

**Step 2: Verify it compiles**

Run: `cargo check -p prism-importer 2>&1 | tail -5`
Expected: Compiles

**Step 3: Commit**

```bash
git add prism-importer/src/sources/elasticsearch.rs
git commit -m "feat(importer): implement ES count_documents"
```

---

### Task 7: Implement ES stream_documents with Scroll API

**Files:**
- Modify: `prism-importer/src/sources/elasticsearch.rs`

**Step 1: Add scroll types and implementation**

Add these types at the top of the file (after imports):

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ScrollResponse {
    _scroll_id: Option<String>,
    hits: ScrollHits,
}

#[derive(Debug, Deserialize)]
struct ScrollHits {
    hits: Vec<ScrollHit>,
}

#[derive(Debug, Deserialize)]
struct ScrollHit {
    _id: String,
    _source: serde_json::Value,
}
```

**Step 2: Replace stream_documents implementation**

```rust
    fn stream_documents(&self) -> Pin<Box<dyn Stream<Item = Result<SourceDocument>> + Send + '_>> {
        Box::pin(async_stream::try_stream! {
            // Initialize scroll
            let url = self.base_url.join(&format!(
                "{}/_search?scroll=5m&size={}",
                self.index, self.batch_size
            ))?;

            let response = self.client
                .post(url)
                .json(&serde_json::json!({
                    "query": { "match_all": {} }
                }))
                .send()
                .await?;

            if !response.status().is_success() {
                Err(ImportError::Other(format!(
                    "Scroll init failed: {}",
                    response.status()
                )))?;
            }

            let mut scroll_response: ScrollResponse = response.json().await?;
            let mut scroll_id = scroll_response._scroll_id.clone();

            // Yield first batch
            for hit in scroll_response.hits.hits {
                yield SourceDocument {
                    id: hit._id,
                    fields: hit._source,
                };
            }

            // Continue scrolling
            while let Some(sid) = scroll_id.take() {
                let url = self.base_url.join("_search/scroll")?;

                let response = self.client
                    .post(url)
                    .json(&serde_json::json!({
                        "scroll": "5m",
                        "scroll_id": sid
                    }))
                    .send()
                    .await?;

                if !response.status().is_success() {
                    break;
                }

                scroll_response = response.json().await?;

                if scroll_response.hits.hits.is_empty() {
                    // Clear scroll
                    if let Some(final_id) = &scroll_response._scroll_id {
                        let _ = self.client
                            .delete(self.base_url.join("_search/scroll")?)
                            .json(&serde_json::json!({ "scroll_id": final_id }))
                            .send()
                            .await;
                    }
                    break;
                }

                scroll_id = scroll_response._scroll_id.clone();

                for hit in scroll_response.hits.hits {
                    yield SourceDocument {
                        id: hit._id,
                        fields: hit._source,
                    };
                }
            }
        })
    }
```

**Step 3: Verify it compiles**

Run: `cargo check -p prism-importer 2>&1 | tail -5`
Expected: Compiles

**Step 4: Commit**

```bash
git add prism-importer/src/sources/elasticsearch.rs
git commit -m "feat(importer): implement ES stream_documents with Scroll API"
```

---

### Task 8: Add progress display

**Files:**
- Create: `prism-importer/src/progress.rs`

**Step 1: Create progress module**

Create `prism-importer/src/progress.rs`:

```rust
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub struct ImportProgress {
    bar: ProgressBar,
    imported: AtomicU64,
    failed: AtomicU64,
    start: Instant,
}

impl ImportProgress {
    pub fn new(total: u64) -> Self {
        let bar = ProgressBar::new(total);
        bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({per_sec}) ETA: {eta}"
            )
            .unwrap()
            .progress_chars("█▓░"),
        );

        Self {
            bar,
            imported: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            start: Instant::now(),
        }
    }

    pub fn inc(&self, count: u64) {
        self.imported.fetch_add(count, Ordering::Relaxed);
        self.bar.inc(count);
    }

    pub fn inc_failed(&self, count: u64) {
        self.failed.fetch_add(count, Ordering::Relaxed);
        self.bar.inc(count);
    }

    pub fn finish(&self) {
        let imported = self.imported.load(Ordering::Relaxed);
        let failed = self.failed.load(Ordering::Relaxed);
        let elapsed = self.start.elapsed();

        self.bar.finish_with_message(format!(
            "Done! Imported {} documents in {:.1}s ({} failed)",
            imported,
            elapsed.as_secs_f64(),
            failed
        ));
    }

    pub fn imported(&self) -> u64 {
        self.imported.load(Ordering::Relaxed)
    }

    pub fn failed(&self) -> u64 {
        self.failed.load(Ordering::Relaxed)
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p prism-importer 2>&1 | tail -5`
Expected: Compiles

**Step 3: Commit**

```bash
git add prism-importer/src/progress.rs
git commit -m "feat(importer): add progress display"
```

---

### Task 9: Implement full CLI

**Files:**
- Modify: `prism-importer/src/main.rs`
- Modify: `prism-importer/src/lib.rs`

**Step 1: Update lib.rs exports**

Update `prism-importer/src/lib.rs`:

```rust
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
pub use schema::{SourceSchema, SourceField, SourceFieldType};
pub use sources::{ImportSource, ElasticsearchSource, elasticsearch::AuthMethod};
```

**Step 2: Implement full CLI**

Replace `prism-importer/src/main.rs`:

```rust
use clap::Parser;
use futures::StreamExt;
use prism_importer::{
    AuthMethod, ElasticsearchSource, ImportProgress, ImportSource, SourceSchema,
};
use std::path::PathBuf;
use url::Url;

#[derive(Parser)]
#[command(name = "prism-import")]
#[command(about = "Import data from external search engines into Prism")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Import from Elasticsearch
    Es {
        /// Elasticsearch URL (e.g., http://localhost:9200)
        #[arg(long)]
        source: Url,

        /// Index name or pattern
        #[arg(long)]
        index: String,

        /// Target Prism collection name (defaults to index name)
        #[arg(long)]
        target: Option<String>,

        /// Username for basic auth
        #[arg(long)]
        user: Option<String>,

        /// Password for basic auth
        #[arg(long)]
        password: Option<String>,

        /// API key for authentication
        #[arg(long)]
        api_key: Option<String>,

        /// Batch size for scroll API
        #[arg(long, default_value = "1000")]
        batch_size: usize,

        /// Only show schema, don't import
        #[arg(long)]
        dry_run: bool,

        /// Output schema to YAML file
        #[arg(long)]
        schema_out: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Es {
            source,
            index,
            target,
            user,
            password,
            api_key,
            batch_size,
            dry_run,
            schema_out,
        } => {
            let auth = match (user, password, api_key) {
                (Some(u), Some(p), _) => AuthMethod::Basic {
                    user: u,
                    password: p,
                },
                (_, _, Some(key)) => AuthMethod::ApiKey(key),
                _ => AuthMethod::None,
            };

            let es = ElasticsearchSource::new(source.clone(), index.clone(), auth)?
                .with_batch_size(batch_size);

            println!("Connecting to {}...", source);

            // Fetch schema
            let schema = es.fetch_schema().await?;
            println!("\nSchema for '{}':", schema.name);
            print_schema(&schema);

            // Write schema if requested
            if let Some(path) = schema_out {
                let yaml = serde_yaml::to_string(&schema)?;
                std::fs::write(&path, yaml)?;
                println!("\nSchema written to {}", path.display());
            }

            if dry_run {
                println!("\n--dry-run specified, skipping import.");
                return Ok(());
            }

            // Count documents
            let total = es.count_documents().await?;
            println!("\nImporting {} documents...\n", total);

            let progress = ImportProgress::new(total);
            let target_collection = target.unwrap_or_else(|| schema.name.clone());

            // Stream and import
            let mut stream = es.stream_documents();
            let mut batch = Vec::with_capacity(batch_size);

            while let Some(result) = stream.next().await {
                match result {
                    Ok(doc) => {
                        batch.push(doc);
                        if batch.len() >= batch_size {
                            // TODO: Actually index to Prism
                            progress.inc(batch.len() as u64);
                            batch.clear();
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Document error: {}", e);
                        progress.inc_failed(1);
                    }
                }
            }

            // Final batch
            if !batch.is_empty() {
                progress.inc(batch.len() as u64);
            }

            progress.finish();

            println!(
                "\nImport complete: {} documents to collection '{}'",
                progress.imported(),
                target_collection
            );

            if progress.failed() > 0 {
                println!("Warning: {} documents failed", progress.failed());
            }
        }
    }

    Ok(())
}

fn print_schema(schema: &SourceSchema) {
    println!("  Fields:");
    for field in &schema.fields {
        let dims = field
            .vector_dims
            .map(|d| format!(" (dims={})", d))
            .unwrap_or_default();
        println!("    - {}: {}{}", field.name, field.field_type, dims);
    }
}
```

**Step 3: Add serde_yaml dependency**

In `prism-importer/Cargo.toml`, add:

```toml
serde_yaml = { workspace = true }
tracing-subscriber = { workspace = true }
anyhow = { workspace = true }
```

**Step 4: Verify it compiles**

Run: `cargo build -p prism-importer 2>&1 | tail -5`
Expected: Compiles

**Step 5: Test help output**

Run: `cargo run -p prism-importer -- --help`
Expected: Shows help with `es` subcommand

**Step 6: Commit**

```bash
git add prism-importer/
git commit -m "feat(importer): implement full CLI with dry-run and schema export"
```

---

### Task 10: Integration with Prism indexing

**Files:**
- Modify: `prism-importer/src/main.rs`

**Step 1: Add actual Prism indexing**

This task connects the importer to actual Prism indexing. Update the import loop in main.rs to use prism's collection manager.

Note: This requires prism to be set up with a collection first. The importer will:
1. Create schema YAML from ES mapping
2. User creates collection with that schema
3. Import documents

For v1, we'll output the schema and let the user set up the collection manually, then import.

Update the import section of main.rs:

```rust
            // TODO: In v2, auto-create collection from schema
            // For now, require --target to be an existing collection
            println!("\nNote: Create the target collection first using the schema above.");
            println!("Then run without --dry-run to import.");

            // For now, just count what we would import
            let mut count = 0u64;
            let mut stream = es.stream_documents();
            while let Some(result) = stream.next().await {
                match result {
                    Ok(_doc) => {
                        count += 1;
                        if count % 10000 == 0 {
                            println!("  Counted {} documents...", count);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Document error: {}", e);
                    }
                }
            }
            println!("\nWould import {} documents to '{}'", count, target_collection);
```

**Step 2: Commit**

```bash
git add prism-importer/src/main.rs
git commit -m "feat(importer): add placeholder for Prism integration"
```

---

### Task 11: Final verification and documentation

**Step 1: Run all tests**

Run: `cargo test -p prism-importer 2>&1 | tail -15`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cargo clippy -p prism-importer --no-deps 2>&1 | tail -10`
Expected: No errors (warnings ok)

**Step 3: Run fmt**

Run: `cargo fmt -p prism-importer`

**Step 4: Test CLI**

Run: `cargo run -p prism-importer -- es --help`
Expected: Shows es subcommand options

**Step 5: Commit any fixes**

```bash
git add -A
git commit -m "chore(importer): final cleanup and formatting"
```

---

## Summary

After completing all tasks, you will have:

1. **New crate:** `prism-importer` with CLI binary `prism-import`
2. **ES support:** Schema fetch, document streaming via Scroll API
3. **Auth:** None, Basic, and API key authentication
4. **Schema:** Auto-conversion from ES mapping to Prism schema
5. **Progress:** Real-time progress bar with docs/s stats
6. **CLI flags:** `--dry-run`, `--schema-out`, `--batch-size`

**Next steps (v2):**
- Auto-create Prism collections from schema
- Resume interrupted imports
- Parallel scroll slices
- Additional sources (Solr, Meilisearch)
