# Kibana Compatibility Requirements

Lessons learned from running **Kibana 9.5.0** against Prism's Elasticsearch
compatibility layer (`/_elastic/*`, `prism-es-compat`). This document records
what Kibana actually requires at runtime — discovered empirically during a
multi-day bring-up with ~1.8M syslog documents — so future work on the ES-compat
layer does not regress it.

Tested stack: Kibana 9.5.0 (tarball) → `elasticsearch.hosts:
["http://…:9201"]` → Prism `prism-server` with the `es-compat` feature.

---

## 1. Product identification

| Requirement | Detail |
| --- | --- |
| Version banner | Kibana 9.x parses `GET /` `version.number`. Reporting `7.17.0` (the old compatibility target) makes modern Kibanas fail capability checks. Prism reports **`9.5.0`** and `build_flavor: "default"`. |
| `X-Elastic-Product: Elasticsearch` | Response header on **every** reply from the ES-compat router. The ES client libraries refuse to talk to a server that omits it. |

## 2. Endpoints exercised during a Kibana 9.5 startup

Kibana's boot sequence probes far more of the API than any single feature
needs. All of these had to exist before the login/landing page rendered:

- `GET /` — version banner (above)
- `GET /_cluster/health` (+ `?wait_for_status=` variants) — must be fast;
  Kibana polls it repeatedly
- `GET /_cluster/state` (with `filter_path` query params — **parse and honor
  `filter_path`, don't reject unknown query params**)
- `GET /_cluster/settings` — returns defaults (`{"defaults": {...}}`)
- `GET /_nodes`, `GET /_nodes/stats`, `GET /_nodes/:id/:metric` — single-node
  fabrications; Kibana renders the node list in Stack Monitoring/About
- `GET /_license` — a **platinum/basic license object**; Kibana gates features
  on license type. Returning 404 makes Kibana treat the cluster as unlicensed
  and disables large parts of the UI
- `GET /_xpack` + `GET /_xpack/usage` — feature flags (`features.{security,
  ilm, ...}.available/enabled`). Kibana reads these at boot
- `GET /_cat/indices`, `/_cat/aliases`, `/_cat/nodes`, `/_cat/shards` —
  **plain text**, `_cat` format, not JSON (unless `format=json`)
- `GET /_tasks/:id` — task registry (returned by `_update_by_query` etc.)

## 3. Saved-objects migration (the hard part)

On first boot Kibana writes its system index `.kibana_ingest_9.5.0` and runs a
**saved-object migration** against it. This is the most demanding client
Prism's ES-compat layer serves:

1. **Bulk API with `update` actions.** The migration writes with
   `{ "index": … }` and `{ "update": … }` bulk actions. Prism originally
   skipped `update` actions and returned `_primary_term`-shaped errors → 500s
   on every Kibana boot. `update` must upsert by `_id` and return
   `result: "updated"`/`"created"`.
2. **Sequence-number metadata.** Every write/read must echo `_seq_no` and
   `_primary_term` (integers; `0`/`1` is fine for single-node). Kibana's
   optimistic-concurrency checks read these from *GET document* and *search
   hit* responses too — not just writes.
3. **Point-in-Time (PIT).** `POST /{index}/_pit` + `DELETE /_pit`. Prism
   implements a **stateless pseudo-PIT**: the PIT id is just the base64
   collection name. Safe for a single node with no concurrent writers.
   A search carrying `pit: {id}` must resolve the index from the PIT id
   (PIT searches have no index in the URL).
4. **`search_after` pagination.** Migration reads pages with
   `search_after: [{"_shard_doc": …}]` and `sort: [{"_shard_doc": "asc"}]`.
   `_shard_doc` must be accepted as a sort key and echoed in each hit's
   `sort` array.
5. **Query-error degradation.** Tantivy's query parser is stricter than ES.
   Saved-object fields contain colons and dots
   (`migrationVersion.core-usage-stats`, `type:dashboard-space`) that cannot
   round-trip through the query-string translator. Prism **degrades
   `InvalidQuery` parse errors to an empty result set** (logged via query
   log) instead of a 400 — a hard 400 aborts the migration and Kibana falls
   over. For a fresh system index "no matching documents" is also the
   correct answer.
6. **`_source` round-trip fidelity.** Saved objects must come back
   byte-faithful from `_source`. (Stored-source serving, not re-serialization
   from the index.)

## 4. Index management surface

- **Auto-create on write**: Kibana writes to indices it never explicitly
  created (`.kibana_ingest_*`, alerts, SLOs). Prism auto-creates collections
  with a default schema from `_bulk`/index ops.
- **Dynamic field mapping**: saved-object documents contain arbitrary fields.
  Prism indexes unknown fields into a `_dynamic` JSON catch-all field
  (Tantivy JSON fast fields) so term/range/exists queries on *unknown* flat
  and dotted fields still work (`typeMigrationVersion:7.14.1`,
  `migrationVersion.core-usage-stats:…`).
- **`GET /{index}/_mapping`** — must reflect the *effective* schema,
  including dynamically-seen fields, with ES type names.
- **Aliases**: `GET /_alias`, `GET /{index}/_alias`, `POST /_aliases`
  (actions: add/remove), `GET /_cat/aliases`. Kibana's data-views resolve
  index patterns through aliases.
- **`GET /_resolve/index/:pattern`** — Kibana 9.x data views probe pattern
  resolution (indices + aliases + data streams).
- **Index templates / component templates**: `PUT/GET /_index_template`,
  `/_component_template`, `/_simulate*`. Kibana's Fleet/ingest pipelines PUT
  templates on boot; acknowledging and re-listing them is enough.
- **ILM**: `PUT/GET /_ilm/policy`, `GET /_ilm/status`, data streams
  (`PUT/GET /_data_stream/*`). Policy bodies are accepted and stored as
  metadata; Prism does not execute lifecycle actions.
- **Settings**: `PUT /{index}/_settings` — acknowledged (stored as metadata
  only). `GET /{index}/_settings` returns static/empty defaults.

## 5. Search features Kibana leans on

- **`_msearch`** (`POST /{index}/_msearch`, `POST /_msearch`) — Kibana's
  bundled requests (Discover, dashboard, telemetry) are msearch batches.
  Each sub-request follows the `header\nbody\n` NDJSON format.
- **`_field_caps`** (`GET|POST /{index}/_field_caps`) — data views build
  their field list from this. Must return `type`, `searchable`,
  `aggregatable` per field. Dot-paths for nested fields included.
- **Aggregations**: `terms`, `date_histogram` (with `fixed_interval`,
  calendar intervals), `avg/min/max/sum`, `cardinality`, `missing`, and
  composite/filters as used by Discover's charts.
- **`track_total_hits`**: accept `true`/integer and report `hits.total`
  accordingly (`relation: "eq"`/`"gte"`).
- **`_count`** on patterns.
- **`exists` query + bool `must_not`** (filter-with-exclusion patterns).
- **Date math**: Kibana sends date ranges like `now-7d`, `now-15m` — both in
  range queries and in `@timestamp` defaults. Prism added `date_math.rs` for
  parsing these.

## 6. Auth / security posture

- Kibana 9.x first tries **API-key + service-token** flows:
  `POST /_security/user/_has_privileges` (Prism: allow-all),
  `GET /_security/_authenticate` — plus anonymous reads. Running Prism with
  `security.enabled = false` and empty `elasticsearch.username/password` in
  `kibana.yml` is the tested configuration; the security endpoints still
  respond with permissive answers.
- `GET /_inference` → empty endpoint list (Kibana AI features probe it).
- Ingest pipelines: `PUT /_ingest/pipeline/:id` → ack, `GET` → 404. Kibana
  PUTs pipelines during Fleet setup but tolerates their absence later.

## 7. Operational notes

- **Performance**: syslog bulk-import ran ~1.3–1.4k docs/s single-threaded
  through `/_bulk` with `refresh=false`; explicit refreshes are expensive —
  batch them.
- **Index-pattern/data-view naming**: patterns containing `logs-*` route
  Kibana into the Logs product (stream-enabled UI, requires more surface);
  plain `sys*`-style patterns use the classic Discover documents view which
  works against Prism today. Data views with empty `fields` attributes are
  normal — Kibana populates the field list at render time from
  `_fields_for_wildcard` → `_field_caps`.
- **Server setup that worked**: Kibana with `elasticsearch.requestTimeout:
  120000` (migrations are slow on first boot with big indices), Prism on
  `0.0.0.0:3080` with explicit `--data-dir`/`--schemas-dir`, schema YAMLs
  using top-level `collection:` key (not `name:`).

## 8. Known gaps / future work

- Logs-app (stream) data views and dashboards need more aggregations
  (significant_terms, multi_terms) and field-formats surface.
- `_async_search` returns a synchronous-style response immediately; true
  async is not implemented.
- `_nodes/hot_threads` is fabricated.
- Cross-cluster search (`_resolve/cluster`) reports only the local cluster.
- Aggregations over multi-collection (pattern) searches return empty —
  single-collection only for now.
