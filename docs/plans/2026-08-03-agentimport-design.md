# agentimport — Design

**Date:** 2026-08-03
**Status:** Design (approved, pre-implementation)
**Location:** `clients/python/agentimport/`

## Summary

`agentimport` is a Python client that imports AI-assistant conversation history from
many tools (Claude Code, PI, Codex, Gemini, Copilot, Cursor, Kilo/Cline/Roo, ChatGPT
exports) into Prism. It normalizes every source into a common message model and indexes
it into two Prism collections for cross-tool search, memory/context reuse (e.g. an MCP
lookup service in another project), and usage analytics.

It ships as an open-source **example client** in the Prism repo, but is robust enough to
drive a real memory/lookup backend. Re-running it is always safe: imports are
idempotent (deterministic document IDs → Prism upsert) with a local state file for speed.

**Non-goals (v1):** no deletion/prune — the importer is append/upsert only. Prism is the
durable archive; source-side garbage collection (e.g. Claude Code pruning old sessions)
must never remove data from Prism.

## Goals

- **Cross-tool search** — one searchable index over all AI conversations regardless of tool.
- **Memory / context reuse** — feed a long-term memory / RAG / MCP lookup in another project.
- **Analytics** — aggregate topics, tools, models, token usage, timelines.

## Architecture

```
clients/python/agentimport/
├── pyproject.toml               # hatchling + uv (mirrors prismsearch)
├── README.md
├── schemas/                     # Prism collection schemas (checked in)
│   ├── agent_messages.json
│   └── agent_conversations.json
├── deploy/
│   ├── agentimport.cron         # Linux crontab example
│   ├── agentimport.service      # Linux systemd unit (daemon mode)
│   └── rs.mux.agentimport.plist # macOS launchd LaunchAgent
└── src/agentimport/
    ├── cli.py                   # entrypoint + flags
    ├── config.py                # Prism URL/key, per-source roots
    ├── state.py                 # state.db (mtime/size/hash watermark per file)
    ├── models.py                # NormalizedConversation / NormalizedMessage (pydantic)
    ├── prism.py                 # thin layer over prismsearch client (ensure_schema, upsert)
    ├── pipeline.py              # scan → filter → normalize → dedup → index
    ├── daemon.py                # watch/interval loop around the pipeline
    └── sources/
        ├── base.py              # Source protocol: discover() + parse()
        ├── claude_code.py
        ├── pi.py
        ├── codex.py
        ├── gemini.py
        ├── cline_family.py      # kilo / cline / roo (parametrized by extension id)
        ├── cursor.py
        ├── copilot.py
        └── chatgpt_export.py
```

**Data flow:** `cli` → `pipeline` iterates over enabled `sources` → each adapter
`discover()`s candidate files and `parse()`s them into a `NormalizedMessage` stream →
`pipeline` applies content filters → `state` skips unchanged files → `prism.upsert()` with
deterministic IDs. Adapters are pure (file in → normalized messages out), so they are
unit-testable without a running Prism.

### Parser families

The eight sources collapse into **three parser families**, so adapters share most code:

1. **JSONL event stream** (role + content-block messages): Claude Code, PI, Codex, Gemini.
   Shared `_parse_content_blocks()` helper.
2. **Cline family** (`tasks/<id>/api_conversation_history.json`, Anthropic content blocks,
   tool calls embedded as XML in text): Kilo, Cline, Roo — one adapter parametrized by a
   list of `(name, extension_id, search_roots)`.
3. **Special cases**: Cursor / Copilot (SQLite `state.vscdb`), ChatGPT (`.zip`).

### Sources

| Family | Adapter | Path |
|---|---|---|
| JSONL | `claude_code` | `~/.claude/projects/**/*.jsonl` |
| JSONL | `pi` | `~/.pi/agent/sessions/**/*.jsonl` |
| JSONL | `codex` | `~/.codex/sessions/**` |
| JSONL | `gemini` | `~/.gemini/**` |
| Cline | `cline_family` → kilo | `…/globalStorage/kilocode.kilo-code/tasks/*/api_conversation_history.json` |
| Cline | `cline_family` → cline | `…/globalStorage/saoudrizwan.claude-dev/tasks/*/…` |
| Cline | `cline_family` → roo | `…/globalStorage/rooveterinaryinc.roo-cline/tasks/*/…` |
| SQLite | `cursor` | `…/Cursor/User/workspaceStorage/**/state.vscdb` |
| SQLite | `copilot` | `…/Code/User/workspaceStorage/**/state.vscdb` |
| Zip | `chatgpt_export` | ChatGPT export `.zip` (`conversations.json` + attachments) |

**Confirmed formats (inspected on disk):**

- **PI** (`~/.pi/agent/sessions/<encoded-project>/<ts>_<uuid>.jsonl`): one JSONL file per
  session; events typed `session` (has `id`, `cwd` → project), `model_change`
  (`provider`/`modelId`), `message` (`message.role`, `message.content[]` blocks,
  `message.usage`). `id`/`parentId` form a tree (linearize via parent chain). Nearly
  identical to Claude Code.
- **Cline family / Kilo** (confirmed from Kilo 7.4.17 bundled code): tasks under
  `globalStorage/<ext-id>/tasks/<taskId>/` with `api_conversation_history.json` (array of
  `{role, content:[{type:"text",text}]}`, Anthropic messages format, tool calls as XML in
  text), plus `ui_messages.json` and `history_item.json`/`metadata.json`. A `taskHistory`
  index (titles/timestamps/tokens) lives in VS Code globalState. The importer globs the
  per-task JSON files, so it works wherever the data physically lives.

## Data model

Two collections. `string` = exact (filter/facet), `text` = tokenized full-text, `date` =
timeline/recency. Documents use deterministic IDs so re-import upserts.

### `agent_messages` — one document per message

- `text` (`text`, tokenizer `code` — messages are code-heavy)
- `conversation_id`, `source`, `role`, `content_type`, `tool_name`, `project`, `model` (`string`)
- `seq` (`i64`), `ts` (`date`), `source_path` (`string`, stored, not indexed)
- Facets: `source`, `role`, `content_type`, `model`, `project`; `date_histogram` on `ts`
- Recency boosting on `ts` (exponential, 30d) — newer conversations rank higher (memory)
- **Optional** vector backend (`text_vector`, dim 384) + `embedding_generation` disabled by
  default — enable when a server-side embedding model is available.

`content_type` ∈ {`message`, `tool_call`, `tool_result`, `thinking`} — so the content
filters double as search filters (`content_type:message` for pure conversation).

### `agent_conversations` — one meta document per conversation

`source`, `conversation_id`, `title` (`text`), `project`, `model`,
`started_at`/`ended_at` (`date`), `msg_count` (`i64`), `source_path`. Facets on
`source`/`project`; recency on `started_at`.

Schema format follows Prism's `{collection, backends:{text:{fields:[…]}}, facets, boosting}`
(see `prism/src/schema/types.rs`; `FieldType` ∈ text/string/i64/u64/f64/bool/date/bytes).

## Idempotency & state

**Deterministic IDs** (the correctness guarantee):

- message: `sha1(source + conversation_id + native_msg_id)[:16]`, fallback `source:conv:seq`
- conversation: `sha1(source + conversation_id)[:16]`

**State file** — SQLite at `~/.local/state/agentimport/state.db`:

```sql
CREATE TABLE files (
  path TEXT PRIMARY KEY, mtime REAL, size INTEGER,
  sha1 TEXT, msg_count INTEGER, last_seen TEXT
);
```

**Scan logic:** `stat()` each candidate → if `mtime`+`size` unchanged, skip. Otherwise
parse, filter, index, update the watermark. Growing JSONL files (Claude/PI/Kilo append
during a session) change `mtime`+`size` → the whole file is re-parsed, but deterministic
IDs make already-seen messages a harmless upsert.

**Key property:** the state file is only a *speed* optimization. If it is lost, a full
re-import produces exactly the same documents (upsert by deterministic ID), just slower.
Correctness lives in the IDs, not in the state. This is what makes "everything already
imported" cases safe — run as often as you like, from any machine, with no duplicates.

State is written only after a file's documents index successfully, so a crash mid-file
re-processes it next run (safe due to upsert).

**Batching:** messages are buffered and sent to `prism.index()` in batches (~500); the
per-conversation meta document is emitted to `agent_conversations`.

## CLI, flags & scheduling

```
agentimport init-schema      # create collections from schemas/*.json (idempotent)
agentimport run              # one incremental import, then exit (cron / timer)
agentimport daemon           # long-running: re-scan every --interval (+ optional --watch)
agentimport status           # state statistics (files seen, messages per source)
agentimport sources          # list discovered sources + paths (diagnostics)
```

**Global flags:** `--prism-url` (env `PRISM_URL`, default `http://localhost:3080`),
`--api-key` (env `PRISM_API_KEY` — never in config/git), `--source NAME` (repeatable),
`--since DATE`, `--dry-run`, `--state-file PATH`.

**Content filters:** `--include-tool-results` (default off — can be huge/noisy),
`--include-thinking` (default off), `--roles user,assistant,…`, `--max-chars N`
(truncate/skip long messages). Tool *calls* (name + args, no result) are included by
default as a lightweight signal of what happened.

**Config file** (`~/.config/agentimport/config.toml`): Prism URL, source roots, default
filters — team-shareable; **secrets only via env** (matches the project's "shareable
config, private secrets" principle).

**Daemon:** `--interval 300`, `--watch` (filesystem watch via `watchdog` for near-realtime
on JSONL dirs). Graceful shutdown on SIGTERM/SIGINT; exponential backoff if Prism is down
(the daemon waits and retries rather than dying).

**Scheduling artifacts:** `deploy/agentimport.cron`, `deploy/agentimport.service`
(systemd), and `deploy/rs.mux.agentimport.plist` (macOS launchd — `StartInterval` running
`agentimport run`, with a `KeepAlive` + `agentimport daemon` alternative documented).

## Error handling

Isolation at every level:

- **Per line/file:** a corrupt JSONL line or unparseable file logs a warning and is
  skipped; the run continues. Tolerates an incomplete final line (session mid-write).
- **Per source:** a missing path or failing adapter does not stop other sources.
- **Prism:** batch failure → retry with backoff; a rejected document is logged and
  skipped. Daemon backs off on connection errors and does not die.
- **Append/upsert only** — never delete. Source-side GC never removes data from Prism.

## Testing

- **Adapter unit tests** with fixtures (`tests/fixtures/<source>/`, tiny real-format
  samples): file → `NormalizedMessage[]`; assert roles, `content_type`, tool filtering,
  deterministic IDs.
- **ID stability:** same input → same ID (golden).
- **State:** skip-unchanged + re-parse-on-change.
- **Filters:** `--include-tool-results` on/off, `--max-chars`, `--roles`.
- **ChatGPT zip:** mini `conversations.json` tree → linearization test.
- **Integration** (marked, skipped without `PRISM_URL`): `init-schema` → index fixture →
  search → assert (same pattern as `clients/python/prismsearch/tests/test_integration.py`).

## Follow-up

- Update `clients/python/mailsync` to also ship a defined, checked-in Prism schema
  (`schemas/*.json`), on par with agentimport.
