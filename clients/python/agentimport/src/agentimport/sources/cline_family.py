"""Cline-family adapters — Kilo Code, Cline, and Roo Code.

These VS Code extensions share a lineage (Cline → Roo → Kilo) and the same
on-disk layout. Each task lives under:

  <editor>/User/globalStorage/<extension-id>/tasks/<taskId>/
    api_conversation_history.json   # [{role, content:[{type:"text",text}|tool_use|...]}]
    ui_messages.json                # UI event stream (unused here)
    task_metadata.json / history_item.json  # title, timestamps, tokens

`api_conversation_history.json` is the Anthropic messages format; tool calls
appear both as `tool_use`/`tool_result` blocks and inline XML inside text.
The task directory name is an epoch-millisecond timestamp.
"""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Iterator

from agentimport.models import NormalizedMessage

logger = logging.getLogger(__name__)

# VS Code-derived editors that may host these extensions (macOS + Linux).
_EDITOR_DIRS = [
    Path.home() / "Library" / "Application Support" / "Code" / "User" / "globalStorage",
    Path.home() / "Library" / "Application Support" / "Code - Insiders" / "User" / "globalStorage",
    Path.home() / "Library" / "Application Support" / "Cursor" / "User" / "globalStorage",
    Path.home() / "Library" / "Application Support" / "VSCodium" / "User" / "globalStorage",
    Path.home() / "Library" / "Application Support" / "Windsurf" / "User" / "globalStorage",
    Path.home() / ".config" / "Code" / "User" / "globalStorage",
    Path.home() / ".config" / "Code - Insiders" / "User" / "globalStorage",
    Path.home() / ".config" / "Cursor" / "User" / "globalStorage",
    Path.home() / ".config" / "VSCodium" / "User" / "globalStorage",
]


class _ClineFamilyBase:
    """Shared discovery/parsing for Cline-lineage task stores."""

    source_name: str = "cline"
    extension_id: str = "saoudrizwan.claude-dev"

    @property
    def name(self) -> str:
        return self.source_name

    def default_roots(self) -> list[Path]:
        return [d / self.extension_id for d in _EDITOR_DIRS]

    def discover(self, roots: list[Path]) -> Iterable[Path]:
        for root in roots:
            if not root.exists():
                continue
            yield from sorted(root.glob("tasks/*/api_conversation_history.json"))

    def parse(self, path: Path) -> Iterator[NormalizedMessage]:
        task_dir = path.parent
        conv_id = task_dir.name  # epoch-ms task id
        started = _parse_epoch_ms(conv_id)
        project, title = self._read_metadata(task_dir)

        try:
            messages = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        except (json.JSONDecodeError, OSError):
            logger.warning("Skipping unreadable Cline task %s", path)
            return
        if not isinstance(messages, list):
            return

        for base_seq, message in enumerate(messages):
            if not isinstance(message, dict):
                continue
            yield from self._parse_message(
                message, conv_id, project, started, path, base_seq
            )

    def _parse_message(
        self, message: dict, conv_id: str, project: str | None,
        ts: datetime | None, path: Path, base_seq: int,
    ) -> Iterator[NormalizedMessage]:
        role = message.get("role", "unknown")
        content = message.get("content", "")

        if isinstance(content, str):
            if content.strip():
                yield self._msg(conv_id, f"{base_seq}", role, "message", content,
                                None, ts, base_seq * 100, project, path)
            return

        sub = 0
        for block in content:
            if not isinstance(block, dict):
                continue
            btype = block.get("type", "")

            if btype == "text":
                text = block.get("text", "")
                if text.strip():
                    yield self._msg(conv_id, f"{base_seq}:{sub}", role, "message",
                                    text, None, ts, base_seq * 100 + sub, project, path)
                    sub += 1
            elif btype == "tool_use":
                tool_name = block.get("name", "unknown")
                text = f"Tool: {tool_name}\nInput: {json.dumps(block.get('input', {}), indent=2, default=str)}"
                yield self._msg(conv_id, block.get("id") or f"{base_seq}:{sub}", "assistant",
                                "tool_call", text, tool_name, ts, base_seq * 100 + sub, project, path)
                sub += 1
            elif btype == "tool_result":
                text = _flatten_content(block.get("content", ""))
                yield self._msg(conv_id, block.get("tool_use_id") or f"{base_seq}:{sub}", "tool",
                                "tool_result", text, block.get("tool_use_id"), ts,
                                base_seq * 100 + sub, project, path)
                sub += 1

    def _msg(self, conv_id, msg_id, role, content_type, text, tool_name, ts, seq, project, path):
        return NormalizedMessage(
            conversation_id=conv_id, native_msg_id=msg_id, source=self.source_name,
            role=role, content_type=content_type, text=text, tool_name=tool_name,
            ts=ts, seq=seq, project=project, source_path=str(path),
        )

    @staticmethod
    def _read_metadata(task_dir: Path) -> tuple[str | None, str | None]:
        """Best-effort title/project from history_item.json or task_metadata.json."""
        for fname in ("history_item.json", "task_metadata.json"):
            fp = task_dir / fname
            if not fp.exists():
                continue
            try:
                data = json.loads(fp.read_text(encoding="utf-8", errors="replace"))
            except (json.JSONDecodeError, OSError):
                continue
            if isinstance(data, dict):
                title = data.get("task") or data.get("title")
                project = data.get("cwd") or data.get("workspace")
                return project, (title[:200] if isinstance(title, str) else None)
        return None, None


class KiloSource(_ClineFamilyBase):
    source_name = "kilo"
    extension_id = "kilocode.kilo-code"


class ClineSource(_ClineFamilyBase):
    source_name = "cline"
    extension_id = "saoudrizwan.claude-dev"


class RooSource(_ClineFamilyBase):
    source_name = "roo"
    extension_id = "rooveterinaryinc.roo-cline"


def _parse_epoch_ms(raw: str) -> datetime | None:
    try:
        return datetime.fromtimestamp(int(raw) / 1000, tz=timezone.utc)
    except (ValueError, OSError, TypeError):
        return None


def _flatten_content(content: object) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = []
        for part in content:
            if isinstance(part, dict) and part.get("type") == "text":
                parts.append(part.get("text", ""))
            elif isinstance(part, str):
                parts.append(part)
        return "\n".join(parts)
    return str(content)
