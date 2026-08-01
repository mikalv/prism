"""Tests for Antigravity source adapter."""

import json
from pathlib import Path
import pytest
from agentimport.sources.antigravity import AntigravitySource


class TestAntigravityDiscover:
    def test_finds_transcripts(self, tmp_path: Path):
        source = AntigravitySource()
        session_dir = tmp_path / "session-123" / ".system_generated" / "logs"
        session_dir.mkdir(parents=True)
        transcript = session_dir / "transcript.jsonl"
        transcript.write_text('{"type": "USER_INPUT"}')

        discovered = list(source.discover([tmp_path]))
        assert len(discovered) == 1
        assert discovered[0] == transcript


class TestAntigravityParse:
    def test_parse_events(self, tmp_path: Path):
        source = AntigravitySource()
        transcript = tmp_path / "transcript.jsonl"
        
        events = [
            {"step_index": 0, "source": "USER_EXPLICIT", "type": "USER_INPUT", "created_at": "2026-07-29T14:07:19Z", "content": "<USER_REQUEST>\nHello Antigravity\n</USER_REQUEST>"},
            {"step_index": 1, "source": "MODEL", "type": "PLANNER_RESPONSE", "created_at": "2026-07-29T14:07:20Z", "content": "Hello user!", "tool_calls": [{"name": "list_dir", "args": {"DirectoryPath": "/tmp"}}]},
            {"step_index": 2, "source": "MODEL", "type": "LIST_DIRECTORY", "created_at": "2026-07-29T14:07:21Z", "content": "dir content"}
        ]
        
        transcript.write_text("\n".join(json.dumps(e) for e in events))

        msgs = list(source.parse(transcript))
        assert len(msgs) == 4
        
        # User message
        assert msgs[0].role == "user"
        assert msgs[0].text == "Hello Antigravity"
        assert msgs[0].source == "antigravity"
        
        # Assistant message
        assert msgs[1].role == "assistant"
        assert msgs[1].text == "Hello user!"
        
        # Tool call
        assert msgs[2].role == "assistant"
        assert msgs[2].content_type == "tool_call"
        assert msgs[2].tool_name == "list_dir"
        
        # Tool result
        assert msgs[3].role == "tool"
        assert msgs[3].content_type == "tool_result"
        assert msgs[3].tool_name == "list_directory"
