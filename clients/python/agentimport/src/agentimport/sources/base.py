"""Source adapter protocol and registry."""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING, Iterable, Iterator, Protocol, runtime_checkable

if TYPE_CHECKING:
    from agentimport.models import NormalizedMessage


@runtime_checkable
class Source(Protocol):
    """Protocol for a conversation source adapter.

    Each adapter knows how to find and parse its source format into
    normalized messages. Adapters are pure functions over files — no Prism
    dependency, making them trivially unit-testable with fixture files.
    """

    @property
    def name(self) -> str:
        """Source identifier, e.g. 'claude_code'."""
        ...

    def default_roots(self) -> list[Path]:
        """Default filesystem roots to scan for this source."""
        ...

    def discover(self, roots: list[Path]) -> Iterable[Path]:
        """Find candidate files/archives to import from the given roots."""
        ...

    def parse(self, path: Path) -> Iterator[NormalizedMessage]:
        """Parse a single file into a stream of normalized messages."""
        ...


def get_all_sources() -> list[Source]:
    """Return instances of all available source adapters."""
    from agentimport.sources.antigravity import AntigravitySource
    from agentimport.sources.chatgpt_export import ChatGPTExportSource
    from agentimport.sources.claude_code import ClaudeCodeSource
    from agentimport.sources.cline_family import ClineSource, KiloSource, RooSource
    from agentimport.sources.codex import CodexSource
    from agentimport.sources.copilot import CopilotSource
    from agentimport.sources.cursor import CursorSource
    from agentimport.sources.gemini import GeminiSource
    from agentimport.sources.pi import PiSource

    return [
        ClaudeCodeSource(),
        CodexSource(),
        GeminiSource(),
        CopilotSource(),
        ChatGPTExportSource(),
        AntigravitySource(),
        PiSource(),
        CursorSource(),
        KiloSource(),
        ClineSource(),
        RooSource(),
    ]


def get_source_by_name(name: str) -> Source | None:
    """Look up a source adapter by name."""
    for source in get_all_sources():
        if source.name == name or (source.name == "chatgpt" and name == "chatgpt_export") or (source.name == "chatgpt_export" and name == "chatgpt"):
            return source
    return None
