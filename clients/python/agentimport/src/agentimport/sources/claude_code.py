"""Claude Code adapter — parses ~/.claude/projects/**/*.jsonl."""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Iterator

from agentimport.models import NormalizedMessage

logger = logging.getLogger(__name__)


class ClaudeCodeSource:
    """Parse Claude Code conversation JSONL files.

    Claude Code stores conversations as JSONL in ~/.claude/projects/<project-hash>/*.jsonl.
    Each line is a JSON object representing a conversation event:
      - type: "human" | "assistant"
      - message: {role, content: [{type: "text", text: "..."} | {type: "tool_use", ...} | ...]}
      - cwd: working directory (project context)
      - sessionId: session identifier
      - timestamp: ISO 8601 timestamp
      - uuid: unique message ID
    """

    @property
    def name(self) -> str:
        return "claude_code"

    def default_roots(self) -> list[Path]:
        return [Path.home() / ".claude" / "projects"]

    def discover(self, roots: list[Path]) -> Iterable[Path]:
        """Find all .jsonl files under the given roots."""
        for root in roots:
            if not root.exists():
                logger.debug("Claude Code root %s does not exist, skipping", root)
                continue
            yield from sorted(root.rglob("*.jsonl"))

    def parse(self, path: Path) -> Iterator[NormalizedMessage]:
        """Parse a JSONL file into normalized messages."""
        session_id = path.stem  # filename without extension is typically the session ID
        project = self._extract_project(path)
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

                yield from self._parse_event(event, session_id, project, path, seq)
                seq += 1

    def _parse_event(
        self,
        event: dict,
        session_id: str,
        project: str | None,
        path: Path,
        base_seq: int,
    ) -> Iterator[NormalizedMessage]:
        """Parse a single JSONL event into one or more NormalizedMessages."""
        # Extract conversation/session ID
        conv_id = event.get("sessionId", session_id)

        # Extract timestamp
        ts = None
        ts_raw = event.get("timestamp")
        if ts_raw:
            try:
                ts = datetime.fromisoformat(ts_raw)
                if ts.tzinfo is None:
                    ts = ts.replace(tzinfo=timezone.utc)
            except (ValueError, TypeError):
                pass

        # Extract model
        model = event.get("model")

        # Extract project from cwd if available
        event_project = event.get("cwd") or project

        # Get message content
        message = event.get("message", {})
        role = message.get("role", event.get("type", "unknown"))
        if role == "human":
            role = "user"

        msg_id = event.get("uuid")
        content_blocks = message.get("content", [])

        # Handle string content (simple text message)
        if isinstance(content_blocks, str):
            yield NormalizedMessage(
                conversation_id=conv_id,
                native_msg_id=msg_id,
                source="claude_code",
                role=role,
                content_type="message",
                text=content_blocks,
                ts=ts,
                seq=base_seq,
                project=event_project,
                model=model,
                source_path=str(path),
            )
            return

        # Handle list of content blocks
        sub_seq = 0
        for block in content_blocks:
            if not isinstance(block, dict):
                continue

            block_type = block.get("type", "")

            if block_type == "text":
                text = block.get("text", "")
                if not text.strip():
                    continue
                yield NormalizedMessage(
                    conversation_id=conv_id,
                    native_msg_id=f"{msg_id}:{sub_seq}" if msg_id else None,
                    source="claude_code",
                    role=role,
                    content_type="message",
                    text=text,
                    ts=ts,
                    seq=base_seq * 100 + sub_seq,
                    project=event_project,
                    model=model,
                    source_path=str(path),
                )
                sub_seq += 1

            elif block_type == "tool_use":
                tool_name = block.get("name", "unknown")
                # Include tool name + input as lightweight record
                tool_input = block.get("input", {})
                text = f"Tool: {tool_name}\nInput: {json.dumps(tool_input, indent=2, default=str)}"
                yield NormalizedMessage(
                    conversation_id=conv_id,
                    native_msg_id=block.get("id") or (f"{msg_id}:{sub_seq}" if msg_id else None),
                    source="claude_code",
                    role="assistant",
                    content_type="tool_call",
                    text=text,
                    tool_name=tool_name,
                    ts=ts,
                    seq=base_seq * 100 + sub_seq,
                    project=event_project,
                    model=model,
                    source_path=str(path),
                )
                sub_seq += 1

            elif block_type == "tool_result":
                content = block.get("content", "")
                if isinstance(content, list):
                    # tool_result content can be a list of blocks
                    text_parts = []
                    for part in content:
                        if isinstance(part, dict) and part.get("type") == "text":
                            text_parts.append(part.get("text", ""))
                        elif isinstance(part, str):
                            text_parts.append(part)
                    content = "\n".join(text_parts)
                yield NormalizedMessage(
                    conversation_id=conv_id,
                    native_msg_id=block.get("tool_use_id") or (f"{msg_id}:{sub_seq}" if msg_id else None),
                    source="claude_code",
                    role="tool",
                    content_type="tool_result",
                    text=str(content),
                    tool_name=block.get("tool_use_id"),
                    ts=ts,
                    seq=base_seq * 100 + sub_seq,
                    project=event_project,
                    model=model,
                    source_path=str(path),
                )
                sub_seq += 1

            elif block_type == "thinking":
                text = block.get("thinking", "") or block.get("text", "")
                if text.strip():
                    yield NormalizedMessage(
                        conversation_id=conv_id,
                        native_msg_id=f"{msg_id}:{sub_seq}" if msg_id else None,
                        source="claude_code",
                        role="assistant",
                        content_type="thinking",
                        text=text,
                        ts=ts,
                        seq=base_seq * 100 + sub_seq,
                        project=event_project,
                        model=model,
                        source_path=str(path),
                    )
                    sub_seq += 1

    @staticmethod
    def _extract_project(path: Path) -> str | None:
        """Try to extract a project name from the file path.

        Claude Code stores files in ~/.claude/projects/<hash>/<session>.jsonl.
        The hash maps to a project directory, but we can't reverse it here.
        Return the hash as is — the cwd from messages will give the real path.
        """
        parts = path.parts
        try:
            idx = parts.index("projects")
            if idx + 1 < len(parts):
                return parts[idx + 1]
        except ValueError:
            pass
        return None
