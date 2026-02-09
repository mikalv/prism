# Collection Schema Reference

Collections are defined by YAML schema files in `<data_dir>/schemas/`.

## Basic Schema

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
```

## Full Schema Reference

```yaml
collection: articles
description: "News articles with semantic search"

# Backend configuration
backends:
  # Text search (Tantivy)
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
      - name: category
        type: string
        stored: true
        indexed: true
      - name: published_at
        type: date
        stored: true
        indexed: true
      - name: view_count
        type: i64
        stored: true
        indexed: true
    # BM25 tuning
    bm25_k1: 1.2
    bm25_b: 0.75

  # Vector search (HNSW)
  vector:
    embedding_field: content_vector
    dimension: 384
    distance: cosine
    hnsw_m: 16
    hnsw_ef_construction: 200
    hnsw_ef_search: 100
    vector_weight: 0.5

# Auto-generate embeddings
embedding_generation:
  enabled: true
  model: "all-MiniLM-L6-v2"
  source_field: content
  target_field: content_vector

# Indexing behavior
indexing:
  batch_size: 1000
  commit_interval_secs: 5
  worker_threads: 4

# Quotas
quota:
  max_documents: 1000000
  max_size_mb: 10240

# Faceted search
facets:
  allowed:
    - category
    - published_at
  default:
    - category
  configs:
    - field: category
      type: terms
      size: 10
    - field: published_at
      type: date_histogram
      interval: month

# Boosting/ranking
boosting:
  recency:
    field: published_at
    decay_function: exponential
    scale: "7d"
    offset: "1d"
    decay_rate: 0.5
  context:
    - field: project_id
      match_current: true
      boost: 2.0
  field_weights:
    title: 2.0
    content: 1.0
  signals:
    - name: view_count
      weight: 0.1

# System fields
system_fields:
  indexed_at: true
  document_boost: false

# Hybrid search defaults
hybrid:
  default_strategy: rrf
  rrf_k: 60
  text_weight: 0.5
  vector_weight: 0.5

# Storage (per-collection override)
storage:
  backend: local
  data_dir: "./data/articles"

# Two-phase reranking
reranking:
  type: cross_encoder
  candidates: 100
  text_fields:
    - title
    - content
  cross_encoder:
    model_id: "cross-encoder/ms-marco-MiniLM-L-6-v2"
    max_length: 512
```

---

## Field Types

| Type | Description | Use Case |
|------|-------------|----------|
| `text` | Full-text searchable, tokenized | Titles, content, descriptions |
| `string` | Exact match only, not tokenized | IDs, categories, tags |
| `i64` | Signed 64-bit integer | Counts, scores, timestamps |
| `u64` | Unsigned 64-bit integer | Positive counts, IDs |
| `f64` | 64-bit floating point | Prices, ratings, coordinates |
| `bool` | Boolean | Flags, filters |
| `date` | Timestamp (ISO 8601) | Created/updated dates |
| `bytes` | Binary data | Hashes, binary content |

### Field Options

| Option | Default | Description |
|--------|---------|-------------|
| `stored` | `true` | Store original value for retrieval |
| `indexed` | `true` | Index for searching/filtering |

---

## Text Backend

Full-text search powered by Tantivy (BM25).

```yaml
backends:
  text:
    fields:
      - name: title
        type: text
        stored: true
        indexed: true
    bm25_k1: 1.2
    bm25_b: 0.75
```

### BM25 Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `bm25_k1` | `1.2` | Term frequency saturation. Higher = more weight on term frequency |
| `bm25_b` | `0.75` | Length normalization. 0 = no normalization, 1 = full normalization |

---

## Vector Backend

Semantic search using HNSW (Hierarchical Navigable Small World).

```yaml
backends:
  vector:
    embedding_field: content_vector
    dimension: 384
    distance: cosine
    hnsw_m: 16
    hnsw_ef_construction: 200
    hnsw_ef_search: 100
    vector_weight: 0.5
```

### Vector Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `embedding_field` | required | Field name for vectors |
| `dimension` | required | Vector dimension (must match model) |
| `distance` | `cosine` | Distance metric |
| `hnsw_m` | `16` | Max connections per node (higher = better recall, more memory) |
| `hnsw_ef_construction` | `200` | Build-time search width (higher = better index, slower build) |
| `hnsw_ef_search` | `100` | Query-time search width (higher = better recall, slower query) |
| `vector_weight` | `0.5` | Weight in hybrid search |

### Distance Metrics

| Metric | Description | Best For |
|--------|-------------|----------|
| `cosine` | Cosine similarity | Normalized embeddings (default) |
| `euclidean` | L2 distance | Absolute distances matter |
| `dot` | Dot product | Unnormalized embeddings |

---

## Embedding Generation

Automatically generate embeddings at index time.

```yaml
embedding_generation:
  enabled: true
  model: "all-MiniLM-L6-v2"
  source_field: content
  target_field: content_vector
```

| Parameter | Description |
|-----------|-------------|
| `enabled` | Enable auto-generation |
| `model` | Model name (local ONNX or provider) |
| `source_field` | Field to embed |
| `target_field` | Field to store embedding |

---

## Indexing Configuration

```yaml
indexing:
  batch_size: 1000
  commit_interval_secs: 5
  worker_threads: 4
```

| Parameter | Default | Description |
|-----------|---------|-------------|
| `batch_size` | `1000` | Documents per indexing batch |
| `commit_interval_secs` | `5` | Auto-commit interval |
| `worker_threads` | CPU count | Parallel indexing threads |

---

## Quotas

```yaml
quota:
  max_documents: 1000000
  max_size_mb: 10240
```

| Parameter | Description |
|-----------|-------------|
| `max_documents` | Maximum document count |
| `max_size_mb` | Maximum index size in MB |

Exceeding quotas returns error on indexing.

---

## Facets

Configure faceted search (aggregations on results).

```yaml
facets:
  allowed:
    - category
    - author
    - published_at
  default:
    - category
  configs:
    - field: category
      type: terms
      size: 10
    - field: published_at
      type: date_histogram
      interval: month
```

| Parameter | Description |
|-----------|-------------|
| `allowed` | Fields that can be faceted |
| `default` | Facets returned by default |
| `configs` | Per-field facet settings |

### Facet Types

| Type | Parameters | Description |
|------|------------|-------------|
| `terms` | `size` | Top N terms |
| `date_histogram` | `interval` | Date buckets |
| `range` | `ranges` | Numeric ranges |

---

## Boosting

Configure relevance scoring.

### Recency Decay

Boost recent documents:

```yaml
boosting:
  recency:
    field: published_at
    decay_function: exponential
    scale: "7d"
    offset: "1d"
    decay_rate: 0.5
```

| Parameter | Description |
|-----------|-------------|
| `field` | Date field for decay |
| `decay_function` | `exponential`, `linear`, or `gauss` |
| `scale` | Distance at which score = decay_rate |
| `offset` | No decay within this period |
| `decay_rate` | Score multiplier at scale distance |

### Field Weights

Boost specific fields:

```yaml
boosting:
  field_weights:
    title: 2.0
    content: 1.0
    tags: 1.5
```

### Signals

Incorporate numeric fields:

```yaml
boosting:
  signals:
    - name: view_count
      weight: 0.1
    - name: rating
      weight: 0.2
```

---

## System Fields

```yaml
system_fields:
  indexed_at: true
  document_boost: false
```

| Field | Description |
|-------|-------------|
| `indexed_at` | Auto-add `_indexed_at` timestamp |
| `document_boost` | Enable per-document `_boost` field |

---

## Hybrid Search

Configure default hybrid search behavior:

```yaml
hybrid:
  default_strategy: rrf
  rrf_k: 60
  text_weight: 0.5
  vector_weight: 0.5
```

| Parameter | Default | Description |
|-----------|---------|-------------|
| `default_strategy` | `rrf` | Merge strategy: `rrf` or `weighted` |
| `rrf_k` | `60` | RRF ranking constant |
| `text_weight` | `0.5` | Text score weight (for `weighted`) |
| `vector_weight` | `0.5` | Vector score weight (for `weighted`) |

---

## Reranking (Two-Phase)

Configure a second-phase re-ranker to improve result quality. Phase 1 retrieves candidates cheaply (BM25/vector), Phase 2 re-scores them with an expensive model.

### Cross-encoder

```yaml
reranking:
  type: cross_encoder
  candidates: 100
  text_fields:
    - title
    - content
  cross_encoder:
    model_id: "cross-encoder/ms-marco-MiniLM-L-6-v2"
    max_length: 512
```

### Score function

```yaml
reranking:
  type: score_function
  score_function: "_score * popularity * 0.01"
```

### Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `type` | required | `cross_encoder` or `score_function` |
| `candidates` | `100` | Number of Phase 1 candidates |
| `text_fields` | `[]` | Fields to extract for re-ranking |
| `cross_encoder.model_id` | `cross-encoder/ms-marco-MiniLM-L-6-v2` | HuggingFace model ID |
| `cross_encoder.model_path` | auto-download | Local path to ONNX model |
| `cross_encoder.max_length` | `512` | Max token length |
| `score_function` | none | Arithmetic expression using `_score`, field names, `+`, `-`, `*`, `/`, `log()` |

See [Ranking & Boosting](../guides/ranking.md#two-phase-ranking-re-ranking) for usage examples.

---

## Per-Collection Storage

Override global storage for specific collections:

```yaml
storage:
  backend: s3
  s3:
    bucket: archive-bucket
    prefix: "archive/"
```

---

## See Also

- [Getting Started](../guides/getting-started.md) — Quick setup guide
- [Pipelines](pipelines.md) — Document processing
- [Hybrid Search](../guides/hybrid-search.md) — Combining text and vector
