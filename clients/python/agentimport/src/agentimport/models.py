"""Normalized models for AI assistant conversations."""

from __future__ import annotations

import hashlib
from datetime import datetime
from typing import Any

from pydantic import BaseModel, Field


class NormalizedMessage(BaseModel):
    """A single message from an AI assistant conversation, normalized across sources."""

    conversation_id: str
    native_msg_id: str | None = None
    source: str  # "claude_code" | "codex" | "gemini" | "copilot" | "chatgpt"
    role: str  # "user" | "assistant" | "system" | "tool"
    content_type: str  # "message" | "tool_call" | "tool_result" | "thinking"
    text: str
    tool_name: str | None = None
    ts: datetime | None = None
    seq: int
    project: str | None = None
    model: str | None = None
    source_path: str

    def deterministic_id(self) -> str:
        """Generate a stable, deterministic document ID for Prism.

        Uses SHA1 of (source + conversation_id + native_msg_id) when available,
        falls back to (source + conversation_id + seq).
        """
        if self.native_msg_id:
            key = f"{self.source}:{self.conversation_id}:{self.native_msg_id}"
        else:
            key = f"{self.source}:{self.conversation_id}:{self.seq}"
        return hashlib.sha1(key.encode()).hexdigest()[:16]

    def to_prism_doc(self) -> dict[str, Any]:
        """Convert to Prism document format for indexing."""
        fields: dict[str, Any] = {
            "text": self.text,
            "conversation_id": self.conversation_id,
            "source": self.source,
            "role": self.role,
            "content_type": self.content_type,
            "seq": self.seq,
            "source_path": self.source_path,
        }
        if self.tool_name:
            fields["tool_name"] = self.tool_name
        if self.project:
            fields["project"] = self.project
        if self.model:
            fields["model"] = self.model
        if self.ts:
            fields["ts"] = self.ts.isoformat()
        return {"id": self.deterministic_id(), "fields": fields}


class NormalizedConversation(BaseModel):
    """Metadata for a complete conversation, aggregated from messages."""

    conversation_id: str
    source: str
    title: str | None = None
    project: str | None = None
    model: str | None = None
    started_at: datetime | None = None
    ended_at: datetime | None = None
    msg_count: int = 0
    source_path: str

    def deterministic_id(self) -> str:
        key = f"{self.source}:{self.conversation_id}"
        return hashlib.sha1(key.encode()).hexdigest()[:16]

    def to_prism_doc(self) -> dict[str, Any]:
        fields: dict[str, Any] = {
            "conversation_id": self.conversation_id,
            "source": self.source,
            "msg_count": self.msg_count,
            "source_path": self.source_path,
        }
        if self.title:
            fields["title"] = self.title
        if self.project:
            fields["project"] = self.project
        if self.model:
            fields["model"] = self.model
        if self.started_at:
            fields["started_at"] = self.started_at.isoformat()
        if self.ended_at:
            fields["ended_at"] = self.ended_at.isoformat()
        return {"id": self.deterministic_id(), "fields": fields}


class ContentFilter(BaseModel):
    """Configuration for filtering message content during import."""

    include_tool_results: bool = False
    include_thinking: bool = False
    roles: set[str] | None = None  # None = all roles
    max_chars: int | None = None  # None = no limit

    def should_include(self, msg: NormalizedMessage) -> bool:
        """Check if a message passes this filter."""
        if self.roles and msg.role not in self.roles:
            return False
        if not self.include_tool_results and msg.content_type == "tool_result":
            return False
        if not self.include_thinking and msg.content_type == "thinking":
            return False
        return True

    def apply_limits(self, msg: NormalizedMessage) -> NormalizedMessage:
        """Apply max_chars limit to message text."""
        if self.max_chars and len(msg.text) > self.max_chars:
            return msg.model_copy(update={"text": msg.text[: self.max_chars] + "…"})
        return msg
