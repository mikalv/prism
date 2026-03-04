"""Prismsearch Django configuration."""

from __future__ import annotations
from typing import Any

from django.conf import settings
from prismsearch.client import Prismsearch

_client: Prismsearch | None = None


def get_settings() -> dict[str, Any]:
    """Get PRISMSEARCH settings dict from Django settings."""
    return getattr(settings, "PRISMSEARCH", {})


def get_client() -> Prismsearch:
    """Get or create the singleton Prismsearch client."""
    global _client
    if _client is None:
        conf = get_settings()
        _client = Prismsearch(
            base_url=conf.get("URL", "http://localhost:3080"),
            api_key=conf.get("API_KEY"),
        )
    return _client
