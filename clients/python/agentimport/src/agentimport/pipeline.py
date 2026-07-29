"""Import pipeline — orchestrates source scanning, filtering, and Prism indexing."""

from __future__ import annotations

import logging
from collections import defaultdict
from datetime import datetime
from typing import TYPE_CHECKING

from agentimport.models import ContentFilter, NormalizedConversation, NormalizedMessage
from agentimport.prism import PrismClient
from agentimport.sources.base import Source, get_all_sources, get_source_by_name
from agentimport.state import StateDB

if TYPE_CHECKING:
    from agentimport.config import Config

logger = logging.getLogger(__name__)


class ImportStats:
    """Accumulator for import run statistics."""

    def __init__(self) -> None:
        self.files_scanned = 0
        self.files_imported = 0
        self.files_skipped = 0
        self.messages_indexed = 0
        self.conversations_indexed = 0
        self.errors = 0

    def __repr__(self) -> str:
        return (
            f"ImportStats(scanned={self.files_scanned}, imported={self.files_imported}, "
            f"skipped={self.files_skipped}, messages={self.messages_indexed}, "
            f"conversations={self.conversations_indexed}, errors={self.errors})"
        )


class Pipeline:
    """Orchestrates the full import pipeline:

    sources → discover files → state check → parse → filter → batch → Prism index
    """

    def __init__(self, config: Config) -> None:
        self.config = config
        self.content_filter = ContentFilter(
            include_tool_results=config.include_tool_results,
            include_thinking=config.include_thinking,
            roles=config.roles,
            max_chars=config.max_chars,
        )

    def run(self, source_names: list[str] | None = None) -> ImportStats:
        """Run a full import cycle. Returns statistics."""
        stats = ImportStats()

        # Resolve sources
        if source_names:
            sources = []
            for name in source_names:
                src = get_source_by_name(name)
                if src:
                    sources.append(src)
                else:
                    logger.warning("Unknown source: %s", name)
            if not sources:
                logger.error("No valid sources specified")
                return stats
        else:
            sources = [
                s for s in get_all_sources()
                if s.name in self.config.get_enabled_sources()
            ]

        with (
            StateDB(self.config.state_db) as state,
            PrismClient(self.config.prism_url, self.config.prism_api_key) as prism,
        ):
            # Ensure collections exist
            prism.ensure_collections()

            # Track all messages for conversation aggregation
            conv_messages: dict[str, list[NormalizedMessage]] = defaultdict(list)

            for source in sources:
                logger.info("Processing source: %s", source.name)
                source_cfg = self.config.sources.get(source.name)
                roots = source_cfg.roots if source_cfg and source_cfg.roots else source.default_roots()

                for path in source.discover(roots):
                    stats.files_scanned += 1

                    if not state.should_import(path):
                        stats.files_skipped += 1
                        continue

                    try:
                        messages = list(source.parse(path))
                    except Exception:
                        logger.exception("Error parsing %s", path)
                        stats.errors += 1
                        continue

                    # Apply content filters
                    filtered = []
                    for msg in messages:
                        if self.content_filter.should_include(msg):
                            filtered.append(self.content_filter.apply_limits(msg))

                    if filtered:
                        # Index messages
                        indexed = prism.upsert_messages(filtered, self.config.batch_size)
                        stats.messages_indexed += indexed

                        # Track for conversation aggregation
                        for msg in filtered:
                            key = f"{msg.source}:{msg.conversation_id}"
                            conv_messages[key].append(msg)

                    state.mark_imported(path, source.name)
                    stats.files_imported += 1
                    logger.info("Imported %s: %d messages from %s", source.name, len(filtered), path.name)

            # Aggregate and index conversation metadata
            if conv_messages:
                conversations = self._aggregate_conversations(conv_messages)
                indexed = prism.upsert_conversations(conversations, self.config.batch_size)
                stats.conversations_indexed = indexed

        logger.info("Import complete: %s", stats)
        return stats

    @staticmethod
    def _aggregate_conversations(
        conv_messages: dict[str, list[NormalizedMessage]],
    ) -> list[NormalizedConversation]:
        """Build conversation metadata from collected messages."""
        conversations = []
        for _key, messages in conv_messages.items():
            if not messages:
                continue

            first = messages[0]
            timestamps = [m.ts for m in messages if m.ts]
            projects = [m.project for m in messages if m.project]
            models = [m.model for m in messages if m.model]

            # Try to extract a title from the first user message
            title = None
            for m in messages:
                if m.role == "user" and m.content_type == "message":
                    title = m.text[:200].split("\n")[0]  # First line, max 200 chars
                    break

            conversations.append(NormalizedConversation(
                conversation_id=first.conversation_id,
                source=first.source,
                title=title,
                project=projects[0] if projects else None,
                model=models[0] if models else None,
                started_at=min(timestamps) if timestamps else None,
                ended_at=max(timestamps) if timestamps else None,
                msg_count=len(messages),
                source_path=first.source_path,
            ))

        return conversations
