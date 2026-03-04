"""Auto-sync Django model changes to Prism."""

from __future__ import annotations
import logging

from prismsearch_django.conf import get_client
from prismsearch_django.mixins import SearchableModel, _build_document

logger = logging.getLogger("prismsearch_django")


def post_save_handler(sender, instance, **kwargs):
    """Index document on save."""
    if not isinstance(instance, SearchableModel):
        return
    if not hasattr(sender, "PrismMeta"):
        return

    try:
        meta = sender.PrismMeta
        client = get_client()
        doc = _build_document(instance, meta)
        client.index(meta.collection, [doc])
    except Exception:
        logger.exception("Failed to index %s pk=%s to Prism", sender.__name__, instance.pk)


def post_delete_handler(sender, instance, **kwargs):
    """Remove document on delete (best-effort, Prism re-indexes on next bulk)."""
    if not isinstance(instance, SearchableModel):
        return
    if not hasattr(sender, "PrismMeta"):
        return

    # Prism doesn't have a single-document delete endpoint yet,
    # so we log the deletion for manual cleanup or next reindex.
    logger.info(
        "Document %s deleted from Django model %s — will be removed on next reindex",
        instance.pk,
        sender.__name__,
    )
