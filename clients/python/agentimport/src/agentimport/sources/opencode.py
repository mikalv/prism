"""opencode adapter — parses opencode sessions from SQLite or file storage.

opencode (https://opencode.ai) stores conversations under
``~/.local/share/opencode``. Two on-disk layouts exist:

* **SQLite** (`opencode.db`): tables ``session``, ``message`` and ``part``, each
  ``part``/``message`` carrying a JSON ``data`` blob. Newer/forked builds keep
  message text here.
* **File storage** (`storage/`): ``storage/message/<ses>/<msg>.json`` for message
  metadata and ``storage/part/<ses>/<msg>/<prt>.json`` for the content parts.

The adapter reads whichever is present, preferring the DB when both exist (so a
session isn't imported twice). Parts share one normalization path regardless of
source: ``text`` → message, ``reasoning`` → thinking, ``tool`` → tool_call.
Synthetic hook parts and non-content parts (``step-*``, ``snapshot`` …) are
skipped.
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


class OpencodeSource:
    """Parse opencode conversations from an SQLite DB or file storage."""

    @property
    def name(self) -> str:
        return "opencode"

    def default_roots(self) -> list[Path]:
        return [Path.home() / ".local" / "share" / "opencode"]

    def discover(self, roots: list[Path]) -> Iterable[Path]:
        for root in roots:
            if not root.exists():
                continue
            db = root / "opencode.db"
            if db.exists() and _db_has_tables(db):
                # DB is the source of truth; skip file storage to avoid dupes.
                yield db
                continue
            message_dir = root / "storage" / "message"
            if message_dir.is_dir():
                yield from sorted(p for p in message_dir.iterdir() if p.is_dir())

    def parse(self, path: Path) -> Iterator[NormalizedMessage]:
        if path.is_file() and path.suffix == ".db":
            yield from self._parse_db(path)
        elif path.is_dir():
            yield from self._parse_session_dir(path)

    # -- SQLite -------------------------------------------------------------- #

    def _parse_db(self, db: Path) -> Iterator[NormalizedMessage]:
        try:
            conn = sqlite3.connect(f"file:{db}?mode=ro&immutable=1", uri=True)
        except sqlite3.Error as e:
            logger.warning("Cannot open opencode DB %s: %s", db, e)
            return
        try:
            sessions = {
                row[0]: {"title": row[1], "directory": row[2], "model": row[3]}
                for row in conn.execute(
                    "SELECT id, title, directory, model, time_created "
                    "FROM session ORDER BY time_created"
                )
            }
            # Sessions with no metadata row still get parsed (keyed by message).
            session_ids = list(sessions.keys()) or [
                row[0] for row in conn.execute("SELECT DISTINCT session_id FROM message")
            ]
            for sid in session_ids:
                meta = sessions.get(sid, {})
                rows = conn.execute(
                    "SELECT m.data, p.id, p.data FROM message m "
                    "JOIN part p ON p.message_id = m.id "
                    "WHERE m.session_id = ? ORDER BY m.id, p.id",
                    (sid,),
                )
                yield from self._emit(
                    conv_id=sid,
                    rows=((json.loads(md), pid, json.loads(pd)) for md, pid, pd in rows),
                    session_meta=meta,
                    source_path=f"{db}#session:{sid}",
                )
        finally:
            conn.close()

    # -- File storage -------------------------------------------------------- #

    def _parse_session_dir(self, session_dir: Path) -> Iterator[NormalizedMessage]:
        sid = session_dir.name
        storage = session_dir.parent.parent  # storage/
        part_root = storage / "part" / sid
        meta = _read_session_info(storage, sid)

        def rows():
            for msg_file in sorted(session_dir.glob("*.json")):
                try:
                    mdata = json.loads(msg_file.read_text(encoding="utf-8", errors="replace"))
                except (json.JSONDecodeError, OSError) as e:
                    logger.warning("Failed to read %s: %s", msg_file, e)
                    continue
                msg_parts = part_root / msg_file.stem
                if not msg_parts.is_dir():
                    continue
                for part_file in sorted(msg_parts.glob("*.json")):
                    try:
                        pdata = json.loads(part_file.read_text(encoding="utf-8", errors="replace"))
                    except (json.JSONDecodeError, OSError):
                        continue
                    yield mdata, part_file.stem, pdata

        yield from self._emit(
            conv_id=sid,
            rows=rows(),
            session_meta=meta,
            source_path=str(session_dir),
        )

    # -- Shared normalization ------------------------------------------------ #

    def _emit(
        self,
        conv_id: str,
        rows: Iterable[tuple[dict, str, dict]],
        session_meta: dict,
        source_path: str,
    ) -> Iterator[NormalizedMessage]:
        seq = 0
        for mdata, part_id, pdata in rows:
            content = _part_content(pdata)
            if content is None:
                continue
            content_type, text, tool_name = content

            role = mdata.get("role", "unknown")
            ts = _parse_epoch_ms((mdata.get("time") or {}).get("created"))
            model = (
                mdata.get("modelID")
                or (mdata.get("model") or {}).get("modelID")
                or session_meta.get("model")
            )
            project = (mdata.get("path") or {}).get("cwd") or session_meta.get("directory")

            yield NormalizedMessage(
                conversation_id=conv_id,
                native_msg_id=part_id,
                source="opencode",
                role=role,
                content_type=content_type,
                text=text,
                tool_name=tool_name,
                ts=ts,
                seq=seq,
                project=project,
                model=model,
                source_path=source_path,
            )
            seq += 1


def _part_content(part: dict) -> tuple[str, str, str | None] | None:
    """Map an opencode part to (content_type, text, tool_name), or None to skip."""
    ptype = part.get("type")
    if ptype == "text":
        if part.get("synthetic"):  # hook-injected noise, not real conversation
            return None
        text = (part.get("text") or "").strip()
        return ("message", text, None) if text else None
    if ptype == "reasoning":
        text = (part.get("text") or "").strip()
        return ("thinking", text, None) if text else None
    if ptype == "tool":
        tool = part.get("tool")
        state = part.get("state") or {}
        text = f"Tool: {tool}\nInput: {json.dumps(state.get('input', {}), default=str)}"
        return ("tool_call", text, tool)
    return None


def _read_session_info(storage: Path, sid: str) -> dict:
    """Read optional session metadata from file storage (title/directory)."""
    for candidate in (storage / "session" / f"{sid}.json", storage / "session" / "info" / f"{sid}.json"):
        if candidate.is_file():
            try:
                data = json.loads(candidate.read_text(encoding="utf-8", errors="replace"))
            except (json.JSONDecodeError, OSError):
                continue
            return {"title": data.get("title"), "directory": data.get("directory"),
                    "model": data.get("model")}
    return {}


def _db_has_tables(db: Path) -> bool:
    try:
        conn = sqlite3.connect(f"file:{db}?mode=ro&immutable=1", uri=True)
        try:
            rows = conn.execute(
                "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('message','part')"
            ).fetchall()
            return len(rows) == 2
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
