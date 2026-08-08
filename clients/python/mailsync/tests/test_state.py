"""Tests for the SQLite watermark store (real DB, temp file)."""

from mailsync.state import StateStore


def test_fresh_folder_has_zero_watermark(tmp_path):
    store = StateStore(tmp_path / "s.db")
    assert store.get_watermark("acct", "INBOX", uidvalidity=1) == 0


def test_set_then_get(tmp_path):
    store = StateStore(tmp_path / "s.db")
    store.set_watermark("acct", "INBOX", uidvalidity=1, last_uid=42)
    assert store.get_watermark("acct", "INBOX", uidvalidity=1) == 42


def test_uidvalidity_change_resets_watermark(tmp_path):
    store = StateStore(tmp_path / "s.db")
    store.set_watermark("acct", "INBOX", uidvalidity=1, last_uid=42)
    # Server renumbered the folder -> old watermark is meaningless.
    assert store.get_watermark("acct", "INBOX", uidvalidity=2) == 0


def test_watermark_persists_across_reopen(tmp_path):
    path = tmp_path / "s.db"
    StateStore(path).set_watermark("acct", "INBOX", uidvalidity=1, last_uid=99)
    assert StateStore(path).get_watermark("acct", "INBOX", uidvalidity=1) == 99


def test_folders_are_independent(tmp_path):
    store = StateStore(tmp_path / "s.db")
    store.set_watermark("acct", "INBOX", uidvalidity=1, last_uid=10)
    store.set_watermark("acct", "Sent", uidvalidity=1, last_uid=20)
    assert store.get_watermark("acct", "INBOX", uidvalidity=1) == 10
    assert store.get_watermark("acct", "Sent", uidvalidity=1) == 20
