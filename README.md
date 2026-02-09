# Prism

**High-performance hybrid search engine for AI/RAG applications and as a lightweight Elasticsearch replacement.**

Prism combines vector search (HNSW) with full-text search (Tantivy) to deliver fast, accurate semantic search with optional keyword filtering. Built in Rust for performance and reliability, it serves as a modern alternative to Elasticsearch for applications that need powerful search without the operational complexity.

## Features

- **Hybrid Search** - Combine vector similarity and text relevance with configurable fusion strategies (RRF, weighted)
- **Elasticsearch Compatible** - Drop-in replacement for common ES operations with familiar query DSL
- **Embedded Web UI** - Built-in search interface at `/ui` with collection selector
- **Embedding Cache** - SQLite-backed cache eliminates redundant API calls to embedding providers
- **Multiple Providers** - Ollama (local), OpenAI-compatible APIs, ONNX (local inference)
- **Lucene-compatible DSL** - Familiar query syntax with field targeting, ranges, and boolean operators
- **Collection Schemas** - Define index configuration, field types, and embedding behavior per collection
- **Index Lifecycle Management** - Automatic rollover, phase transitions (hot→warm→cold→delete)
- **Async-First** - Built on tokio for high throughput concurrent operations
- **Single Binary** - No JVM, no cluster coordination required for single-node deployments

## Quick Start

### Running the Server

```bash
cargo run -p prism-server
```

This starts Prism on `http://localhost:3080` with:
- **REST API** - Full search and indexing API
- **Web UI** - Built-in search interface at `/ui`
- **Health check** - Server status at `/health`

Open http://localhost:3080/ui to search through your collections.

### Configuration

Prism can be configured via command-line arguments or environment variables:

```bash
# Command-line
prism-server --host 0.0.0.0 --port 8080 --data-dir /var/lib/prism

# Environment variables
PRISM_HOST=0.0.0.0 PRISM_PORT=8080 PRISM_DATA_DIR=/var/lib/prism prism-server
```

| Variable | CLI Flag | Default | Description |
|----------|----------|---------|-------------|
| `PRISM_CONFIG_PATH` | `--config` | `prism.toml` | Configuration file path |
| `PRISM_HOST` | `--host` | `127.0.0.1` | Bind address |
| `PRISM_PORT` | `--port` | `3080` | Listen port |
| `PRISM_DATA_DIR` | `--data-dir` | `data` | Data directory |
| `PRISM_SCHEMAS_DIR` | `--schemas-dir` | `schemas` | Schema definitions |
| `PRISM_LOG_DIR` | `--log-dir` | - | Log files directory |
| `PRISM_CACHE_DIR` | `--cache-dir` | - | Embedding cache directory |

### Development Mode

For UI development with hot-reload:

```bash
./dev.sh
```

This starts the backend on `:3080` and Vite dev server on `:5173`.

### Using Prism as a Library

Add Prism to your `Cargo.toml`:

```toml
[dependencies]
prism = "0.3"
```

### Basic Vector Search

```rust
use prism::{
    backends::vector::VectorBackend,
    backends::{SearchBackend, Document, Query},
    embedding::{CachedEmbeddingProvider, OllamaProvider},
    cache::{SqliteCache, KeyStrategy},
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize vector backend
    let backend = VectorBackend::new("./data")?;

    // Set up embedding provider with caching
    let ollama = OllamaProvider::new("http://localhost:11434", "nomic-embed-text").await?;
    let cache = Arc::new(SqliteCache::new("./cache.db")?);
    let provider = Arc::new(CachedEmbeddingProvider::new(
        Box::new(ollama),
        cache,
        KeyStrategy::ModelText,
    ));
    backend.set_embedding_provider(provider);

    // Search with natural language
    let results = backend.search_text("test_collection", "machine learning papers", 10).await?;

    for result in results.results {
        println!("{}: {:.3}", result.id, result.score);
    }

    Ok(())
}
```

### Collection Manager (Hybrid Search)

```rust
use prism::collection::CollectionManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let manager = CollectionManager::new("./data").await?;

    // Hybrid search combining text + vector
    let results = manager.hybrid_search(
        "documents",
        "rust async programming",  // text query
        None,                       // optional vector override
        10,                         // limit
        Some("rrf"),               // merge strategy
        None,                       // text weight
        None,                       // vector weight
    ).await?;

    Ok(())
}
```

## Elasticsearch Compatibility

Prism provides an Elasticsearch-compatible API layer at `/_elastic/*` for easy migration:

```bash
# Index a document
curl -X POST "localhost:3080/_elastic/myindex/_doc/1" -H "Content-Type: application/json" -d '{
  "title": "Hello World",
  "content": "This is a test document"
}'

# Search
curl -X POST "localhost:3080/_elastic/myindex/_search" -H "Content-Type: application/json" -d '{
  "query": {
    "match": {
      "content": "test"
    }
  }
}'

# Multi-search
curl -X POST "localhost:3080/_elastic/_msearch" -H "Content-Type: application/json" -d '
{"index": "myindex"}
{"query": {"match_all": {}}}
'
```

**Why Prism over Elasticsearch?**

| | Prism | Elasticsearch |
|---|-------|---------------|
| **Deployment** | Single binary, no JVM | JVM + cluster coordination |
| **Memory** | ~50MB baseline | 1GB+ heap required |
| **Vector Search** | Native HNSW | Plugin required (8.x+) |
| **Hybrid Search** | Built-in fusion | Manual scripting |
| **Embeddings** | Automatic generation | External pipeline |

## Architecture

```text
                     CollectionManager
         - Schema validation    - Lifecycle management
                    |                      |
            --------+--------      --------+--------
            |  TextBackend  |      | VectorBackend |
            |   (Tantivy)   |      |    (HNSW)     |
            ----------------       --------+-------
                                          |
                              ------------+------------
                              |CachedEmbeddingProvider|
                              |   +---------------+   |
                              |   |  SqliteCache  |   |
                              |   +---------------+   |
                              |   +---------------+   |
                              |   |OllamaProvider |   |
                              |   |OpenAIProvider |   |
                              |   +---------------+   |
                              +-----------------------+
```

## Embedding Providers

### Ollama (Local)

```rust
let provider = OllamaProvider::new(
    "http://localhost:11434",
    "nomic-embed-text"
).await?;
```

### OpenAI-Compatible

```rust
let provider = OpenAIProvider::new(
    "https://api.openai.com/v1",
    &api_key,
    "text-embedding-3-small"
)?;
```

Works with: OpenAI, Azure OpenAI, Together.ai, Groq, and other compatible APIs.

## Embedding Cache

The cache stores embeddings persistently to avoid redundant API calls:

```rust
use prism::cache::{SqliteCache, KeyStrategy};

// Create persistent cache
let cache = SqliteCache::new("./embeddings.db")?;

// Or in-memory for testing
let cache = SqliteCache::in_memory()?;

// Key strategies
KeyStrategy::TextOnly       // Hash of text only
KeyStrategy::ModelText      // Hash of model + text (recommended)
KeyStrategy::ModelVersionText  // Hash of model + version + text
```

Cache stats:

```rust
let stats = cached_provider.cache_stats().await?;
println!("Entries: {}, Size: {} bytes", stats.entry_count, stats.size_bytes);
```

## Query DSL

Prism supports Lucene-compatible query syntax:

```text
# Simple terms
machine learning

# Phrases
"natural language processing"

# Field targeting
title:rust content:async

# Boolean operators
rust AND (async OR tokio)
NOT deprecated

# Range queries
year:[2020 TO 2024]
price:{10 TO *}

# Wildcards
prog*mming
te?t
```

## Configuration

### Collection Schema

```yaml
name: documents
backends:
  text:
    fields:
      - name: title
        type: text
        stored: true
        indexed: true
      - name: content
        type: text
        stored: true
        indexed: true
  vector:
    dimension: 768
    distance: cosine
    hnsw_m: 16
    hnsw_ef_construction: 200
    hnsw_ef_search: 50

embedding_generation:
  enabled: true
  source_field: content
  target_field: embedding
```

## Feature Flags

```toml
[dependencies]
prism = { version = "0.3", features = ["full"] }

# Individual features:
# - provider-ollama: Ollama embedding provider
# - provider-openai: OpenAI-compatible provider
# - provider-onnx: Local ONNX model inference
# - cache-redis: Redis cache backend (coming soon)
# - vector-instant: instant-distance HNSW (default)
# - vector-usearch: usearch HNSW backend
```

## Crates

| Crate | Description |
|-------|-------------|
| `prism` | Core library with backends, embedding, and search |
| `prism-server` | HTTP server with REST API and embedded web UI |
| `prism-cli` | Command-line interface |
| `prism-core` | Core types and traits |
| `prism-storage` | Storage abstractions (local, S3) |
| `prism-cluster` | Distributed clustering support |
| `prism-es-compat` | Elasticsearch compatibility layer |
| `prism-ui` | Embedded web UI assets |
| `prism-importer` | Bulk data import utilities |

## Performance

- **HNSW Index**: Sub-millisecond approximate nearest neighbor search
- **Embedding Cache**: 100% cache hit rate eliminates API latency for repeated queries
- **Batch Operations**: Efficient bulk indexing with batched embedding generation
- **Async I/O**: Non-blocking operations throughout the stack

## License

MIT
