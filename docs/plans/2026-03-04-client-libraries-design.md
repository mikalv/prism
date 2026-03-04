# Prism Client Libraries Design

**Date:** 2026-03-04
**Status:** Approved
**Scope:** Elixir, Python (+ Django), Rust clients for Prism search engine

---

## Overview

Three client libraries for the Prism HTTP API, developed in the monorepo under `clients/`. An OpenAPI 3.1 spec serves as the source of truth for types and endpoints. Each client is hand-written for idiomatic ergonomics with query builders and typed models.

**Naming:** All clients use the name `prismsearch` (Hex, PyPI, crates.io).

**v1 scope:** Core CRUD (collections, documents, search) + advanced features (aggregations, MLT, suggestions, multi-search, segments/optimize, ILM, graph). Admin endpoints (debug, export, templates) are excluded from v1.

## Repository Structure

```
clients/
├── openapi/
│   └── prism-openapi.yaml          # Source of truth
├── elixir/
│   └── prismsearch/                # Mix project (:prismsearch)
├── python/
│   ├── prismsearch/                # Core client (PyPI: prismsearch)
│   └── prismsearch-django/         # Django integration (PyPI: prismsearch-django)
└── rust/
    └── prismsearch/                # Cargo crate (crates.io: prismsearch)
```

## OpenAPI Spec

Hand-written YAML covering all v1 endpoints. Used as reference for type names and field types. Clients implement types idiomatically per language rather than using codegen.

### Core Models

| Model | Fields |
|-------|--------|
| `Document` | id, fields: map<string, any> |
| `SearchRequest` | query?, vector?, fields[], limit, offset, highlight?, rerank?, min_score?, score_function? |
| `SearchResults` | results: SearchResult[], total |
| `SearchResult` | id, score, fields: map<string, any>, highlights? |
| `CollectionSchema` | collection, description?, backends, embedding_generation? |
| `SegmentsInfo` | segments[], total_docs, total_deleted, delete_ratio |
| `AggregateRequest` | query?, aggregations[], scan_limit |
| `SuggestRequest` | prefix, field, size, fuzzy, max_distance |
| `MltRequest` | like?, like_text?, fields[], min_term_freq, min_doc_freq, max_query_terms, size |
| `MultiSearchRequest` | collections[], query?, vector?, fields[], limit |
| `GraphNode` | id, label, properties |
| `GraphEdge` | from, to, edge_type, weight?, properties? |

---

## Elixir Client (`:prismsearch`)

**HTTP client:** Req (retries, connection pooling, JSON built-in)
**Pattern:** Pipe-friendly query builder, `{:ok, result}` / `{:error, reason}` returns

### Usage

```elixir
client = Prismsearch.client(base_url: "http://localhost:3080", api_key: "optional")

# Collection CRUD
Prismsearch.create_collection(client, "products", schema)
Prismsearch.list_collections(client)
Prismsearch.delete_collection(client, "products")

# Indexing
Prismsearch.index(client, "products", [
  %{id: "1", fields: %{title: "Widget", price: 29.99}}
])

# Search with query builder
import Prismsearch.Query

"products"
|> Query.new("wireless headphones")
|> Query.fields(["title", "description"])
|> Query.limit(20)
|> Query.highlight(fields: ["title"], pre_tag: "<b>", post_tag: "</b>")
|> Query.min_score(0.5)
|> Prismsearch.search(client)

# Aggregations
"products"
|> Query.new()
|> Query.aggregate(:price_stats, type: :stats, field: "price")
|> Prismsearch.aggregate(client)

# MLT, Suggest, Multi-search
Prismsearch.more_like_this(client, "products", like: %{id: "1"}, fields: ["title"])
Prismsearch.suggest(client, "products", prefix: "wire", field: "title", fuzzy: true)
Prismsearch.multi_search(client, ["products", "articles"], query: "headphones")

# Graph
Prismsearch.Graph.add_node(client, "knowledge", %{id: "n1", label: "Concept"})
Prismsearch.Graph.bfs(client, "knowledge", start: "n1", edge_type: "relates_to")

# Health & Segments
Prismsearch.health(client)
Prismsearch.segments(client, "products")
Prismsearch.optimize(client, "products", max_segments: 1)
```

### Module Structure

```
lib/
├── prismsearch.ex              # Main API facade
├── prismsearch/
│   ├── client.ex               # Req HTTP client, config, auth
│   ├── query.ex                # Query builder (pipe-friendly)
│   ├── models/
│   │   ├── document.ex
│   │   ├── collection.ex
│   │   ├── aggregation.ex
│   │   └── graph.ex
│   ├── collections.ex
│   ├── search.ex
│   ├── suggest.ex
│   ├── graph.ex
│   └── ilm.ex
```

---

## Python Client (`prismsearch`)

**HTTP client:** httpx (async + sync)
**Models:** Pydantic v2
**Pattern:** Method chaining query builder, both sync and async

### Usage

```python
from prismsearch import Prismsearch, Query

client = Prismsearch("http://localhost:3080", api_key="optional")

# Collection CRUD
client.collections.create("products", schema={...})
client.collections.list()
client.collections.delete("products")

# Indexing
client.index("products", [
    {"id": "1", "fields": {"title": "Widget", "price": 29.99}},
])

# Search
results = (
    Query("products", "wireless headphones")
    .fields(["title", "description"])
    .limit(20)
    .highlight(fields=["title"], pre_tag="<b>", post_tag="</b>")
    .min_score(0.5)
    .execute(client)
)

# Async
async with Prismsearch("http://localhost:3080", async_mode=True) as client:
    results = await Query("products", "search term").execute(client)

# Aggregations
agg_results = (
    Query("products")
    .aggregate("price_stats", type="stats", field="price")
    .execute_aggs(client)
)

# Suggest, MLT, Multi-search
client.suggest("products", prefix="wire", field="title", fuzzy=True)
client.mlt("products", like={"_id": "1"}, fields=["title"])
client.multi_search(["products", "articles"], query="headphones")

# Graph
client.graph.add_node("knowledge", {"id": "n1", "label": "Concept"})
client.graph.bfs("knowledge", start="n1", edge_type="relates_to")
```

### Package Structure

```
prismsearch/
├── __init__.py
├── client.py
├── query.py
├── models.py
├── collections.py
├── search.py
├── suggest.py
├── graph.py
├── ilm.py
└── async_client.py
```

### Django Integration (`prismsearch-django`)

```python
# settings.py
PRISMSEARCH = {
    "URL": "http://localhost:3080",
    "API_KEY": "optional",
    "DEFAULT_COLLECTION": "products",
}

# models.py
from prismsearch.django import SearchableModel, SearchField

class Product(SearchableModel):
    class PrismMeta:
        collection = "products"
        fields = [
            SearchField("title", indexed=True, stored=True, boost=2.0),
            SearchField("description", indexed=True, stored=True),
            SearchField("price", field_type="f64", indexed=True),
        ]

    title = models.CharField(max_length=200)
    description = models.TextField()
    price = models.DecimalField(...)

# Auto-sync via post_save/post_delete signals
# Management command: python manage.py prismsearch_reindex
# Search: Product.prism.search("headphones", limit=20)
```

```
prismsearch-django/
├── __init__.py
├── conf.py
├── mixins.py
├── signals.py
└── management/commands/
    └── prismsearch_reindex.py
```

---

## Rust Client (`prismsearch`)

**HTTP client:** reqwest
**Pattern:** Owned builder pattern, async-first
**Dependencies:** reqwest, serde, serde_json, thiserror

### Usage

```rust
use prismsearch::{Client, Query, Document, Highlight, Aggregation};

let client = Client::new("http://localhost:3080")
    .api_key("optional")
    .timeout(Duration::from_secs(30))
    .build()?;

// Collection CRUD
client.collections().create("products", &schema).await?;
client.collections().list().await?;
client.collections().delete("products").await?;

// Indexing
client.index("products", &[
    Document::new("1").field("title", "Widget").field("price", 29.99),
]).await?;

// Search
let results = Query::new("products", "wireless headphones")
    .fields(&["title", "description"])
    .limit(20)
    .highlight(Highlight::new(&["title"]).pre_tag("<b>").post_tag("</b>"))
    .min_score(0.5)
    .execute(&client)
    .await?;

// Aggregations
let aggs = Query::new("products", "*")
    .aggregate("price_stats", Aggregation::stats("price"))
    .execute_aggs(&client)
    .await?;

// Suggest, MLT
client.suggest("products", "wire", "title").fuzzy(true).send().await?;
client.mlt("products").like_id("1").fields(&["title"]).send().await?;

// Graph
client.graph("knowledge").add_node(&node).await?;
client.graph("knowledge").bfs("n1", "relates_to").max_depth(3).send().await?;
```

### Crate Structure

```
src/
├── lib.rs
├── client.rs
├── query.rs
├── models.rs
├── collections.rs
├── search.rs
├── suggest.rs
├── graph.rs
├── ilm.rs
└── error.rs
```

Independent of the server crate — defines its own lightweight models. Can be used without pulling in Tantivy or any server dependencies.

---

## Implementation Priority

1. **Foundation:** OpenAPI spec (`clients/openapi/prism-openapi.yaml`)
2. **Elixir:** `:prismsearch` — Req, query builder, ExUnit tests, Hex publish
3. **Python:** `prismsearch` — httpx, Pydantic v2, pytest, then `prismsearch-django`
4. **Rust:** `prismsearch` crate — reqwest, builder pattern, integration tests

## Testing Strategy

All clients: unit tests with mocked HTTP responses + optional integration tests against a running Prism instance (controlled via env var like `PRISM_TEST_URL`).
