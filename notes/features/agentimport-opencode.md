# agentimport: opencode source adapter

Add an `opencode` source to the agentimport client so opencode
(https://opencode.ai) conversations are imported into Prism.

## Status
- [x] `OpencodeSource` (DB + file storage), registered in get_all_sources,
      config default, CLI help, README. Tested (`tests/test_opencode.py`).
      Smoke-tested against the real `~/.local/share/opencode/opencode.db`.

## Storage formats (both handled)
opencode stores under `~/.local/share/opencode`:
- **SQLite `opencode.db`**: `session(id,title,directory,model,time_created,…)`,
  `message(id,session_id,data)`, `part(id,message_id,session_id,data)`. `data`
  is a JSON blob. Forked/newer builds keep message text here (this machine).
- **File `storage/`**: `storage/message/<ses>/<msg>.json` (metadata) +
  `storage/part/<ses>/<msg>/<prt>.json` (content). Upstream default.

`discover` prefers the DB when both exist (skips file storage) so a session
isn't imported twice.

## Part → NormalizedMessage
Shared `_part_content` for both layouts:
- `text` → `message` (skip `synthetic:true` hook noise)
- `reasoning` → `thinking`
- `tool` → `tool_call` (`tool_name` + JSON input, like the codex adapter)
- everything else (`step-*`, `snapshot`, …) → skipped

Per-message: role from `data.role`; ts from `data.time.created` (epoch ms);
model from `data.modelID` or nested `data.model.modelID` or session `model`;
project from `data.path.cwd` or session `directory`. `native_msg_id` = part id
(stable dedup key). seq increments per session.
