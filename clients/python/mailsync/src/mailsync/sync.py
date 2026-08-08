"""Sync orchestration: tie IMAP, message mapping, bulk indexing, and state.

Kept dependency-injected (source/prism/state passed in) so the control flow is
testable and the I/O adapters stay thin.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Callable, List, Optional

from .bulk import advance_watermark, results_from_response
from .config import Config
from .message import message_to_doc
from .schema import build_mail_schema


@dataclass
class SyncStats:
    folders_synced: int = 0
    folders_failed: int = 0
    messages_indexed: int = 0
    messages_failed: int = 0
    errors: List[str] = field(default_factory=list)


def _chunks(seq, size):
    for i in range(0, len(seq), size):
        yield seq[i : i + size]


def sync(
    cfg: Config,
    source,
    prism,
    state,
    *,
    only_folders: Optional[List[str]] = None,
    full_resync: bool = False,
    limit: Optional[int] = None,
    dry_run: bool = False,
    log: Callable[[str], None] = print,
) -> SyncStats:
    stats = SyncStats()

    if not dry_run:
        created = prism.ensure_collection(build_mail_schema(cfg.collection))
        log(f"collection {cfg.collection!r}: {'created' if created else 'exists'}")

    folders = source.list_folders()
    if only_folders:
        wanted = set(only_folders)
        folders = [f for f in folders if f in wanted]
    log(f"{len(folders)} folder(s) to scan")

    for folder in folders:
        if limit is not None and stats.messages_indexed >= limit:
            break
        try:
            uidvalidity = source.select_folder(folder)
            watermark = 0 if full_resync else state.get_watermark(cfg.account, folder, uidvalidity)
            uids = source.search_since(watermark)
            if not uids:
                stats.folders_synced += 1
                continue
            log(f"  {folder}: {len(uids)} new message(s) (uidvalidity={uidvalidity})")

            for chunk in _chunks(uids, cfg.batch_size):
                if limit is not None:
                    remaining = limit - stats.messages_indexed
                    if remaining <= 0:
                        break
                    chunk = chunk[:remaining]

                docs = []
                for uid, raw, flags in source.fetch_messages(chunk):
                    if not raw:
                        continue
                    docs.append(
                        message_to_doc(
                            raw,
                            folder=folder,
                            uid=uid,
                            uidvalidity=uidvalidity,
                            flags=flags,
                            body_cap=cfg.body_cap,
                        )
                    )
                if not docs:
                    continue

                if dry_run:
                    stats.messages_indexed += len(docs)
                    continue

                resp = prism.bulk_index(cfg.collection, docs)
                results = results_from_response(docs, resp)
                ok = sum(1 for _, st in results if st < 400)
                stats.messages_indexed += ok
                stats.messages_failed += len(results) - ok

                wm = advance_watermark(results)
                if wm is not None and wm > watermark:
                    state.set_watermark(cfg.account, folder, uidvalidity, wm)
                    watermark = wm

            stats.folders_synced += 1
        except Exception as e:  # folder isolation: one bad folder must not abort
            stats.folders_failed += 1
            msg = f"  {folder}: ERROR {type(e).__name__}: {e}"
            stats.errors.append(msg)
            log(msg)

    return stats
