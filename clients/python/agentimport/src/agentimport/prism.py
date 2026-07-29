"""Prism integration layer — schema management and document indexing."""

from __future__ import annotations

import json
import logging
from pathlib import Path
from typing import TYPE_CHECKING

from prismsearch import Prismsearch
from prismsearch.models import PrismError

if TYPE_CHECKING:
    from agentimport.models import NormalizedConversation, NormalizedMessage

logger = logging.getLogger(__name__)

SCHEMAS_DIR = Path(__file__).parent.parent.parent / "schemas"

COLLECTION_MESSAGES = "agent_messages"
COLLECTION_CONVERSATIONS = "agent_conversations"


class PrismClient:
    """Thin wrapper over prismsearch for agentimport-specific operations."""

    def __init__(self, base_url: str = "http://localhost:3080", api_key: str | None = None) -> None:
        self._client = Prismsearch(base_url, api_key=api_key)

    def close(self) -> None:
        self._client.close()

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    def ensure_collections(self) -> None:
        """Create agent_messages and agent_conversations collections if they don't exist."""
        existing = set(self._client.list_collections())

        for schema_file, collection in [
            ("messages.json", COLLECTION_MESSAGES),
            ("conversations.json", COLLECTION_CONVERSATIONS),
        ]:
            if collection in existing:
                logger.info("Collection %s already exists, skipping", collection)
                continue

            schema_path = SCHEMAS_DIR / schema_file
            if not schema_path.exists():
                raise FileNotFoundError(f"Schema file not found: {schema_path}")

            schema = json.loads(schema_path.read_text())
            logger.info("Creating collection %s from %s", collection, schema_file)
            self._client.create_collection(collection, schema)

    def upsert_messages(self, messages: list[NormalizedMessage], batch_size: int = 100) -> int:
        """Index normalized messages into Prism. Returns count of indexed docs."""
        docs = [msg.to_prism_doc() for msg in messages]
        return self._batch_index(COLLECTION_MESSAGES, docs, batch_size)

    def upsert_conversations(self, conversations: list[NormalizedConversation], batch_size: int = 100) -> int:
        """Index conversation metadata into Prism. Returns count of indexed docs."""
        docs = [conv.to_prism_doc() for conv in conversations]
        return self._batch_index(COLLECTION_CONVERSATIONS, docs, batch_size)

    def _batch_index(self, collection: str, docs: list[dict], batch_size: int) -> int:
        """Index documents in batches."""
        total = 0
        for i in range(0, len(docs), batch_size):
            batch = docs[i : i + batch_size]
            try:
                result = self._client.index(collection, batch)
                total += result.indexed
                if result.failed > 0:
                    logger.warning(
                        "Batch %d: %d indexed, %d failed: %s",
                        i // batch_size,
                        result.indexed,
                        result.failed,
                        result.errors,
                    )
            except PrismError as e:
                logger.error("Batch %d failed: %s", i // batch_size, e)
                raise
        return total

    def search_messages(self, query: str, limit: int = 10, **filters) -> dict:
        """Quick search across agent messages."""
        body: dict = {"query": query, "limit": limit, "fields": ["text", "source", "role", "project", "model", "conversation_id", "content_type"]}
        if filters:
            filter_clauses = []
            for field, value in filters.items():
                filter_clauses.append({"field": field, "value": value})
            body["filter"] = {"must": filter_clauses}
        _, data = self._client._post(f"/collections/{COLLECTION_MESSAGES}/search", json=body)
        return data

    def get_stats(self) -> dict:
        """Get collection stats for both agent collections."""
        stats = {}
        for collection in [COLLECTION_MESSAGES, COLLECTION_CONVERSATIONS]:
            try:
                s = self._client.get_stats(collection)
                stats[collection] = {"documents": s.document_count, "storage_bytes": s.storage_bytes}
            except PrismError:
                stats[collection] = {"documents": 0, "storage_bytes": 0}
        return stats
