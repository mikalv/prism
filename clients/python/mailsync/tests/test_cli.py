"""Tests for CLI helpers and the non-network command paths."""

import os

import pytest

from mailsync.cli import load_dotenv, main


@pytest.fixture(autouse=True)
def _clean_env(monkeypatch):
    for k in list(os.environ):
        if k.startswith("MAILSYNC_"):
            monkeypatch.delenv(k, raising=False)


def test_load_dotenv_parses_and_ignores_comments(tmp_path, monkeypatch):
    env = tmp_path / ".env"
    env.write_text(
        "# a comment\n"
        "\n"
        'MAILSYNC_IMAP_HOST=imap.example.com\n'
        "MAILSYNC_IMAP_PORT=993   # inline comment\n"
        'MAILSYNC_IMAP_USER="quoted@example.com"\n'
    )
    loaded = load_dotenv(env)
    assert loaded["MAILSYNC_IMAP_HOST"] == "imap.example.com"
    assert loaded["MAILSYNC_IMAP_PORT"] == "993"
    assert loaded["MAILSYNC_IMAP_USER"] == "quoted@example.com"


def test_load_dotenv_does_not_override_existing_env(tmp_path, monkeypatch):
    monkeypatch.setenv("MAILSYNC_IMAP_HOST", "already-set")
    env = tmp_path / ".env"
    env.write_text("MAILSYNC_IMAP_HOST=from-file\n")
    load_dotenv(env)
    assert os.environ["MAILSYNC_IMAP_HOST"] == "already-set"


def test_init_writes_template_then_refuses_overwrite(tmp_path, monkeypatch, capsys):
    monkeypatch.chdir(tmp_path)
    assert main(["--init"]) == 0
    assert (tmp_path / ".env").exists()
    # second run must not clobber
    assert main(["--init"]) == 1


def test_missing_config_returns_exit_code_2(tmp_path, monkeypatch, capsys):
    monkeypatch.chdir(tmp_path)  # no .env present
    assert main([]) == 2
    assert "config error" in capsys.readouterr().err


def test_print_config_redacts_and_exits_zero(tmp_path, monkeypatch, capsys):
    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv("MAILSYNC_IMAP_HOST", "imap.example.com")
    monkeypatch.setenv("MAILSYNC_IMAP_USER", "me@example.com")
    monkeypatch.setenv("MAILSYNC_IMAP_PASSWORD", "supersecret")
    rc = main(["--print-config"])
    out = capsys.readouterr().out
    assert rc == 0
    assert "supersecret" not in out
    assert "imap.example.com" in out
