"""Integration tests — only run when PRISM_URL is set."""

import os
import time
import uuid

import pytest

pytestmark = pytest.mark.skipif(
    not os.environ.get("PRISM_URL"),
    reason="PRISM_URL not set",
)


@pytest.fixture
def prism_client():
    from agentimport.prism import PrismClient

    url = os.environ["PRISM_URL"]
    api_key = os.environ.get("PRISM_API_KEY")
    with PrismClient(url, api_key=api_key) as client:
        yield client


@pytest.fixture
def unique_suffix():
    return uuid.uuid4().hex[:8]


def test_schema_apply(prism_client):
    """Applying schemas should not error (idempotent)."""
    prism_client.ensure_collections()
    prism_client.ensure_collections()  # Second call should be fine


def test_index_and_search(prism_client):
    """Index sample messages and verify search returns results."""
    from datetime import datetime, timezone
    from agentimport.models import NormalizedMessage

    prism_client.ensure_collections()

    # Create test messages with unique text to avoid collisions
    tag = uuid.uuid4().hex[:8]
    messages = [
        NormalizedMessage(
            conversation_id=f"test-conv-{tag}",
            native_msg_id=f"msg-{tag}-1",
            source="claude_code",
            role="user",
            content_type="message",
            text=f"How do I fix the auth bug in module {tag}?",
            ts=datetime.now(timezone.utc),
            seq=0,
            project="test-project",
            model="claude-4",
            source_path="/tmp/test.jsonl",
        ),
        NormalizedMessage(
            conversation_id=f"test-conv-{tag}",
            native_msg_id=f"msg-{tag}-2",
            source="claude_code",
            role="assistant",
            content_type="message",
            text=f"The auth bug in {tag} is caused by an expired token.",
            ts=datetime.now(timezone.utc),
            seq=1,
            project="test-project",
            model="claude-4",
            source_path="/tmp/test.jsonl",
        ),
    ]

    indexed = prism_client.upsert_messages(messages)
    assert indexed == 2

    # Wait for indexing
    time.sleep(1)

    # Search should find our messages
    results = prism_client.search_messages(f"auth bug {tag}", limit=5)
    assert results.get("total", 0) > 0

    # Re-index same messages (idempotent)
    indexed2 = prism_client.upsert_messages(messages)
    assert indexed2 == 2


def test_stats(prism_client):
    """Stats should return collection info."""
    prism_client.ensure_collections()
    stats = prism_client.get_stats()
    assert "agent_messages" in stats
    assert "agent_conversations" in stats
