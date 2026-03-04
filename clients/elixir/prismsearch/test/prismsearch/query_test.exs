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
