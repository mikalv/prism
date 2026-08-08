"""PI adapter — parses ~/.pi/agent/sessions/**/*.jsonl.

PI (pi.dev) is an LLM harness that stores each session as a JSONL event
stream, one directory per project. The format is close to Claude Code:

  {"type":"session","id":"<uuid>","cwd":"/path","timestamp":"ISO"}
  {"type":"model_change","provider":"google","modelId":"gemini-2.0-flash"}
  {"type":"message","id":"..","parentId":"..","timestamp":"ISO",
   "message":{"role":"user"|"assistant","content":[{"type":"text","text":".."}],
              "model":"..","provider":"..","usage":{..},"timestamp":<epoch ms>}}
  {"type":"custom",...}   # extension state — ignored
"""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Iterator

from agentimport.models import NormalizedMessage

logger = logging.getLogger(__name__)


class PiSource:
    """Parse PI (pi.dev) session JSONL files."""

    @property
    def name(self) -> str:
        return "pi"

    def default_roots(self) -> list[Path]:
        return [Path.home() / ".pi" / "agent" / "sessions"]

    def discover(self, roots: list[Path]) -> Iterable[Path]:
        for root in roots:
            if not root.exists():
                logger.debug("PI root %s does not exist, skipping", root)
                continue
            yield from sorted(root.rglob("*.jsonl"))

    def parse(self, path: Path) -> Iterator[NormalizedMessage]:
        conv_id = path.stem  # <ts>_<uuid>; overridden by session header if present
        project: str | None = None
        model: str | None = None
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

                etype = event.get("type")

                if etype == "session":
                    conv_id = event.get("id", conv_id)
                    project = event.get("cwd") or project
                    continue

                if etype == "model_change":
                    mid = event.get("modelId")
                    provider = event.get("provider")
                    model = f"{provider}/{mid}" if provider and mid else (mid or model)
                    continue

                if etype != "message":
                    continue  # thinking_level_change, custom, etc.

                message = event.get("message", {})
                yield from self._parse_message(
                    message, event, conv_id, project, model, path, seq
                )
                seq += 1

    def _parse_message(
        self,
        message: dict,
        event: dict,
        conv_id: str,
        project: str | None,
        default_model: str | None,
        path: Path,
        base_seq: int,
    ) -> Iterator[NormalizedMessage]:
        role = message.get("role", "unknown")
        msg_id = event.get("id")
        ts = _parse_ts(event.get("timestamp")) or _parse_epoch_ms(message.get("timestamp"))

        model = default_model
        mprov, mmodel = message.get("provider"), message.get("model")
        if mmodel:
            model = f"{mprov}/{mmodel}" if mprov else mmodel

        content = message.get("content", [])

        if isinstance(content, str):
            if content.strip():
                yield NormalizedMessage(
                    conversation_id=conv_id, native_msg_id=msg_id, source="pi",
                    role=role, content_type="message", text=content, ts=ts,
                    seq=base_seq * 100, project=project, model=model,
                    source_path=str(path),
                )
            return

        sub = 0
        for block in content:
            if not isinstance(block, dict):
                continue
            btype = block.get("type", "")

            if btype == "text":
                text = block.get("text", "")
                if not text.strip():
                    continue
                yield NormalizedMessage(
                    conversation_id=conv_id,
                    native_msg_id=f"{msg_id}:{sub}" if msg_id else None,
                    source="pi", role=role, content_type="message", text=text,
                    ts=ts, seq=base_seq * 100 + sub, project=project, model=model,
                    source_path=str(path),
                )
                sub += 1

            elif btype in ("tool_use", "tool_call"):
                tool_name = block.get("name") or block.get("toolName") or "unknown"
                tool_input = block.get("input", block.get("arguments", {}))
                text = f"Tool: {tool_name}\nInput: {json.dumps(tool_input, indent=2, default=str)}"
                yield NormalizedMessage(
                    conversation_id=conv_id,
                    native_msg_id=block.get("id") or (f"{msg_id}:{sub}" if msg_id else None),
                    source="pi", role="assistant", content_type="tool_call", text=text,
                    tool_name=tool_name, ts=ts, seq=base_seq * 100 + sub,
                    project=project, model=model, source_path=str(path),
                )
                sub += 1

            elif btype == "tool_result":
                content_val = block.get("content", block.get("output", ""))
                text = _flatten_content(content_val)
                yield NormalizedMessage(
                    conversation_id=conv_id,
                    native_msg_id=block.get("tool_use_id") or (f"{msg_id}:{sub}" if msg_id else None),
                    source="pi", role="tool", content_type="tool_result", text=text,
                    tool_name=block.get("tool_use_id"), ts=ts, seq=base_seq * 100 + sub,
                    project=project, model=model, source_path=str(path),
                )
                sub += 1

            elif btype in ("thinking", "reasoning"):
                text = block.get("thinking") or block.get("text") or ""
                if text.strip():
                    yield NormalizedMessage(
                        conversation_id=conv_id,
                        native_msg_id=f"{msg_id}:{sub}" if msg_id else None,
                        source="pi", role="assistant", content_type="thinking", text=text,
                        ts=ts, seq=base_seq * 100 + sub, project=project, model=model,
                        source_path=str(path),
                    )
                    sub += 1


def _parse_ts(raw: object) -> datetime | None:
    if not isinstance(raw, str):
        return None
    try:
        ts = datetime.fromisoformat(raw.replace("Z", "+00:00"))
        return ts if ts.tzinfo else ts.replace(tzinfo=timezone.utc)
    except (ValueError, TypeError):
        return None


def _parse_epoch_ms(raw: object) -> datetime | None:
    if not isinstance(raw, (int, float)):
        return None
    try:
        return datetime.fromtimestamp(raw / 1000, tz=timezone.utc)
    except (ValueError, OSError):
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
