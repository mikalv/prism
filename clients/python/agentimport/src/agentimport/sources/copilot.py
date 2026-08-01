"""GitHub Copilot adapter — parses VS Code Copilot chat history."""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Iterator

from agentimport.models import NormalizedMessage

logger = logging.getLogger(__name__)


class CopilotSource:
    """Parse GitHub Copilot / VS Code chat history.

    Copilot Chat stores conversations in VS Code's workspaceStorage or
    ~/.copilot/ directory as JSON files.
    """

    @property
    def name(self) -> str:
        return "copilot"

    def default_roots(self) -> list[Path]:
        home = Path.home()
        roots = [home / ".copilot"]
        # Also check VS Code workspace storage for Copilot chat data
        for vscode_dir in [
            home / ".config" / "Code" / "User" / "workspaceStorage",
            home / "Library" / "Application Support" / "Code" / "User" / "workspaceStorage",
        ]:
            if vscode_dir.exists():
                roots.append(vscode_dir)
        return roots

    def discover(self, roots: list[Path]) -> Iterable[Path]:
        for root in roots:
            if not root.exists():
                continue
            # Look for Copilot-related chat JSON files
            for path in sorted(root.rglob("*.json")):
                name_lower = path.name.lower()
                if any(k in name_lower for k in ("chat", "copilot", "conversation")):
                    yield path
            # Look for session-state jsonl files
            for path in sorted(root.rglob("*.jsonl")):
                if "session-state" in str(path) or "session" in path.name.lower():
                    yield path

    def parse(self, path: Path) -> Iterator[NormalizedMessage]:
        if path.suffix == ".jsonl":
            yield from self._parse_jsonl_stream(path)
        else:
            yield from self._parse_json_dump(path)

    def _parse_jsonl_stream(self, path: Path) -> Iterator[NormalizedMessage]:
        conv_id = path.stem
        seq = 0
        try:
            with open(path, "r", encoding="utf-8", errors="replace") as f:
                for line in f:
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        item = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    
                    msg_type = item.get("type", "")
                    data = item.get("data", {})
                    
                    if msg_type == "session.start":
                        if "sessionId" in data:
                            conv_id = data["sessionId"]
                        continue

                    if msg_type in ("user.message", "assistant.message"):
                        role = "user" if msg_type == "user.message" else "assistant"
                        content = data.get("content", "")
                        
                        if isinstance(content, str) and content.strip():
                            yield NormalizedMessage(
                                conversation_id=conv_id,
                                source="copilot",
                                role=role,
                                content_type="message",
                                text=content,
                                ts=self._parse_ts(item),
                                seq=seq,
                                project=None,
                                model=None,
                                source_path=str(path),
                            )
                            seq += 1
        except OSError as e:
            logger.warning("Failed to read %s: %s", path, e)

    def _parse_json_dump(self, path: Path) -> Iterator[NormalizedMessage]:
        conv_id = path.stem
        seq = 0

        try:
            data = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        except (json.JSONDecodeError, OSError) as e:
            logger.warning("Failed to parse %s: %s", path, e)
            return

        # Handle various Copilot chat formats
        conversations = []
        if isinstance(data, list):
            conversations = data
        elif isinstance(data, dict):
            conversations = data.get("conversations", data.get("chats", [data]))

        for conv in conversations:
            if not isinstance(conv, dict):
                continue

            c_id = conv.get("id", conv.get("conversationId", conv_id))
            messages = conv.get("messages", conv.get("turns", conv.get("exchanges", [])))

            for item in messages:
                if not isinstance(item, dict):
                    continue

                role = item.get("role", item.get("author", "unknown"))
                if role in ("bot", "copilot"):
                    role = "assistant"

                content = item.get("content", item.get("text", item.get("message", "")))
                if isinstance(content, str) and content.strip():
                    yield NormalizedMessage(
                        conversation_id=c_id,
                        source="copilot",
                        role=role,
                        content_type="message",
                        text=content,
                        ts=self._parse_ts(item),
                        seq=seq,
                        project=conv.get("workspace", conv.get("cwd")),
                        model=item.get("model", conv.get("model")),
                        source_path=str(path),
                    )
                    seq += 1

    @staticmethod
    def _parse_ts(item: dict) -> datetime | None:
        for key in ("timestamp", "date", "createdAt", "created_at"):
            val = item.get(key)
            if val is None:
                continue
            if isinstance(val, (int, float)):
                # Handle millisecond timestamps
                if val > 1e12:
                    val = val / 1000
                return datetime.fromtimestamp(val, tz=timezone.utc)
            try:
                dt = datetime.fromisoformat(str(val))
                return dt if dt.tzinfo else dt.replace(tzinfo=timezone.utc)
            except (ValueError, TypeError):
                pass
        return None
