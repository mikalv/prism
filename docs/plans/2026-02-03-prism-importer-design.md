# prism-importer Design

> **Goal:** Create a standalone crate for importing data from Elasticsearch (7.x/8.x) into Prism collections via REST API.

**Related issues:** #58 (Elasticsearch import), #59 (prism-importer crate)

---

## Architecture

```
prism-importer/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Public API for embedding
│   ├── main.rs             # CLI entrypoint
│   ├── cli.rs              # Clap argument parsing
│   ├── sources/
│   │   ├── mod.rs
│   │   ├── elasticsearch.rs  # ES 7.x/8.x client
│   │   └── trait.rs          # ImportSource trait
│   ├── schema/
│   │   ├── mod.rs
│   │   ├── mapping.rs        # ES mapping → Prism schema
│   │   └── types.rs          # Type conversion logic
│   └── progress.rs           # Progress bar + stats
```

### Core Abstractions

```rust
#[async_trait]
pub trait ImportSource {
    async fn fetch_schema(&self) -> Result<SourceSchema>;
    async fn stream_documents(&self) -> Result<impl Stream<Item = Document>>;
    fn source_name(&self) -> &str;
}

pub struct ElasticsearchSource {
    client: reqwest::Client,
    base_url: Url,
    index_pattern: String,
    auth: AuthMethod,
    batch_size: usize,
}
```

This trait-based design allows adding more sources (Solr, Meilisearch, Vespa) later.

---

## Schema Mapping

### ES → Prism Type Conversion

| ES Type | Prism Type |
|---------|------------|
| `text` | `text` (indexed) |
| `keyword` | `keyword` |
| `long`, `integer`, `short`, `byte` | `i64` |
| `float`, `double`, `half_float` | `f64` |
| `boolean` | `bool` |
| `date` | `date` |
| `dense_vector` | `vector` (with dims) |
| `object`, `nested` | `json` (flattened) |
| unknown | `text` (with warning) |

### Conversion Logic

```rust
pub fn convert_es_mapping(es_mapping: &EsMapping) -> Result<PrismSchema> {
    let mut fields = Vec::new();

    for (name, prop) in &es_mapping.properties {
        let prism_type = match prop.field_type.as_str() {
            "text" => FieldType::Text { indexed: true },
            "keyword" => FieldType::Keyword,
            "long" | "integer" | "short" | "byte" => FieldType::I64,
            "float" | "double" | "half_float" => FieldType::F64,
            "boolean" => FieldType::Bool,
            "date" => FieldType::Date,
            "dense_vector" => FieldType::Vector {
                dims: prop.dims.unwrap_or(384)
            },
            "object" | "nested" => FieldType::Json,
            unknown => {
                warn!("Unknown ES type '{}', falling back to text", unknown);
                FieldType::Text { indexed: true }
            }
        };
        fields.push(Field { name: name.clone(), field_type: prism_type });
    }

    Ok(PrismSchema { fields, ..Default::default() })
}
```

---

## CLI Interface

### Commands

```rust
#[derive(Parser)]
#[command(name = "prism-import")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Import from Elasticsearch
    Es {
        #[arg(long)]
        source: Url,

        #[arg(long)]
        index: String,

        #[arg(long)]
        target: Option<String>,  // Default: same as index

        #[arg(long)]
        user: Option<String>,

        #[arg(long)]
        password: Option<String>,

        #[arg(long)]
        api_key: Option<String>,

        #[arg(long, default_value = "1000")]
        batch_size: usize,

        #[arg(long)]
        dry_run: bool,

        #[arg(long)]
        interactive: bool,
    },
}
```

### Usage Examples

```bash
# Basic import (no auth, local ES)
prism-import es --source http://localhost:9200 --index products

# With basic auth
prism-import es --source https://es:9200 --index products \
  --user elastic --password secret

# With API key
prism-import es --source https://es:9200 --index products \
  --api-key "base64encodedkey"

# Dry run - preview schema only
prism-import es --source http://es:9200 --index products --dry-run

# Interactive mode - confirm schema before import
prism-import es --source http://es:9200 --index products --interactive

# Custom batch size
prism-import es --source http://es:9200 --index logs-* --batch-size 500
```

---

## Document Streaming

Uses ES Scroll API for efficient batched retrieval:

```rust
async fn stream_documents(&self) -> Result<impl Stream<Item = Document>> {
    // 1. Initialize scroll
    let scroll_id = self.init_scroll().await?;

    // 2. Stream batches
    stream! {
        loop {
            let batch = self.fetch_batch(&scroll_id).await?;
            if batch.is_empty() { break; }

            for hit in batch {
                yield Document {
                    id: hit._id,
                    fields: hit._source,
                };
            }
        }
        self.clear_scroll(&scroll_id).await?;
    }
}
```

---

## Progress Display

```rust
pub struct ImportProgress {
    bar: ProgressBar,
    total: u64,
    imported: AtomicU64,
    failed: AtomicU64,
    start: Instant,
}

impl ImportProgress {
    pub fn tick(&self, count: u64) {
        self.imported.fetch_add(count, Ordering::Relaxed);
        let imported = self.imported.load(Ordering::Relaxed);
        let elapsed = self.start.elapsed().as_secs_f64();
        let rate = imported as f64 / elapsed;

        self.bar.set_message(format!(
            "{}/{} ({:.0} docs/s)",
            imported, self.total, rate
        ));
    }
}
```

Output:
```
[████████████░░░░░░░░] 62% 1.76M/2.85M (12,847 docs/s) ETA: 1m 24s
```

---

## Error Handling

```rust
pub enum ImportError {
    #[error("Connection failed: {0}")]
    Connection(#[from] reqwest::Error),

    #[error("Auth failed: {status}")]
    Auth { status: u16 },

    #[error("Index not found: {0}")]
    IndexNotFound(String),

    #[error("Schema conversion failed: {field} - {reason}")]
    SchemaConversion { field: String, reason: String },

    #[error("Document error at {id}: {reason}")]
    Document { id: String, reason: String },
}
```

**Strategy:** Log errors, continue import, report summary at end:
```
Imported 2,847,102 docs. 189 failed (see errors.log)
```

---

## Dependencies

```toml
[package]
name = "prism-importer"
version = "0.1.0"
edition = "2021"

[dependencies]
prism = { path = "../prism" }
reqwest = { version = "0.12", features = ["json", "stream"] }
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
url = "2"
thiserror = "2"
indicatif = "0.17"
async-stream = "0.3"
tracing = { workspace = true }
```

---

## Testing Strategy

### Unit Tests (Mock ES)

```rust
#[tokio::test]
async fn test_schema_conversion() {
    let es_mapping = json!({
        "properties": {
            "title": { "type": "text" },
            "price": { "type": "float" }
        }
    });
    let schema = convert_es_mapping(&es_mapping).unwrap();
    assert_eq!(schema.fields.len(), 2);
}
```

### Integration Tests (testcontainers)

```rust
#[tokio::test]
#[ignore]  // Requires Docker
async fn test_full_import() {
    let es = ElasticsearchContainer::start().await;
    // ... seed data, run import, verify
}
```

---

## Authentication Support (v1)

| Method | Flag | Example |
|--------|------|---------|
| None | (default) | `--source http://localhost:9200` |
| Basic | `--user` + `--password` | `--user elastic --password secret` |
| API Key | `--api-key` | `--api-key "abc123..."` |

Cloud ID and mTLS deferred to v2.

---

## Future Enhancements (v2+)

- Resume support for interrupted imports
- Parallel slice scroll for higher throughput
- Cloud ID authentication
- Direct Lucene shard reading (suntan-style)
- Additional sources: Solr, Meilisearch, Vespa
