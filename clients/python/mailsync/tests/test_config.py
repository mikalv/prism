"""Tests for env-driven configuration parsing (pure, dict-in)."""

import pytest

from mailsync.config import Config, ConfigError

BASE = {
    "MAILSYNC_IMAP_HOST": "imap.example.com",
    "MAILSYNC_IMAP_USER": "me@example.com",
    "MAILSYNC_IMAP_PASSWORD": "secret",
}


def test_minimal_password_config():
    c = Config.from_env(BASE)
    assert c.imap_host == "imap.example.com"
    assert c.imap_user == "me@example.com"
    assert c.imap_password == "secret"
    assert c.imap_auth == "password"
    assert c.prism_url == "http://localhost:3080"
    assert c.collection == "mail"


def test_missing_host_raises():
    env = {"MAILSYNC_IMAP_USER": "u", "MAILSYNC_IMAP_PASSWORD": "p"}
    with pytest.raises(ConfigError):
        Config.from_env(env)


def test_password_or_oauth2_credential_required():
    env = {"MAILSYNC_IMAP_HOST": "h", "MAILSYNC_IMAP_USER": "u"}
    with pytest.raises(ConfigError):
        Config.from_env(env)


def test_port_993_defaults_to_ssl():
    c = Config.from_env({**BASE, "MAILSYNC_IMAP_PORT": "993"})
    assert c.imap_port == 993
    assert c.imap_security == "ssl"


def test_port_143_defaults_to_starttls():
    c = Config.from_env({**BASE, "MAILSYNC_IMAP_PORT": "143"})
    assert c.imap_security == "starttls"


def test_explicit_security_overrides_port_default():
    c = Config.from_env({**BASE, "MAILSYNC_IMAP_PORT": "993", "MAILSYNC_IMAP_SECURITY": "starttls"})
    assert c.imap_security == "starttls"


def test_oauth2_auth():
    env = {
        "MAILSYNC_IMAP_HOST": "h",
        "MAILSYNC_IMAP_USER": "u",
        "MAILSYNC_IMAP_AUTH": "oauth2",
        "MAILSYNC_IMAP_OAUTH2_TOKEN": "ya29.token",
    }
    c = Config.from_env(env)
    assert c.imap_auth == "oauth2"
    assert c.imap_oauth2_token == "ya29.token"


def test_ssh_tunnel_enabled_when_host_present():
    c = Config.from_env(
        {
            **BASE,
            "MAILSYNC_SSH_HOST": "bastion.example.com",
            "MAILSYNC_SSH_USER": "tunnel",
            "MAILSYNC_SSH_KEY": "~/.ssh/id_ed25519",
        }
    )
    assert c.ssh_enabled is True
    assert c.ssh_host == "bastion.example.com"
    assert c.ssh_port == 22
    assert c.ssh_user == "tunnel"


def test_no_ssh_by_default():
    assert Config.from_env(BASE).ssh_enabled is False


def test_account_identity_is_user_at_host():
    # Used as the SQLite state key; must be stable regardless of tunnel.
    c = Config.from_env(BASE)
    assert c.account == "me@example.com@imap.example.com"


def test_sync_tunables_parse():
    c = Config.from_env({**BASE, "MAILSYNC_BATCH_SIZE": "500", "MAILSYNC_BODY_CAP": "2048"})
    assert c.batch_size == 500
    assert c.body_cap == 2048


def test_tls_verify_defaults_true():
    assert Config.from_env(BASE).imap_tls_verify is True


def test_tls_verify_can_be_disabled_explicitly():
    c = Config.from_env({**BASE, "MAILSYNC_IMAP_TLS_VERIFY": "false"})
    assert c.imap_tls_verify is False
