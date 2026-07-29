"""Configuration management for agentimport."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class SourceConfig:
    """Configuration for a specific source adapter."""

    enabled: bool = True
    roots: list[Path] = field(default_factory=list)


@dataclass
class Config:
    """Global configuration for agentimport."""

    prism_url: str = "http://localhost:3080"
    prism_api_key: str | None = None
    state_db: Path = Path("~/.local/share/agentimport/state.db").expanduser()

    # Content filters
    include_tool_results: bool = False
    include_thinking: bool = False
    roles: set[str] | None = None
    max_chars: int | None = None

    # Batch size for Prism indexing
    batch_size: int = 1000

    # Source configurations with platform-aware defaults
    sources: dict[str, SourceConfig] = field(default_factory=dict)

    def __post_init__(self) -> None:
        if not self.sources:
            self.sources = self._default_sources()
        # Ensure state DB parent dir exists
        self.state_db.parent.mkdir(parents=True, exist_ok=True)

    @staticmethod
    def _default_sources() -> dict[str, SourceConfig]:
        home = Path.home()
        return {
            "claude_code": SourceConfig(roots=[home / ".claude" / "projects"]),
            "codex": SourceConfig(roots=[home / ".codex-cli"]),
            "gemini": SourceConfig(roots=[home / ".gemini"]),
            "copilot": SourceConfig(roots=[home / ".copilot"]),
            "chatgpt_export": SourceConfig(enabled=False, roots=[]),
            "antigravity": SourceConfig(roots=[home / ".gemini" / "antigravity" / "brain"]),
        }

    def get_enabled_sources(self) -> list[str]:
        return [name for name, cfg in self.sources.items() if cfg.enabled]
