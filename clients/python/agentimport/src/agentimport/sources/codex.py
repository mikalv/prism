"""Codex CLI adapter — parses ~/.codex-cli/ conversation logs."""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Iterator

from agentimport.models import NormalizedMessage

logger = logging.getLogger(__name__)


class CodexSource:
    """Parse OpenAI Codex CLI conversation files.

    Codex CLI stores conversations in ~/.codex-cli/conversations/ as JSON files.
    Each file represents a complete conversation with an array of messages.
    """

    @property
    def name(self) -> str:
        return "codex"

    def default_roots(self) -> list[Path]:
        return [Path.home() / ".codex-cli"]

    def discover(self, roots: list[Path]) -> Iterable[Path]:
        for root in roots:
            if not root.exists():
                continue
            # Look for JSON/JSONL files in conversations subdirs
            for pattern in ["**/*.json", "**/*.jsonl"]:
                yield from sorted(root.rglob(pattern))

    def parse(self, path: Path) -> Iterator[NormalizedMessage]:
        conv_id = path.stem
        seq = 0

        try:
            data = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        except (json.JSONDecodeError, OSError) as e:
            logger.warning("Failed to parse %s: %s", path, e)
            return

        # Handle both array-of-messages and object-with-messages formats
        messages = data if isinstance(data, list) else data.get("messages", data.get("items", []))
        model = data.get("model") if isinstance(data, dict) else None
        project = data.get("cwd") if isinstance(data, dict) else None

        for item in messages:
            if not isinstance(item, dict):
                continue

            role = item.get("role", "unknown")
            content = item.get("content", "")
            if isinstance(content, list):
                # Content blocks format
                for block in content:
                    if isinstance(block, dict):
                        block_type = block.get("type", "text")
                        if block_type == "text":
                            text = block.get("text", "")
                        elif block_type in ("tool_use", "function_call"):
                            text = f"Tool: {block.get('name', 'unknown')}\nInput: {json.dumps(block.get('input', block.get('arguments', {})), default=str)}"
                            yield NormalizedMessage(
                                conversation_id=conv_id,
                                source="codex",
                                role=role,
                                content_type="tool_call",
                                text=text,
                                tool_name=block.get("name"),
                                ts=self._parse_ts(item),
                                seq=seq,
                                project=project,
                                model=model or item.get("model"),
                                source_path=str(path),
                            )
                            seq += 1
                            continue
                        else:
                            continue
                    elif isinstance(block, str):
                        text = block
                    else:
                        continue

                    if text.strip():
                        yield NormalizedMessage(
                            conversation_id=conv_id,
                            source="codex",
                            role=role,
                            content_type="message",
                            text=text,
                            ts=self._parse_ts(item),
                            seq=seq,
                            project=project,
                            model=model or item.get("model"),
                            source_path=str(path),
                        )
                        seq += 1
            elif isinstance(content, str) and content.strip():
                yield NormalizedMessage(
                    conversation_id=conv_id,
                    source="codex",
                    role=role,
                    content_type="message",
                    text=content,
                    ts=self._parse_ts(item),
                    seq=seq,
                    project=project,
                    model=model or item.get("model"),
                    source_path=str(path),
                )
                seq += 1

    @staticmethod
    def _parse_ts(item: dict) -> datetime | None:
        for key in ("timestamp", "created_at", "ts"):
            val = item.get(key)
            if val is None:
                continue
            if isinstance(val, (int, float)):
                return datetime.fromtimestamp(val, tz=timezone.utc)
            try:
                dt = datetime.fromisoformat(str(val))
                return dt if dt.tzinfo else dt.replace(tzinfo=timezone.utc)
            except (ValueError, TypeError):
                pass
        return None
