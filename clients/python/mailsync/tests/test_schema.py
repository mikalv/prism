"""Tests for the mail collection schema builder."""

from mailsync.schema import build_mail_schema


def _fields(schema):
    return {f["name"]: f for f in schema["backends"]["text"]["fields"]}


def test_schema_has_collection_name_and_text_backend():
    schema = build_mail_schema("mail")
    assert schema["collection"] == "mail"
    assert "text" in schema["backends"]


def test_body_and_subject_are_tokenized_text():
    fields = _fields(build_mail_schema("mail"))
    assert fields["subject"]["type"] == "text"
    assert fields["body"]["type"] == "text"
    assert fields["subject"]["indexed"] is True
    assert fields["body"]["stored"] is True


def test_message_id_and_folder_are_raw_strings():
    fields = _fields(build_mail_schema("mail"))
    assert fields["message_id"]["type"] == "string"
    assert fields["message_id"].get("tokenizer") == "raw"
    assert fields["folder"]["type"] == "string"


def test_date_epoch_is_i64_for_sorting():
    fields = _fields(build_mail_schema("mail"))
    assert fields["date_epoch"]["type"] == "i64"


def test_all_message_doc_fields_are_present_in_schema():
    # Every key produced by message_to_doc must have a schema field, or Prism
    # would silently drop it.
    from mailsync.message import message_to_doc

    md = message_to_doc(
        b"From: a@b\nSubject: s\nMessage-ID: <1@x>\n\nbody",
        folder="INBOX",
        uid=1,
        uidvalidity=1,
    )
    fields = _fields(build_mail_schema("mail"))
    for key in md.fields:
        assert key in fields, f"schema missing field {key!r}"
