"""Prism collection schema for indexed mail.

Field types must stay in sync with :func:`mailsync.message.message_to_doc`:
every key that mapping emits needs a field here, or Prism drops it silently.
"""

from __future__ import annotations


def _text(name: str) -> dict:
    return {"name": name, "type": "text", "stored": True, "indexed": True}


def _raw(name: str) -> dict:
    # Exact-match keyword field: no tokenization, filterable/facetable.
    return {
        "name": name,
        "type": "string",
        "stored": True,
        "indexed": True,
        "tokenizer": "raw",
    }


def _num(name: str, ftype: str) -> dict:
    return {"name": name, "type": ftype, "stored": True, "indexed": True}


def build_mail_schema(collection: str) -> dict:
    """Build the ``CollectionSchema`` JSON body for the mail collection."""
    fields = [
        _raw("message_id"),
        _raw("folder"),
        _text("subject"),
        _raw("from"),
        _text("from_name"),
        _text("to"),
        _text("cc"),
        _raw("date"),
        _num("date_epoch", "i64"),
        _text("body"),
        _raw("flags"),
        _num("has_attachments", "bool"),
        _num("size", "u64"),
        _num("uid", "u64"),
        _num("uidvalidity", "u64"),
    ]
    return {
        "collection": collection,
        "description": "Mail indexed from IMAP by prism-mailsync",
        "backends": {"text": {"fields": fields}},
    }
