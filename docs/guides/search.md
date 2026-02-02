# Search

Prism provides multiple search endpoints for different use cases. This guide covers text search, query syntax, filtering, and pagination.

## Basic search

The primary search endpoint is `POST /collections/:collection/search`:

```bash
curl -X POST http://localhost:3080/collections/articles/search \
  -H "Content-Type: application/json" \
  -d '{
    "query": "machine learning",
    "limit": 10,
    "offset": 0
  }'
```

### Request fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `query` | string | `""` | Text query (BM25 full-text search) |
| `vector` | float[] | null | Explicit vector for similarity search |
| `fields` | string[] | all | Restrict search to specific fields |
| `limit` | integer | 10 | Maximum results to return |
| `offset` | integer | 0 | Skip this many results (pagination) |
| `merge_strategy` | string | null | Hybrid merge: `"rrf"` or `"weighted"` |
| `text_weight` | float | 0.5 | Text score weight (hybrid mode) |
| `vector_weight` | float | 0.5 | Vector score weight (hybrid mode) |
| `highlight` | object | null | Highlight configuration (see [Highlighting](highlighting.md)) |

### Response format

```json
{
  "results": [
    {
      "id": "doc-123",
      "score": 4.82,
      "fields": {
        "title": "Introduction to Machine Learning",
        "content": "Machine learning is a subset of artificial intelligence...",
        "author": "Alice",
        "published_at": "2025-01-15T10:00:00Z"
      },
      "highlight": {
        "title": ["Introduction to <b>Machine</b> <b>Learning</b>"],
        "content": ["<b>Machine</b> <b>learning</b> is a subset of artificial intelligence..."]
      }
    }
  ],
  "total": 42
}
```

## Field-scoped search

Restrict the query to specific indexed fields:

```json
{
  "query": "rust async",
  "fields": ["title", "tags"]
}
```

When `fields` is empty (default), all indexed text fields are searched.

## Pagination

Use `limit` and `offset` for pagination:

```json
{ "query": "search engine", "limit": 20, "offset": 0 }    // Page 1
{ "query": "search engine", "limit": 20, "offset": 20 }   // Page 2
{ "query": "search engine", "limit": 20, "offset": 40 }   // Page 3
```

## Simple search API

For quick integrations, there's a simpler endpoint at `POST /api/search`:

```bash
curl -X POST http://localhost:3080/api/search \
  -H "Content-Type: application/json" \
  -d '{"query": "machine learning", "limit": 5}'
```

This searches across all collections and returns simplified results:

```json
{
  "results": [
    {
      "id": "doc-123",
      "title": "Introduction to ML",
      "url": null,
      "snippet": "Machine learning is a subset of...",
      "score": 4.82
    }
  ],
  "total": 1
}
```

The simple API maps `title`, `url`/`link`, and `snippet`/`content`/`description` fields automatically.

## Lucene query DSL

For advanced queries with boosting, faceting, and context, use `POST /search/lucene`:

```bash
curl -X POST http://localhost:3080/search/lucene \
  -H "Content-Type: application/json" \
  -d '{
    "collection": "articles",
    "query": "rust AND (async OR tokio)",
    "limit": 10,
    "offset": 0,
    "facets": [
      { "field": "category", "agg_type": "terms", "size": 10 }
    ],
    "boosting": {
      "recency_enabled": true,
      "recency_field": "published_at",
      "recency_decay_days": 30,
      "context_boost": 1.5
    },
    "context": {
      "project_id": "my-project"
    }
  }'
```

### Query syntax

The Lucene-style parser supports:

| Syntax | Example | Description |
|--------|---------|-------------|
| Terms | `machine learning` | Match any term |
| Phrases | `"exact phrase"` | Match exact sequence |
| Field targeting | `title:rust` | Search specific field |
| AND | `rust AND async` | Both terms required |
| OR | `rust OR go` | Either term matches |
| NOT | `rust NOT unsafe` | Exclude term |
| Grouping | `(async OR tokio) AND rust` | Precedence control |
| Boost | `title:rust^2.0` | Boost term weight |
| Wildcards | `prog*` or `te?t` | Prefix/single-char wildcard |
| Ranges | `year:[2020 TO 2024]` | Inclusive range |
| Open ranges | `price:{10 TO *}` | Exclusive/unbounded |

### Lucene response

```json
{
  "results": [
    {
      "id": "doc-1",
      "score": 5.23,
      "fields": { "title": "Async Rust", "category": "programming" }
    }
  ],
  "total": 15,
  "facets": {
    "category": {
      "buckets": [
        { "key": "programming", "count": 12 },
        { "key": "tutorial", "count": 3 }
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

### Facets in Lucene search

Request facets alongside search results:

```json
{
  "facets": [
    { "field": "category", "agg_type": "terms", "size": 10 },
    {
      "field": "created_at",
      "agg_type": "date_histogram",
      "interval": "month"
    }
  ]
}
```

### Context-aware boosting

Provide context to boost documents matching the current user's environment:

```json
{
  "context": {
    "project_id": "prism",
    "session_id": "abc-123"
  },
  "boosting": {
    "recency_enabled": true,
    "recency_field": "timestamp",
    "recency_decay_days": 7,
    "context_boost": 2.0
  }
}
```

Documents with a `project_id` field matching `"prism"` receive a 2x score boost.

## Retrieving documents by ID

```bash
curl http://localhost:3080/collections/articles/documents/doc-123
```

Returns the full document with all stored fields, or 404 if not found.

## Index inspection

Inspect the internal state of your index:

```bash
# Top terms in a field (useful for understanding term distribution)
curl "http://localhost:3080/collections/articles/terms/title?limit=25"

# Segment information
curl http://localhost:3080/collections/articles/segments

# Reconstruct a document from the index
curl http://localhost:3080/collections/articles/doc/doc-123/reconstruct
```

### Top terms response

```json
{
  "field": "title",
  "terms": [
    { "term": "search", "doc_freq": 142 },
    { "term": "engine", "doc_freq": 98 },
    { "term": "vector", "doc_freq": 76 }
  ]
}
```
