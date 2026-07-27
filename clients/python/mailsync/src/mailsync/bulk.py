"""Pure helpers for the Elasticsearch ``_bulk`` protocol and watermark logic."""

from __future__ import annotations

import json
from typing import Iterable, Optional, Sequence, Tuple

from .message import MailDoc


def build_ndjson(collection: str, docs: Sequence[MailDoc]) -> str:
    """Render docs as an ES ``_bulk`` NDJSON body (``index`` actions, upsert by id).

    Returns ``""`` for an empty batch. The body ends with a newline, which the
    ES ``_bulk`` protocol requires.
    """
    if not docs:
        return ""
    out = []
    for d in docs:
        action = {"index": {"_index": collection, "_id": d.doc_id}}
        out.append(json.dumps(action, separators=(",", ":")))
        out.append(json.dumps(d.fields, separators=(",", ":"), default=str))
    return "\n".join(out) + "\n"


def results_from_response(docs: Sequence[MailDoc], response: dict) -> list:
    """Zip sent docs with the ``_bulk`` response items into ``(uid, status)``.

    ES returns ``items`` in the same order as the actions were sent, so the
    positional zip is reliable. An action wraps its result under one of
    ``index`` / ``create`` / ``delete``; we read whichever is present.
    """
    items = response.get("items", [])
    out = []
    for doc, item in zip(docs, items):
        result = item.get("index") or item.get("create") or item.get("delete") or {}
        status = int(result.get("status", 0))
        out.append((int(doc.fields["uid"]), status))
    return out


def advance_watermark(results: Iterable[Tuple[int, int]]) -> Optional[int]:
    """Highest UID safe to checkpoint given ``(uid, http_status)`` bulk results.

    Advances only over the contiguous successful prefix (ascending by UID):
    the first item with ``status >= 400`` halts advancement, so a failed message
    is retried on the next run instead of being skipped. Returns ``None`` if the
    lowest-UID item failed or there are no results.
    """
    ordered = sorted(results, key=lambda r: r[0])
    watermark: Optional[int] = None
    for uid, status in ordered:
        if status >= 400:
            break
        watermark = uid
    return watermark
