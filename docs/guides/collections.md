# Collections & Schema Configuration

A collection in Prism is a named index with a defined schema. Collections are configured through YAML files placed in the `schemas/` directory.

## Schema file structure

```yaml
collection: <name>              # Required: unique collection name
description: <text>             # Optional: human-readable description
backends:                       # Required: at least one backend
  text: { ... }                 # Full-text search (Tantivy)
  vector: { ... }               # Vector similarity search (HNSW)
  graph: { ... }                # Graph traversal
indexing: { ... }               # Optional: indexing performance tuning
quota: { ... }                  # Optional: size limits
embedding_generation: { ... }   # Optional: auto-embed on index
facets: { ... }                 # Optional: faceted search config
boosting: { ... }               # Optional: ranking adjustments
storage: { ... }                # Optional: storage backend (local/S3)
system_fields: { ... }          # Optional: automatic system fields
hybrid: { ... }                 # Optional: hybrid search defaults
```

## Text backend

The text backend uses Tantivy for full-text indexing with BM25 scoring.

```yaml
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
      - name: body
        type: text
        stored: true
        indexed: true
      - name: category
        type: string
        stored: true
        indexed: true
      - name: price
        type: f64
        stored: true
        indexed: false
      - name: created_at
        type: date
        stored: true
        indexed: true
      - name: view_count
        type: u64
        stored: true
        indexed: false

    # Optional: BM25 parameter tuning
    bm25_k1: 1.2   # Term saturation (default 1.2, higher = more term frequency impact)
    bm25_b: 0.75    # Length normalization (default 0.75, 0 = none, 1 = full)
```

### Field types

| Type | Description | Use case |
|------|-------------|----------|
| `text` | Tokenized, full-text searchable | Titles, content, descriptions |
| `string` | Exact match, not tokenized | IDs, categories, tags, status |
| `i64` | Signed 64-bit integer | Offsets, signed counters |
| `u64` | Unsigned 64-bit integer | Counts, timestamps (epoch) |
| `f64` | 64-bit floating point | Prices, scores, coordinates |
| `bool` | Boolean true/false | Flags, toggles |
| `date` | RFC 3339 timestamp | Created/updated dates |
| `bytes` | Raw byte data | Binary blobs |

### Field options

| Option | Default | Description |
|--------|---------|-------------|
| `stored` | `false` | Field value returned in search results |
| `indexed` | `false` | Field is searchable/filterable |

You typically want `stored: true` for fields you display in results and `indexed: true` for fields you search or filter on. A field can be both.

## Vector backend

The vector backend provides approximate nearest-neighbor (ANN) search using HNSW.

```yaml
backends:
  vector:
    embedding_field: content_vector   # Field name for embeddings
    dimension: 384                     # Vector dimensionality
    distance: cosine                   # cosine | euclidean | dot
    hnsw_m: 16                         # Graph connectivity (default 16)
    hnsw_ef_construction: 200          # Build-time quality (default 200)
    hnsw_ef_search: 100                # Search-time quality (default 100)
    vector_weight: 0.5                 # Weight in hybrid search (default 0.5)
```

### HNSW parameters

| Parameter | Default | Effect |
|-----------|---------|--------|
| `hnsw_m` | 16 | Number of connections per node. Higher = better recall, more memory |
| `hnsw_ef_construction` | 200 | Build quality. Higher = slower indexing, better graph quality |
| `hnsw_ef_search` | 100 | Search quality. Higher = slower search, better recall |

### Distance metrics

| Metric | Description | When to use |
|--------|-------------|-------------|
| `cosine` | Angular similarity (normalized) | Most embedding models (default) |
| `euclidean` | L2 distance | When magnitude matters |
| `dot` | Dot product | Pre-normalized vectors, max inner product |

## Graph backend

The graph backend stores relationships between documents for traversal queries.

```yaml
backends:
  graph:
    path: ./data/graph
    edges:
      - edge_type: references
        from_field: id
        to_field: referenced_id
      - edge_type: authored_by
        from_field: id
        to_field: author_id
```

## Automatic embedding generation

Prism can automatically generate embeddings when documents are indexed:

```yaml
embedding_generation:
  enabled: true
  model: nomic-embed-text           # Model name (Ollama/OpenAI)
  source_field: content             # Field to embed
  target_field: content_vector      # Where to store the embedding
```

This requires an embedding provider configured in the server config (Ollama, OpenAI, or ONNX).

## Indexing configuration

Control batching and commit behavior:

```yaml
indexing:
  batch_size: 1000              # Documents per batch (default 1000)
  commit_interval_secs: 5       # Auto-commit interval (default 5)
  worker_threads: 4             # Indexing worker threads (default 4)
```

## Quotas

Limit collection size:

```yaml
quota:
  max_documents: 1000000         # Max document count
  max_size_mb: 5000              # Max index size in MB
```

## System fields

Prism can automatically add system fields to every document:

```yaml
system_fields:
  indexed_at: true          # Add _indexed_at timestamp (default: true)
  document_boost: false     # Enable _boost field per document (default: false)
```

- `_indexed_at` — microsecond-precision timestamp, required for recency scoring
- `_boost` — per-document numeric multiplier for popularity-based ranking

When `document_boost` is enabled, you can set a `_boost` value on each document:

```json
{
  "id": "popular-article",
  "title": "Viral Post",
  "_boost": 2.5
}
```

## Faceted search configuration

Pre-configure which fields support faceting and default behavior:

```yaml
facets:
  allowed:
    - category
    - status
    - author
  default:
    - category
  configs:
    - field: category
      type: terms
      size: 10
    - field: created_at
      type: date_histogram
      interval: month
```

### Facet types

| Type | Description |
|------|-------------|
| `terms` | Count unique values |
| `date_histogram` | Bucket timestamps by interval |
| `range` | Bucket numbers by ranges |
| `stats` | Compute min/max/avg/sum |

## Boosting configuration

Configure ranking adjustments applied after search scoring:

```yaml
boosting:
  # Recency: prefer newer documents
  recency:
    field: published_at
    decay_function: exponential   # exponential | linear | gauss
    scale: 7d                      # Time scale for decay
    offset: 1d                     # No decay within this period
    decay_rate: 0.5                # Decay factor at scale distance

  # Context: boost documents matching current context
  context:
    - field: project_id
      match_current: true
      boost: 2.0
    - field: author
      match_current: true
      boost: 1.5

  # Field weights: scale relevance by field
  field_weights:
    title: 2.0
    content: 1.0

  # Custom ranking signals: numeric fields contribute to scoring
  signals:
    - name: view_count
      weight: 0.001
    - name: like_count
      weight: 0.01
```

See the [Ranking & Boosting](ranking.md) guide for detailed documentation.

## Hybrid search defaults

Configure the default behavior when both text and vector backends are present:

```yaml
hybrid:
  default_strategy: rrf       # rrf | weighted
  rrf_k: 60                    # RRF smoothing parameter (default 60)
  text_weight: 0.5             # Text score weight (default 0.5)
  vector_weight: 0.5           # Vector score weight (default 0.5)
```

See the [Hybrid Search](hybrid-search.md) guide for detailed documentation.

## Storage backend

By default, Prism stores data locally. You can use S3 or S3-compatible storage:

```yaml
# Local storage (default)
storage:
  type: local
  path: /data/prism/articles

# S3 storage
storage:
  type: s3
  bucket: my-prism-bucket
  region: us-east-1
  prefix: collections/articles
  # endpoint: http://minio:9000     # For MinIO/S3-compatible
  # force_path_style: true           # Required for MinIO
  # cache_dir: /tmp/prism-cache      # Local cache for S3 data
  # cache_max_size_mb: 1000
```

## Complete example

A production-ready schema combining all features:

```yaml
collection: products
description: "E-commerce product catalog with hybrid search"

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
      - name: description
        type: text
        stored: true
        indexed: true
      - name: category
        type: string
        stored: true
        indexed: true
      - name: brand
        type: string
        stored: true
        indexed: true
      - name: price
        type: f64
        stored: true
        indexed: false
      - name: rating
        type: f64
        stored: true
        indexed: false
      - name: in_stock
        type: bool
        stored: true
        indexed: true
      - name: created_at
        type: date
        stored: true
        indexed: true
    bm25_k1: 1.5
    bm25_b: 0.75

  vector:
    embedding_field: description_vector
    dimension: 384
    distance: cosine
    hnsw_m: 32
    hnsw_ef_construction: 400

embedding_generation:
  enabled: true
  model: nomic-embed-text
  source_field: description
  target_field: description_vector

indexing:
  batch_size: 2000
  commit_interval_secs: 10
  worker_threads: 8

quota:
  max_documents: 5000000
  max_size_mb: 20000

system_fields:
  indexed_at: true
  document_boost: true

facets:
  allowed: [category, brand, in_stock]
  default: [category]
  configs:
    - field: category
      type: terms
      size: 20
    - field: brand
      type: terms
      size: 15

boosting:
  recency:
    field: created_at
    decay_function: exponential
    scale: 30d
    decay_rate: 0.7
  field_weights:
    title: 3.0
    description: 1.0
  signals:
    - name: rating
      weight: 0.1

hybrid:
  default_strategy: rrf
  rrf_k: 60
  text_weight: 0.6
  vector_weight: 0.4
```
