"""Tests for agentimport models — deterministic IDs and content filtering."""

from datetime import datetime, timezone

from agentimport.models import ContentFilter, NormalizedMessage


def _make_msg(**kwargs) -> NormalizedMessage:
    defaults = dict(
        conversation_id="conv-1",
        source="claude_code",
        role="assistant",
        content_type="message",
        text="Hello, world!",
        seq=0,
        source_path="/tmp/test.jsonl",
    )
    defaults.update(kwargs)
    return NormalizedMessage(**defaults)


class TestDeterministicId:
    def test_stable_with_native_id(self):
        msg = _make_msg(native_msg_id="msg-abc-123")
        id1 = msg.deterministic_id()
        id2 = msg.deterministic_id()
        assert id1 == id2
        assert len(id1) == 16

    def test_different_msgs_different_ids(self):
        msg1 = _make_msg(native_msg_id="msg-1")
        msg2 = _make_msg(native_msg_id="msg-2")
        assert msg1.deterministic_id() != msg2.deterministic_id()

    def test_fallback_to_seq(self):
        msg = _make_msg(native_msg_id=None, seq=42)
        id1 = msg.deterministic_id()
        assert len(id1) == 16
        # Different seq → different ID
        msg2 = _make_msg(native_msg_id=None, seq=43)
        assert id1 != msg2.deterministic_id()

    def test_different_sources_different_ids(self):
        msg1 = _make_msg(source="claude_code", native_msg_id="msg-1")
        msg2 = _make_msg(source="codex", native_msg_id="msg-1")
        assert msg1.deterministic_id() != msg2.deterministic_id()


class TestToPrismDoc:
    def test_basic_fields(self):
        msg = _make_msg(project="myapp", model="claude-4")
        doc = msg.to_prism_doc()
        assert doc["id"] == msg.deterministic_id()
        assert doc["fields"]["text"] == "Hello, world!"
        assert doc["fields"]["source"] == "claude_code"
        assert doc["fields"]["project"] == "myapp"
        assert doc["fields"]["model"] == "claude-4"

    def test_optional_fields_omitted(self):
        msg = _make_msg()
        doc = msg.to_prism_doc()
        assert "tool_name" not in doc["fields"]
        assert "project" not in doc["fields"]
        assert "model" not in doc["fields"]

    def test_timestamp_serialized(self):
        ts = datetime(2025, 7, 29, 10, 0, 0, tzinfo=timezone.utc)
        msg = _make_msg(ts=ts)
        doc = msg.to_prism_doc()
        assert doc["fields"]["ts"] == "2025-07-29T10:00:00+00:00"


class TestContentFilter:
    def test_default_excludes_tool_results(self):
        f = ContentFilter()
        assert not f.should_include(_make_msg(content_type="tool_result"))

    def test_default_excludes_thinking(self):
        f = ContentFilter()
        assert not f.should_include(_make_msg(content_type="thinking"))

    def test_default_includes_messages(self):
        f = ContentFilter()
        assert f.should_include(_make_msg(content_type="message"))

    def test_default_includes_tool_calls(self):
        f = ContentFilter()
        assert f.should_include(_make_msg(content_type="tool_call"))

    def test_include_tool_results_flag(self):
        f = ContentFilter(include_tool_results=True)
        assert f.should_include(_make_msg(content_type="tool_result"))

    def test_include_thinking_flag(self):
        f = ContentFilter(include_thinking=True)
        assert f.should_include(_make_msg(content_type="thinking"))

    def test_roles_filter(self):
        f = ContentFilter(roles={"user"})
        assert f.should_include(_make_msg(role="user"))
        assert not f.should_include(_make_msg(role="assistant"))

    def test_max_chars_truncation(self):
        f = ContentFilter(max_chars=5)
        msg = _make_msg(text="Hello, world!")
        result = f.apply_limits(msg)
        assert result.text == "Hello…"
        assert len(result.text) == 6  # 5 chars + ellipsis

    def test_max_chars_no_truncation_if_short(self):
        f = ContentFilter(max_chars=100)
        msg = _make_msg(text="Hi")
        result = f.apply_limits(msg)
        assert result.text == "Hi"
