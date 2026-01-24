# Prism

**High-performance hybrid search engine for AI/RAG applications.**

Prism combines vector search (HNSW) with full-text search (Tantivy) to deliver fast, accurate semantic search with optional keyword filtering.

## Features

- **Hybrid Search** - Combine vector similarity and text relevance with configurable fusion strategies (RRF, weighted)
- **Embedding Cache** - SQLite-backed cache eliminates redundant API calls to embedding providers
- **Multiple Providers** - Ollama (local), OpenAI-compatible APIs, ONNX (local inference)
- **Lucene-compatible DSL** - Familiar query syntax with field targeting, ranges, and boolean operators
- **Collection Schemas** - Define index configuration, field types, and embedding behavior per collection
- **Async-First** - Built on tokio for high throughput concurrent operations

## Quick Start

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
| `prism-server` | HTTP server with REST API |
| `prism-cli` | Command-line interface |

## Performance

- **HNSW Index**: Sub-millisecond approximate nearest neighbor search
- **Embedding Cache**: 100% cache hit rate eliminates API latency for repeated queries
- **Batch Operations**: Efficient bulk indexing with batched embedding generation
- **Async I/O**: Non-blocking operations throughout the stack

## License

MIT
