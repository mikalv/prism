"""Thin HTTP adapter for Prism: create the collection and bulk-index documents."""

from __future__ import annotations

import time
from typing import Optional, Sequence

import httpx

from .bulk import build_ndjson
from .message import MailDoc


class PrismError(RuntimeError):
    pass


class PrismClient:
    def __init__(self, base_url: str, api_key: Optional[str] = None, timeout: float = 120.0):
        self.base_url = base_url.rstrip("/")
        headers = {}
        if api_key:
            headers["Authorization"] = f"Bearer {api_key}"
        self._http = httpx.Client(base_url=self.base_url, headers=headers, timeout=timeout)

    def close(self) -> None:
        self._http.close()

    def __enter__(self) -> "PrismClient":
        return self

    def __exit__(self, *exc) -> None:
        self.close()

    def ensure_collection(self, schema: dict) -> bool:
        """Create the collection if absent. Returns True if newly created.

        A 409 (already exists) is treated as success — the tool is idempotent.
        """
        name = schema["collection"]
        resp = self._http.put(f"/collections/{name}", json=schema)
        if resp.status_code in (200, 201):
            return True
        if resp.status_code == 409:
            return False
        raise PrismError(
            f"create collection {name!r} failed: {resp.status_code} {resp.text[:500]}"
        )

    def bulk_index(self, collection: str, docs: Sequence[MailDoc], retries: int = 3) -> dict:
        """POST an ES ``_bulk`` batch, retrying transient network/5xx failures."""
        body = build_ndjson(collection, docs)
        if not body:
            return {"took": 0, "errors": False, "items": []}

        last_err: Optional[Exception] = None
        for attempt in range(retries):
            try:
                resp = self._http.post(
                    "/_elastic/_bulk",
                    content=body.encode("utf-8"),
                    headers={"Content-Type": "application/x-ndjson"},
                )
                if resp.status_code >= 500:
                    raise PrismError(f"bulk HTTP {resp.status_code}: {resp.text[:300]}")
                if resp.status_code >= 400:
                    # 4xx is a client error (bad batch) — no point retrying.
                    raise PrismError(f"bulk HTTP {resp.status_code}: {resp.text[:300]}")
                return resp.json()
            except (httpx.TransportError, PrismError) as e:
                last_err = e
                if attempt == retries - 1:
                    break
                time.sleep(2 ** attempt)
        raise PrismError(f"bulk failed after {retries} attempts: {last_err}")
