# prism-mailsync

Incrementally sync a **remote IMAP mailbox** (many folders, very large) into a
**Prism** collection via the Elasticsearch-compatible `/_elastic/_bulk` endpoint,
making all your mail full-text searchable.

- **Resumable** — tracks a per-folder UID watermark in SQLite; a re-run only
  fetches new mail, and a crash resumes exactly where it stopped.
- **Idempotent** — `Message-ID` is the document `_id`, so the same mail in
  several folders collapses to one document and re-runs never duplicate.
- **Flexible connections** — password or OAuth2 (XOAUTH2), implicit SSL or
  STARTTLS, and an optional built-in **SSH tunnel** to the IMAP host.
- **Easy to run** — one `.env` file, then `uv run mailsync`.

---

## Quick start

```bash
cd clients/python/mailsync

# 1. Create a config file and fill it in (secrets stay out of git)
uv run mailsync --init          # writes .env
$EDITOR .env

# 2. See what will happen without writing anything
uv run mailsync --dry-run

# 3. Real sync (safe to run repeatedly — it's incremental)
uv run mailsync
```

Check the resolved config at any time (secrets redacted):

```bash
uv run mailsync --print-config
```

---

## Configuration

All settings come from the environment (or the `.env` file loaded from the
working directory). See [`.env.example`](.env.example) for the full list.

| Variable | Default | Meaning |
| --- | --- | --- |
| `MAILSYNC_IMAP_HOST` | — (required) | IMAP server hostname |
| `MAILSYNC_IMAP_PORT` | `993` | `993` = implicit SSL, `143` = STARTTLS |
| `MAILSYNC_IMAP_USER` | — (required) | Login / email address |
| `MAILSYNC_IMAP_PASSWORD` | — | App password (password auth) |
| `MAILSYNC_IMAP_AUTH` | `password` | `password` or `oauth2` |
| `MAILSYNC_IMAP_OAUTH2_TOKEN` | — | Access token when `AUTH=oauth2` |
| `MAILSYNC_IMAP_SECURITY` | from port | `ssl` \| `starttls` \| `none` |
| `MAILSYNC_IMAP_TLS_VERIFY` | `true` | Set `false` only for self-signed servers |
| `MAILSYNC_SSH_HOST` | — | Enables the SSH tunnel when set |
| `MAILSYNC_SSH_PORT` / `_USER` / `_KEY` / `_PASSWORD` | `22` / — | SSH tunnel auth |
| `MAILSYNC_PRISM_URL` | `http://localhost:3080` | Prism base URL |
| `MAILSYNC_COLLECTION` | `mail` | Target collection (auto-created) |
| `MAILSYNC_PRISM_API_KEY` | — | Only if the server has auth enabled |
| `MAILSYNC_BATCH_SIZE` | `300` | Messages per bulk request (server max 10000) |
| `MAILSYNC_BODY_CAP` | `1000000` | Truncate stored plaintext body (bytes) |
| `MAILSYNC_STATE_PATH` | `mailsync.db` | SQLite watermark file |

CLI flags override the env for the common knobs: `--collection`, `--prism-url`,
`--batch-size`, `--body-cap`, `--state`.

### Connecting through an SSH tunnel (recommended)

If the IMAP server is only reachable from inside a network, let `mailsync` open
the tunnel for you (install the extra once with `uv sync --extra ssh`):

```ini
MAILSYNC_SSH_HOST=bastion.example.com
MAILSYNC_SSH_USER=tunnel
MAILSYNC_SSH_KEY=~/.ssh/id_ed25519
MAILSYNC_IMAP_HOST=mail.internal      # resolved on the bastion side
MAILSYNC_IMAP_PORT=993
```

`mailsync` forwards a local port to `MAILSYNC_IMAP_HOST:PORT` over SSH and
connects through it. Because the hop terminates on `127.0.0.1`, the TLS
**certificate chain is still verified** but hostname pinning is relaxed; SSH
provides the transport security. For a fully internal server with a self-signed
cert, add `MAILSYNC_IMAP_SECURITY=none` (SSH already encrypts) or
`MAILSYNC_IMAP_TLS_VERIFY=false`.

### Gmail / OAuth2

Use an [app password](https://support.google.com/accounts/answer/185833) with
`AUTH=password`, or supply a short-lived OAuth2 access token:

```ini
MAILSYNC_IMAP_HOST=imap.gmail.com
MAILSYNC_IMAP_AUTH=oauth2
MAILSYNC_IMAP_OAUTH2_TOKEN=ya29....
```

---

## Everyday flags

| Flag | Effect |
| --- | --- |
| `--dry-run` | Fetch + parse + report, but never write to Prism or state |
| `--folder NAME` | Sync only one folder (repeatable) |
| `--full-resync` | Ignore watermarks and re-index everything (idempotent) |
| `--limit N` | Stop after N messages (smoke testing) |
| `--batch-size N` | Messages per bulk request |
| `--init` | Write a `.env` template |
| `--print-config` | Show resolved config with secrets redacted |

---

## Searching your mail

Once synced, query through Prism's Elasticsearch-compatible API:

```bash
curl -s "$MAILSYNC_PRISM_URL/_elastic/mail/_search" -H 'Content-Type: application/json' -d '{
  "query": { "match": { "body": "invoice overdue" } },
  "sort":  [ { "date_epoch": "desc" } ],
  "size":  10
}'
```

Indexed fields: `subject`, `body`, `from`, `from_name`, `to`, `cc` (full-text);
`folder`, `message_id`, `flags`, `date` (exact/keyword); `date_epoch`
(sortable/range), `has_attachments`, `size`, `uid`, `uidvalidity`.

---

## How resumability works

Every IMAP folder has a `UIDVALIDITY` stamp and monotonically increasing per-
message `UID`s. `mailsync` records the highest UID it has **durably indexed** per
`(account, folder, uidvalidity)` in `mailsync.db`, and the next run fetches only
`UID > watermark`. The watermark advances **only over the contiguous run of
messages Prism acknowledged**, so a failure leaves that message (and everything
after it) to be retried next run rather than silently skipped. If a folder's
`UIDVALIDITY` changes (the server renumbered it), the watermark resets and the
folder is re-scanned — re-indexing is harmless because `_id` is the `Message-ID`.

---

## Development

```bash
uv run --with pytest pytest          # 50 unit tests, no live IMAP/Prism needed
```

The pure logic — message→document mapping, NDJSON building, watermark
advancement, config parsing, and the sync control flow (via in-memory fakes) —
is fully unit-tested. The IMAP and HTTP adapters are thin and verified against a
live Prism instance.

### Layout

| Module | Responsibility |
| --- | --- |
| `config.py` | Env → `Config` (connections, auth, tunables) |
| `message.py` | Raw RFC 822 bytes → Prism document (pure) |
| `schema.py` | The `mail` collection schema |
| `bulk.py` | ES `_bulk` NDJSON + watermark logic (pure) |
| `state.py` | SQLite per-folder UID watermark store |
| `imap.py` | IMAP connection (SSL/STARTTLS, password/OAuth2, SSH tunnel) |
| `prism.py` | HTTP: create collection + bulk index |
| `sync.py` | Orchestration (dependency-injected, testable) |
| `cli.py` | `.env` loading + argument parsing |
