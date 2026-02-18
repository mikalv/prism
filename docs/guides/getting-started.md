# Getting Started with Prism

Prism is a hybrid search engine combining full-text search (Tantivy/BM25) and vector search (HNSW) for AI/RAG applications. This guide covers installation, configuration, and your first collection.

## Installation

### From source

```bash
git clone https://github.com/mikalv/prism.git
cd prism
cargo build --release
```

Binaries are placed in `target/release/`:
- `prism-server` — the HTTP server
- `prism-cli` — command-line tools for managing collections, importing data, and benchmarking

### Feature flags

| Flag | Description |
|------|-------------|
| `default` | Text search + vector search (instant-distance) |
| `provider-onnx` | Local ONNX embedding model support |
| `provider-openai` | OpenAI-compatible embedding API |
| `provider-ollama` | Ollama local embedding server |
| `cache-redis` | Redis embedding cache backend |
| `storage-s3` | S3/MinIO object storage |
| `vector-usearch` | USearch vector backend (alternative to instant-distance) |
| `full` | All features enabled |

```bash
# Build with specific features
cargo build --release -p prism-server --features "provider-ollama,cache-redis"

# Build with everything
cargo build --release -p prism-server --features full
```

## Server configuration

Prism reads its configuration from a TOML file. By default it looks for `prism.toml` in the working directory, or you can pass `-c /path/to/config.toml`.

### Minimal config

```toml
[server]
bind_addr = "127.0.0.1:3080"

[storage]
data_dir = "~/.prism"

[logging]
level = "info"
```

### Full config reference

```toml
[server]
bind_addr = "127.0.0.1:3080"   # Host:port to listen on
# unix_socket = "/tmp/prism.sock"  # Optional Unix socket

[server.cors]
enabled = true
origins = [
  "http://localhost:5173",
  "http://localhost:3000",
]

[storage]
data_dir = "~/.prism"      # Base directory for all data
max_local_gb = 5.0          # Max local storage in GB

[embedding]
enabled = true

[embedding.provider]
type = "ollama"
url = "http://localhost:11434"
model = "nomic-embed-text"

[logging]
level = "info"              # debug | info | warn | error
# file = "/var/log/prism.log"  # Optional log file (stdout if omitted)
```

### Directory layout

When Prism starts, it creates the following under `data_dir`:

```
~/.prism/
  config.toml         # Server configuration
  schemas/            # Collection schema YAML files
    articles.yaml
    products.yaml
  data/
    text/             # Tantivy indexes (one per collection)
    vector/           # HNSW vector indexes
  cache/
    models/           # Model cache directory
  logs/               # Log files
```

## Starting the server

```bash
# Default config
prism-server

# Custom config file and port
prism-server -c /etc/prism/config.toml -p 3080

# Override bind host
prism-server --host 0.0.0.0 -p 3080
```

The server is ready when you see:

```
INFO prism_server: Prism server listening on 127.0.0.1:3080
```

Verify with:

```bash
curl http://localhost:3080/health
```

## Creating your first collection

Collections are defined by YAML schema files in the `schemas/` directory. Create `~/.prism/schemas/articles.yaml`:

```yaml
collection: articles
description: "Blog articles with full-text search"
backends:
  text:
    fields:
      - name: id
        type: string
        stored: true
        indexed: true
      - name: title
        type: text
        stored: true
        indexed: true
      - name: content
        type: text
        stored: true
        indexed: true
      - name: author
        type: string
        stored: true
        indexed: true
      - name: published_at
        type: date
        stored: true
        indexed: true
```

Restart the server (or it picks up schemas on startup).

### Verify the collection

```bash
# List all collections
curl http://localhost:3080/admin/collections

# Get collection schema
curl http://localhost:3080/collections/articles/schema

# Get collection stats
curl http://localhost:3080/collections/articles/stats
```

## Indexing documents

```bash
curl -X POST http://localhost:3080/collections/articles/documents \
  -H "Content-Type: application/json" \
  -d '[
    {
      "id": "1",
      "title": "Introduction to Hybrid Search",
      "content": "Hybrid search combines full-text BM25 scoring with vector similarity...",
      "author": "Alice",
      "published_at": "2025-01-15T10:00:00Z"
    },
    {
      "id": "2",
      "title": "Vector Embeddings Explained",
      "content": "Dense vector embeddings capture semantic meaning of text...",
      "author": "Bob",
      "published_at": "2025-02-01T12:00:00Z"
    }
  ]'
```

### Bulk import from JSONL

```bash
# Each line is a JSON document
prism-cli document import -c articles -f articles.jsonl --batch-size 500
```

## Searching

```bash
# Basic search
curl -X POST http://localhost:3080/collections/articles/search \
  -H "Content-Type: application/json" \
  -d '{
    "query": "hybrid search",
    "limit": 10
  }'
```

Response:

```json
{
  "results": [
    {
      "id": "1",
      "score": 2.45,
      "fields": {
        "title": "Introduction to Hybrid Search",
        "content": "Hybrid search combines full-text BM25 scoring...",
        "author": "Alice"
      }
    }
  ],
  "total": 1
}
```

## CLI tools

```bash
# List collections
prism-cli collection list

# Inspect a collection
prism-cli collection inspect -n articles -v

# Export documents to JSONL
prism-cli document export -c articles -o articles-backup.jsonl

# Optimize index (merge segments)
prism-cli index optimize -c articles

# Run search benchmarks
prism-cli benchmark -c articles -q queries.txt -r 10

# View cache stats
prism-cli cache-stats -p ~/.prism/cache

# Clear old cache entries
prism-cli cache-clear -p ~/.prism/cache --older-than-days 30
```

## Next steps

- [Collections & Schema](collections.md) — field types, storage backends, system fields
- [Search](search.md) — query syntax, filtering, pagination
- [Vector Search](vector-search.md) — embeddings, HNSW configuration
- [Hybrid Search](hybrid-search.md) — combining text and vector, merge strategies
- [Aggregations](aggregations.md) — metrics, histograms, percentiles, filters
- [Highlighting](highlighting.md) — matched term snippets
- [Suggestions](suggestions.md) — autocomplete and "did you mean"
- [More Like This](more-like-this.md) — find similar documents
- [Ranking & Boosting](ranking.md) — recency decay, field weights, signals
- [API Reference](api-reference.md) — complete endpoint documentation
