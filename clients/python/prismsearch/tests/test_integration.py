"""Integration tests — only run when PRISM_TEST_URL is set."""

import os
import uuid

import pytest

pytestmark = pytest.mark.skipif(
    not os.environ.get("PRISM_TEST_URL"),
    reason="PRISM_TEST_URL not set",
)


@pytest.fixture
def client():
    from prismsearch import Prismsearch

    url = os.environ["PRISM_TEST_URL"]
    with Prismsearch(url) as c:
        yield c


@pytest.fixture
def test_collection():
    return f"prismsearch_py_test_{uuid.uuid4().hex[:8]}"


def test_full_lifecycle(client, test_collection):
    import time
    from prismsearch import Query

    # Health
    health = client.health()
    assert health.status == "ok"

    # Create collection
    schema = {
        "backends": {
            "text": {
                "fields": [
                    {"name": "title", "type": "text", "stored": True, "indexed": True},
                    {"name": "content", "type": "text", "stored": True, "indexed": True},
                ]
            }
        }
    }
    client.create_collection(test_collection, schema)

    # Index documents
    docs = [
        {"id": "1", "fields": {"title": "Python Testing", "content": "Integration test doc"}},
        {"id": "2", "fields": {"title": "Django Framework", "content": "Web framework for Python"}},
    ]
    result = client.index(test_collection, docs)
    assert result.indexed == 2

    # Wait for indexing
    time.sleep(0.5)

    # Search
    results = Query(test_collection, "Python").execute(client)
    assert results.total > 0

    # List collections
    collections = client.list_collections()
    assert test_collection in collections

    # Cleanup
    client.delete_collection(test_collection)
