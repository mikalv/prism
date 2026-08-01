"""Gemini CLI adapter — parses ~/.gemini/ conversation history."""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Iterator

from agentimport.models import NormalizedMessage

logger = logging.getLogger(__name__)


class GeminiSource:
    """Parse Gemini CLI conversation files.

    Gemini CLI stores conversations in ~/.gemini/history/ or similar
    directories as JSON/JSONL files.
    """

    @property
    def name(self) -> str:
        return "gemini"

    def default_roots(self) -> list[Path]:
        return [Path.home() / ".gemini"]

    def discover(self, roots: list[Path]) -> Iterable[Path]:
        for root in roots:
            if not root.exists():
                continue
            for pattern in ["**/*.json", "**/*.jsonl"]:
                for path in sorted(root.rglob(pattern)):
                    # Skip config files, only want conversation data
                    if path.name in ("settings.json", "config.json", "gemini.json"):
                        continue
                    yield path

    def parse(self, path: Path) -> Iterator[NormalizedMessage]:
        conv_id = path.stem
        seq = 0

        # Try JSONL first (one JSON object per line)
        if path.suffix == ".jsonl":
            yield from self._parse_jsonl(path, conv_id)
            return

        try:
            data = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        except (json.JSONDecodeError, OSError) as e:
            logger.warning("Failed to parse %s: %s", path, e)
            return

        # Handle Gemini's format
        messages = data if isinstance(data, list) else data.get("messages", data.get("history", []))
        model = data.get("model") if isinstance(data, dict) else None
        project = data.get("cwd", data.get("context", {}).get("cwd")) if isinstance(data, dict) else None

        for item in messages:
            if not isinstance(item, dict):
                continue

            role = item.get("role", "unknown")
            # Gemini uses "model" for assistant role
            if role == "model":
                role = "assistant"

            # Gemini content can be parts array
            parts = item.get("parts", [])
            if not parts:
                content = item.get("content", item.get("text", ""))
                if isinstance(content, str) and content.strip():
                    yield NormalizedMessage(
                        conversation_id=conv_id,
                        source="gemini",
                        role=role,
                        content_type="message",
                        text=content,
                        ts=self._parse_ts(item),
                        seq=seq,
                        project=project,
                        model=model,
                        source_path=str(path),
                    )
                    seq += 1
                continue

            for part in parts:
                if isinstance(part, str):
                    text = part
                elif isinstance(part, dict):
                    if "text" in part:
                        text = part["text"]
                    elif "functionCall" in part:
                        fc = part["functionCall"]
                        text = f"Tool: {fc.get('name', 'unknown')}\nArgs: {json.dumps(fc.get('args', {}), default=str)}"
                        yield NormalizedMessage(
                            conversation_id=conv_id,
                            source="gemini",
                            role="assistant",
                            content_type="tool_call",
                            text=text,
                            tool_name=fc.get("name"),
                            ts=self._parse_ts(item),
                            seq=seq,
                            project=project,
                            model=model,
                            source_path=str(path),
                        )
                        seq += 1
                        continue
                    elif "functionResponse" in part:
                        fr = part["functionResponse"]
                        text = f"Result: {json.dumps(fr.get('response', {}), default=str)}"
                        yield NormalizedMessage(
                            conversation_id=conv_id,
                            source="gemini",
                            role="tool",
                            content_type="tool_result",
                            text=text,
                            tool_name=fr.get("name"),
                            ts=self._parse_ts(item),
                            seq=seq,
                            project=project,
                            model=model,
                            source_path=str(path),
                        )
                        seq += 1
                        continue
                    elif "thought" in part:
                        text = part["thought"]
                        yield NormalizedMessage(
                            conversation_id=conv_id,
                            source="gemini",
                            role="assistant",
                            content_type="thinking",
                            text=text,
                            ts=self._parse_ts(item),
                            seq=seq,
                            project=project,
                            model=model,
                            source_path=str(path),
                        )
                        seq += 1
                        continue
                    else:
                        continue
                else:
                    continue

                if text.strip():
                    yield NormalizedMessage(
                        conversation_id=conv_id,
                        source="gemini",
                        role=role,
                        content_type="message",
                        text=text,
                        ts=self._parse_ts(item),
                        seq=seq,
                        project=project,
                        model=model,
                        source_path=str(path),
                    )
                    seq += 1

    def _parse_jsonl(self, path: Path, conv_id: str) -> Iterator[NormalizedMessage]:
        seq = 0
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    item = json.loads(line)
                except json.JSONDecodeError:
                    continue

                role = item.get("role", "unknown")
                if role == "model":
                    role = "assistant"
                content = item.get("content", item.get("text", ""))
                if isinstance(content, str) and content.strip():
                    yield NormalizedMessage(
                        conversation_id=conv_id,
                        source="gemini",
                        role=role,
                        content_type="message",
                        text=content,
                        ts=self._parse_ts(item),
                        seq=seq,
                        model=item.get("model"),
                        source_path=str(path),
                    )
                    seq += 1

    @staticmethod
    def _parse_ts(item: dict) -> datetime | None:
        for key in ("timestamp", "createTime", "create_time", "ts"):
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
