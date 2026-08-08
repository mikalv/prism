"""Command-line entry point.

Loads a local ``.env`` (if present) so secrets stay out of the shell history,
then runs an incremental, resumable sync. Designed to be runnable with a single
``uv run mailsync`` once ``.env`` is filled in.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path
from typing import Dict

from .config import Config, ConfigError

ENV_TEMPLATE = """\
# prism-mailsync configuration. Fill in and keep this file OUT of git.

# --- IMAP source ---
MAILSYNC_IMAP_HOST=imap.example.com
MAILSYNC_IMAP_PORT=993                 # 993=implicit SSL, 143=STARTTLS
MAILSYNC_IMAP_USER=you@example.com
MAILSYNC_IMAP_PASSWORD=                # app password; or use OAuth2 below
# MAILSYNC_IMAP_AUTH=oauth2            # 'password' (default) or 'oauth2'
# MAILSYNC_IMAP_OAUTH2_TOKEN=          # access token when AUTH=oauth2
# MAILSYNC_IMAP_SECURITY=ssl           # ssl | starttls | none (default from port)
# MAILSYNC_IMAP_TLS_VERIFY=true        # set false only for self-signed servers

# --- Optional SSH tunnel to the IMAP host (recommended) ---
# MAILSYNC_SSH_HOST=bastion.example.com
# MAILSYNC_SSH_PORT=22
# MAILSYNC_SSH_USER=tunnel
# MAILSYNC_SSH_KEY=~/.ssh/id_ed25519   # or MAILSYNC_SSH_PASSWORD=

# --- Prism target ---
MAILSYNC_PRISM_URL=http://192.168.88.212:3080
MAILSYNC_COLLECTION=mail
# MAILSYNC_PRISM_API_KEY=              # only if the server has auth enabled

# --- Tunables ---
# MAILSYNC_BATCH_SIZE=300              # messages per bulk request (max 10000)
# MAILSYNC_BODY_CAP=1000000            # truncate stored plaintext body (bytes)
# MAILSYNC_STATE_PATH=mailsync.db      # SQLite watermark store
"""


def load_dotenv(path: Path) -> Dict[str, str]:
    """Minimal ``.env`` parser (``KEY=VALUE`` lines). Existing env wins."""
    loaded: Dict[str, str] = {}
    if not path.exists():
        return loaded
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, value = line.partition("=")
        key, value = key.strip(), value.split("#", 1)[0].strip().strip('"').strip("'")
        if key and key not in os.environ:
            os.environ[key] = value
            loaded[key] = value
    return loaded


def _build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="mailsync",
        description="Incrementally sync a remote IMAP mailbox into a Prism collection.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    p.add_argument("--init", action="store_true", help="Write a .env template and exit")
    p.add_argument("--env-file", default=".env", help="Path to the .env file to load")
    p.add_argument("--dry-run", action="store_true", help="Fetch + parse but never write")
    p.add_argument("--folder", action="append", dest="folders", metavar="NAME",
                   help="Only sync this folder (repeatable)")
    p.add_argument("--full-resync", action="store_true",
                   help="Ignore watermarks and re-index everything (idempotent)")
    p.add_argument("--limit", type=int, help="Stop after N messages")
    p.add_argument("--batch-size", type=int, help="Messages per bulk request")
    p.add_argument("--body-cap", type=int, help="Truncate stored plaintext body (bytes)")
    p.add_argument("--collection", help="Target Prism collection name")
    p.add_argument("--prism-url", help="Prism base URL")
    p.add_argument("--state", dest="state_path", help="Path to the SQLite state file")
    p.add_argument("--print-config", action="store_true",
                   help="Show the resolved config (secrets redacted) and exit")
    return p


_CLI_TO_ENV = {
    "batch_size": "MAILSYNC_BATCH_SIZE",
    "body_cap": "MAILSYNC_BODY_CAP",
    "collection": "MAILSYNC_COLLECTION",
    "prism_url": "MAILSYNC_PRISM_URL",
    "state_path": "MAILSYNC_STATE_PATH",
}


def _redacted(cfg: Config) -> str:
    def mask(v):
        return "***" if v else None
    lines = [
        f"imap:    {cfg.imap_user}@{cfg.imap_host}:{cfg.imap_port} "
        f"({cfg.imap_security}, auth={cfg.imap_auth}, tls_verify={cfg.imap_tls_verify})",
        f"ssh:     {'via ' + str(cfg.ssh_user) + '@' + str(cfg.ssh_host) if cfg.ssh_enabled else 'disabled'}",
        f"prism:   {cfg.prism_url} -> collection {cfg.collection!r} (api_key={mask(cfg.prism_api_key)})",
        f"tunables: batch={cfg.batch_size} body_cap={cfg.body_cap} state={cfg.state_path}",
    ]
    return "\n".join(lines)


def main(argv=None) -> int:
    args = _build_parser().parse_args(argv)

    if args.init:
        dest = Path(args.env_file)
        if dest.exists():
            print(f"{dest} already exists; not overwriting.", file=sys.stderr)
            return 1
        dest.write_text(ENV_TEMPLATE)
        print(f"Wrote {dest}. Fill it in, then run: uv run mailsync")
        return 0

    load_dotenv(Path(args.env_file))

    # CLI flags override env for the overridable knobs.
    for attr, env_key in _CLI_TO_ENV.items():
        val = getattr(args, attr, None)
        if val is not None:
            os.environ[env_key] = str(val)

    try:
        cfg = Config.from_env(os.environ)
    except ConfigError as e:
        print(f"config error: {e}\n\nRun 'mailsync --init' to create a .env template.",
              file=sys.stderr)
        return 2

    if args.print_config:
        print(_redacted(cfg))
        return 0

    # Import I/O adapters lazily so --init/--print-config work without a server.
    from .imap import ImapSource
    from .prism import PrismClient
    from .state import StateStore
    from .sync import sync

    print(_redacted(cfg))
    print("connecting...")
    try:
        with ImapSource(cfg) as source, PrismClient(cfg.prism_url, cfg.prism_api_key) as prism:
            state = StateStore(cfg.state_path)
            try:
                stats = sync(
                    cfg, source, prism, state,
                    only_folders=args.folders,
                    full_resync=args.full_resync,
                    limit=args.limit,
                    dry_run=args.dry_run,
                )
            finally:
                state.close()
    except Exception as e:
        print(f"\nsync aborted: {type(e).__name__}: {e}", file=sys.stderr)
        return 1

    print(
        f"\ndone: {stats.messages_indexed} indexed, {stats.messages_failed} failed, "
        f"{stats.folders_synced} folders ok, {stats.folders_failed} folders failed"
        + ("  [DRY RUN]" if args.dry_run else "")
    )
    return 1 if stats.folders_failed or stats.messages_failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
