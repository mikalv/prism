"""Tests for Claude Code source adapter."""

import json
from pathlib import Path

from agentimport.sources.claude_code import ClaudeCodeSource


def _write_jsonl(path: Path, events: list[dict]) -> None:
    with open(path, "w") as f:
        for event in events:
            f.write(json.dumps(event) + "\n")


class TestClaudeCodeDiscover:
    def test_finds_jsonl_files(self, tmp_path):
        root = tmp_path / ".claude" / "projects" / "abc123"
        root.mkdir(parents=True)
        (root / "session1.jsonl").write_text("")
        (root / "session2.jsonl").write_text("")
        (root / "other.txt").write_text("")

        source = ClaudeCodeSource()
        files = list(source.discover([tmp_path / ".claude" / "projects"]))
        assert len(files) == 2
        assert all(f.suffix == ".jsonl" for f in files)

    def test_skips_nonexistent_root(self, tmp_path):
        source = ClaudeCodeSource()
        files = list(source.discover([tmp_path / "nonexistent"]))
        assert files == []


class TestClaudeCodeParse:
    def test_simple_text_message(self, tmp_path):
        path = tmp_path / "session.jsonl"
        _write_jsonl(path, [
            {
                "type": "human",
                "message": {"role": "user", "content": "Hello"},
                "sessionId": "sess-1",
                "timestamp": "2025-07-29T10:00:00Z",
                "uuid": "msg-001",
            }
        ])

        source = ClaudeCodeSource()
        msgs = list(source.parse(path))
        assert len(msgs) == 1
        assert msgs[0].role == "user"
        assert msgs[0].text == "Hello"
        assert msgs[0].source == "claude_code"
        assert msgs[0].conversation_id == "sess-1"

    def test_content_blocks(self, tmp_path):
        path = tmp_path / "session.jsonl"
        _write_jsonl(path, [
            {
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "Let me help you."},
                        {"type": "tool_use", "name": "read_file", "id": "tool-1", "input": {"path": "/tmp/foo"}},
                    ],
                },
                "sessionId": "sess-1",
                "timestamp": "2025-07-29T10:00:01Z",
                "uuid": "msg-002",
            }
        ])

        source = ClaudeCodeSource()
        msgs = list(source.parse(path))
        assert len(msgs) == 2

        # Text message
        assert msgs[0].content_type == "message"
        assert msgs[0].text == "Let me help you."

        # Tool call
        assert msgs[1].content_type == "tool_call"
        assert msgs[1].tool_name == "read_file"
        assert "read_file" in msgs[1].text

    def test_tool_result_block(self, tmp_path):
        path = tmp_path / "session.jsonl"
        _write_jsonl(path, [
            {
                "type": "human",
                "message": {
                    "role": "user",
                    "content": [
                        {"type": "tool_result", "tool_use_id": "tool-1", "content": "file contents here"},
                    ],
                },
                "sessionId": "sess-1",
                "uuid": "msg-003",
            }
        ])

        source = ClaudeCodeSource()
        msgs = list(source.parse(path))
        assert len(msgs) == 1
        assert msgs[0].content_type == "tool_result"
        assert msgs[0].text == "file contents here"

    def test_thinking_block(self, tmp_path):
        path = tmp_path / "session.jsonl"
        _write_jsonl(path, [
            {
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "thinking", "thinking": "Let me think about this..."},
                        {"type": "text", "text": "Here is my answer."},
                    ],
                },
                "sessionId": "sess-1",
                "uuid": "msg-004",
            }
        ])

        source = ClaudeCodeSource()
        msgs = list(source.parse(path))
        assert len(msgs) == 2
        assert msgs[0].content_type == "thinking"
        assert msgs[0].text == "Let me think about this..."
        assert msgs[1].content_type == "message"

    def test_malformed_json_skipped(self, tmp_path):
        path = tmp_path / "session.jsonl"
        path.write_text('{"valid": true}\nnot json\n{"also": "valid"}\n')

        source = ClaudeCodeSource()
        # Should not crash, just skip the bad line
        msgs = list(source.parse(path))
        # The valid lines don't have message/role so won't produce messages,
        # but the point is no crash

    def test_cwd_as_project(self, tmp_path):
        path = tmp_path / "session.jsonl"
        _write_jsonl(path, [
            {
                "type": "human",
                "message": {"role": "user", "content": "test"},
                "sessionId": "sess-1",
                "cwd": "/Users/me/projects/myapp",
            }
        ])

        source = ClaudeCodeSource()
        msgs = list(source.parse(path))
        assert msgs[0].project == "/Users/me/projects/myapp"

    def test_deterministic_ids_stable(self, tmp_path):
        path = tmp_path / "session.jsonl"
        _write_jsonl(path, [
            {
                "type": "human",
                "message": {"role": "user", "content": "Hello"},
                "sessionId": "sess-1",
                "uuid": "msg-001",
            }
        ])

        source = ClaudeCodeSource()
        msgs1 = list(source.parse(path))
        msgs2 = list(source.parse(path))
        assert msgs1[0].deterministic_id() == msgs2[0].deterministic_id()
