"""Cursor adapter — parses Cursor's global state.vscdb (cursorDiskKV).

Cursor stores AI "composer"/chat conversations in the global
`state.vscdb` SQLite database, table `cursorDiskKV`:

  composerData:<composerId>          -> conversation (ordered bubble headers)
  bubbleId:<composerId>:<bubbleId>   -> a single message (type 1=user, 2=assistant)

A composer's `fullConversationHeadersOnly` lists its bubbles in order; each
bubble's message text lives in the separate bubbleId row. The whole DB is a
single file containing every conversation, so `parse()` yields messages
across all composers found in it.
"""

from __future__ import annotations

import json
import logging
import sqlite3
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Iterator

from agentimport.models import NormalizedMessage

logger = logging.getLogger(__name__)

_GLOBAL_STORAGE_DIRS = [
    Path.home() / "Library" / "Application Support" / "Cursor" / "User" / "globalStorage",
    Path.home() / ".config" / "Cursor" / "User" / "globalStorage",
]

# Cursor bubble type -> role
_TYPE_ROLE = {1: "user", 2: "assistant"}


class CursorSource:
    """Parse Cursor conversations from the global cursorDiskKV store."""

    @property
    def name(self) -> str:
        return "cursor"

    def default_roots(self) -> list[Path]:
        return list(_GLOBAL_STORAGE_DIRS)

    def discover(self, roots: list[Path]) -> Iterable[Path]:
        for root in roots:
            db = root / "state.vscdb"
            if db.exists() and _has_cursor_kv(db):
                yield db

    def parse(self, path: Path) -> Iterator[NormalizedMessage]:
        try:
            conn = sqlite3.connect(f"file:{path}?mode=ro&immutable=1", uri=True)
        except sqlite3.Error as e:
            logger.warning("Cannot open Cursor DB %s: %s", path, e)
            return
        try:
            cur = conn.cursor()
            cur.execute("SELECT key, value FROM cursorDiskKV WHERE key LIKE 'composerData:%'")
            for key, value in cur.fetchall():
                composer_id = key.split(":", 1)[1]
                try:
                    composer = json.loads(value)
                except (json.JSONDecodeError, TypeError):
                    continue
                yield from self._parse_composer(conn, composer_id, composer, path)
        finally:
            conn.close()

    def _parse_composer(
        self, conn: sqlite3.Connection, composer_id: str, composer: dict, path: Path
    ) -> Iterator[NormalizedMessage]:
        headers = composer.get("fullConversationHeadersOnly") or []
        if not isinstance(headers, list) or not headers:
            return

        title = composer.get("name") or None
        ts = _parse_epoch_ms(composer.get("createdAt")) or _parse_epoch_ms(
            composer.get("lastUpdatedAt")
        )

        cur = conn.cursor()
        for seq, header in enumerate(headers):
            if not isinstance(header, dict):
                continue
            bubble_id = header.get("bubbleId")
            if not bubble_id:
                continue
            cur.execute(
                "SELECT value FROM cursorDiskKV WHERE key = ?",
                (f"bubbleId:{composer_id}:{bubble_id}",),
            )
            row = cur.fetchone()
            if not row:
                continue
            try:
                bubble = json.loads(row[0])
            except (json.JSONDecodeError, TypeError):
                continue

            text = (bubble.get("text") or "").strip()
            if not text:
                continue
            role = _TYPE_ROLE.get(bubble.get("type") or header.get("type"), "unknown")

            yield NormalizedMessage(
                conversation_id=composer_id,
                native_msg_id=bubble_id,
                source="cursor",
                role=role,
                content_type="message",
                text=text,
                ts=ts,
                seq=seq,
                project=None,
                model=None,
                source_path=f"{path}#composer:{composer_id}",
            )


def _has_cursor_kv(db: Path) -> bool:
    try:
        conn = sqlite3.connect(f"file:{db}?mode=ro&immutable=1", uri=True)
        try:
            cur = conn.execute(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='cursorDiskKV'"
            )
            return cur.fetchone() is not None
        finally:
            conn.close()
    except sqlite3.Error:
        return False


def _parse_epoch_ms(raw: object) -> datetime | None:
    if not isinstance(raw, (int, float)):
        return None
    try:
        return datetime.fromtimestamp(raw / 1000, tz=timezone.utc)
    except (ValueError, OSError):
        return None
