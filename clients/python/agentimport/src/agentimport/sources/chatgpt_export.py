"""ChatGPT export adapter — parses ChatGPT data export .zip files."""

from __future__ import annotations

import json
import logging
import zipfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Iterator

from agentimport.models import NormalizedMessage

logger = logging.getLogger(__name__)


class ChatGPTExportSource:
    """Parse ChatGPT data export ZIP files.

    ChatGPT exports come as a .zip containing:
      - conversations.json: array of conversation objects
      - Each conversation has a `mapping` dict with message nodes
      - Nodes have parent/children pointers forming a tree
      - `current_node` points to the last message in the main branch
    """

    @property
    def name(self) -> str:
        return "chatgpt"

    def default_roots(self) -> list[Path]:
        home = Path.home()
        downloads = home / "Downloads"
        return [downloads] if downloads.exists() else []

    def discover(self, roots: list[Path]) -> Iterable[Path]:
        for root in roots:
            if not root.exists():
                continue
            if root.is_file() and root.suffix == ".zip":
                yield root
            else:
                yield from sorted(root.rglob("*.zip"))
                # Also handle pre-extracted directories
                for conversations_json in sorted(root.rglob("conversations.json")):
                    yield conversations_json

    def parse(self, path: Path) -> Iterator[NormalizedMessage]:
        if path.suffix == ".zip":
            yield from self._parse_zip(path)
        elif path.name == "conversations.json":
            yield from self._parse_conversations_json(path, path)

    def _parse_zip(self, zip_path: Path) -> Iterator[NormalizedMessage]:
        try:
            with zipfile.ZipFile(zip_path, "r") as zf:
                conv_files = [name for name in zf.namelist() if name == "conversations.json" or (name.startswith("conversations-") and name.endswith(".json"))]
                if not conv_files:
                    logger.warning("No conversations*.json in %s", zip_path)
                    return
                for name in sorted(conv_files):
                    with zf.open(name) as f:
                        conversations = json.loads(f.read())
                        yield from self._process_conversations(conversations, zip_path)
        except (zipfile.BadZipFile, json.JSONDecodeError, OSError) as e:
            logger.warning("Failed to parse %s: %s", zip_path, e)

    def _parse_conversations_json(self, json_path: Path, source_path: Path) -> Iterator[NormalizedMessage]:
        try:
            conversations = json.loads(json_path.read_text(encoding="utf-8"))
            yield from self._process_conversations(conversations, source_path)
        except (json.JSONDecodeError, OSError) as e:
            logger.warning("Failed to parse %s: %s", json_path, e)

    def _process_conversations(self, conversations: list, source_path: Path) -> Iterator[NormalizedMessage]:
        for conv in conversations:
            if not isinstance(conv, dict):
                continue
            yield from self._parse_conversation(conv, source_path)

    def _parse_conversation(self, conv: dict, source_path: Path) -> Iterator[NormalizedMessage]:
        conv_id = conv.get("conversation_id", conv.get("id", "unknown"))
        title = conv.get("title", "")
        model_slug = conv.get("default_model_slug")
        mapping = conv.get("mapping", {})

        if not mapping:
            return

        # Linearize the tree: walk from current_node back to root, then reverse
        current_node_id = conv.get("current_node")
        if not current_node_id:
            # Fall back: find leaf nodes (nodes with no children, or children not in mapping)
            current_node_id = self._find_leaf(mapping)

        if not current_node_id:
            return

        # Walk backward from current_node to root
        chain = []
        node_id = current_node_id
        while node_id and node_id in mapping:
            chain.append(mapping[node_id])
            node_id = mapping[node_id].get("parent")

        chain.reverse()  # Now root-to-leaf order

        seq = 0
        for node in chain:
            message = node.get("message")
            if not message:
                continue

            author = message.get("author", {})
            role = author.get("role", "unknown")
            if role == "system" and not message.get("content", {}).get("parts"):
                continue  # Skip empty system messages

            content = message.get("content", {})
            content_type_raw = content.get("content_type", "text")
            parts = content.get("parts", [])

            ts = None
            create_time = message.get("create_time")
            if create_time is not None:
                try:
                    ts = datetime.fromtimestamp(float(create_time), tz=timezone.utc)
                except (ValueError, TypeError, OSError):
                    pass

            msg_model = message.get("metadata", {}).get("model_slug", model_slug)
            msg_id = message.get("id")

            # Determine content_type
            if content_type_raw == "code":
                ct = "tool_call"
            elif role == "tool":
                ct = "tool_result"
            else:
                ct = "message"

            # Extract text from parts
            text_parts = []
            for part in parts:
                if isinstance(part, str):
                    text_parts.append(part)
                elif isinstance(part, dict):
                    # Image or other media — record as reference
                    if "asset_pointer" in part:
                        text_parts.append(f"[attachment: {part.get('asset_pointer', '')}]")
                    elif "text" in part:
                        text_parts.append(part["text"])

            text = "\n".join(text_parts).strip()
            if not text:
                continue

            yield NormalizedMessage(
                conversation_id=conv_id,
                native_msg_id=msg_id,
                source="chatgpt",
                role=role,
                content_type=ct,
                text=text,
                ts=ts,
                seq=seq,
                model=msg_model,
                source_path=str(source_path),
            )
            seq += 1

    @staticmethod
    def _find_leaf(mapping: dict) -> str | None:
        """Find a leaf node in the mapping tree (node with no children in the mapping)."""
        all_children = set()
        for node in mapping.values():
            for child_id in node.get("children", []):
                all_children.add(child_id)

        # Find nodes that are not children of any other node — those are roots
        # We want leaves: nodes whose children are all outside the mapping
        for node_id, node in mapping.items():
            children = node.get("children", [])
            if not children or all(c not in mapping for c in children):
                # This is a leaf or a node with no mapped children
                if node.get("message"):
                    return node_id
        return None
