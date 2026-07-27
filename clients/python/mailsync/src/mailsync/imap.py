"""IMAP source: connection (SSL/STARTTLS/plain, password/XOAUTH2, optional SSH
tunnel) plus UID-based incremental fetch.

Resumability primitive: ``UIDVALIDITY`` per folder and monotonically increasing
``UID``s let a re-run fetch only ``UID > watermark``.
"""

from __future__ import annotations

import os
import ssl
from typing import Iterator, List, Optional, Tuple

from imapclient import IMAPClient

from .config import Config


def _tls_context(cfg: Config, tunnelled: bool) -> ssl.SSLContext:
    """Build a TLS context appropriate for the connection.

    Direct connections verify normally (chain + hostname). Over an SSH tunnel we
    reach the server via 127.0.0.1, so the cert's hostname can't match; we keep
    **certificate-chain verification on** and relax only the hostname check —
    a MITM would still need a CA-trusted cert. Full verification-off
    (``CERT_NONE``) is opt-in via ``MAILSYNC_IMAP_TLS_VERIFY=false`` for
    self-signed servers.
    """
    ctx = ssl.create_default_context()
    if not cfg.imap_tls_verify:
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
    elif tunnelled:
        # Chain still verified against system CAs; only hostname pinning relaxed.
        ctx.check_hostname = False
    return ctx


class ImapSource:
    """Context-managed IMAP connection, optionally through an SSH tunnel."""

    def __init__(self, cfg: Config):
        self.cfg = cfg
        self._tunnel = None
        self._client: Optional[IMAPClient] = None

    # -- connection lifecycle -------------------------------------------------

    def __enter__(self) -> "ImapSource":
        host, port, tunnelled = self._maybe_open_tunnel()
        cfg = self.cfg

        use_ssl = cfg.imap_security == "ssl"
        ssl_context = _tls_context(cfg, tunnelled) if use_ssl else None

        client = IMAPClient(host, port=port, ssl=use_ssl, ssl_context=ssl_context)
        if cfg.imap_security == "starttls":
            client.starttls(ssl_context=_tls_context(cfg, tunnelled))

        if cfg.imap_auth == "oauth2":
            client.oauth2_login(cfg.imap_user, cfg.imap_oauth2_token)
        else:
            client.login(cfg.imap_user, cfg.imap_password)

        self._client = client
        return self

    def __exit__(self, *exc) -> None:
        if self._client is not None:
            try:
                self._client.logout()
            except Exception:
                pass
        if self._tunnel is not None:
            self._tunnel.stop()

    def _maybe_open_tunnel(self) -> Tuple[str, int, bool]:
        cfg = self.cfg
        if not cfg.ssh_enabled:
            return cfg.imap_host, cfg.imap_port, False
        try:
            from sshtunnel import SSHTunnelForwarder
        except ImportError as e:  # pragma: no cover - import guard
            raise RuntimeError(
                "SSH tunnel requested but 'sshtunnel' is not installed. "
                "Install with: uv sync --extra ssh"
            ) from e

        kwargs = {
            "ssh_username": cfg.ssh_user,
            "remote_bind_address": (cfg.imap_host, cfg.imap_port),
        }
        if cfg.ssh_key:
            kwargs["ssh_pkey"] = os.path.expanduser(cfg.ssh_key)
        if cfg.ssh_password:
            kwargs["ssh_password"] = cfg.ssh_password

        self._tunnel = SSHTunnelForwarder((cfg.ssh_host, cfg.ssh_port), **kwargs)
        self._tunnel.start()
        return "127.0.0.1", self._tunnel.local_bind_port, True

    # -- operations -----------------------------------------------------------

    def list_folders(self) -> List[str]:
        folders = []
        for _flags, _delim, name in self._client.list_folders():
            folders.append(name)
        return folders

    def select_folder(self, name: str) -> int:
        """Select a folder read-only; return its UIDVALIDITY."""
        info = self._client.select_folder(name, readonly=True)
        return int(info[b"UIDVALIDITY"])

    def search_since(self, watermark: int) -> List[int]:
        """UIDs strictly greater than the watermark, ascending.

        Note the IMAP ``lo:*`` quirk: it always matches the highest UID even
        when ``lo`` is past the end, so we filter client-side.
        """
        lo = watermark + 1
        uids = self._client.search(["UID", f"{lo}:*"])
        return sorted(u for u in uids if u >= lo)

    def fetch_messages(self, uids: List[int]) -> Iterator[Tuple[int, bytes, List[str]]]:
        """Yield ``(uid, raw_rfc822_bytes, flags)`` for the given UIDs."""
        data = self._client.fetch(uids, ["RFC822", "FLAGS"])
        for uid in uids:
            item = data.get(uid)
            if not item:
                continue
            raw = item.get(b"RFC822") or b""
            flags = [f.decode() if isinstance(f, bytes) else str(f) for f in item.get(b"FLAGS", ())]
            yield uid, raw, flags
