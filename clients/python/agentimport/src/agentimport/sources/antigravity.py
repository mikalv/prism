"""Antigravity (Google Gemini Agent) adapter — parses ~/.gemini/antigravity/brain/*/ .system_generated/logs/transcript.jsonl."""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Iterator

from agentimport.models import NormalizedMessage

logger = logging.getLogger(__name__)


class AntigravitySource:
    """Parse Antigravity (Google Gemini Agent) transcript JSONL files.

    Antigravity stores session logs in:
    ~/.gemini/antigravity/brain/<session-id>/.system_generated/logs/transcript.jsonl
    """

    @property
    def name(self) -> str:
        return "antigravity"

    def default_roots(self) -> list[Path]:
        return [Path.home() / ".gemini" / "antigravity" / "brain"]

    def discover(self, roots: list[Path]) -> Iterable[Path]:
        for root in roots:
            if not root.exists():
                continue
            for path in sorted(root.glob("*/.system_generated/logs/transcript.jsonl")):
                yield path

    def parse(self, path: Path) -> Iterator[NormalizedMessage]:
        session_id = path.parts[-4] if len(path.parts) >= 4 else path.stem
        seq = 0

        with open(path, encoding="utf-8", errors="replace") as f:
            for line_num, line in enumerate(f, 1):
                line = line.strip()
                if not line:
                    continue

                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    logger.warning("Skipping malformed JSON at %s:%d", path, line_num)
                    continue

                yield from self._parse_event(event, session_id, path, seq)
                seq += 1

    def _parse_event(
        self,
        event: dict,
        session_id: str,
        path: Path,
        seq: int,
    ) -> Iterator[NormalizedMessage]:
        event_type = event.get("type", "")
        if event_type in ("CONVERSATION_HISTORY", "CHECKPOINT"):
            return

        ts = None
        ts_raw = event.get("created_at")
        if ts_raw:
            try:
                ts = datetime.fromisoformat(ts_raw)
                if ts.tzinfo is None:
                    ts = ts.replace(tzinfo=timezone.utc)
            except (ValueError, TypeError):
                pass

        if event_type == "USER_INPUT":
            content = event.get("content", "")
            # Extract plain text if wrapped in <USER_REQUEST> tags
            text = self._extract_user_request(content)
            if text.strip():
                yield NormalizedMessage(
                    conversation_id=session_id,
                    source="antigravity",
                    role="user",
                    content_type="message",
                    text=text,
                    ts=ts,
                    seq=seq,
                    source_path=str(path),
                )

        elif event_type == "PLANNER_RESPONSE":
            content = event.get("content")
            if content and isinstance(content, str) and content.strip():
                yield NormalizedMessage(
                    conversation_id=session_id,
                    source="antigravity",
                    role="assistant",
                    content_type="message",
                    text=content,
                    ts=ts,
                    seq=seq,
                    source_path=str(path),
                )

            tool_calls = event.get("tool_calls", [])
            for sub_seq, tc in enumerate(tool_calls):
                if not isinstance(tc, dict):
                    continue
                name = tc.get("name", "unknown")
                args = tc.get("args", {})
                text = f"Tool: {name}\nArgs: {json.dumps(args, indent=2, default=str)}"
                yield NormalizedMessage(
                    conversation_id=session_id,
                    source="antigravity",
                    role="assistant",
                    content_type="tool_call",
                    text=text,
                    tool_name=name,
                    ts=ts,
                    seq=seq * 100 + sub_seq,
                    source_path=str(path),
                )

        else:
            # Tool results: RUN_COMMAND, VIEW_FILE, LIST_DIRECTORY, GREP_SEARCH, etc.
            content = event.get("content", "")
            if isinstance(content, str) and content.strip():
                yield NormalizedMessage(
                    conversation_id=session_id,
                    source="antigravity",
                    role="tool",
                    content_type="tool_result",
                    text=content,
                    tool_name=event_type.lower(),
                    ts=ts,
                    seq=seq,
                    source_path=str(path),
                )

    @staticmethod
    def _extract_user_request(raw_content: str) -> str:
        if "<USER_REQUEST>" in raw_content and "</USER_REQUEST>" in raw_content:
            start = raw_content.find("<USER_REQUEST>") + len("<USER_REQUEST>")
            end = raw_content.find("</USER_REQUEST>")
            return raw_content[start:end].strip()
        return raw_content
