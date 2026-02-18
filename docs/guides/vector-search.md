# Vector Search

Vector search finds documents by semantic similarity using dense vector embeddings. Prism uses HNSW (Hierarchical Navigable Small World) graphs for sub-millisecond approximate nearest-neighbor search.

## Setup

### 1. Configure the vector backend in your schema

```yaml
collection: articles
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
  vector:
    embedding_field: content_vector
    dimension: 384
    distance: cosine
```

### 2. Configure an embedding provider

In your server config (`prism.toml`), set up one of the supported providers:

**Ollama (local, recommended for development):**

```toml
[embedding]
enabled = true

[embedding.provider]
type = "ollama"
url = "http://localhost:11434"
model = "nomic-embed-text"
```

Configure the provider in your collection or server setup. Prism supports three providers:

#### Ollama

Runs embedding models locally via Ollama:

```json
{
  "type": "ollama",
  "url": "http://localhost:11434",
  "model": "nomic-embed-text"
}
```

Popular Ollama models:
| Model | Dimensions | Description |
|-------|-----------|-------------|
| `nomic-embed-text` | 768 | Good general-purpose, fast |
| `all-minilm` | 384 | Lightweight, fast |
| `mxbai-embed-large` | 1024 | High quality, slower |

#### OpenAI-compatible

Works with OpenAI, Azure OpenAI, Together.ai, and other compatible APIs:

```json
{
  "type": "openai",
  "url": "https://api.openai.com/v1",
  "api_key": "sk-...",
  "model": "text-embedding-3-small"
}
```

| Model | Dimensions | Description |
|-------|-----------|-------------|
| `text-embedding-3-small` | 1536 | Fast, cost-effective |
| `text-embedding-3-large` | 3072 | Highest quality |
| `text-embedding-ada-002` | 1536 | Legacy model |

#### ONNX (local, feature-gated)

Run models locally without a server (requires `provider-onnx` feature):

```json
{
  "type": "onnx",
  "model_id": "all-MiniLM-L6-v2"
}
```

### 3. Enable automatic embedding generation

Prism can automatically embed documents on indexing:

```yaml
embedding_generation:
  enabled: true
  model: nomic-embed-text
  source_field: content
  target_field: content_vector
```

With this enabled, you index plain text and Prism generates embeddings automatically:

```bash
curl -X POST http://localhost:3080/collections/articles/documents \
  -d '[{ "id": "1", "title": "About Rust", "content": "Rust is a systems programming language..." }]'
```

The `content_vector` field is populated automatically.

## Searching with vectors

### Explicit vector query

If you pre-compute embeddings client-side:

```bash
curl -X POST http://localhost:3080/collections/articles/search \
  -H "Content-Type: application/json" \
  -d '{
    "vector": [0.1, -0.05, 0.23, ...],
    "limit": 10
  }'
```

### Text query with auto-embedding

When an embedding provider is configured, text queries are automatically embedded for vector search:

```bash
curl -X POST http://localhost:3080/collections/articles/search \
  -d '{ "query": "systems programming language", "limit": 10 }'
```

## HNSW parameters

The HNSW index has three tunable parameters that control the trade-off between speed, memory, and accuracy:

### `hnsw_m` (default: 16)

Number of bi-directional links per node in the graph. Higher values create a denser graph.

| Value | Index size | Search speed | Recall |
|-------|-----------|--------------|--------|
| 8 | Smallest | Fastest | Lower |
| 16 | Medium | Fast | Good |
| 32 | Large | Slower | Very good |
| 64 | Very large | Slowest | Best |

### `hnsw_ef_construction` (default: 200)

Search depth during index construction. Higher values build a better graph but take longer.

| Value | Build time | Graph quality |
|-------|-----------|---------------|
| 100 | Fast | Good |
| 200 | Medium | Very good |
| 400 | Slow | Excellent |

### `hnsw_ef_search` (default: 100)

Search depth during queries. Higher values explore more candidates for better recall.

| Value | Query time | Recall |
|-------|-----------|--------|
| 50 | ~0.1ms | ~95% |
| 100 | ~0.2ms | ~98% |
| 200 | ~0.5ms | ~99.5% |

### Tuning guidelines

- **High-precision RAG**: `hnsw_m: 32, ef_construction: 400, ef_search: 200`
- **Balanced**: `hnsw_m: 16, ef_construction: 200, ef_search: 100` (defaults)
- **Speed-optimized**: `hnsw_m: 8, ef_construction: 100, ef_search: 50`

## Distance metrics

| Metric | Formula | When to use |
|--------|---------|-------------|
| `cosine` | `1 - cos(a, b)` | Most embedding models (default). Invariant to vector magnitude. |
| `euclidean` | `\|a - b\|₂` | When absolute distance matters, not just direction |
| `dot` | `a · b` | Pre-normalized vectors, maximum inner product search |

Most embedding models produce vectors where cosine similarity is the intended metric. Use `euclidean` or `dot` only if your model documentation specifically recommends it.

## Embedding cache

Prism caches embeddings to avoid redundant API calls. The cache uses batch operations for bulk imports — a 500-document batch requires only 2 database operations instead of 1000.

### SQLite cache (default)

Persistent, single-file, no external dependencies. Uses WAL mode for high write throughput:

```toml
# In your embedding cache configuration
backend = "sqlite"
path = "~/.prism/cache/embeddings.db"
max_entries = 1000000
```

### Redis cache (optional)

Distributed, shared across servers (requires `cache-redis` feature). Uses MGET/pipeline for batch operations:

```toml
backend = "redis"
url = "redis://localhost:6379"
```

### Bulk import tuning

For large imports, adjust batch size and concurrency in `prism.toml`:

```toml
[embedding]
batch_size = 128    # Texts per embedding API call (default: 128)
concurrency = 4     # Concurrent API calls (default: 4)
```

Lower `batch_size` if your provider has request size limits. Increase `concurrency` if your provider supports parallel requests (e.g. OpenAI).

### Cache key strategies

| Strategy | Cache key includes | Use case |
|----------|-------------------|----------|
| `TextOnly` | Text hash | Single model deployments |
| `ModelText` | Model + text hash | Multi-model (default, recommended) |
| `ModelVersionText` | Model + version + text | Version-pinned reproducibility |

### Cache statistics

```bash
curl http://localhost:3080/stats/cache
```

```json
{
  "total_entries": 45000,
  "total_bytes": 128000000,
  "hits": 12000,
  "misses": 3000,
  "hit_rate": 0.80
}
```

## Notes

- Vector dimension must match exactly between the schema, embedding provider, and indexed documents
- If you change embedding models, you need to re-index all documents (dimensions and vector space change)
- The embedding cache is keyed by model + text, so switching models doesn't corrupt the cache
- For large collections (>1M vectors), consider increasing `hnsw_m` and `ef_construction` for better recall
