"""Environment-driven configuration.

Secrets come from the environment only (never committed). Supports password and
XOAUTH2 IMAP auth, implicit SSL or STARTTLS, and an optional SSH tunnel to the
IMAP host.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Mapping, Optional


class ConfigError(ValueError):
    """Raised when required configuration is missing or invalid."""


def _bool(v: Optional[str], default: bool = False) -> bool:
    if v is None:
        return default
    return v.strip().lower() in ("1", "true", "yes", "on")


@dataclass(frozen=True)
class Config:
    imap_host: str
    imap_port: int
    imap_user: str
    imap_password: Optional[str]
    imap_auth: str          # "password" | "oauth2"
    imap_oauth2_token: Optional[str]
    imap_security: str      # "ssl" | "starttls" | "none"
    imap_tls_verify: bool

    ssh_enabled: bool
    ssh_host: Optional[str]
    ssh_port: int
    ssh_user: Optional[str]
    ssh_password: Optional[str]
    ssh_key: Optional[str]

    prism_url: str
    collection: str
    prism_api_key: Optional[str]

    batch_size: int
    body_cap: int
    state_path: str

    @property
    def account(self) -> str:
        """Stable identity for the SQLite state key (independent of tunnelling)."""
        return f"{self.imap_user}@{self.imap_host}"

    @classmethod
    def from_env(cls, env: Mapping[str, str]) -> "Config":
        def get(key: str, default: Optional[str] = None) -> Optional[str]:
            val = env.get(key)
            return val if val not in (None, "") else default

        host = get("MAILSYNC_IMAP_HOST")
        user = get("MAILSYNC_IMAP_USER")
        if not host:
            raise ConfigError("MAILSYNC_IMAP_HOST is required")
        if not user:
            raise ConfigError("MAILSYNC_IMAP_USER is required")

        auth = (get("MAILSYNC_IMAP_AUTH", "password") or "password").lower()
        password = get("MAILSYNC_IMAP_PASSWORD")
        oauth2_token = get("MAILSYNC_IMAP_OAUTH2_TOKEN")
        if auth == "oauth2":
            if not oauth2_token:
                raise ConfigError("MAILSYNC_IMAP_OAUTH2_TOKEN required for oauth2 auth")
        elif auth == "password":
            if not password:
                raise ConfigError("MAILSYNC_IMAP_PASSWORD required for password auth")
        else:
            raise ConfigError(f"unknown MAILSYNC_IMAP_AUTH: {auth!r}")

        port = int(get("MAILSYNC_IMAP_PORT", "993"))
        security = get("MAILSYNC_IMAP_SECURITY")
        if not security:
            security = "starttls" if port == 143 else "ssl"
        security = security.lower()
        if security not in ("ssl", "starttls", "none"):
            raise ConfigError(f"unknown MAILSYNC_IMAP_SECURITY: {security!r}")

        ssh_host = get("MAILSYNC_SSH_HOST")
        ssh_enabled = _bool(get("MAILSYNC_SSH_ENABLED"), default=bool(ssh_host))
        if ssh_enabled and not ssh_host:
            raise ConfigError("MAILSYNC_SSH_ENABLED set but MAILSYNC_SSH_HOST missing")

        return cls(
            imap_host=host,
            imap_port=port,
            imap_user=user,
            imap_password=password,
            imap_auth=auth,
            imap_oauth2_token=oauth2_token,
            imap_security=security,
            imap_tls_verify=_bool(get("MAILSYNC_IMAP_TLS_VERIFY"), default=True),
            ssh_enabled=ssh_enabled,
            ssh_host=ssh_host,
            ssh_port=int(get("MAILSYNC_SSH_PORT", "22")),
            ssh_user=get("MAILSYNC_SSH_USER"),
            ssh_password=get("MAILSYNC_SSH_PASSWORD"),
            ssh_key=get("MAILSYNC_SSH_KEY"),
            prism_url=(get("MAILSYNC_PRISM_URL", "http://localhost:3080") or "").rstrip("/"),
            collection=get("MAILSYNC_COLLECTION", "mail"),
            prism_api_key=get("MAILSYNC_PRISM_API_KEY"),
            batch_size=int(get("MAILSYNC_BATCH_SIZE", "300")),
            body_cap=int(get("MAILSYNC_BODY_CAP", "1000000")),
            state_path=get("MAILSYNC_STATE_PATH", "mailsync.db"),
        )
