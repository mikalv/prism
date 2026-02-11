# API Reference

Complete reference for all Prism HTTP endpoints.

Base URL: `http://localhost:3080` (default)

---

## Health

### GET /health

Check server health.

**Response:** `200 OK`

```json
{ "status": "ok" }
```

---

## Search

### POST /collections/:collection/search

Full-featured search endpoint.

**Request:**

```json
{
  "query": "search terms",
  "vector": [0.1, 0.2, ...],
  "fields": ["title", "content"],
  "limit": 10,
  "offset": 0,
  "merge_strategy": "rrf",
  "text_weight": 0.5,
  "vector_weight": 0.5,
  "highlight": {
    "fields": ["title", "content"],
    "pre_tag": "<b>",
    "post_tag": "</b>",
    "fragment_size": 150,
    "number_of_fragments": 3
  }
}
```

| Field | Type | Default | Required |
|-------|------|---------|----------|
| `query` | string | `""` | No |
| `vector` | float[] | null | No |
| `fields` | string[] | all fields | No |
| `limit` | integer | 10 | No |
| `offset` | integer | 0 | No |
| `merge_strategy` | string | null | No |
| `text_weight` | float | 0.5 | No |
| `vector_weight` | float | 0.5 | No |
| `highlight` | object | null | No |

**Response:** `200 OK`

```json
{
  "results": [
    {
      "id": "doc-1",
      "score": 4.82,
      "fields": { "title": "...", "content": "..." },
      "highlight": { "title": ["..."], "content": ["..."] }
    }
  ],
  "total": 42
}
```

**Errors:**
- `404` — Collection not found
- `500` — Internal search error

---

### POST /api/search

Simple search across all collections.

**Request:**

```json
{
  "query": "search terms",
  "limit": 10
}
```

**Response:** `200 OK`

```json
{
  "results": [
    {
      "id": "doc-1",
      "title": "Document Title",
      "url": "https://...",
      "snippet": "Matched text excerpt...",
      "score": 4.82
    }
  ],
  "total": 1
}
```

---

### POST /search/lucene

Advanced Lucene-style query DSL with facets, boosting, and context.

**Request:**

```json
{
  "collection": "articles",
  "query": "rust AND async",
  "limit": 10,
  "offset": 0,
  "enable_semantic": false,
  "vector": null,
  "facets": [
    { "field": "category", "agg_type": "terms", "size": 10, "interval": null }
  ],
  "context": {
    "project_id": "my-project",
    "session_id": "abc-123"
  },
  "boosting": {
    "recency_enabled": true,
    "recency_field": "timestamp",
    "recency_decay_days": 30,
    "context_boost": 1.5
  },
  "merge_strategy": "rrf",
  "text_weight": 0.5,
  "vector_weight": 0.5
}
```

**Response:** `200 OK`

```json
{
  "results": [
    { "id": "doc-1", "score": 5.23, "fields": { ... } }
  ],
  "total": 15,
  "facets": {
    "category": {
      "buckets": [
        { "key": "programming", "count": 12 }
      ]
    }
  },
  "suggestions": null,
  "meta": {
    "query_time_ms": 2.45,
    "parse_time_ms": 0.12,
    "search_time_ms": 2.33,
    "query_type": "BooleanQuery",
    "result_count": 15,
    "total_matches": 15
  }
}
```

---

## Documents

### POST /collections/:collection/documents

Index documents (bulk).

**Request:** Array of JSON documents.

```json
[
  { "id": "1", "title": "First Document", "content": "..." },
  { "id": "2", "title": "Second Document", "content": "..." }
]
```

Each document must have an `id` field. Other fields must match the collection schema.

**Response:** `200 OK`

**Errors:**
- `404` — Collection not found
- `500` — Indexing error

---

### GET /collections/:collection/documents/:id

Retrieve a single document by ID.

**Response:** `200 OK`

```json
{
  "id": "doc-1",
  "fields": {
    "title": "Document Title",
    "content": "Full document content..."
  }
}
```

**Errors:**
- `404` — Document or collection not found

---

## Aggregations

### POST /collections/:collection/aggregate

Run aggregations with an optional query filter.

**Request:**

```json
{
  "query": "optional filter",
  "scan_limit": 10000,
  "aggregations": [
    { "name": "total", "type": "count" },
    { "name": "price_avg", "type": "avg", "field": "price" },
    { "name": "price_stats", "type": "stats", "field": "price" },
    {
      "name": "price_pct",
      "type": "percentiles",
      "field": "price",
      "percents": [50, 95, 99]
    },
    {
      "name": "categories",
      "type": "terms",
      "field": "category",
      "size": 10,
      "aggs": [
        { "name": "avg_price", "type": "avg", "field": "price" }
      ]
    },
    {
      "name": "price_hist",
      "type": "histogram",
      "field": "price",
      "interval": 50,
      "min_doc_count": 1,
      "extended_bounds": { "min": 0, "max": 500 }
    },
    {
      "name": "over_time",
      "type": "date_histogram",
      "field": "created_at",
      "calendar_interval": "month",
      "min_doc_count": 0
    },
    {
      "name": "price_ranges",
      "type": "range",
      "field": "price",
      "ranges": [
        { "key": "cheap", "to": 50 },
        { "key": "mid", "from": 50, "to": 200 },
        { "key": "expensive", "from": 200 }
      ]
    },
    {
      "name": "active_only",
      "type": "filter",
      "filter": "status:active",
      "aggs": [
        { "name": "count", "type": "count" }
      ]
    },
    {
      "name": "by_status",
      "type": "filters",
      "filters": {
        "active": "status:active",
        "archived": "status:archived"
      }
    },
    {
      "name": "everything",
      "type": "global",
      "aggs": [
        { "name": "total", "type": "count" }
      ]
    }
  ]
}
```

**Response:** `200 OK`

```json
{
  "results": {
    "total": { "name": "total", ... },
    "price_avg": { "name": "price_avg", ... },
    ...
  },
  "took_ms": 12
}
```

**Aggregation types:**

| Type | Fields | Description |
|------|--------|-------------|
| `count` | — | Document count |
| `sum` | `field` | Sum of numeric field |
| `avg` | `field` | Average of numeric field |
| `min` | `field` | Minimum value |
| `max` | `field` | Maximum value |
| `stats` | `field` | Count, min, max, sum, avg |
| `percentiles` | `field`, `percents?` | Percentile values |
| `terms` | `field`, `size?` | Group by unique values |
| `histogram` | `field`, `interval`, `min_doc_count?`, `extended_bounds?` | Fixed-width numeric buckets |
| `date_histogram` | `field`, `calendar_interval`, `min_doc_count?` | Calendar-aligned time buckets |
| `range` | `field`, `ranges` | Custom numeric ranges |
| `filter` | `filter` | Filter by query |
| `filters` | `filters` | Multiple named filters |
| `global` | — | Ignore query, run on all docs |

All bucket aggregations accept `aggs` for nested sub-aggregations.

---

## Suggestions

### POST /collections/:collection/_suggest

Prefix completion and fuzzy suggestions.

**Request:**

```json
{
  "prefix": "mach",
  "field": "title",
  "size": 5,
  "fuzzy": true,
  "max_distance": 2
}
```

| Field | Type | Default | Required |
|-------|------|---------|----------|
| `prefix` | string | — | Yes |
| `field` | string | — | Yes |
| `size` | integer | 5 | No |
| `fuzzy` | boolean | false | No |
| `max_distance` | integer | 2 | No |

**Response:** `200 OK`

```json
{
  "suggestions": [
    { "term": "machine", "score": 1.0, "doc_freq": 142 }
  ],
  "did_you_mean": "machine learning"
}
```

---

## More Like This

### POST /collections/:collection/_mlt

Find similar documents.

**Request (by document ID):**

```json
{
  "like": { "_id": "doc-123" },
  "fields": ["content"],
  "min_term_freq": 2,
  "min_doc_freq": 5,
  "max_query_terms": 25,
  "size": 10
}
```

**Request (by text):**

```json
{
  "like_text": "Rust async programming with tokio",
  "fields": ["content"],
  "size": 10
}
```

| Field | Type | Default | Required |
|-------|------|---------|----------|
| `like` | object | null | One of `like`/`like_text` |
| `like_text` | string | null | One of `like`/`like_text` |
| `fields` | string[] | all text fields | No |
| `min_term_freq` | integer | 2 | No |
| `min_doc_freq` | integer | 5 | No |
| `max_query_terms` | integer | 25 | No |
| `size` | integer | 10 | No |

**Response:** `200 OK` — Standard search results format.

---

## Index Inspection

### GET /collections/:collection/terms/:field

Get top terms in a field.

**Query parameters:**
- `limit` (integer, default 25) — Max terms to return

**Response:** `200 OK`

```json
{
  "field": "title",
  "terms": [
    { "term": "search", "doc_freq": 142 },
    { "term": "engine", "doc_freq": 98 }
  ]
}
```

---

### GET /collections/:collection/segments

Get segment information.

**Response:** `200 OK`

```json
{
  "collection": "articles",
  "segments": [
    {
      "id": "...",
      "num_docs": 5000,
      "num_deleted_docs": 12,
      "size_bytes": 1048576
    }
  ]
}
```

---

### GET /collections/:collection/doc/:id/reconstruct

Reconstruct a document from the index (all stored fields).

**Response:** `200 OK`

```json
{
  "id": "doc-1",
  "fields": {
    "title": "...",
    "content": "...",
    "_indexed_at": 1706745600000000
  }
}
```

---

## Collection Management

### GET /admin/collections

List all collections.

**Response:** `200 OK`

```json
["articles", "products", "logs"]
```

---

### GET /collections/:collection/schema

Get collection schema.

**Response:** `200 OK` — The full YAML schema as JSON.

---

### GET /collections/:collection/stats

Get collection statistics.

**Response:** `200 OK`

```json
{
  "document_count": 50000,
  "size_bytes": 104857600
}
```

---

### GET /admin/lint-schemas

Validate all collection schemas.

**Response:** `200 OK`

```json
{
  "valid": ["articles", "products"],
  "errors": {
    "broken_collection": "Missing required field: backends"
  }
}
```

---

## Server Stats

### GET /stats/cache

Get embedding cache statistics.

**Response:** `200 OK`

```json
{
  "total_entries": 45000,
  "total_bytes": 128000000,
  "hits": 12000,
  "misses": 3000,
  "hit_rate": 0.80
}
```

---

### GET /stats/server

Get server information.

**Response:** `200 OK`

```json
{
  "version": "0.3.0",
  "features": ["text", "vector", "embedding"]
}
```

---

## Cluster (Federation)

Available when built with the `cluster` feature and `[cluster] enabled = true`.

### POST /cluster/collections/:collection/search

Federated search across all shards in the cluster.

**Request:**

```json
{
  "query": "distributed search",
  "limit": 10
}
```

**Response:** `200 OK`

```json
{
  "results": [
    { "id": "doc1", "score": 1.756, "fields": { ... }, "highlight": null }
  ],
  "total": 3,
  "latency_ms": 46,
  "is_partial": false,
  "shard_status": { "total": 3, "successful": 3, "failed": 0 }
}
```

`is_partial` is `true` when some shards failed but partial results are returned.

---

### POST /cluster/collections/:collection/documents

Federated index — routes documents to the correct shard by ID hash.

**Request:**

```json
{
  "documents": [
    { "id": "doc1", "fields": { "title": "hello", "body": "world" } }
  ]
}
```

**Response:** `201 Created`

```json
{
  "total_docs": 1,
  "successful_docs": 1,
  "failed_docs": 0,
  "latency_ms": 32
}
```

---

### GET /cluster/health

Returns cluster status.

**Response:** `200 OK`

```json
{
  "status": "ok",
  "message": "cluster operational"
}
```

See [Clustering & Federation](../guides/clustering.md) for the full guide.

---

## Error responses

All error responses use standard HTTP status codes:

| Code | Meaning |
|------|---------|
| `400` | Bad request (invalid query syntax, missing required field) |
| `404` | Collection or document not found |
| `500` | Internal server error |

Error bodies vary by endpoint but typically include a message string.
