"""Tests for the pure NDJSON builder and watermark-advancement logic."""

import json

from mailsync.bulk import advance_watermark, build_ndjson
from mailsync.message import MailDoc


def _doc(doc_id, uid):
    return MailDoc(doc_id=doc_id, fields={"message_id": doc_id, "uid": uid, "subject": "s"})


def test_build_ndjson_pairs_action_and_document():
    docs = [_doc("<a@x>", 1), _doc("<b@x>", 2)]
    body = build_ndjson("mail", docs)

    lines = body.splitlines()
    assert len(lines) == 4  # action + doc, twice
    assert json.loads(lines[0]) == {"index": {"_index": "mail", "_id": "<a@x>"}}
    assert json.loads(lines[1])["uid"] == 1
    assert json.loads(lines[2]) == {"index": {"_index": "mail", "_id": "<b@x>"}}
    assert body.endswith("\n")  # ES requires a trailing newline


def test_build_ndjson_empty():
    assert build_ndjson("mail", []) == ""


def test_watermark_all_succeed_returns_max_uid():
    assert advance_watermark([(1, 201), (2, 201), (3, 201)]) == 3


def test_watermark_stops_before_first_failure():
    # uid 3 failed -> we must not advance past it, even though uid 4 succeeded
    assert advance_watermark([(1, 201), (2, 201), (3, 500), (4, 201)]) == 2


def test_watermark_first_failure_returns_none():
    assert advance_watermark([(1, 429), (2, 201)]) is None


def test_watermark_empty_returns_none():
    assert advance_watermark([]) is None


def test_watermark_sorts_by_uid_before_scanning():
    # response order is not guaranteed to match uid order
    assert advance_watermark([(3, 201), (1, 201), (2, 500)]) == 1


from mailsync.bulk import results_from_response


def test_results_from_response_zips_docs_with_item_status():
    docs = [_doc("<a@x>", 5), _doc("<b@x>", 6)]
    resp = {
        "took": 3,
        "errors": False,
        "items": [
            {"index": {"_id": "<a@x>", "status": 201}},
            {"index": {"_id": "<b@x>", "status": 201}},
        ],
    }
    assert results_from_response(docs, resp) == [(5, 201), (6, 201)]


def test_results_from_response_reads_error_status():
    docs = [_doc("<a@x>", 5), _doc("<b@x>", 6)]
    resp = {
        "items": [
            {"index": {"_id": "<a@x>", "status": 201}},
            {"index": {"_id": "<b@x>", "status": 500, "error": {"reason": "boom"}}},
        ]
    }
    assert results_from_response(docs, resp) == [(5, 201), (6, 500)]
