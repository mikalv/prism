"""SQLite-backed per-folder UID watermark store.

Resumability lives here: one row per ``(account, folder)`` recording the
``UIDVALIDITY`` it was captured under and the highest UID durably indexed. A
changed ``UIDVALIDITY`` means the server renumbered the folder, so the old
watermark is treated as absent (0) and the folder is re-scanned.
"""

from __future__ import annotations

import sqlite3
from pathlib import Path
from typing import Union

_SCHEMA = """
CREATE TABLE IF NOT EXISTS folders (
    account     TEXT NOT NULL,
    folder      TEXT NOT NULL,
    uidvalidity INTEGER NOT NULL,
    last_uid    INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (account, folder)
);
"""


class StateStore:
    """Durable watermark store backed by a single SQLite file."""

    def __init__(self, path: Union[str, Path]):
        self.path = str(path)
        self._conn = sqlite3.connect(self.path)
        self._conn.execute("PRAGMA journal_mode=WAL")
        self._conn.executescript(_SCHEMA)
        self._conn.commit()

    def get_watermark(self, account: str, folder: str, uidvalidity: int) -> int:
        row = self._conn.execute(
            "SELECT uidvalidity, last_uid FROM folders WHERE account=? AND folder=?",
            (account, folder),
        ).fetchone()
        if row is None or row[0] != uidvalidity:
            return 0
        return int(row[1])

    def set_watermark(
        self, account: str, folder: str, uidvalidity: int, last_uid: int
    ) -> None:
        self._conn.execute(
            """
            INSERT INTO folders (account, folder, uidvalidity, last_uid, updated_at)
            VALUES (?, ?, ?, ?, strftime('%s','now'))
            ON CONFLICT(account, folder) DO UPDATE SET
                uidvalidity=excluded.uidvalidity,
                last_uid=excluded.last_uid,
                updated_at=excluded.updated_at
            """,
            (account, folder, uidvalidity, last_uid),
        )
        self._conn.commit()

    def close(self) -> None:
        self._conn.close()

    def __enter__(self) -> "StateStore":
        return self

    def __exit__(self, *exc) -> None:
        self.close()
