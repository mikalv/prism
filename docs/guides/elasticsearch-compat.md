# Elasticsearch Compatibility

Prism includes an Elasticsearch-compatible REST API layer, allowing existing ES clients (elasticsearch-py, elasticsearch-js, PrismEx, etc.) to connect with minimal changes.

## Enabling

Build with the `es-compat` feature flag:

```bash
cargo build --release -p prismsearch-server --features es-compat
```

When enabled, ES-compatible endpoints are mounted at their standard Elasticsearch paths on the root router (no `/_elastic` prefix — Kibana and ES clients connect directly).

## Supported Endpoints

### Cluster

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/` | Cluster info (version, name) |
| `GET` | `/_cat/indices` | List all indices |
| `GET` | `/_cluster/health` | Cluster health |

### Document CRUD

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/{index}/_doc/{id}` | Get document by ID |
| `HEAD` | `/{index}/_doc/{id}` | Check if document exists |
| `POST` | `/{index}/_doc` | Index document (auto-ID) |
| `PUT` | `/{index}/_doc/{id}` | Index document (explicit ID) |
| `DELETE` | `/{index}/_doc/{id}` | Delete document |

### Search

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/{index}/_search` | Search with ES query DSL |
| `GET` | `/{index}/_search?q=...` | Query string search |
| `POST` | `/_msearch` | Multi-search |
| `GET` | `/{index}/_count` | Count documents |

### Index Management

| Method | Endpoint | Description |
|--------|----------|-------------|
| `HEAD` | `/{index}` | Check if index exists |
| `GET` | `/{index}/_mapping` | Get index mapping |
| `POST` | `/{index}/_bulk` | Bulk index/delete operations |

## Response Format

Responses follow Elasticsearch 7+ conventions with `_index`, `_id`, `_version`, and `_source` fields.

### Get Document

```bash
curl http://localhost:3080/articles/_doc/doc-1
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
curl -X PUT http://localhost:3080/articles/_doc/doc-1 \
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
curl -X DELETE http://localhost:3080/articles/_doc/doc-1
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
curl "http://localhost:3080/articles/_search?q=hello&size=5"
```

### Count

```bash
curl http://localhost:3080/articles/_count
```

```json
{
  "count": 42,
  "_shards": { "total": 1, "successful": 1, "failed": 0 }
}
```

### Bulk Operations

```bash
curl -X POST http://localhost:3080/articles/_bulk \
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

See the [Feature matrix](#feature-matrix) below for the full list. The layer
translates a subset of the ES Query DSL to Prism's query string and structured
filters. Two behaviors are worth calling out explicitly:

- **`bool` occur semantics.** `must`/`filter` are required, `must_not` excludes,
  and `should` is optional when a `must`/`filter` is present (otherwise at least
  one `should` must match, per ES's default `minimum_should_match`). A bool with
  only `must_not` correctly returns all documents except the excluded ones. This
  means `must` + `must_not` combinations (the standard Kibana
  filter-with-exclusion) behave as in Elasticsearch.
- **`exists` requires a fast field.** Standalone, or inside a `bool`'s
  `must`/`filter` (required) and `must_not` (forbidden). Collections created in
  this version mark numeric, date, bool, and string/keyword fields fast. Existing
  collections must be recreated to enable `exists` on their fields (otherwise a
  clear error is returned). `exists` inside a `bool`'s `should` is not yet
  supported.


## Client Libraries

Every `/*` response carries the `X-Elastic-Product: Elasticsearch`
header, which the official clients (elasticsearch-py/js/java ≥ 7.14) require
before they will talk to the server.

### Python (elasticsearch-py)

```python
from elasticsearch import Elasticsearch

es = Elasticsearch("http://localhost:3080")
es.index(index="articles", id="1", document={"title": "Hello"})
result = es.search(index="articles", query={"match": {"title": "hello"}})
```

### Elixir (PrismEx)

```elixir
# PrismEx uses the ES-compat layer for document operations
PrismEx.index("articles", %{id: "1", title: "Hello"})
PrismEx.search("articles", %{query: %{match: %{title: "hello"}}})
```

## Feature matrix

A quick reference for what the ES compatibility layer supports, what
is partially supported, and what is not supported. Verified against
`prism-es-compat/src/router.rs`, `query/types.rs`, and `query/translator.rs`.

### REST endpoints

| ES endpoint | Prism ES-compat | Notes |
|---|:---:|---|
| `GET /` (cluster info) | ✅ | Version, name |
| `GET /_cluster/health` | ✅ | |
| `GET /_cat/indices` | ✅ | |
| `GET|POST /{index}/_search` | ✅ | ES query DSL or `?q=` |
| `POST /_search` (all indices) | ✅ | |
| `POST /_msearch` | ✅ | Multi-search |
| `GET /{index}/_count` | ✅ | |
| `HEAD /{index}` | ✅ | Index exists |
| `HEAD /{index}/_doc/{id}` | ✅ | Document exists |
| `GET /{index}/_doc/{id}` | ✅ | |
| `POST /{index}/_doc` | ✅ | Auto-ID |
| `PUT /{index}/_doc/{id}` | ✅ | Explicit ID |
| `DELETE /{index}/_doc/{id}` | ✅ | |
| `POST /{index}/_bulk` | ✅ | NDJSON, `index`/`delete` actions |
| `POST /_bulk` | ✅ | Default-index form |
| `GET /{index}/_mapping` | ✅ | Read-only |
| `PUT /{index}` (create index) | ❌ | Use Prism native API or schema files |
| `DELETE /{index}` | ❌ | Use Prism native API |
| `PUT /{index}/_mapping` | ❌ | Schema set at creation only |
| `POST /_search/scroll` (scroll API) | ❌ | Use `from`/`size` |
| `POST /_refresh`, `_flush` | ❌ | No-op (Prism is near-real-time by design) |

### Query DSL

| Query type | Support | Notes |
|---|:---:|---|
| `match` | ✅ | `operator` (`and`/`or`), `fuzziness`, `boost` parsed |
| `match_phrase` | ✅ | `slop`, `boost` |
| `match_all` | ✅ | `boost` |
| `multi_match` | ✅ | `best_fields` style; `operator` |
| `term` | ✅ | Not analyzed |
| `terms` | ✅ | Multiple exact values |
| `range` | ✅ | `gte`/`gt`/`lte`/`lt`, `format`, `time_zone`, `boost` |
| `bool` | ✅ | `must`/`should`/`must_not`/`filter`, `minimum_should_match` |
| `exists` | ⚠️ | Requires the field to be a fast field; not inside `should` |
| `query_string` | ✅ | Lucene syntax, `default_field`, `analyze_wildcard` |
| `simple_query_string` | ✅ | |
| `wildcard` | ✅ | `case_insensitive`, `boost` |
| `prefix` | ✅ | `boost` |
| `ids` | ✅ | |
| `nested` | ❌ | Fields are stored flat |
| `constant_score` | ❌ | |
| `dis_max` | ❌ | |
| `fuzzy` | ⚠️ | Accepted on `match`; no standalone `fuzzy` query |
| `regexp` | ❌ | |
| `more_like_this` | ❌ | Use native `/collections/{c}/_mlt` |
| `geo_*` | ❌ | No geo type |
| `script` | ❌ | No scripting engine |

### Aggregations

| Aggregation | Support | Notes |
|---|:---:|---|
| `avg` | ✅ | `missing` |
| `sum` | ✅ | `missing` |
| `min` | ✅ | `missing` |
| `max` | ✅ | `missing` |
| `stats` | ✅ | `missing` |
| `value_count` | ✅ | |
| `cardinality` | ✅ | |
| `percentiles` | ✅ | `percents` |
| `terms` | ✅ | `size`, `order`, `min_doc_count` |
| `histogram` | ✅ | `interval`, `min_doc_count`, `extended_bounds` |
| `date_histogram` | ✅ | `calendar_interval`/`fixed_interval`, `format`, `time_zone` |
| `range` | ✅ | named buckets |
| `date_range` | ✅ | `format` |
| `filter` | ✅ | single sub-query |
| `filters` | ✅ | named/anonymous buckets |
| `global` | ✅ | |
| nested sub-`aggs` | ✅ | bucket aggregations can nest |
| `composite` | ❌ | |
| `auto_date_histogram` | ❌ | |
| `significant_terms` | ❌ | |
| `geohash_grid` / `geo_*` | ❌ | No geo type |
| `top_hits` | ❌ | |

### Search request body fields

| Field | Support | Notes |
|---|:---:|---|
| `query` | ✅ | See Query DSL above |
| `from` / `size` | ✅ | Deep pagination only (no scroll) |
| `_source` | ✅ | bool / field list / `{includes, excludes}` with `*` globs |
| `aggs` (aggregations) | ✅ | See Aggregations above |
| `sort` | ✅ | Over the first 10k window; single fast-field sort is un-capped |
| `highlight` | ✅ | fields, pre/post tags, fragment size/count |
| `track_total_hits` | ✅ | Exact up to 10k; `gte` beyond |
| `stored_fields` | ❌ | Use `_source` |
| `script_fields` | ❌ | No scripting |
| `collapse` | ❌ | Use native `collapse` on `/collections/{c}/search` |
| `search_after` | ❌ | |
| `suggest` | ❌ | Use native `/collections/{c}/_suggest` |

### Bulk actions

| Action | Support | Notes |
|---|:---:|---|
| `index` | ✅ | with `_id` or auto-ID |
| `delete` | ✅ | |
| `create` | ✅ | Treated like `index` (does not error on existing `_id`) |
| `update` | ❌ | Prism has no partial update |

### Response fidelity

| ES behavior | Prism | Notes |
|---|:---:|---|
| `X-Elastic-Product` header | ✅ | Stamped on every response, including errors |
| `_index` / `_id` / `_source` | ✅ | ES 7+ shape |
| `_version` | ⚠️ | Always `1` (no version tracking) |
| `_shards` | ✅ | Echoed as `{total:1, successful:1, failed:0}` |
| `hits.total.relation` | ✅ | `eq` under 10k, `gte` beyond |
| `result: "created"\|"updated"\|"deleted"` | ✅ | |
| Result `_score` on hybrid collections | ⚠️ | Text-backend score; use native API for hybrid |

## Limitations & nuances

The matrix above is the quick reference. A few behaviors deserve prose:

- **Hybrid collections.** On text + vector collections, ES `_search` returns
  text-search hits and computes aggregations over the text backend. Pass a query
  vector via the native `/collections/{name}/search` endpoint for RRF/weighted
  hybrid ranking.
- **`sort` windowing.** Sorting considers the first 10 000 matching documents
  — exact when the match count is within that window, matching ES's own
  deep-sort limit. Missing values sort last. A single-key sort on a fast
  numeric/date/bool field (the common "last N by timestamp" case) is done at
  the collector level with no window cap and much lower cost.
- **`_source` wildcards.** `_source: false` omits the source; a field list
  (`_source: ["a","b"]`) or `{"includes":[...], "excludes":[...]}` selects
  fields. Include/exclude patterns support `*` globs (e.g. `"*"`, `"user.*"`,
  `"_*"`, `"*_id"`). Dotted names match flat field names literally, since Prism
  stores fields flat (no nested objects).
- **`exists` requires a fast field** (see Query DSL above). On collections
  predating fast-field support it returns a clear error until recreated.
- **`hits.total`** is exact up to 10 000 (`relation: "eq"`) and reports
  `{"value": 10000, "relation": "gte"}` beyond it. `track_total_hits: <n>`
  lowers the tracking limit (exact tracking beyond 10 000 is not yet supported).

## See Also

- [API Reference](../reference/api-reference.md) — Native Prism REST API
- [Search](search.md) — Prism query syntax
