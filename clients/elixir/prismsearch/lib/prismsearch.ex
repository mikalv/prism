defmodule Prismsearch do
  @moduledoc """
  Elixir client for [Prism](https://github.com/mikalv/prism) — a high-performance
  hybrid search engine combining full-text search (Tantivy/BM25) and vector search
  (HNSW) for AI/RAG applications.

  ## Quick Start

      client = Prismsearch.client(base_url: "http://localhost:3080")

      # Index documents
      docs = [%{"id" => "1", "fields" => %{"title" => "Hello", "content" => "World"}}]
      {:ok, _} = Prismsearch.index(client, "my_collection", docs)

      # Search
      query = Prismsearch.Query.new("my_collection", "hello")
      {:ok, results} = Prismsearch.search(client, query)

  ## Authentication

      client = Prismsearch.client(base_url: "http://localhost:3080", api_key: "sk-...")

  See the [Prism documentation](https://mikalv.github.io/prism/) for server setup and configuration.
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
