# Field collapse (Elasticsearch-style)

Issue #2. Keep at most K results per distinct value of a field, preserving
score order — e.g. one hit per `conversation_id` instead of 50 from the same
conversation.

## Status
- [x] Pure core: `ranking::collapse::collapse_results` + `CollapseConfig`
      (commit 4c46271, unit-tested).
- [x] Wire into API `SearchRequest` + apply in the search route
      (e2e test `test_search_collapse_by_field`).

## Design decisions
- **Where applied:** in the `search` route handler, post-search, alongside
  `min_score` / `score_function` (which are set to `None` in `Query` and
  applied after `manager.search`). Collapse runs *after* `min_score` so
  filtered-out hits never occupy a group slot.
- **API shape (ES-like):**
  ```json
  "collapse": { "field": "category", "max_per_group": 1 }
  ```
  `max_per_group` defaults to `1` (ES default is one hit per group).
- **Group key:** stored field value, stringified (`group_key` in collapse.rs).
  Results missing the field are always kept (cannot be grouped).

## Not wired (by design, consistent with existing behaviour)
`multi_index_search` (`/a,b/_search`) takes `SearchRequest` but already ignores
`min_score` and `score_function` (they're set to `None` and never applied
post-search). Collapse follows the same handler: not applied there. Wiring all
three post-search steps into multi-index search is a separate, larger change.

## Known limitation
Collapse runs on the already-paginated result page (backend applied
`limit`/`offset` first), so a collapsed page can contain fewer than `limit`
hits. Same post-limit behaviour as the existing `min_score` filter. Over-fetch
+ re-collapse could be a later improvement; out of scope for first wiring.
