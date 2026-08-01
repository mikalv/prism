"""SQLite-backed state tracker for incremental imports."""

from __future__ import annotations

import sqlite3
from datetime import datetime, timezone
from pathlib import Path


class StateDB:
    """Tracks which files have been imported and when, enabling incremental re-imports.

    Uses mtime + size as a fast watermark — if neither changed, skip the file.
    """

    def __init__(self, db_path: Path) -> None:
        self._db_path = db_path
        db_path.parent.mkdir(parents=True, exist_ok=True)
        self._conn = sqlite3.connect(str(db_path))
        self._conn.execute("PRAGMA journal_mode=WAL")
        self._init_schema()

    def _init_schema(self) -> None:
        self._conn.execute("""
            CREATE TABLE IF NOT EXISTS file_state (
                source_path TEXT PRIMARY KEY,
                mtime REAL NOT NULL,
                size INTEGER NOT NULL,
                last_imported TEXT NOT NULL,
                source TEXT DEFAULT ''
            )
        """)
        self._conn.commit()

    def should_import(self, path: Path) -> bool:
        """Return True if the file has changed since last import (or was never imported)."""
        try:
            stat = path.stat()
        except OSError:
            return False

        row = self._conn.execute(
            "SELECT mtime, size FROM file_state WHERE source_path = ?",
            (str(path),),
        ).fetchone()

        if row is None:
            return True
        return row[0] != stat.st_mtime or row[1] != stat.st_size

    def mark_imported(self, path: Path, source: str = "") -> None:
        """Record that a file has been successfully imported."""
        stat = path.stat()
        now = datetime.now(timezone.utc).isoformat()
        self._conn.execute(
            """INSERT OR REPLACE INTO file_state (source_path, mtime, size, last_imported, source)
               VALUES (?, ?, ?, ?, ?)""",
            (str(path), stat.st_mtime, stat.st_size, now, source),
        )
        self._conn.commit()

    def get_stats(self) -> dict[str, int]:
        """Return import stats per source."""
        rows = self._conn.execute(
            "SELECT source, COUNT(*) FROM file_state GROUP BY source"
        ).fetchall()
        return {row[0] or "unknown": row[1] for row in rows}

    def close(self) -> None:
        self._conn.close()

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()
