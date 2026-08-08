# agentimport

Import AI assistant conversations into [Prism](https://github.com/mikalv/prism) for cross-tool search, memory/MCP lookup, and analytics.

## Supported Sources

| Source | Name(s) | Format | Default Root |
|--------|---------|--------|-------------|
| Claude Code | `claude_code` | JSONL | `~/.claude/projects/` |
| Codex CLI | `codex` | JSON/JSONL | `~/.codex-cli/` |
| Gemini CLI | `gemini` | JSON/JSONL | `~/.gemini/` |
| GitHub Copilot | `copilot` | JSON | `~/.copilot/` + VS Code workspaceStorage |
| ChatGPT Export | `chatgpt` | ZIP / `conversations.json` | (user-provided) |
| Antigravity | `antigravity` | JSON | Antigravity storage |
| PI (pi.dev) | `pi` | JSONL event stream | `~/.pi/agent/sessions/` |
| Cursor | `cursor` | SQLite `state.vscdb` (`cursorDiskKV`) | Cursor `globalStorage/` |
| Kilo Code | `kilo` | Cline-family task JSON | `…/globalStorage/kilocode.kilo-code/tasks/` |
| Cline | `cline` | Cline-family task JSON | `…/globalStorage/saoudrizwan.claude-dev/tasks/` |
| Roo Code | `roo` | Cline-family task JSON | `…/globalStorage/rooveterinaryinc.roo-cline/tasks/` |
| opencode | `opencode` | SQLite `opencode.db` or `storage/` JSON | `~/.local/share/opencode/` |

The `Name(s)` column is what you pass to `--sources` (e.g. `--sources pi,cursor,kilo`).

## Install

```bash
cd clients/python/agentimport
uv sync
```

## Quick Start

```bash
# Apply schemas to Prism
uv run agentimport schema --apply

# Import from all enabled sources
uv run agentimport run

# Import only Claude Code conversations
uv run agentimport run --sources claude_code

# Include tool results and thinking blocks
uv run agentimport run --include-tool-results --include-thinking

# Search across imported conversations
uv run python quicksearch.py "how to fix auth"
uv run python quicksearch.py "database migration" --source claude_code --limit 5
```

## Usage Examples

### 1. Running an Import
When you run the importer, it will scan the configured sources and index new messages into Prism incrementally:

```bash
uv run agentimport run --sources claude_code,gemini
```
*Output:*
```text
14:39:46 INFO     agentimport.pipeline — Import complete: ImportStats(scanned=4134, imported=1849, skipped=2285, messages=56320, conversations=1418, errors=0)

✓ Import complete
  Files: 4134 scanned, 1849 imported, 2285 skipped
  Messages indexed: 56320
  Conversations indexed: 1418
```

### 2. Checking Statistics
You can check how many files have been imported per source, and the total documents in Prism:

```bash
uv run agentimport stats
```
*Output:*
```text
── State DB ──
  claude_code: 1462 files imported
  gemini: 2672 files imported

── Prism Collections ──
  agent_conversations: 1579 docs (4000 bytes)
  agent_messages: 73222 docs (0 bytes)
```

### 3. Searching with Quicksearch
The `quicksearch.py` script allows you to rapidly query your indexed conversations:

```bash
uv run python quicksearch.py "test"
```
*Output:*
```text
Found 4106 results:

#1 [claude_code] assistant (tool_call) score=6.04
  project=/Users/mikalv/Repos/MeehProjects/meeh-chemistry/mychemicalinventory/claudestine  ts=2026-07-29T08:41:11Z  conv=9055046c-477
  Tool: Bash
  Input: {
    "command": "cd claudestine/apps/session 2>/dev/null && mix test test/roles_test.exs test/role_schema_test.exs ..."
  ...

#2 [claude_code] assistant (tool_call) score=6.01
  project=/Users/mikalv/Repos/MeehProjects/meeh-chemistry/mychemicalinventory/claudestine/apps/session  ts=2026-07-29T08:41:58Z  conv=9055046c-477
  Tool: Bash
  Input: {
    "command": "cd /Users/mikalv/Repos/MeehProjects/meeh-chemistry/mychemicalinventory/claudestine/apps/session && mix test test/roles_test.exs ..."
  ...
```

## CLI Reference

```
agentimport run     [--sources SRC,...] [--include-tool-results] [--include-thinking]
                    [--roles user,assistant] [--max-chars N] [--batch-size N]

agentimport daemon  [--interval 300] [--watch] [--sources SRC,...]

agentimport schema  [--apply]         # Show/apply Prism collection schemas
agentimport stats                     # Show import statistics
```

### Global Options

```
--prism-url URL     Prism server URL (env: PRISM_URL, default: http://localhost:3080)
--api-key KEY       Prism API key (env: PRISM_API_KEY)
--state-db PATH     State database path (env: AGENTIMPORT_STATE_DB)
-v, --verbose       Debug logging
```

## Prism Collections

### `agent_messages`

One document per message. Fields: `text` (code tokenizer), `conversation_id`, `source`, `role`, `content_type`, `tool_name`, `project`, `model`, `seq`, `ts`, `source_path`.

Facets on `source`, `role`, `content_type`, `model`, `project`. Recency boost on `ts` (30d exponential decay).

### `agent_conversations`

One document per conversation. Fields: `title`, `conversation_id`, `source`, `project`, `model`, `started_at`, `ended_at`, `msg_count`, `source_path`.

## Design Principles

- **Append/upsert-only**: Never deletes from Prism, even if source GCs (e.g. Claude deleting old sessions)
- **Deterministic IDs**: SHA1-based IDs ensure idempotent re-imports
- **Incremental**: SQLite state file tracks mtime+size watermarks per file
- **Pluggable**: Each source adapter is a pure function (file → messages), easy to add new ones

## Daemon Mode

```bash
# Polling mode (every 5 minutes)
uv run agentimport daemon --interval 300

# Filesystem watch mode (near-realtime, requires watchdog)
uv sync --extra daemon
uv run agentimport daemon --watch
```

## Development

```bash
uv sync --extra dev
uv run pytest tests/ -v
```
