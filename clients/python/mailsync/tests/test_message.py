"""Tests for the pure email -> Prism document mapping."""

from mailsync.message import message_to_doc

SIMPLE = b"""From: Alice Example <alice@example.com>
To: Bob <bob@example.org>, carol@example.org
Cc: dave@example.net
Subject: Hello there
Date: Wed, 16 Jul 2025 10:30:00 +0200
Message-ID: <abc123@example.com>
Content-Type: text/plain; charset=utf-8

This is the body.
Second line.
"""


def test_maps_core_headers_and_body():
    md = message_to_doc(SIMPLE, folder="INBOX", uid=42, uidvalidity=1000, flags=["\\Seen"])

    assert md.doc_id == "<abc123@example.com>"
    d = md.fields
    assert d["message_id"] == "<abc123@example.com>"
    assert d["folder"] == "INBOX"
    assert d["subject"] == "Hello there"
    assert d["from"] == "alice@example.com"
    assert d["from_name"] == "Alice Example"
    assert "bob@example.org" in d["to"]
    assert "carol@example.org" in d["to"]
    assert d["cc"] == "dave@example.net"
    assert "This is the body." in d["body"]
    assert "Second line." in d["body"]
    assert d["uid"] == 42
    assert d["uidvalidity"] == 1000
    assert d["flags"] == "\\Seen"


def test_date_parsed_to_epoch_seconds():
    md = message_to_doc(SIMPLE, folder="INBOX", uid=1, uidvalidity=1)
    # 2025-07-16 10:30:00 +0200 == 08:30:00 UTC == 1752654600
    assert md.fields["date_epoch"] == 1752654600
    assert md.fields["date"].startswith("2025-07-16T")


HTML_ONLY = b"""From: x@example.com
Subject: HTML mail
Message-ID: <h1@example.com>
Content-Type: text/html; charset=utf-8

<html><body><p>Hello&nbsp;<b>world</b></p></body></html>
"""

NO_MSGID = b"""From: x@example.com
Subject: No id
Content-Type: text/plain

body
"""

WITH_ATTACH = b"""From: x@example.com
Subject: Has attachment
Message-ID: <a1@example.com>
Content-Type: multipart/mixed; boundary="B"

--B
Content-Type: text/plain

see attached
--B
Content-Type: application/pdf; name="doc.pdf"
Content-Disposition: attachment; filename="doc.pdf"

JVBERi0=
--B--
"""


def test_html_only_body_is_stripped_to_text():
    md = message_to_doc(HTML_ONLY, folder="INBOX", uid=1, uidvalidity=1)
    assert "Hello" in md.fields["body"]
    assert "world" in md.fields["body"]
    assert "<b>" not in md.fields["body"]


def test_missing_message_id_falls_back_to_folder_uid():
    md = message_to_doc(NO_MSGID, folder="Archive", uid=7, uidvalidity=99)
    assert md.doc_id == "Archive:99:7"
    assert md.fields["message_id"] == "Archive:99:7"


def test_attachment_is_detected():
    md = message_to_doc(WITH_ATTACH, folder="INBOX", uid=1, uidvalidity=1)
    assert md.fields["has_attachments"] is True
    assert "see attached" in md.fields["body"]


def test_body_is_capped():
    big = b"From: x@example.com\nSubject: big\nMessage-ID: <b@x>\nContent-Type: text/plain\n\n" + b"a" * 5000
    md = message_to_doc(big, folder="INBOX", uid=1, uidvalidity=1, body_cap=1000)
    assert len(md.fields["body"]) == 1000
