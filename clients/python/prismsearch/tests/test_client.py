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
