# End-to-End Integration Tests Design

**Goal:** Add tiered E2E tests that build and run Prism in Docker, index real data, and verify search results via HTTP.

**Two tiers:**
- **Smoke test** - fast (~30s), text search only, runs on every PR
- **Full suite** - comprehensive with hybrid/vector search via Ollama, runs nightly or on-demand

---

## Architecture

### Docker Compose

`docker-compose.e2e.yml` at project root:

- **prism** service: builds from Dockerfile, mounts test schemas/pipelines from `tests/e2e/testdata/`, port 3080, healthcheck via `prism-server --health-check`
- **ollama** service: behind `profiles: ["full"]`, only started for the full suite. Port 11434.

Test schemas and pipelines are mounted as volumes, not baked into the image.

### Test Scripts (Bash + curl + jq)

**`tests/e2e/lib.sh`** - Shared helpers:
- `wait_for_healthy` - polls `/health` until 200 or timeout
- `assert_status` - curl + check HTTP status code
- `assert_json_field` - curl + jq path + expected value
- `index_docs` - POST documents to a collection
- `search` - POST search query, return results
- `log_pass` / `log_fail` - colored output with test name

**`tests/e2e/smoke.sh`** - CI smoke test:
1. `docker compose -f docker-compose.e2e.yml up -d prism`
2. Wait for healthy (up to 30s)
3. Test cases:
   - `GET /health` returns 200
   - `GET /admin/collections` lists expected collections
   - `GET /admin/pipelines` lists test pipeline
   - Index 5 documents to text collection
   - Search "rust programming" - assert correct doc found
   - Search "nonexistent gibberish" - assert 0 results
   - Index with `?pipeline=normalize` - verify response
   - Verify pipeline set `indexed_at` timestamp
4. Cleanup: `docker compose down -v`
5. Exit with pass/fail summary

**`tests/e2e/full.sh`** - Comprehensive suite:
- Runs all smoke tests first
- Accepts `OLLAMA_URL` env var (defaults to docker-compose Ollama)
- Starts with `--profile full`
- Additional tests:
  - Vector search: index docs, search by semantic meaning
  - Hybrid search: combine text + vector, verify ranking
  - Multiple collections with different schemas
  - Pipeline error handling
  - Larger dataset (~50 documents)

### Test Data

Located in `tests/e2e/testdata/`:

- `schemas/articles.yaml` - text-only collection (title, content, category)
- `pipelines/normalize.yaml` - lowercase + set indexed_at
- `documents/articles.json` - 5 sample documents for smoke test
- Full suite adds a vector-enabled schema and ~50 documents

### CI Integration

**Smoke test** - new job in `.github/workflows/ci.yml`:
- Runs after existing `test` job passes
- `needs: [test]`
- Simply executes `tests/e2e/smoke.sh`

**Full suite** - separate `.github/workflows/e2e-full.yml`:
- Triggers: `workflow_dispatch` (manual) + `schedule` (nightly at 03:00 UTC)
- Executes `tests/e2e/full.sh`

### Environment Variables

| Variable | Default | Used by |
|----------|---------|---------|
| `OLLAMA_URL` | `http://localhost:11434` | `full.sh` |
| `PRISM_PORT` | `3080` | Both |
| `E2E_TIMEOUT` | `60` | Both (max wait for healthy) |
