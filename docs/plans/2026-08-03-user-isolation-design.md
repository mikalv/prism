# User Isolation Mode — Design & Plan

**Date:** 2026-08-03
**Status:** Proposal
**Effort estimate:** Small–Medium (~1 focused day for the core; ES-compat audit is the long pole)

## Goal

An **opt-in** mode where an authenticated identity sees and can act on **only its
own collections** — never enumerating, searching, or reading collections it has
no grant for. Off by default (single-tenant behavior unchanged).

Motivating convention: collections on prod are already namespaced by owner as
`ws_<user>_<type>_<name>` (e.g. `ws_mikalv_code_...`, `ws_eyrmedical_...`), so
per-user isolation maps naturally onto a collection-name prefix.

## Current state (what already exists)

The per-collection ACL model is **largely built** — this is not a from-scratch
feature.

- **Identity → roles → per-collection permissions.** `SecurityConfig` has
  `api_keys` (`key → roles`) and `roles` (`RoleConfig.collections: HashMap<pattern,
  Vec<permission>>`). `config/mod.rs:209-243`.
- **`PermissionChecker`** (`security/permissions.rs`): `authenticate(api_key) →
  AuthUser`; `check_permission(user, collection, Permission)` iterates the user's
  roles → collection patterns → returns true on a `glob_match` + permission hit.
  **Defaults to `false` (default-deny).**
- **`Permission`** enum: `Read, Write, Delete, Search, Admin` (`security/types.rs`).
- **`glob_match`** supports exact and trailing-`*` prefix patterns (`ws_mikalv_*`).
- **Search surfaces already enforce it:**
  - `simple_search` (`/api/search`) filters the all-collections fan-out by
    `Search` permission and returns `403` on an explicit unauthorized collection
    (`routes.rs:292-338`).
  - `multi_search` (`/_msearch`) checks `Search` on every requested collection
    (`routes.rs:1665-1667`).

So isolation is **already configurable today** on the search paths: give a key a
role with `collections: { "ws_mikalv_*": ["search", "read"] }`.

## Gap analysis (what's missing)

1. **`GET /admin/collections` does not filter** — `list_collections`
   (`routes.rs:692`) returns *all* collection names to any caller. This is the
   primary enumeration leak: the web UI dropdown, `agentimport`, and any client
   see every collection name. **Fix: add `user_ext`/`checker_ext` and filter by
   `check_permission(user, c, Search|Read)` — the same pattern as `simple_search`.**
2. **Un-audited single-collection read surfaces.** Each per-collection route must
   gate on `check_permission` (Read/Search): `/collections/:c/schema`, `/stats`,
   `/documents/:id`, `/segments`, `/_suggest`, `/_mlt`, `/aggregate`, graph
   endpoints. Any that skip the check leak data by direct name access.
3. **ES-compat surface.** `/_elastic/_cat/indices`, `/_aliases`, and cross-index
   `_search`/`_msearch` must apply the same filter — without breaking official ES
   clients' expectations. This is the fiddliest area and the main effort/risk.
4. **Cluster / federated search** must propagate the identity filter so a
   federated fan-out cannot reach unauthorized shards/collections.
5. **Ergonomics.** Writing one role per user is tedious. Add an opt-in mode plus a
   convention that auto-derives the allowed pattern from key identity.

## Design

### Config

```toml
[security]
enabled = true
isolation = true            # NEW — opt-in; off by default

[[security.api_keys]]
key = "..."
name = "mikalv"
namespace = "ws_mikalv_"    # NEW — auto-grants ws_mikalv_* (search+read) under isolation
roles = []                  # still composable with explicit roles
```

- `isolation = false` (default): behavior unchanged.
- `isolation = true`: a key's `namespace` is compiled into an implicit
  `search`+`read` grant on `<namespace>*` and **all enumeration/read/search
  surfaces default-deny** anything outside the union of the key's explicit-role
  patterns and its namespace pattern.

### Single enforcement point

Introduce one resolver used by **every** enumeration and fan-out path:

```rust
// returns the subset of `all` the identity may see, honoring isolation mode
fn visible_collections(user: Option<&AuthUser>, checker: Option<&PermissionChecker>,
                       all: Vec<String>, perm: Permission) -> Vec<String>
```

`list_collections`, `simple_search`, `multi_search`, ES-compat `_cat`/cross-index
search, and cluster fan-out all call this — no duplicated filtering logic, no
missed surface. Single-collection routes call `check_permission` directly and
return `403`/`404` (prefer `404` under isolation so names don't leak via status).

## Implementation plan (phased)

1. **Enumeration filter (highest value, lowest risk).** Add the resolver; wire
   `list_collections` to it. Tests: key A cannot see key B's collections.
2. **Audit + gate single-collection read routes.** Add `check_permission` to every
   `/collections/:c/*` read/stat route; `404` (not `403`) under isolation.
3. **Config + auto-namespace + default-deny.** Add `isolation` flag and
   `ApiKeyConfig.namespace`; compile the implicit grant; make listing default-deny
   when isolation is on.
4. **ES-compat surface.** Filter `_cat/indices`, `_aliases`, cross-index
   `_search`/`_msearch`; keep official-client compatibility.
5. **Cluster/federated.** Propagate the identity filter through federated search.
6. **UI.** The collection browser/dropdown just consumes the now-filtered
   `/admin/collections` — little or no UI change.

## Risks

- **ES-compat breakage** — the largest surface; external ES clients have
  behavioral expectations for `_cat`/`_search`. Needs integration tests.
- **Leak completeness** — any missed enumeration path (aggregations, suggest, mlt,
  graph, stats, segments, health `collections` count) undermines isolation. The
  single-resolver approach plus a route audit checklist mitigates this.
- **Status-code side channels** — `403` vs `404` reveals existence; prefer `404`
  under isolation.
- **Cluster propagation** — federated search must not bypass the filter.

## Testing

- Unit: `visible_collections` / `check_permission` with namespace + explicit roles.
- Integration per surface: two keys with disjoint namespaces; assert each sees/searches
  only its own across `/admin/collections`, `/api/search`, `/_msearch`, every
  `/collections/:c/*` route, and the ES-compat endpoints. Assert unauthorized
  direct access returns `404` under isolation.
- Regression: `isolation = false` leaves all current behavior unchanged.

## Effort

Core (steps 1–3) is roughly a day including tests — the model, default-deny
`check_permission`, glob patterns, and filtered search already exist. Steps 4–5
(ES-compat + cluster) carry most of the remaining effort and risk and can land
incrementally behind the same `isolation` flag.
