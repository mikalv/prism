"""Tests for the SQLite state tracker."""

import tempfile
from pathlib import Path

from agentimport.state import StateDB


def test_new_file_should_import(tmp_path):
    """A file never seen before should be imported."""
    db_path = tmp_path / "state.db"
    test_file = tmp_path / "test.jsonl"
    test_file.write_text("test")

    with StateDB(db_path) as state:
        assert state.should_import(test_file) is True


def test_after_mark_should_not_import(tmp_path):
    """After marking imported, same file should be skipped."""
    db_path = tmp_path / "state.db"
    test_file = tmp_path / "test.jsonl"
    test_file.write_text("test")

    with StateDB(db_path) as state:
        state.mark_imported(test_file, source="claude_code")
        assert state.should_import(test_file) is False


def test_modified_file_should_reimport(tmp_path):
    """If a file changes (different size), it should be re-imported."""
    db_path = tmp_path / "state.db"
    test_file = tmp_path / "test.jsonl"
    test_file.write_text("test")

    with StateDB(db_path) as state:
        state.mark_imported(test_file, source="claude_code")

        # Modify the file
        test_file.write_text("test with more content")
        assert state.should_import(test_file) is True


def test_nonexistent_file_should_not_import(tmp_path):
    """A file that doesn't exist should not be imported (no crash)."""
    db_path = tmp_path / "state.db"
    with StateDB(db_path) as state:
        assert state.should_import(tmp_path / "nope.jsonl") is False


def test_stats(tmp_path):
    """Stats should count files per source."""
    db_path = tmp_path / "state.db"
    f1 = tmp_path / "a.jsonl"
    f2 = tmp_path / "b.jsonl"
    f3 = tmp_path / "c.jsonl"
    f1.write_text("a")
    f2.write_text("b")
    f3.write_text("c")

    with StateDB(db_path) as state:
        state.mark_imported(f1, source="claude_code")
        state.mark_imported(f2, source="claude_code")
        state.mark_imported(f3, source="codex")

        stats = state.get_stats()
        assert stats["claude_code"] == 2
        assert stats["codex"] == 1


def test_persistence_across_sessions(tmp_path):
    """State should persist across StateDB instances."""
    db_path = tmp_path / "state.db"
    test_file = tmp_path / "test.jsonl"
    test_file.write_text("test")

    with StateDB(db_path) as state:
        state.mark_imported(test_file, source="claude_code")

    # New instance should see previous state
    with StateDB(db_path) as state:
        assert state.should_import(test_file) is False
