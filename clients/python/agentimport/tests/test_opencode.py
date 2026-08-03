"""Tests for the opencode source adapter (SQLite DB + file-storage fallback)."""

import json
import sqlite3
from pathlib import Path

from agentimport.sources.base import get_source_by_name
from agentimport.sources.opencode import OpencodeSource


# --------------------------------------------------------------------------- #
# Fixtures
# --------------------------------------------------------------------------- #

def _make_db(path: Path, sessions, messages, parts) -> None:
    """Build a minimal opencode.db with the columns the adapter reads."""
    conn = sqlite3.connect(path)
    conn.executescript(
        """
        CREATE TABLE session (
            id TEXT PRIMARY KEY, title TEXT, directory TEXT,
            model TEXT, time_created INTEGER
        );
        CREATE TABLE message (
            id TEXT PRIMARY KEY, session_id TEXT, data TEXT
        );
        CREATE TABLE part (
            id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, data TEXT
        );
        """
    )
    conn.executemany("INSERT INTO session VALUES (?,?,?,?,?)", sessions)
    conn.executemany("INSERT INTO message VALUES (?,?,?)", messages)
    conn.executemany("INSERT INTO part VALUES (?,?,?,?)", parts)
    conn.commit()
    conn.close()


def _sample_db(path: Path) -> None:
    sessions = [("ses_1", "My chat", "/home/u/proj", "gpt-x", 1_700_000_000_000)]
    messages = [
        ("msg_1", "ses_1", json.dumps({"role": "user", "time": {"created": 1_700_000_000_000},
                                       "model": {"modelID": "gpt-x"}})),
        ("msg_2", "ses_1", json.dumps({"role": "assistant", "time": {"created": 1_700_000_001_000},
                                       "modelID": "gpt-x", "path": {"cwd": "/home/u/proj"}})),
    ]
    parts = [
        ("prt_1", "msg_1", "ses_1", json.dumps({"type": "text", "text": "Hello there"})),
        # synthetic hook noise → skipped
        ("prt_2", "msg_1", "ses_1", json.dumps({"type": "text", "text": "hook", "synthetic": True})),
        ("prt_3", "msg_2", "ses_1", json.dumps({"type": "reasoning", "text": "thinking..."})),
        ("prt_4", "msg_2", "ses_1", json.dumps({"type": "text", "text": "Here is the answer"})),
        ("prt_5", "msg_2", "ses_1", json.dumps({"type": "tool", "tool": "bash",
                                                "state": {"input": {"cmd": "ls"}}})),
        # non-content part → skipped
        ("prt_6", "msg_2", "ses_1", json.dumps({"type": "step-finish"})),
    ]
    _make_db(path, sessions, messages, parts)


def _write_file_storage(root: Path) -> None:
    """Upstream file layout: storage/message/<ses>/<msg>.json + storage/part/<ses>/<msg>/<prt>.json."""
    msg_dir = root / "storage" / "message" / "ses_1"
    msg_dir.mkdir(parents=True)
    (msg_dir / "msg_1.json").write_text(json.dumps(
        {"role": "user", "time": {"created": 1_700_000_000_000}, "path": {"cwd": "/home/u/proj"}}))
    (msg_dir / "msg_2.json").write_text(json.dumps(
        {"role": "assistant", "time": {"created": 1_700_000_001_000}, "modelID": "gpt-x"}))

    p1 = root / "storage" / "part" / "ses_1" / "msg_1"
    p1.mkdir(parents=True)
    (p1 / "prt_1.json").write_text(json.dumps({"type": "text", "text": "Hello there"}))
    p2 = root / "storage" / "part" / "ses_1" / "msg_2"
    p2.mkdir(parents=True)
    (p2 / "prt_1.json").write_text(json.dumps({"type": "text", "text": "Here is the answer"}))


# --------------------------------------------------------------------------- #
# Registration
# --------------------------------------------------------------------------- #

def test_registered_by_name():
    assert get_source_by_name("opencode") is not None
    assert OpencodeSource().name == "opencode"


# --------------------------------------------------------------------------- #
# Discover
# --------------------------------------------------------------------------- #

class TestDiscover:
    def test_yields_db_when_present(self, tmp_path):
        _sample_db(tmp_path / "opencode.db")
        found = list(OpencodeSource().discover([tmp_path]))
        assert found == [tmp_path / "opencode.db"]

    def test_prefers_db_over_files(self, tmp_path):
        _sample_db(tmp_path / "opencode.db")
        _write_file_storage(tmp_path)
        found = list(OpencodeSource().discover([tmp_path]))
        # DB present → only the DB is imported (avoids double-import).
        assert found == [tmp_path / "opencode.db"]

    def test_yields_session_dirs_when_only_files(self, tmp_path):
        _write_file_storage(tmp_path)
        found = list(OpencodeSource().discover([tmp_path]))
        assert found == [tmp_path / "storage" / "message" / "ses_1"]

    def test_skips_missing_root(self, tmp_path):
        assert list(OpencodeSource().discover([tmp_path / "nope"])) == []


# --------------------------------------------------------------------------- #
# Parse — DB
# --------------------------------------------------------------------------- #

class TestParseDB:
    def test_parses_messages_and_parts(self, tmp_path):
        db = tmp_path / "opencode.db"
        _sample_db(db)
        msgs = list(OpencodeSource().parse(db))

        # prt_1 (user text), prt_3 (thinking), prt_4 (assistant text), prt_5 (tool).
        # Synthetic prt_2 and step-finish prt_6 are dropped.
        assert [m.text for m in msgs] == [
            "Hello there",
            "thinking...",
            "Here is the answer",
            "Tool: bash\nInput: {\"cmd\": \"ls\"}",
        ]
        assert [m.content_type for m in msgs] == ["message", "thinking", "message", "tool_call"]
        assert [m.role for m in msgs] == ["user", "assistant", "assistant", "assistant"]
        assert all(m.source == "opencode" for m in msgs)
        assert all(m.conversation_id == "ses_1" for m in msgs)
        assert all(m.project == "/home/u/proj" for m in msgs)
        assert [m.seq for m in msgs] == [0, 1, 2, 3]
        assert msgs[3].tool_name == "bash"
        assert msgs[0].native_msg_id == "prt_1"

    def test_empty_db_yields_nothing(self, tmp_path):
        db = tmp_path / "opencode.db"
        _make_db(db, [], [], [])
        assert list(OpencodeSource().parse(db)) == []


# --------------------------------------------------------------------------- #
# Parse — files
# --------------------------------------------------------------------------- #

class TestParseFiles:
    def test_parses_session_dir(self, tmp_path):
        _write_file_storage(tmp_path)
        session_dir = tmp_path / "storage" / "message" / "ses_1"
        msgs = list(OpencodeSource().parse(session_dir))

        assert [m.text for m in msgs] == ["Hello there", "Here is the answer"]
        assert [m.role for m in msgs] == ["user", "assistant"]
        assert all(m.conversation_id == "ses_1" for m in msgs)
        assert all(m.source == "opencode" for m in msgs)
        assert [m.seq for m in msgs] == [0, 1]
