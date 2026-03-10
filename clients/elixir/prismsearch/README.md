# Prismsearch

Elixir client for [Prism](https://github.com/mikalv/prism) — a high-performance hybrid search engine combining full-text search (Tantivy/BM25) and vector search (HNSW) for AI/RAG applications.

## Installation

Add `prismsearch` to your dependencies in `mix.exs`:

```elixir
def deps do
  [
    {:prismsearch, "~> 0.1.0"}
  ]
end
```

## Quick Start

```elixir
# Connect to Prism
client = Prismsearch.client(base_url: "http://localhost:3080")

# Check health
{:ok, health} = Prismsearch.health(client)

# Index documents
docs = [
  %{"id" => "1", "fields" => %{"title" => "Elixir in Action", "content" => "..."}},
  %{"id" => "2", "fields" => %{"title" => "Programming Phoenix", "content" => "..."}}
]
{:ok, _} = Prismsearch.index(client, "books", docs)

# Search
query = Prismsearch.Query.new("books", "elixir concurrency")
{:ok, results} = Prismsearch.search(client, query)
```

## Features

- **Search** — Full-text, vector, and hybrid search with highlighting
- **Query builder** — Fluent API for constructing search queries
- **Collections** — Create, delete, list, and inspect collections
- **Documents** — Index, retrieve, and manage documents
- **Aggregations** — Metrics, histograms, terms, and nested aggregations
- **Suggestions** — Prefix completion and fuzzy autocomplete
- **More Like This** — Find similar documents
- **Multi-search** — Search across multiple collections
- **Graph** — Node/edge CRUD, BFS traversal, shortest path
- **ILM** — Index lifecycle management policies

## Authentication

```elixir
client = Prismsearch.client(
  base_url: "http://localhost:3080",
  api_key: "sk-your-api-key"
)
```

## Query Builder

```elixir
alias Prismsearch.Query

query =
  Query.new("products", "wireless headphones")
  |> Query.limit(20)
  |> Query.offset(0)
  |> Query.fields(["title", "description"])
  |> Query.highlight(["title", "description"])
  |> Query.min_score(0.5)
  |> Query.weights(text: 0.7, vector: 0.3)

{:ok, results} = Prismsearch.search(client, query)
```

## Links

- [Prism Search Engine](https://github.com/mikalv/prism) — The search engine this client connects to
- [Documentation](https://mikalv.github.io/prism/) — Full Prism documentation
- [API Reference](https://mikalv.github.io/prism/reference/api-reference/) — REST API endpoints

## License

MIT
