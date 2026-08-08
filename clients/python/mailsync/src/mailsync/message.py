"""Pure mapping from a raw RFC 822 message to a Prism document.

Kept free of IMAP and HTTP concerns so it can be unit-tested against raw bytes.
"""

from __future__ import annotations

import html as _html
import re
from dataclasses import dataclass
from email import message_from_bytes
from email.policy import default as default_policy
from email.utils import getaddresses, parseaddr, parsedate_to_datetime

_TAG_RE = re.compile(r"<[^>]+>")
_WS_RE = re.compile(r"[ \t\r\f\v]*\n[ \t\r\f\v]*")


@dataclass(frozen=True)
class MailDoc:
    """A message rendered as a Prism document plus its stable `_id`."""

    doc_id: str
    fields: dict


def _html_to_text(html: str) -> str:
    text = _TAG_RE.sub(" ", html)
    return _html.unescape(text)


def _extract_body(msg, body_cap: int) -> str:
    body_part = msg.get_body(preferencelist=("plain", "html"))
    if body_part is None:
        return ""
    try:
        content = body_part.get_content()
    except (LookupError, ValueError):
        payload = body_part.get_payload(decode=True) or b""
        content = payload.decode("utf-8", errors="replace")
    if body_part.get_content_type() == "text/html":
        content = _html_to_text(content)
    content = _WS_RE.sub("\n", content).strip()
    if body_cap and len(content) > body_cap:
        content = content[:body_cap]
    return content


def message_to_doc(
    raw: bytes,
    *,
    folder: str,
    uid: int,
    uidvalidity: int,
    flags=(),
    body_cap: int = 1_000_000,
) -> MailDoc:
    """Parse raw message bytes into a :class:`MailDoc`."""
    msg = message_from_bytes(raw, policy=default_policy)

    message_id = (msg.get("Message-ID") or "").strip()
    doc_id = message_id or f"{folder}:{uidvalidity}:{uid}"

    from_name, from_addr = parseaddr(msg.get("From", ""))
    to_addrs = [addr for _, addr in getaddresses(msg.get_all("To", [])) if addr]
    cc_addrs = [addr for _, addr in getaddresses(msg.get_all("Cc", [])) if addr]

    date_str = ""
    date_epoch = 0
    raw_date = msg.get("Date")
    if raw_date:
        try:
            dt = parsedate_to_datetime(raw_date)
            if dt is not None:
                date_str = dt.isoformat()
                date_epoch = int(dt.timestamp())
        except (TypeError, ValueError):
            pass

    body = _extract_body(msg, body_cap)

    fields = {
        "message_id": doc_id,
        "folder": folder,
        "subject": (msg.get("Subject") or "").strip(),
        "from": from_addr,
        "from_name": from_name,
        "to": ", ".join(to_addrs),
        "cc": ", ".join(cc_addrs),
        "date": date_str,
        "date_epoch": date_epoch,
        "body": body,
        "flags": " ".join(flags),
        "has_attachments": _has_attachments(msg),
        "size": len(raw),
        "uid": uid,
        "uidvalidity": uidvalidity,
    }
    return MailDoc(doc_id=doc_id, fields=fields)


def _has_attachments(msg) -> bool:
    for part in msg.walk():
        if part.get_content_disposition() == "attachment":
            return True
    return False
