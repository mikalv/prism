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
