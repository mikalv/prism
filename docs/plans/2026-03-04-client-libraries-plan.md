# Prism Client Libraries Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build idiomatic client libraries for Prism's HTTP API in Elixir, Python (+Django), and Rust.

**Architecture:** OpenAPI 3.1 spec as source of truth for types. Each client hand-writes HTTP logic and query builders idiomatically. Monorepo under `clients/`. Unit tests with mocked HTTP, optional integration tests via `PRISM_TEST_URL` env var.

**Tech Stack:** Elixir/Req, Python/httpx+Pydantic v2, Rust/reqwest+serde. All clients target the same v1 API surface: CRUD, search, aggregations, suggest, MLT, multi-search, graph, ILM, segments/optimize.

**Design doc:** `docs/plans/2026-03-04-client-libraries-design.md`

---

## Phase 1: OpenAPI Spec

### Task 1: Create OpenAPI 3.1 Spec

**Files:**
- Create: `clients/openapi/prism-openapi.yaml`

**Step 1: Create directory and write spec**

Write the full OpenAPI 3.1 spec covering all v1 endpoints. Reference `docs/reference/api-reference.md` and `prism/src/api/routes.rs` for exact request/response shapes.

The spec must cover these endpoint groups:
- `GET /health` — HealthResponse
- `POST /collections/{collection}/search` — SearchRequest/SearchResults
- `POST /collections/{collection}/documents` — IndexRequest/IndexResponse
- `GET /collections/{collection}/documents/{id}` — Document
- `PUT /collections/{name}` — CreateCollectionResponse
- `DELETE /collections/{name}` — JSON status
- `GET /admin/collections` — CollectionsList
- `GET /collections/{collection}/schema` — CollectionSchemaResponse
- `GET /collections/{collection}/stats` — CollectionStatsResponse
- `POST /collections/{collection}/aggregate` — AggregateRequest/AggregateResponse
- `POST /collections/{collection}/_suggest` — SuggestRequest/SuggestResponse
- `POST /collections/{collection}/_mlt` — MltRequest/SearchResults
- `POST /_msearch` — MultiSearchRequest/MultiSearchResults
- `GET /collections/{collection}/segments` — SegmentsInfo
- `POST /collections/{collection}/optimize` — OptimizeResult
- `GET /collections/{collection}/doc/{id}/reconstruct` — ReconstructedDocument
- `GET /collections/{collection}/terms/{field}` — TopTermsResponse
- Graph endpoints (nodes CRUD, edges, bfs, shortest-path, stats)
- ILM endpoints (policies CRUD, status, explain, rollover, move, attach, aliases)

All models must have complete field definitions with types, defaults, and required markers matching the Rust structs in `routes.rs`.

**Step 2: Validate spec**

Run: `npx @redocly/cli lint clients/openapi/prism-openapi.yaml`

If `npx` is not available, validate manually by checking the YAML parses correctly.

**Step 3: Commit**

```bash
git add clients/openapi/prism-openapi.yaml
git commit -m "feat: add OpenAPI 3.1 spec for Prism HTTP API"
```

---

## Phase 2: Elixir Client

### Task 2: Scaffold Mix Project

**Files:**
- Create: `clients/elixir/prismsearch/mix.exs`
- Create: `clients/elixir/prismsearch/.formatter.exs`
- Create: `clients/elixir/prismsearch/.gitignore`
- Create: `clients/elixir/prismsearch/lib/prismsearch.ex`
- Create: `clients/elixir/prismsearch/test/test_helper.exs`

**Step 1: Create mix project**

```bash
cd clients/elixir
mix new prismsearch
```

**Step 2: Configure mix.exs with dependencies**

Edit `clients/elixir/prismsearch/mix.exs`:

```elixir
defmodule Prismsearch.MixProject do
  use Mix.Project

  @version "0.1.0"
  @source_url "https://github.com/mikalv/prism"

  def project do
    [
      app: :prismsearch,
      version: @version,
      elixir: "~> 1.15",
      start_permanent: Mix.env() == :prod,
      deps: deps(),
      package: package(),
      description: "Elixir client for Prism search engine",
      source_url: @source_url,
      docs: docs()
    ]
  end

  def application do
    [extra_applications: [:logger]]
  end

  defp deps do
    [
      {:req, "~> 0.5"},
      {:jason, "~> 1.4"},
      {:ex_doc, "~> 0.34", only: :dev, runtime: false}
    ]
  end

  defp package do
    [
      licenses: ["MIT"],
      links: %{"GitHub" => @source_url}
    ]
  end

  defp docs do
    [main: "Prismsearch", source_ref: "v#{@version}"]
  end
end
```

**Step 3: Fetch dependencies**

```bash
cd clients/elixir/prismsearch
mix deps.get
```

**Step 4: Commit**

```bash
git add clients/elixir/prismsearch/
git commit -m "feat(elixir): scaffold prismsearch mix project"
```

---

### Task 3: Elixir Client & Models

**Files:**
- Create: `clients/elixir/prismsearch/lib/prismsearch/client.ex`
- Create: `clients/elixir/prismsearch/lib/prismsearch/error.ex`
- Create: `clients/elixir/prismsearch/lib/prismsearch/models/document.ex`
- Create: `clients/elixir/prismsearch/lib/prismsearch/models/search.ex`
- Create: `clients/elixir/prismsearch/lib/prismsearch/models/collection.ex`
- Create: `clients/elixir/prismsearch/test/prismsearch/client_test.exs`

**Step 1: Write client test**

```elixir
# test/prismsearch/client_test.exs
defmodule Prismsearch.ClientTest do
  use ExUnit.Case, async: true

  test "creates client with base_url" do
    client = Prismsearch.Client.new(base_url: "http://localhost:3080")
    assert client.base_url == "http://localhost:3080"
    assert client.api_key == nil
  end

  test "creates client with api_key" do
    client = Prismsearch.Client.new(
      base_url: "http://localhost:3080",
      api_key: "test-key"
    )
    assert client.api_key == "test-key"
  end

  test "default base_url" do
    client = Prismsearch.Client.new()
    assert client.base_url == "http://localhost:3080"
  end
end
```

**Step 2: Run test to verify it fails**

```bash
cd clients/elixir/prismsearch
mix test test/prismsearch/client_test.exs
```

Expected: Compilation error — `Prismsearch.Client` not defined.

**Step 3: Implement Client**

```elixir
# lib/prismsearch/client.ex
defmodule Prismsearch.Client do
  @moduledoc "HTTP client for Prism search engine."

  defstruct [:base_url, :api_key, :req]

  @default_base_url "http://localhost:3080"

  @type t :: %__MODULE__{
    base_url: String.t(),
    api_key: String.t() | nil,
    req: Req.Request.t()
  }

  @doc "Create a new Prism client."
  def new(opts \\ []) do
    base_url = Keyword.get(opts, :base_url, @default_base_url)
    api_key = Keyword.get(opts, :api_key)
    timeout = Keyword.get(opts, :timeout, 30_000)

    headers = if api_key, do: [{"authorization", "Bearer #{api_key}"}], else: []

    req =
      Req.new(
        base_url: base_url,
        headers: headers,
        receive_timeout: timeout
      )

    %__MODULE__{base_url: base_url, api_key: api_key, req: req}
  end

  @doc false
  def get(%__MODULE__{req: req}, path, opts \\ []) do
    params = Keyword.get(opts, :params, [])

    case Req.get(req, url: path, params: params) do
      {:ok, %Req.Response{status: status, body: body}} when status in 200..299 ->
        {:ok, body}
      {:ok, %Req.Response{status: status, body: body}} ->
        {:error, %Prismsearch.Error{status: status, message: error_message(body)}}
      {:error, reason} ->
        {:error, %Prismsearch.Error{status: nil, message: inspect(reason)}}
    end
  end

  @doc false
  def post(%__MODULE__{req: req}, path, body) do
    case Req.post(req, url: path, json: body) do
      {:ok, %Req.Response{status: status, body: body}} when status in 200..299 ->
        {:ok, body}
      {:ok, %Req.Response{status: 201, body: body}} ->
        {:ok, body}
      {:ok, %Req.Response{status: status, body: body}} ->
        {:error, %Prismsearch.Error{status: status, message: error_message(body)}}
      {:error, reason} ->
        {:error, %Prismsearch.Error{status: nil, message: inspect(reason)}}
    end
  end

  @doc false
  def put(%__MODULE__{req: req}, path, body) do
    case Req.put(req, url: path, json: body) do
      {:ok, %Req.Response{status: status, body: body}} when status in 200..299 ->
        {:ok, body}
      {:ok, %Req.Response{status: status, body: body}} ->
        {:error, %Prismsearch.Error{status: status, message: error_message(body)}}
      {:error, reason} ->
        {:error, %Prismsearch.Error{status: nil, message: inspect(reason)}}
    end
  end

  @doc false
  def delete(%__MODULE__{req: req}, path) do
    case Req.delete(req, url: path) do
      {:ok, %Req.Response{status: status, body: body}} when status in 200..299 ->
        {:ok, body}
      {:ok, %Req.Response{status: status, body: body}} ->
        {:error, %Prismsearch.Error{status: status, message: error_message(body)}}
      {:error, reason} ->
        {:error, %Prismsearch.Error{status: nil, message: inspect(reason)}}
    end
  end

  defp error_message(body) when is_map(body), do: Map.get(body, "error", inspect(body))
  defp error_message(body) when is_binary(body), do: body
  defp error_message(body), do: inspect(body)
end
```

```elixir
# lib/prismsearch/error.ex
defmodule Prismsearch.Error do
  @moduledoc "Error returned from Prism API."
  defexception [:status, :message]

  @type t :: %__MODULE__{
    status: integer() | nil,
    message: String.t()
  }

  @impl true
  def message(%__MODULE__{status: nil, message: msg}), do: "Prism error: #{msg}"
  def message(%__MODULE__{status: status, message: msg}), do: "Prism error (#{status}): #{msg}"
end
```

```elixir
# lib/prismsearch/models/document.ex
defmodule Prismsearch.Document do
  @moduledoc "A Prism document."
  defstruct [:id, fields: %{}]

  @type t :: %__MODULE__{
    id: String.t(),
    fields: map()
  }
end
```

```elixir
# lib/prismsearch/models/search.ex
defmodule Prismsearch.SearchResult do
  @moduledoc "A single search result."
  defstruct [:id, :score, fields: %{}, highlight: nil]

  @type t :: %__MODULE__{
    id: String.t(),
    score: float(),
    fields: map(),
    highlight: map() | nil
  }
end

defmodule Prismsearch.SearchResults do
  @moduledoc "Search results container."
  defstruct [results: [], total: 0]

  @type t :: %__MODULE__{
    results: [Prismsearch.SearchResult.t()],
    total: non_neg_integer()
  }

  def from_map(map) when is_map(map) do
    results =
      (map["results"] || [])
      |> Enum.map(fn r ->
        %Prismsearch.SearchResult{
          id: r["id"],
          score: r["score"],
          fields: r["fields"] || %{},
          highlight: r["highlight"]
        }
      end)

    %__MODULE__{results: results, total: map["total"] || 0}
  end
end
```

```elixir
# lib/prismsearch/models/collection.ex
defmodule Prismsearch.Collection do
  @moduledoc "Collection metadata."
  defstruct [:name, :description, :document_count, :storage_bytes, :schema]
end
```

**Step 4: Run tests**

```bash
cd clients/elixir/prismsearch
mix test test/prismsearch/client_test.exs
```

Expected: 3 tests, 0 failures.

**Step 5: Commit**

```bash
git add clients/elixir/prismsearch/
git commit -m "feat(elixir): add Client, Error, and core models"
```

---

### Task 4: Elixir Query Builder

**Files:**
- Create: `clients/elixir/prismsearch/lib/prismsearch/query.ex`
- Create: `clients/elixir/prismsearch/test/prismsearch/query_test.exs`

**Step 1: Write query builder test**

```elixir
# test/prismsearch/query_test.exs
defmodule Prismsearch.QueryTest do
  use ExUnit.Case, async: true

  alias Prismsearch.Query

  test "builds basic query" do
    q = Query.new("products", "headphones")
    assert q.collection == "products"
    assert q.query == "headphones"
    assert q.limit == 10
  end

  test "pipe-based building" do
    q =
      "products"
      |> Query.new("test query")
      |> Query.fields(["title", "content"])
      |> Query.limit(20)
      |> Query.offset(5)
      |> Query.min_score(0.5)

    assert q.fields == ["title", "content"]
    assert q.limit == 20
    assert q.offset == 5
    assert q.min_score == 0.5
  end

  test "highlight configuration" do
    q =
      "products"
      |> Query.new("test")
      |> Query.highlight(fields: ["title"], pre_tag: "<b>", post_tag: "</b>")

    assert q.highlight == %{
      "fields" => ["title"],
      "pre_tag" => "<b>",
      "post_tag" => "</b>"
    }
  end

  test "to_request_body/1 produces correct JSON-ready map" do
    q =
      "products"
      |> Query.new("wireless")
      |> Query.fields(["title"])
      |> Query.limit(5)

    body = Query.to_request_body(q)
    assert body["query"] == "wireless"
    assert body["fields"] == ["title"]
    assert body["limit"] == 5
    refute Map.has_key?(body, "collection")
  end

  test "query without search term" do
    q = Query.new("products")
    assert q.query == nil
    body = Query.to_request_body(q)
    refute Map.has_key?(body, "query")
  end

  test "vector query" do
    q =
      "products"
      |> Query.new()
      |> Query.vector([0.1, 0.2, 0.3])

    body = Query.to_request_body(q)
    assert body["vector"] == [0.1, 0.2, 0.3]
  end

  test "aggregation builder" do
    q =
      "products"
      |> Query.new()
      |> Query.aggregate("price_stats", type: "stats", field: "price")
      |> Query.aggregate("by_cat", type: "terms", field: "category", size: 10)

    assert length(q.aggregations) == 2
  end
end
```

**Step 2: Run test — expect failure**

```bash
cd clients/elixir/prismsearch && mix test test/prismsearch/query_test.exs
```

**Step 3: Implement Query builder**

```elixir
# lib/prismsearch/query.ex
defmodule Prismsearch.Query do
  @moduledoc """
  Pipe-friendly query builder for Prism searches.

  ## Example

      "products"
      |> Query.new("wireless headphones")
      |> Query.fields(["title", "description"])
      |> Query.limit(20)
      |> Query.highlight(fields: ["title"])
      |> Prismsearch.search(client)
  """

  defstruct [
    :collection,
    :query,
    :vector,
    :merge_strategy,
    :text_weight,
    :vector_weight,
    :highlight,
    :rerank,
    :min_score,
    :score_function,
    :rrf_k,
    fields: [],
    limit: 10,
    offset: 0,
    aggregations: []
  ]

  @type t :: %__MODULE__{}

  @doc "Create a new query for a collection."
  def new(collection, query \\ nil) do
    %__MODULE__{collection: collection, query: query}
  end

  def fields(%__MODULE__{} = q, fields) when is_list(fields), do: %{q | fields: fields}
  def limit(%__MODULE__{} = q, n) when is_integer(n), do: %{q | limit: n}
  def offset(%__MODULE__{} = q, n) when is_integer(n), do: %{q | offset: n}
  def min_score(%__MODULE__{} = q, s) when is_number(s), do: %{q | min_score: s}
  def score_function(%__MODULE__{} = q, expr) when is_binary(expr), do: %{q | score_function: expr}
  def vector(%__MODULE__{} = q, vec) when is_list(vec), do: %{q | vector: vec}
  def merge_strategy(%__MODULE__{} = q, s), do: %{q | merge_strategy: s}
  def text_weight(%__MODULE__{} = q, w), do: %{q | text_weight: w}
  def vector_weight(%__MODULE__{} = q, w), do: %{q | vector_weight: w}
  def rrf_k(%__MODULE__{} = q, k) when is_integer(k), do: %{q | rrf_k: k}

  @doc "Set highlight configuration."
  def highlight(%__MODULE__{} = q, opts) do
    h =
      opts
      |> Enum.into(%{}, fn {k, v} -> {to_string(k), v} end)

    %{q | highlight: h}
  end

  @doc "Add an aggregation to the query."
  def aggregate(%__MODULE__{} = q, name, opts) do
    agg =
      opts
      |> Enum.into(%{}, fn {k, v} -> {to_string(k), v} end)
      |> Map.put("name", name)

    %{q | aggregations: q.aggregations ++ [agg]}
  end

  @doc "Convert query to request body map (without collection)."
  def to_request_body(%__MODULE__{} = q) do
    %{}
    |> maybe_put("query", q.query)
    |> maybe_put("vector", q.vector)
    |> maybe_put_list("fields", q.fields)
    |> Map.put("limit", q.limit)
    |> maybe_put_nonzero("offset", q.offset)
    |> maybe_put("merge_strategy", q.merge_strategy)
    |> maybe_put("text_weight", q.text_weight)
    |> maybe_put("vector_weight", q.vector_weight)
    |> maybe_put("highlight", q.highlight)
    |> maybe_put("rerank", q.rerank)
    |> maybe_put("min_score", q.min_score)
    |> maybe_put("score_function", q.score_function)
    |> maybe_put("rrf_k", q.rrf_k)
  end

  @doc "Convert query to aggregate request body."
  def to_aggregate_body(%__MODULE__{} = q) do
    %{"aggregations" => q.aggregations}
    |> maybe_put("query", q.query)
    |> Map.put("scan_limit", q.limit)
  end

  defp maybe_put(map, _key, nil), do: map
  defp maybe_put(map, key, value), do: Map.put(map, key, value)

  defp maybe_put_list(map, _key, []), do: map
  defp maybe_put_list(map, key, list), do: Map.put(map, key, list)

  defp maybe_put_nonzero(map, _key, 0), do: map
  defp maybe_put_nonzero(map, key, value), do: Map.put(map, key, value)
end
```

**Step 4: Run tests**

```bash
cd clients/elixir/prismsearch && mix test test/prismsearch/query_test.exs
```

Expected: All tests pass.

**Step 5: Commit**

```bash
git add clients/elixir/prismsearch/
git commit -m "feat(elixir): add pipe-friendly Query builder"
```

---

### Task 5: Elixir Main Facade (CRUD, Search, Health)

**Files:**
- Modify: `clients/elixir/prismsearch/lib/prismsearch.ex`
- Create: `clients/elixir/prismsearch/test/prismsearch/facade_test.exs`

**Step 1: Write facade test**

```elixir
# test/prismsearch/facade_test.exs
defmodule Prismsearch.FacadeTest do
  use ExUnit.Case, async: true

  # These tests verify the public API exists and delegates correctly.
  # Full integration tests require PRISM_TEST_URL env var.

  describe "client/1" do
    test "creates a client struct" do
      client = Prismsearch.client(base_url: "http://test:3080")
      assert %Prismsearch.Client{} = client
      assert client.base_url == "http://test:3080"
    end
  end

  # Integration tests — only run when PRISM_TEST_URL is set
  if System.get_env("PRISM_TEST_URL") do
    @tag :integration
    describe "integration" do
      setup do
        client = Prismsearch.client(base_url: System.get_env("PRISM_TEST_URL"))
        %{client: client}
      end

      test "health/1", %{client: client} do
        assert {:ok, health} = Prismsearch.health(client)
        assert health["status"] == "ok"
      end

      test "list_collections/1", %{client: client} do
        assert {:ok, %{"collections" => collections}} = Prismsearch.list_collections(client)
        assert is_list(collections)
      end
    end
  end
end
```

**Step 2: Implement main facade**

```elixir
# lib/prismsearch.ex
defmodule Prismsearch do
  @moduledoc """
  Elixir client for Prism search engine.

  ## Quick Start

      client = Prismsearch.client(base_url: "http://localhost:3080")
      {:ok, results} = Prismsearch.search(client, Prismsearch.Query.new("products", "headphones"))
  """

  alias Prismsearch.{Client, Query, SearchResults}

  @doc "Create a new Prism client."
  def client(opts \\ []), do: Client.new(opts)

  # Health
  def health(%Client{} = c), do: Client.get(c, "/health")

  # Collections
  def list_collections(%Client{} = c), do: Client.get(c, "/admin/collections")

  def create_collection(%Client{} = c, name, schema) when is_binary(name) do
    Client.put(c, "/collections/#{name}", schema)
  end

  def delete_collection(%Client{} = c, name) when is_binary(name) do
    Client.delete(c, "/collections/#{name}")
  end

  def get_schema(%Client{} = c, collection) do
    Client.get(c, "/collections/#{collection}/schema")
  end

  def get_stats(%Client{} = c, collection) do
    Client.get(c, "/collections/#{collection}/stats")
  end

  # Documents
  def index(%Client{} = c, collection, documents) when is_list(documents) do
    Client.post(c, "/collections/#{collection}/documents", %{"documents" => documents})
  end

  def get_document(%Client{} = c, collection, id) do
    Client.get(c, "/collections/#{collection}/documents/#{id}")
  end

  # Search
  def search(%Client{} = c, %Query{} = q) do
    body = Query.to_request_body(q)

    case Client.post(c, "/collections/#{q.collection}/search", body) do
      {:ok, data} -> {:ok, SearchResults.from_map(data)}
      error -> error
    end
  end

  # Aggregations
  def aggregate(%Client{} = c, %Query{} = q) do
    body = Query.to_aggregate_body(q)
    Client.post(c, "/collections/#{q.collection}/aggregate", body)
  end

  # Suggest
  def suggest(%Client{} = c, collection, opts) do
    body = %{
      "prefix" => Keyword.fetch!(opts, :prefix),
      "field" => Keyword.fetch!(opts, :field),
      "size" => Keyword.get(opts, :size, 5),
      "fuzzy" => Keyword.get(opts, :fuzzy, false),
      "max_distance" => Keyword.get(opts, :max_distance, 2)
    }

    Client.post(c, "/collections/#{collection}/_suggest", body)
  end

  # More Like This
  def more_like_this(%Client{} = c, collection, opts) do
    body = %{}
    body = if Keyword.has_key?(opts, :like), do: Map.put(body, "like", Keyword.get(opts, :like)), else: body
    body = if Keyword.has_key?(opts, :like_text), do: Map.put(body, "like_text", Keyword.get(opts, :like_text)), else: body
    body = if Keyword.has_key?(opts, :fields), do: Map.put(body, "fields", Keyword.get(opts, :fields)), else: body
    body = Map.put(body, "size", Keyword.get(opts, :size, 10))

    case Client.post(c, "/collections/#{collection}/_mlt", body) do
      {:ok, data} -> {:ok, SearchResults.from_map(data)}
      error -> error
    end
  end

  # Multi-search
  def multi_search(%Client{} = c, collections, opts \\ []) when is_list(collections) do
    body = %{
      "collections" => collections,
      "limit" => Keyword.get(opts, :limit, 10)
    }
    body = if Keyword.has_key?(opts, :query), do: Map.put(body, "query", Keyword.get(opts, :query)), else: body
    body = if Keyword.has_key?(opts, :vector), do: Map.put(body, "vector", Keyword.get(opts, :vector)), else: body

    Client.post(c, "/_msearch", body)
  end

  # Segments & Optimize
  def segments(%Client{} = c, collection) do
    Client.get(c, "/collections/#{collection}/segments")
  end

  def optimize(%Client{} = c, collection, opts \\ []) do
    body =
      case Keyword.get(opts, :max_segments) do
        nil -> nil
        n -> %{"max_segments" => n}
      end

    Client.post(c, "/collections/#{collection}/optimize", body)
  end

  # Cache & Server stats
  def cache_stats(%Client{} = c), do: Client.get(c, "/stats/cache")
  def server_info(%Client{} = c), do: Client.get(c, "/stats/server")
end
```

**Step 3: Run all tests**

```bash
cd clients/elixir/prismsearch && mix test
```

Expected: All tests pass.

**Step 4: Commit**

```bash
git add clients/elixir/prismsearch/
git commit -m "feat(elixir): add main Prismsearch facade with all core operations"
```

---

### Task 6: Elixir Graph & ILM Modules

**Files:**
- Create: `clients/elixir/prismsearch/lib/prismsearch/graph.ex`
- Create: `clients/elixir/prismsearch/lib/prismsearch/ilm.ex`
- Create: `clients/elixir/prismsearch/test/prismsearch/graph_test.exs`

**Step 1: Write graph test**

```elixir
# test/prismsearch/graph_test.exs
defmodule Prismsearch.GraphTest do
  use ExUnit.Case, async: true

  test "module exists and has expected functions" do
    assert function_exported?(Prismsearch.Graph, :add_node, 3)
    assert function_exported?(Prismsearch.Graph, :get_node, 3)
    assert function_exported?(Prismsearch.Graph, :remove_node, 3)
    assert function_exported?(Prismsearch.Graph, :add_edge, 3)
    assert function_exported?(Prismsearch.Graph, :get_edges, 3)
    assert function_exported?(Prismsearch.Graph, :bfs, 3)
    assert function_exported?(Prismsearch.Graph, :shortest_path, 3)
    assert function_exported?(Prismsearch.Graph, :stats, 2)
  end
end
```

**Step 2: Implement Graph module**

```elixir
# lib/prismsearch/graph.ex
defmodule Prismsearch.Graph do
  @moduledoc "Graph API operations for Prism."

  alias Prismsearch.Client

  def add_node(%Client{} = c, collection, node) when is_map(node) do
    Client.post(c, "/collections/#{collection}/graph/nodes", node)
  end

  def get_node(%Client{} = c, collection, id) do
    Client.get(c, "/collections/#{collection}/graph/nodes/#{id}")
  end

  def remove_node(%Client{} = c, collection, id) do
    Client.delete(c, "/collections/#{collection}/graph/nodes/#{id}")
  end

  def add_edge(%Client{} = c, collection, edge) when is_map(edge) do
    Client.post(c, "/collections/#{collection}/graph/edges", edge)
  end

  def get_edges(%Client{} = c, collection, node_id) do
    Client.get(c, "/collections/#{collection}/graph/nodes/#{node_id}/edges")
  end

  def bfs(%Client{} = c, collection, opts) do
    body = %{
      "start" => Keyword.fetch!(opts, :start),
      "edge_type" => Keyword.fetch!(opts, :edge_type),
      "max_depth" => Keyword.get(opts, :max_depth, 3)
    }
    Client.post(c, "/collections/#{collection}/graph/bfs", body)
  end

  def shortest_path(%Client{} = c, collection, opts) do
    body = %{
      "start" => Keyword.fetch!(opts, :start),
      "target" => Keyword.fetch!(opts, :target)
    }
    body = if Keyword.has_key?(opts, :edge_types),
      do: Map.put(body, "edge_types", Keyword.get(opts, :edge_types)),
      else: body
    Client.post(c, "/collections/#{collection}/graph/shortest-path", body)
  end

  def stats(%Client{} = c, collection) do
    Client.get(c, "/collections/#{collection}/graph/stats")
  end
end
```

**Step 3: Implement ILM module**

```elixir
# lib/prismsearch/ilm.ex
defmodule Prismsearch.ILM do
  @moduledoc "Index Lifecycle Management operations."

  alias Prismsearch.Client

  def list_policies(%Client{} = c), do: Client.get(c, "/_ilm/policy")
  def get_policy(%Client{} = c, name), do: Client.get(c, "/_ilm/policy/#{name}")

  def create_policy(%Client{} = c, name, config) when is_map(config) do
    Client.put(c, "/_ilm/policy/#{name}", config)
  end

  def delete_policy(%Client{} = c, name), do: Client.delete(c, "/_ilm/policy/#{name}")
  def status(%Client{} = c), do: Client.get(c, "/_ilm/status")
  def explain(%Client{} = c, index), do: Client.get(c, "/#{index}/_ilm/explain")
  def rollover(%Client{} = c, index), do: Client.post(c, "/#{index}/_rollover", %{})

  def move_phase(%Client{} = c, index, phase) do
    Client.post(c, "/#{index}/_ilm/move/#{phase}", %{})
  end

  def attach_policy(%Client{} = c, collection, policy) do
    Client.post(c, "/#{collection}/_ilm/attach", %{"policy" => policy})
  end

  def list_aliases(%Client{} = c), do: Client.get(c, "/_aliases")

  def update_aliases(%Client{} = c, actions) when is_list(actions) do
    Client.put(c, "/_aliases", %{"actions" => actions})
  end
end
```

**Step 4: Run all tests**

```bash
cd clients/elixir/prismsearch && mix test
```

**Step 5: Commit**

```bash
git add clients/elixir/prismsearch/
git commit -m "feat(elixir): add Graph and ILM modules"
```

---

## Phase 3: Python Client

### Task 7: Scaffold Python Package

**Files:**
- Create: `clients/python/prismsearch/pyproject.toml`
- Create: `clients/python/prismsearch/src/prismsearch/__init__.py`
- Create: `clients/python/prismsearch/src/prismsearch/py.typed`
- Create: `clients/python/prismsearch/tests/__init__.py`

**Step 1: Create pyproject.toml**

```toml
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "prismsearch"
version = "0.1.0"
description = "Python client for Prism search engine"
readme = "README.md"
license = "MIT"
requires-python = ">=3.10"
dependencies = [
    "httpx>=0.27",
    "pydantic>=2.0",
]

[project.optional-dependencies]
dev = [
    "pytest>=8.0",
    "pytest-asyncio>=0.24",
    "respx>=0.21",
]

[tool.hatch.build.targets.wheel]
packages = ["src/prismsearch"]

[tool.pytest.ini_options]
testpaths = ["tests"]
asyncio_mode = "auto"
```

**Step 2: Create `__init__.py` with re-exports**

```python
# src/prismsearch/__init__.py
"""Prismsearch - Python client for Prism search engine."""

from prismsearch.client import Prismsearch
from prismsearch.query import Query

__version__ = "0.1.0"
__all__ = ["Prismsearch", "Query"]
```

**Step 3: Create empty marker files**

```bash
mkdir -p clients/python/prismsearch/src/prismsearch
mkdir -p clients/python/prismsearch/tests
touch clients/python/prismsearch/src/prismsearch/py.typed
touch clients/python/prismsearch/tests/__init__.py
```

**Step 4: Commit**

```bash
git add clients/python/prismsearch/
git commit -m "feat(python): scaffold prismsearch package"
```

---

### Task 8: Python Models (Pydantic v2)

**Files:**
- Create: `clients/python/prismsearch/src/prismsearch/models.py`
- Create: `clients/python/prismsearch/tests/test_models.py`

**Step 1: Write model test**

```python
# tests/test_models.py
from prismsearch.models import (
    SearchResult, SearchResults, HealthResponse,
    IndexResponse, CollectionStatsResponse, SuggestResponse,
    SegmentsInfo, OptimizeResult,
)


def test_search_result_from_dict():
    data = {"id": "doc-1", "score": 4.82, "fields": {"title": "Test"}}
    r = SearchResult.model_validate(data)
    assert r.id == "doc-1"
    assert r.score == 4.82
    assert r.fields["title"] == "Test"


def test_search_results():
    data = {
        "results": [{"id": "1", "score": 1.0, "fields": {}}],
        "total": 1,
    }
    rs = SearchResults.model_validate(data)
    assert rs.total == 1
    assert len(rs.results) == 1


def test_health_response():
    data = {"status": "ok", "version": "0.6.6", "collections": 4, "uptime_secs": 100}
    h = HealthResponse.model_validate(data)
    assert h.status == "ok"
    assert h.collections == 4


def test_index_response():
    data = {"indexed": 5, "failed": 0, "errors": []}
    r = IndexResponse.model_validate(data)
    assert r.indexed == 5


def test_segments_info():
    data = {
        "segments": [{"id": "s1", "doc_count": 100, "deleted_count": 2, "size_bytes": 5000}],
        "total_docs": 100,
        "total_deleted": 2,
        "delete_ratio": 0.02,
    }
    s = SegmentsInfo.model_validate(data)
    assert len(s.segments) == 1


def test_optimize_result():
    data = {"segments_before": 5, "segments_after": 1, "merged": True}
    o = OptimizeResult.model_validate(data)
    assert o.merged is True
```

**Step 2: Run test — expect failure**

```bash
cd clients/python/prismsearch
pip install -e ".[dev]" && pytest tests/test_models.py -v
```

**Step 3: Implement models**

```python
# src/prismsearch/models.py
"""Pydantic v2 models for Prism API types."""

from __future__ import annotations
from typing import Any
from pydantic import BaseModel, Field


class SearchResult(BaseModel):
    id: str
    score: float
    fields: dict[str, Any] = Field(default_factory=dict)
    highlight: dict[str, list[str]] | None = None


class SearchResults(BaseModel):
    results: list[SearchResult] = Field(default_factory=list)
    total: int = 0


class HealthResponse(BaseModel):
    status: str
    version: str
    collections: int
    uptime_secs: int


class IndexResponse(BaseModel):
    indexed: int
    failed: int
    errors: list[dict[str, str]] = Field(default_factory=list)


class CollectionStatsResponse(BaseModel):
    collection: str = ""
    document_count: int = 0
    storage_bytes: int = 0


class SegmentInfo(BaseModel):
    id: str
    doc_count: int
    deleted_count: int
    size_bytes: int


class SegmentsInfo(BaseModel):
    segments: list[SegmentInfo] = Field(default_factory=list)
    total_docs: int = 0
    total_deleted: int = 0
    delete_ratio: float = 0.0


class OptimizeResult(BaseModel):
    segments_before: int
    segments_after: int
    merged: bool


class SuggestionEntry(BaseModel):
    term: str
    score: float
    doc_freq: int


class SuggestResponse(BaseModel):
    suggestions: list[SuggestionEntry] = Field(default_factory=list)
    did_you_mean: str | None = None


class BfsResponse(BaseModel):
    nodes: list[str] = Field(default_factory=list)
    count: int = 0


class ShortestPathResponse(BaseModel):
    path: list[str] | None = None
    length: int | None = None


class GraphStats(BaseModel):
    node_count: int = 0
    edge_count: int = 0


class PrismError(Exception):
    """Error from Prism API."""

    def __init__(self, status: int, message: str):
        self.status = status
        self.message = message
        super().__init__(f"Prism error ({status}): {message}")
```

**Step 4: Run tests**

```bash
cd clients/python/prismsearch && pytest tests/test_models.py -v
```

Expected: All pass.

**Step 5: Commit**

```bash
git add clients/python/prismsearch/
git commit -m "feat(python): add Pydantic v2 models for all API types"
```

---

### Task 9: Python HTTP Client

**Files:**
- Create: `clients/python/prismsearch/src/prismsearch/client.py`
- Create: `clients/python/prismsearch/tests/test_client.py`

**Step 1: Write client test with mocked HTTP**

```python
# tests/test_client.py
import pytest
import respx
import httpx
from prismsearch import Prismsearch


@respx.mock
def test_health():
    respx.get("http://test:3080/health").mock(
        return_value=httpx.Response(200, json={
            "status": "ok", "version": "0.6.6", "collections": 4, "uptime_secs": 100
        })
    )
    client = Prismsearch("http://test:3080")
    health = client.health()
    assert health.status == "ok"
    assert health.collections == 4


@respx.mock
def test_list_collections():
    respx.get("http://test:3080/admin/collections").mock(
        return_value=httpx.Response(200, json={"collections": ["a", "b"]})
    )
    client = Prismsearch("http://test:3080")
    result = client.list_collections()
    assert result == ["a", "b"]


@respx.mock
def test_index_documents():
    respx.post("http://test:3080/collections/products/documents").mock(
        return_value=httpx.Response(201, json={"indexed": 2, "failed": 0, "errors": []})
    )
    client = Prismsearch("http://test:3080")
    result = client.index("products", [
        {"id": "1", "fields": {"title": "A"}},
        {"id": "2", "fields": {"title": "B"}},
    ])
    assert result.indexed == 2


@respx.mock
def test_search():
    respx.post("http://test:3080/collections/products/search").mock(
        return_value=httpx.Response(200, json={
            "results": [{"id": "1", "score": 1.5, "fields": {"title": "Test"}}],
            "total": 1,
        })
    )
    client = Prismsearch("http://test:3080")
    from prismsearch import Query
    results = Query("products", "test").execute(client)
    assert results.total == 1
    assert results.results[0].id == "1"


@respx.mock
def test_error_handling():
    respx.get("http://test:3080/collections/missing/stats").mock(
        return_value=httpx.Response(404, text="Collection not found")
    )
    client = Prismsearch("http://test:3080")
    from prismsearch.models import PrismError
    with pytest.raises(PrismError) as exc_info:
        client.get_stats("missing")
    assert exc_info.value.status == 404


@respx.mock
def test_api_key_header():
    route = respx.get("http://test:3080/health").mock(
        return_value=httpx.Response(200, json={
            "status": "ok", "version": "0.6.6", "collections": 0, "uptime_secs": 0
        })
    )
    client = Prismsearch("http://test:3080", api_key="secret")
    client.health()
    assert route.calls[0].request.headers["authorization"] == "Bearer secret"
```

**Step 2: Implement client**

```python
# src/prismsearch/client.py
"""Prismsearch HTTP client."""

from __future__ import annotations
from typing import Any

import httpx

from prismsearch.models import (
    HealthResponse, IndexResponse, SearchResults,
    CollectionStatsResponse, SuggestResponse, SegmentsInfo,
    OptimizeResult, BfsResponse, ShortestPathResponse, GraphStats,
    PrismError,
)


class Prismsearch:
    """Synchronous Prism client.

    Usage::

        client = Prismsearch("http://localhost:3080")
        health = client.health()
    """

    def __init__(self, base_url: str = "http://localhost:3080", *, api_key: str | None = None, timeout: float = 30.0):
        headers = {}
        if api_key:
            headers["authorization"] = f"Bearer {api_key}"
        self._http = httpx.Client(base_url=base_url, headers=headers, timeout=timeout)

    def close(self) -> None:
        self._http.close()

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    # -- Internal helpers --

    def _get(self, path: str, **kwargs) -> Any:
        resp = self._http.get(path, **kwargs)
        if resp.status_code >= 400:
            raise PrismError(resp.status_code, resp.text)
        return resp.json()

    def _post(self, path: str, json: Any = None) -> tuple[int, Any]:
        resp = self._http.post(path, json=json)
        if resp.status_code >= 400:
            raise PrismError(resp.status_code, resp.text)
        return resp.status_code, resp.json()

    def _put(self, path: str, json: Any = None) -> Any:
        resp = self._http.put(path, json=json)
        if resp.status_code >= 400:
            raise PrismError(resp.status_code, resp.text)
        return resp.json()

    def _delete(self, path: str) -> Any:
        resp = self._http.delete(path)
        if resp.status_code >= 400:
            raise PrismError(resp.status_code, resp.text)
        return resp.json()

    # -- Health --

    def health(self) -> HealthResponse:
        return HealthResponse.model_validate(self._get("/health"))

    # -- Collections --

    def list_collections(self) -> list[str]:
        data = self._get("/admin/collections")
        return data.get("collections", data) if isinstance(data, dict) else data

    def create_collection(self, name: str, schema: dict) -> dict:
        return self._put(f"/collections/{name}", json=schema)

    def delete_collection(self, name: str) -> dict:
        return self._delete(f"/collections/{name}")

    def get_schema(self, collection: str) -> dict:
        return self._get(f"/collections/{collection}/schema")

    def get_stats(self, collection: str) -> CollectionStatsResponse:
        return CollectionStatsResponse.model_validate(self._get(f"/collections/{collection}/stats"))

    # -- Documents --

    def index(self, collection: str, documents: list[dict]) -> IndexResponse:
        _, data = self._post(f"/collections/{collection}/documents", json={"documents": documents})
        return IndexResponse.model_validate(data)

    def get_document(self, collection: str, doc_id: str) -> dict | None:
        return self._get(f"/collections/{collection}/documents/{doc_id}")

    # -- Search --

    def search(self, collection: str, body: dict) -> SearchResults:
        _, data = self._post(f"/collections/{collection}/search", json=body)
        return SearchResults.model_validate(data)

    # -- Aggregations --

    def aggregate(self, collection: str, body: dict) -> dict:
        _, data = self._post(f"/collections/{collection}/aggregate", json=body)
        return data

    # -- Suggest --

    def suggest(self, collection: str, *, prefix: str, field: str, size: int = 5, fuzzy: bool = False, max_distance: int = 2) -> SuggestResponse:
        body = {"prefix": prefix, "field": field, "size": size, "fuzzy": fuzzy, "max_distance": max_distance}
        _, data = self._post(f"/collections/{collection}/_suggest", json=body)
        return SuggestResponse.model_validate(data)

    # -- More Like This --

    def mlt(self, collection: str, *, like: dict | None = None, like_text: str | None = None, fields: list[str] | None = None, size: int = 10) -> SearchResults:
        body: dict[str, Any] = {"size": size}
        if like:
            body["like"] = like
        if like_text:
            body["like_text"] = like_text
        if fields:
            body["fields"] = fields
        _, data = self._post(f"/collections/{collection}/_mlt", json=body)
        return SearchResults.model_validate(data)

    # -- Multi-search --

    def multi_search(self, collections: list[str], *, query: str | None = None, vector: list[float] | None = None, limit: int = 10) -> dict:
        body: dict[str, Any] = {"collections": collections, "limit": limit}
        if query:
            body["query"] = query
        if vector:
            body["vector"] = vector
        _, data = self._post("/_msearch", json=body)
        return data

    # -- Segments & Optimize --

    def segments(self, collection: str) -> SegmentsInfo:
        return SegmentsInfo.model_validate(self._get(f"/collections/{collection}/segments"))

    def optimize(self, collection: str, *, max_segments: int | None = None) -> OptimizeResult:
        body = {"max_segments": max_segments} if max_segments else None
        _, data = self._post(f"/collections/{collection}/optimize", json=body)
        return OptimizeResult.model_validate(data)

    # -- Graph --

    class _GraphNamespace:
        def __init__(self, client: "Prismsearch"):
            self._c = client

        def add_node(self, collection: str, node: dict) -> None:
            self._c._post(f"/collections/{collection}/graph/nodes", json=node)

        def get_node(self, collection: str, node_id: str) -> dict:
            return self._c._get(f"/collections/{collection}/graph/nodes/{node_id}")

        def remove_node(self, collection: str, node_id: str) -> None:
            self._c._delete(f"/collections/{collection}/graph/nodes/{node_id}")

        def add_edge(self, collection: str, edge: dict) -> None:
            self._c._post(f"/collections/{collection}/graph/edges", json=edge)

        def get_edges(self, collection: str, node_id: str) -> list[dict]:
            return self._c._get(f"/collections/{collection}/graph/nodes/{node_id}/edges")

        def bfs(self, collection: str, *, start: str, edge_type: str, max_depth: int = 3) -> BfsResponse:
            _, data = self._c._post(f"/collections/{collection}/graph/bfs", json={"start": start, "edge_type": edge_type, "max_depth": max_depth})
            return BfsResponse.model_validate(data)

        def shortest_path(self, collection: str, *, start: str, target: str, edge_types: list[str] | None = None) -> ShortestPathResponse:
            body: dict[str, Any] = {"start": start, "target": target}
            if edge_types:
                body["edge_types"] = edge_types
            _, data = self._c._post(f"/collections/{collection}/graph/shortest-path", json=body)
            return ShortestPathResponse.model_validate(data)

        def stats(self, collection: str) -> GraphStats:
            return GraphStats.model_validate(self._c._get(f"/collections/{collection}/graph/stats"))

    @property
    def graph(self) -> _GraphNamespace:
        return self._GraphNamespace(self)

    # -- Stats --

    def cache_stats(self) -> dict:
        return self._get("/stats/cache")

    def server_info(self) -> dict:
        return self._get("/stats/server")
```

**Step 3: Run tests**

```bash
cd clients/python/prismsearch && pytest tests/ -v
```

**Step 4: Commit**

```bash
git add clients/python/prismsearch/
git commit -m "feat(python): add Prismsearch client with all API operations"
```

---

### Task 10: Python Query Builder

**Files:**
- Create: `clients/python/prismsearch/src/prismsearch/query.py`
- Create: `clients/python/prismsearch/tests/test_query.py`

**Step 1: Write query builder test**

```python
# tests/test_query.py
from prismsearch.query import Query


def test_basic_query():
    q = Query("products", "headphones")
    assert q.collection == "products"
    assert q._query == "headphones"


def test_chaining():
    q = (
        Query("products", "test")
        .fields(["title", "content"])
        .limit(20)
        .offset(5)
        .min_score(0.5)
    )
    body = q.to_request_body()
    assert body["query"] == "test"
    assert body["fields"] == ["title", "content"]
    assert body["limit"] == 20
    assert body["offset"] == 5
    assert body["min_score"] == 0.5


def test_highlight():
    q = Query("products", "test").highlight(
        fields=["title"], pre_tag="<b>", post_tag="</b>"
    )
    body = q.to_request_body()
    assert body["highlight"]["fields"] == ["title"]
    assert body["highlight"]["pre_tag"] == "<b>"


def test_vector_query():
    q = Query("products").vector([0.1, 0.2, 0.3])
    body = q.to_request_body()
    assert body["vector"] == [0.1, 0.2, 0.3]
    assert "query" not in body


def test_aggregations():
    q = (
        Query("products")
        .aggregate("price_stats", type="stats", field="price")
        .aggregate("by_cat", type="terms", field="category", size=10)
    )
    assert len(q._aggregations) == 2
    agg_body = q.to_aggregate_body()
    assert len(agg_body["aggregations"]) == 2
    assert agg_body["aggregations"][0]["name"] == "price_stats"


def test_no_optional_fields_when_default():
    q = Query("products", "test")
    body = q.to_request_body()
    assert "offset" not in body
    assert "vector" not in body
    assert "highlight" not in body
    assert "min_score" not in body
```

**Step 2: Implement Query builder**

```python
# src/prismsearch/query.py
"""Pipe-style query builder for Prism searches."""

from __future__ import annotations
from typing import Any, TYPE_CHECKING

if TYPE_CHECKING:
    from prismsearch.client import Prismsearch
    from prismsearch.models import SearchResults


class Query:
    """Chainable query builder.

    Usage::

        results = (
            Query("products", "wireless headphones")
            .fields(["title", "description"])
            .limit(20)
            .highlight(fields=["title"])
            .execute(client)
        )
    """

    def __init__(self, collection: str, query: str | None = None):
        self.collection = collection
        self._query = query
        self._vector: list[float] | None = None
        self._fields: list[str] = []
        self._limit: int = 10
        self._offset: int = 0
        self._merge_strategy: str | None = None
        self._text_weight: float | None = None
        self._vector_weight: float | None = None
        self._highlight: dict | None = None
        self._rerank: dict | None = None
        self._min_score: float | None = None
        self._score_function: str | None = None
        self._rrf_k: int | None = None
        self._aggregations: list[dict] = []

    def fields(self, fields: list[str]) -> Query:
        self._fields = fields
        return self

    def limit(self, n: int) -> Query:
        self._limit = n
        return self

    def offset(self, n: int) -> Query:
        self._offset = n
        return self

    def vector(self, vec: list[float]) -> Query:
        self._vector = vec
        return self

    def min_score(self, s: float) -> Query:
        self._min_score = s
        return self

    def score_function(self, expr: str) -> Query:
        self._score_function = expr
        return self

    def merge_strategy(self, s: str) -> Query:
        self._merge_strategy = s
        return self

    def text_weight(self, w: float) -> Query:
        self._text_weight = w
        return self

    def vector_weight(self, w: float) -> Query:
        self._vector_weight = w
        return self

    def rrf_k(self, k: int) -> Query:
        self._rrf_k = k
        return self

    def highlight(self, *, fields: list[str], pre_tag: str = "<em>", post_tag: str = "</em>", fragment_size: int = 150, number_of_fragments: int = 3) -> Query:
        self._highlight = {
            "fields": fields,
            "pre_tag": pre_tag,
            "post_tag": post_tag,
            "fragment_size": fragment_size,
            "number_of_fragments": number_of_fragments,
        }
        return self

    def aggregate(self, name: str, **kwargs: Any) -> Query:
        agg = {"name": name, **kwargs}
        self._aggregations.append(agg)
        return self

    def to_request_body(self) -> dict[str, Any]:
        body: dict[str, Any] = {"limit": self._limit}
        if self._query is not None:
            body["query"] = self._query
        if self._vector is not None:
            body["vector"] = self._vector
        if self._fields:
            body["fields"] = self._fields
        if self._offset > 0:
            body["offset"] = self._offset
        if self._merge_strategy:
            body["merge_strategy"] = self._merge_strategy
        if self._text_weight is not None:
            body["text_weight"] = self._text_weight
        if self._vector_weight is not None:
            body["vector_weight"] = self._vector_weight
        if self._highlight:
            body["highlight"] = self._highlight
        if self._rerank:
            body["rerank"] = self._rerank
        if self._min_score is not None:
            body["min_score"] = self._min_score
        if self._score_function:
            body["score_function"] = self._score_function
        if self._rrf_k is not None:
            body["rrf_k"] = self._rrf_k
        return body

    def to_aggregate_body(self) -> dict[str, Any]:
        body: dict[str, Any] = {
            "aggregations": self._aggregations,
            "scan_limit": self._limit,
        }
        if self._query:
            body["query"] = self._query
        return body

    def execute(self, client: Prismsearch) -> SearchResults:
        """Execute search and return typed results."""
        return client.search(self.collection, self.to_request_body())

    def execute_aggs(self, client: Prismsearch) -> dict:
        """Execute aggregation query."""
        return client.aggregate(self.collection, self.to_aggregate_body())
```

**Step 3: Run tests**

```bash
cd clients/python/prismsearch && pytest tests/ -v
```

**Step 4: Commit**

```bash
git add clients/python/prismsearch/
git commit -m "feat(python): add chainable Query builder"
```

---

### Task 11: Django Integration Package

**Files:**
- Create: `clients/python/prismsearch-django/pyproject.toml`
- Create: `clients/python/prismsearch-django/src/prismsearch_django/__init__.py`
- Create: `clients/python/prismsearch-django/src/prismsearch_django/conf.py`
- Create: `clients/python/prismsearch-django/src/prismsearch_django/mixins.py`
- Create: `clients/python/prismsearch-django/src/prismsearch_django/signals.py`
- Create: `clients/python/prismsearch-django/src/prismsearch_django/management/__init__.py`
- Create: `clients/python/prismsearch-django/src/prismsearch_django/management/commands/__init__.py`
- Create: `clients/python/prismsearch-django/src/prismsearch_django/management/commands/prismsearch_reindex.py`
- Create: `clients/python/prismsearch-django/src/prismsearch_django/apps.py`

This task is larger — implement the full Django integration in one go, as the components are tightly coupled. Detailed code for each file should follow the design doc patterns. The key pieces:

**pyproject.toml** — depends on `prismsearch>=0.1.0` and `django>=4.2`.

**conf.py** — reads `settings.PRISMSEARCH` dict, provides `get_client()` singleton.

**mixins.py** — `SearchableModel` mixin with `PrismMeta` inner class. Adds `prism` manager with `.search()`, `.reindex()`. `SearchField` dataclass for field definitions.

**signals.py** — `post_save` and `post_delete` handlers that auto-sync to Prism. Connected in `apps.py` `ready()`.

**apps.py** — Django `AppConfig` that connects signals in `ready()`.

**management/commands/prismsearch_reindex.py** — Iterates all `SearchableModel` subclasses, reads all objects, indexes in batches.

**Step 1: Implement all files**

(Each file follows the design doc's Django section exactly.)

**Step 2: Commit**

```bash
git add clients/python/prismsearch-django/
git commit -m "feat(python): add prismsearch-django integration package"
```

---

## Phase 4: Rust Client

### Task 12: Scaffold Rust Crate

**Files:**
- Create: `clients/rust/prismsearch/Cargo.toml`
- Create: `clients/rust/prismsearch/src/lib.rs`

**Step 1: Create Cargo.toml**

```toml
[package]
name = "prismsearch"
version = "0.1.0"
edition = "2021"
description = "Rust client for Prism search engine"
license = "MIT"
repository = "https://github.com/mikalv/prism"

[dependencies]
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["rt"] }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
wiremock = "0.6"
```

**Step 2: Create lib.rs with module declarations**

```rust
// src/lib.rs
//! Prismsearch - Rust client for Prism search engine.

pub mod client;
pub mod error;
pub mod models;
pub mod query;

pub use client::Client;
pub use error::Error;
pub use models::*;
pub use query::Query;
```

**Step 3: Commit**

```bash
git add clients/rust/prismsearch/
git commit -m "feat(rust): scaffold prismsearch crate"
```

---

### Task 13: Rust Models & Error Types

**Files:**
- Create: `clients/rust/prismsearch/src/error.rs`
- Create: `clients/rust/prismsearch/src/models.rs`

**Step 1: Write tests (in models.rs)**

```rust
// At bottom of src/models.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_search_results() {
        let json = r#"{"results":[{"id":"1","score":1.5,"fields":{"title":"Test"}}],"total":1}"#;
        let results: SearchResults = serde_json::from_str(json).unwrap();
        assert_eq!(results.total, 1);
        assert_eq!(results.results[0].id, "1");
    }

    #[test]
    fn document_builder() {
        let doc = Document::new("1")
            .field("title", "Hello")
            .field("score", 42);
        assert_eq!(doc.id, "1");
        assert_eq!(doc.fields["title"], serde_json::json!("Hello"));
        assert_eq!(doc.fields["score"], serde_json::json!(42));
    }

    #[test]
    fn deserialize_health() {
        let json = r#"{"status":"ok","version":"0.6.6","collections":4,"uptime_secs":100}"#;
        let h: HealthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(h.status, "ok");
        assert_eq!(h.collections, 4);
    }
}
```

**Step 2: Implement error.rs**

```rust
// src/error.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Prism API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
```

**Step 3: Implement models.rs**

```rust
// src/models.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    #[serde(default)]
    pub fields: HashMap<String, serde_json::Value>,
}

impl Document {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), fields: HashMap::new() }
    }

    pub fn field(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
    #[serde(default)]
    pub fields: HashMap<String, serde_json::Value>,
    pub highlight: Option<HashMap<String, Vec<String>>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResults {
    pub results: Vec<SearchResult>,
    pub total: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub collections: usize,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IndexResponse {
    pub indexed: usize,
    pub failed: usize,
    #[serde(default)]
    pub errors: Vec<IndexError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IndexError {
    pub doc_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SegmentInfo {
    pub id: String,
    pub doc_count: u32,
    pub deleted_count: u32,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SegmentsInfo {
    pub segments: Vec<SegmentInfo>,
    pub total_docs: u64,
    pub total_deleted: u64,
    pub delete_ratio: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OptimizeResult {
    pub segments_before: usize,
    pub segments_after: usize,
    pub merged: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Highlight {
    pub fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fragment_size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_of_fragments: Option<usize>,
}

impl Highlight {
    pub fn new(fields: &[&str]) -> Self {
        Self {
            fields: fields.iter().map(|s| s.to_string()).collect(),
            pre_tag: None, post_tag: None,
            fragment_size: None, number_of_fragments: None,
        }
    }

    pub fn pre_tag(mut self, tag: impl Into<String>) -> Self { self.pre_tag = Some(tag.into()); self }
    pub fn post_tag(mut self, tag: impl Into<String>) -> Self { self.post_tag = Some(tag.into()); self }
}

// Tests at bottom (from Step 1)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_search_results() {
        let json = r#"{"results":[{"id":"1","score":1.5,"fields":{"title":"Test"}}],"total":1}"#;
        let results: SearchResults = serde_json::from_str(json).unwrap();
        assert_eq!(results.total, 1);
        assert_eq!(results.results[0].id, "1");
    }

    #[test]
    fn document_builder() {
        let doc = Document::new("1")
            .field("title", "Hello")
            .field("score", 42);
        assert_eq!(doc.id, "1");
        assert_eq!(doc.fields["title"], serde_json::json!("Hello"));
        assert_eq!(doc.fields["score"], serde_json::json!(42));
    }

    #[test]
    fn deserialize_health() {
        let json = r#"{"status":"ok","version":"0.6.6","collections":4,"uptime_secs":100}"#;
        let h: HealthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(h.status, "ok");
        assert_eq!(h.collections, 4);
    }
}
```

**Step 4: Run tests**

```bash
cd clients/rust/prismsearch && cargo test
```

**Step 5: Commit**

```bash
git add clients/rust/prismsearch/
git commit -m "feat(rust): add models, error types, and document builder"
```

---

### Task 14: Rust Client & Query Builder

**Files:**
- Create: `clients/rust/prismsearch/src/client.rs`
- Create: `clients/rust/prismsearch/src/query.rs`

**Step 1: Implement client.rs**

The client wraps reqwest with builder pattern: `Client::new(url).api_key("key").timeout(Duration).build()`. Methods for each endpoint: `health()`, `list_collections()`, `create_collection()`, `delete_collection()`, `index()`, `search()`, `aggregate()`, `suggest()`, `mlt()`, `multi_search()`, `segments()`, `optimize()`. Each returns `Result<T>`.

**Step 2: Implement query.rs**

Builder pattern: `Query::new("collection", "text").fields(&[...]).limit(20).highlight(h).min_score(0.5).execute(&client).await?`. Methods: `to_request_body() -> serde_json::Value`, `to_aggregate_body() -> serde_json::Value`.

**Step 3: Run tests**

```bash
cd clients/rust/prismsearch && cargo test
```

**Step 4: Compile check**

```bash
cd clients/rust/prismsearch && cargo check
```

**Step 5: Commit**

```bash
git add clients/rust/prismsearch/
git commit -m "feat(rust): add Client and Query builder"
```

---

## Phase 5: Final

### Task 15: Integration Tests & README

**Files:**
- Create: `clients/elixir/prismsearch/test/integration_test.exs`
- Create: `clients/python/prismsearch/tests/test_integration.py`
- Create: `clients/rust/prismsearch/tests/integration.rs`

Each integration test file checks `PRISM_TEST_URL` env var. If set, runs real HTTP calls against a Prism instance: health check, create temp collection, index documents, search, delete collection. If not set, tests are skipped.

**Step 1: Write integration tests for all three languages**

**Step 2: Run against local Prism**

```bash
PRISM_TEST_URL=http://localhost:3080 mix test --only integration
PRISM_TEST_URL=http://localhost:3080 pytest tests/test_integration.py -v
PRISM_TEST_URL=http://localhost:3080 cargo test --test integration
```

**Step 3: Commit**

```bash
git add clients/
git commit -m "test: add integration tests for all client libraries"
```

---

### Task 16: Final Commit & Tag

**Step 1: Final check — all tests pass**

```bash
cd clients/elixir/prismsearch && mix test
cd clients/python/prismsearch && pytest
cd clients/rust/prismsearch && cargo test
```

**Step 2: Commit any remaining changes**

```bash
git add -A clients/
git commit -m "feat: complete v0.1.0 client libraries for Elixir, Python, and Rust"
```
