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
