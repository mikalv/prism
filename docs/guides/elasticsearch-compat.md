# Elasticsearch Compatibility

Prism includes an Elasticsearch-compatible REST API layer, allowing existing ES clients (elasticsearch-py, elasticsearch-js, PrismEx, etc.) to connect with minimal changes.

## Enabling

Build with the `es-compat` feature flag:

```bash
cargo build --release -p prismsearch-server --features es-compat
```

When enabled, ES-compatible endpoints are mounted at the `/_elastic/` prefix.

## Supported Endpoints

### Cluster

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/_elastic/` | Cluster info (version, name) |
| `GET` | `/_elastic/_cat/indices` | List all indices |
| `GET` | `/_elastic/_cluster/health` | Cluster health |

### Document CRUD

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/_elastic/{index}/_doc/{id}` | Get document by ID |
| `HEAD` | `/_elastic/{index}/_doc/{id}` | Check if document exists |
| `POST` | `/_elastic/{index}/_doc` | Index document (auto-ID) |
| `PUT` | `/_elastic/{index}/_doc/{id}` | Index document (explicit ID) |
| `DELETE` | `/_elastic/{index}/_doc/{id}` | Delete document |

### Search

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/_elastic/{index}/_search` | Search with ES query DSL |
| `GET` | `/_elastic/{index}/_search?q=...` | Query string search |
| `POST` | `/_elastic/_msearch` | Multi-search |
| `GET` | `/_elastic/{index}/_count` | Count documents |

### Index Management

| Method | Endpoint | Description |
|--------|----------|-------------|
| `HEAD` | `/_elastic/{index}` | Check if index exists |
| `GET` | `/_elastic/{index}/_mapping` | Get index mapping |
| `POST` | `/_elastic/{index}/_bulk` | Bulk index/delete operations |

## Response Format

Responses follow Elasticsearch 7+ conventions with `_index`, `_id`, `_version`, and `_source` fields.

### Get Document

```bash
curl http://localhost:3080/_elastic/articles/_doc/doc-1
```

```json
{
  "_index": "articles",
  "_id": "doc-1",
  "_version": 1,
  "found": true,
  "_source": {
    "title": "Hello World",
    "content": "Document content..."
  }
}
```

### Index Document

```bash
curl -X PUT http://localhost:3080/_elastic/articles/_doc/doc-1 \
  -H "Content-Type: application/json" \
  -d '{"title": "Hello", "content": "World"}'
```

```json
{
  "_index": "articles",
  "_id": "doc-1",
  "_version": 1,
  "result": "created",
  "_shards": {
    "total": 1,
    "successful": 1,
    "failed": 0
  }
}
```

### Delete Document

```bash
curl -X DELETE http://localhost:3080/_elastic/articles/_doc/doc-1
```

```json
{
  "_index": "articles",
  "_id": "doc-1",
  "_version": 1,
  "result": "deleted",
  "_shards": { "total": 1, "successful": 1, "failed": 0 }
}
```

### Query String Search

```bash
curl "http://localhost:3080/_elastic/articles/_search?q=hello&size=5"
```

### Count

```bash
curl http://localhost:3080/_elastic/articles/_count
```

```json
{
  "count": 42,
  "_shards": { "total": 1, "successful": 1, "failed": 0 }
}
```

### Bulk Operations

```bash
curl -X POST http://localhost:3080/_elastic/articles/_bulk \
  -H "Content-Type: application/x-ndjson" \
  -d '
{"index": {"_id": "1"}}
{"title": "First", "content": "..."}
{"index": {"_id": "2"}}
{"title": "Second", "content": "..."}
{"delete": {"_id": "3"}}
'
```

## Supported Query DSL

The ES compatibility layer translates a subset of the Elasticsearch query DSL:

- `match` — Full-text search on a field. Multi-word values are analyzed into
  terms combined with OR by default; `{"match": {"f": {"query": "a b",
  "operator": "and"}}}` requires every term.
- `match_phrase` — Exact adjacent-phrase match
- `match_all` — Match all documents
- `term` / `terms` — Exact value matching
- `multi_match` — Search one query across several fields (best_fields: a match
  in any field matches)
- `bool` — Compound queries. `must`/`filter` are required, `must_not` excludes,
  and `should` is optional when a `must`/`filter` is present (otherwise at least
  one `should` must match, per ES's default `minimum_should_match`). A bool with
  only `must_not` correctly returns all documents except the excluded ones.
- `query_string` / `simple_query_string` — Lucene-style query strings
- `range` — Numeric and date range queries
- `wildcard` / `prefix` — Pattern matching
- `ids` — Look up documents by `_id`

> **Note:** `bool` occur semantics (`+`/`-`) are translated directly to the
> underlying engine, so `must` + `must_not` combinations (the standard Kibana
> filter-with-exclusion) behave as in Elasticsearch.

## Client Libraries

Every `/_elastic/*` response carries the `X-Elastic-Product: Elasticsearch`
header, which the official clients (elasticsearch-py/js/java ≥ 7.14) require
before they will talk to the server.

### Python (elasticsearch-py)

```python
from elasticsearch import Elasticsearch

es = Elasticsearch("http://localhost:3080/_elastic")
es.index(index="articles", id="1", document={"title": "Hello"})
result = es.search(index="articles", query={"match": {"title": "hello"}})
```

### Elixir (PrismEx)

```elixir
# PrismEx uses the ES-compat layer for document operations
PrismEx.index("articles", %{id: "1", title: "Hello"})
PrismEx.search("articles", %{query: %{match: %{title: "hello"}}})
```

## Limitations

- `_version` is always `1` (Prism does not track document versions)
- Scroll API is not supported (use `from`/`size` pagination)
- Only a subset of ES query DSL is translated
- Index creation must be done via Prism's native API or schema files
- Aggregations in ES format are partially supported
- `exists` queries are not yet supported and return `400`
  (`parsing_exception`) rather than silently mismatching
- On hybrid (text + vector) collections, ES `_search` returns text-search hits;
  aggregations are computed over the text backend. Pass a query vector via the
  native `/collections/{name}/search` endpoint for RRF/weighted hybrid ranking.
- `sort` sorts over the first `index.max_result_window` (10,000) matching
  documents — exact when the match count is within that window, matching ES's
  own deep-sort limit. Sorting on a field reads its stored value; missing values
  sort last. `_source` filtering is accepted but not yet applied.

## See Also

- [API Reference](../reference/api-reference.md) — Native Prism REST API
- [Search](search.md) — Prism query syntax
