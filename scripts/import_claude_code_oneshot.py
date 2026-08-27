#!/usr/bin/env python3
"""One-shot import of Claude Code conversations from remote servers into Prism.

Harvests ~/.claude/projects/**/*.jsonl from a fixed list of (user, host) pairs
via rsync (read-only on the remotes — nothing is installed there), parses each
file with agentimport's ClaudeCodeSource, tags every message's `project` field
with its origin as `[<user>@<host_short>] <original_project>`, and indexes into
the global `agent_messages` + `agent_conversations` collections on the target
Prism server.

Idempotent: document IDs are deterministic SHA1 of (source, conversation_id,
native_msg_id), so re-running updates the same docs rather than duplicating.

Usage:
    clients/python/agentimport/.venv/bin/python scripts/import_claude_code_oneshot.py --dry-run
    clients/python/agentimport/.venv/bin/python scripts/import_claude_code_oneshot.py
    clients/python/agentimport/.venv/bin/python scripts/import_claude_code_oneshot.py --skip-rsync  # reuse last fetch

Environment:
    PRISM_URL   default http://192.168.88.212:3080
    PRISM_API_KEY  optional bearer token
"""

from __future__ import annotations

import argparse
import logging
import shutil
import subprocess
import sys
import tempfile
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

# --- agentimport lives in the repo; make it importable when run as a script ---
REPO_ROOT = Path(__file__).resolve().parent.parent
AGENTIMPORT_SRC = REPO_ROOT / "clients" / "python" / "agentimport" / "src"
if str(AGENTIMPORT_SRC) not in sys.path:
    sys.path.insert(0, str(AGENTIMPORT_SRC))
PRISMSEARCH_SRC = REPO_ROOT / "clients" / "python" / "prismsearch" / "src"
if str(PRISMSEARCH_SRC) not in sys.path:
    sys.path.insert(0, str(PRISMSEARCH_SRC))

from agentimport.models import NormalizedConversation, NormalizedMessage  # noqa: E402
from agentimport.prism import COLLECTION_CONVERSATIONS, COLLECTION_MESSAGES, PrismClient  # noqa: E402
from agentimport.sources.claude_code import ClaudeCodeSource  # noqa: E402

log = logging.getLogger("import_cc_oneshot")

# --- configuration -----------------------------------------------------------

DEFAULT_PRISM_URL = "http://192.168.88.212:3080"

# (user, host, short tag). short tag keeps facet values tidy.
SOURCES: list[tuple[str, str, str]] = [
    ("m", "192.168.88.35", "m@35"),
    ("mikalv", "192.168.88.35", "mikalv@35"),
    ("m", "192.168.88.195", "m@195"),
    ("mikalv", "192.168.88.195", "mikalv@195"),
]

REMOTE_DIR = "~/.claude/projects/"  # trailing slash → rsync copies contents
BATCH_SIZE = 500


@dataclass
class Origin:
    user: str
    host: str
    tag: str  # e.g. "m@35"
    local_root: Path  # .../<tag>/  where rsync lands the files


# --- rsync -------------------------------------------------------------------

def rsync_origin(origin: Origin) -> int:
    """Rsync ~/.claude/projects/ from origin.host into origin.local_root.

    Returns number of .jsonl files fetched. Idempotent: re-running refreshes.
    """
    if not shutil.which("rsync"):
        raise RuntimeError("rsync not found on PATH")
    origin.local_root.mkdir(parents=True, exist_ok=True)
    remote = f"{origin.user}@{origin.host}:{REMOTE_DIR}"
    cmd = [
        "rsync", "-az", "--delete",
        "--include=*.jsonl", "--include=*/", "--exclude=*",
        "-e", "ssh -o BatchMode=yes -o ConnectTimeout=10",
        remote, str(origin.local_root) + "/",
    ]
    log.info("rsync %s → %s", remote, origin.local_root)
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(
            f"rsync {origin.user}@{origin.host} failed (rc={proc.returncode}): {proc.stderr.strip()[:300]}"
        )
    files = sorted(origin.local_root.rglob("*.jsonl"))
    log.info("  fetched %d .jsonl files from %s (%.1f MB)", len(files), origin.tag,
             sum(f.stat().st_size for f in files) / 1e6)
    return len(files)


# --- parse + tag -------------------------------------------------------------

def parse_origin(origin: Origin, source: ClaudeCodeSource) -> list[NormalizedMessage]:
    """Parse all jsonl files for one origin, tagging each message's project."""
    files = list(source.discover([origin.local_root]))
    msgs: list[NormalizedMessage] = []
    for path in files:
        parsed = list(source.parse(path))
        for m in parsed:
            tag = f"[{origin.tag}]"
            m.project = f"{tag} {m.project}" if m.project else tag
            msgs.append(m)
    return msgs


# --- indexing ----------------------------------------------------------------

def index_messages(prism: PrismClient, msgs: list[NormalizedMessage], dry_run: bool) -> int:
    if dry_run:
        return len(msgs)
    total = 0
    for i in range(0, len(msgs), BATCH_SIZE):
        batch = msgs[i:i + BATCH_SIZE]
        total += prism.upsert_messages(batch, batch_size=len(batch))
        if (i // BATCH_SIZE) % 5 == 0:
            log.info("  messages: %d/%d indexed", total, len(msgs))
    return total


def index_conversations(prism: PrismClient, msgs: list[NormalizedMessage], dry_run: bool) -> int:
    convs = _aggregate(msgs)
    if dry_run:
        return len(convs)
    if not convs:
        return 0
    total = 0
    for i in range(0, len(convs), BATCH_SIZE):
        batch = convs[i:i + BATCH_SIZE]
        total += prism.upsert_conversations(batch, batch_size=len(batch))
    return total


def _aggregate(msgs: list[NormalizedMessage]) -> list[NormalizedConversation]:
    by_conv: dict[str, list[NormalizedMessage]] = defaultdict(list)
    for m in msgs:
        by_conv[m.conversation_id].append(m)
    out: list[NormalizedConversation] = []
    for messages in by_conv.values():
        if not messages:
            continue
        first = messages[0]
        timestamps = [m.ts for m in messages if m.ts]
        projects = [m.project for m in messages if m.project]
        models = [m.model for m in messages if m.model]
        title = None
        for m in messages:
            if m.role == "user" and m.content_type == "message":
                title = m.text[:200].split("\n")[0]
                break
        out.append(NormalizedConversation(
            conversation_id=first.conversation_id,
            source=first.source,
            title=title,
            project=projects[0] if projects else None,
            model=models[0] if models else None,
            started_at=min(timestamps) if timestamps else None,
            ended_at=max(timestamps) if timestamps else None,
            msg_count=len(messages),
            source_path=first.source_path,
        ))
    return out


# --- main --------------------------------------------------------------------

def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--prism-url", default=os_get("PRISM_URL", DEFAULT_PRISM_URL))
    ap.add_argument("--api-key", default=os_get("PRISM_API_KEY"))
    ap.add_argument("--work-dir", default=None, help="rsync target (default: tempdir)")
    ap.add_argument("--dry-run", action="store_true", help="parse + count, do not index")
    ap.add_argument("--skip-rsync", action="store_true", help="reuse files already in work-dir")
    ap.add_argument("--only", default=None, help="comma-separated tags to limit to, e.g. m@35,mikalv@195")
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()

    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.INFO,
        format="%(asctime)s %(levelname)-7s %(name)s — %(message)s",
        datefmt="%H:%M:%S",
    )

    work_dir = Path(args.work_dir) if args.work_dir else Path(tempfile.mkdtemp(prefix="cc_import_"))
    work_dir.mkdir(parents=True, exist_ok=True)
    log.info("work dir: %s", work_dir)

    origins = [Origin(u, h, t, work_dir / t) for (u, h, t) in SOURCES]
    if args.only:
        wanted = {x.strip() for x in args.only.split(",")}
        origins = [o for o in origins if o.tag in wanted]
        log.info("limited to tags: %s", [o.tag for o in origins])

    # 1. fetch
    if not args.skip_rsync:
        for o in origins:
            try:
                rsync_origin(o)
            except Exception as e:
                log.error("fetch failed for %s: %s", o.tag, e)
    else:
        log.info("skipping rsync; reusing %s", work_dir)

    # 2. parse
    cc = ClaudeCodeSource()
    all_msgs: list[NormalizedMessage] = []
    per_origin_counts: list[tuple[str, int, int]] = []  # (tag, files, msgs)
    for o in origins:
        msgs = parse_origin(o, cc)
        files_n = len(list(cc.discover([o.local_root])))
        all_msgs.extend(msgs)
        per_origin_counts.append((o.tag, files_n, len(msgs)))
        log.info("parsed %-12s : %3d files → %6d messages", o.tag, files_n, len(msgs))

    total_files = sum(c[1] for c in per_origin_counts)
    total_msgs = sum(c[2] for c in per_origin_counts)
    log.info("─" * 50)
    log.info("TOTAL: %d files, %d messages across %d origins", total_files, total_msgs, len(origins))

    if total_msgs == 0:
        log.warning("nothing to index; exiting")
        return 0
    if args.dry_run:
        log.info("DRY-RUN: would index %d messages + %d conversations into %s",
                 total_msgs, len(_aggregate(all_msgs)), args.prism_url)
        return 0

    # 3. index
    log.info("indexing into %s (collections: %s, %s)", args.prism_url, COLLECTION_MESSAGES, COLLECTION_CONVERSATIONS)
    with PrismClient(args.prism_url, args.api_key) as prism:
        prism.ensure_collections()
        n_msgs = index_messages(prism, all_msgs, dry_run=False)
        log.info("indexed %d/%d messages", n_msgs, total_msgs)
        n_convs = index_conversations(prism, all_msgs, dry_run=False)
        log.info("indexed %d conversations", n_convs)

    log.info("✓ done. work dir kept at %s (re-run with --skip-rsync to refresh index only)", work_dir)
    return 0


def os_get(key: str, default: str | None = None) -> str | None:
    import os
    return os.environ.get(key, default)


if __name__ == "__main__":
    raise SystemExit(main())
