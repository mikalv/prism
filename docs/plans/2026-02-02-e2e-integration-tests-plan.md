# E2E Integration Tests Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add Docker-based E2E tests that build Prism, index documents, and verify search results via curl — with a fast smoke tier (every PR) and a full suite (nightly/manual with Ollama).

**Architecture:** A `docker-compose.e2e.yml` starts Prism from the existing Dockerfile (with Ollama behind a compose profile). Bash scripts in `tests/e2e/` use curl + jq to hit the HTTP API. Test data (schemas, pipelines, documents) lives in `tests/e2e/testdata/` and is mounted into the container.

**Tech Stack:** Docker Compose, bash, curl, jq

---

### Task 1: Fix Dockerfile CMD to match actual CLI args

The Dockerfile currently uses `--bind` and `--data-dir` flags that don't exist on prism-server. The actual CLI accepts `--config`, `--host`, and `--port`. This must be fixed first or the Docker image won't start.

**Files:**
- Modify: `/home/meeh/prism/Dockerfile` (lines 64-65)

**Step 1: Fix the CMD**

Replace the current CMD in the Dockerfile:

```dockerfile
ENTRYPOINT ["/usr/local/bin/prism-server"]
CMD ["--host", "0.0.0.0", "--port", "3000"]
```

The config file doesn't exist in the container so `load_or_create` will create defaults. The default `data_dir` is `~/.engraph` which maps to the nonroot home in distroless. That's fine for E2E.

**Step 2: Fix the healthcheck**

The `--health-check` flag also doesn't exist. Replace with a curl-based check. But distroless has no curl. Use a simple TCP check or remove the Docker-level healthcheck (our E2E scripts do their own health polling). Simplest: remove the HEALTHCHECK line from the Dockerfile; the compose file will define its own.

Remove or comment out:

```dockerfile
# Remove this line:
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/prism-server", "--health-check"] || exit 1
```

**Step 3: Verify Docker build succeeds**

Run: `docker build -t prism:e2e-test . 2>&1 | tail -5`
Expected: Successfully built

**Step 4: Commit**

```bash
git add Dockerfile
git commit -m "fix(docker): correct CMD args to match prism-server CLI"
```

---

### Task 2: Create test data (schemas, pipelines, documents)

**Files:**
- Create: `/home/meeh/prism/tests/e2e/testdata/schemas/articles.yaml`
- Create: `/home/meeh/prism/tests/e2e/testdata/pipelines/normalize.yaml`
- Create: `/home/meeh/prism/tests/e2e/testdata/documents/articles.json`
- Create: `/home/meeh/prism/tests/e2e/testdata/config/prism.toml`

**Step 1: Create directory structure**

```bash
mkdir -p tests/e2e/testdata/{schemas,pipelines,documents,config}
```

**Step 2: Create text-only collection schema**

Create `/home/meeh/prism/tests/e2e/testdata/schemas/articles.yaml`:

```yaml
collection: articles
backends:
  text:
    fields:
      - name: title
        type: text
        stored: true
        indexed: true
      - name: content
        type: text
        stored: true
        indexed: true
      - name: category
        type: string
        indexed: true
        stored: true
      - name: indexed_at
        type: string
        stored: true
        indexed: false
```

**Step 3: Create test pipeline**

Create `/home/meeh/prism/tests/e2e/testdata/pipelines/normalize.yaml`:

```yaml
name: normalize
description: Normalize text fields for E2E tests
processors:
  - lowercase:
      field: title
  - set:
      field: indexed_at
      value: "{{_now}}"
```

**Step 4: Create sample documents**

Create `/home/meeh/prism/tests/e2e/testdata/documents/articles.json`:

```json
{
  "documents": [
    {"id": "1", "fields": {"title": "Rust Programming Language", "content": "Learn Rust for systems programming and memory safety", "category": "tech"}},
    {"id": "2", "fields": {"title": "Python Data Science", "content": "Analyze data with pandas numpy and scikit-learn", "category": "tech"}},
    {"id": "3", "fields": {"title": "Sourdough Bread Recipe", "content": "Mix flour water salt and sourdough starter then bake", "category": "cooking"}},
    {"id": "4", "fields": {"title": "Nordic Hiking Trails", "content": "Explore fjords and mountains in Norway and Sweden", "category": "travel"}},
    {"id": "5", "fields": {"title": "Machine Learning Basics", "content": "Neural networks deep learning and gradient descent introduction", "category": "tech"}}
  ]
}
```

**Step 5: Create E2E config file**

The prism-server needs a config that points `data_dir` to a writable path and `schemas_dir` falls under that. Pipelines load from `config_dir/conf/pipelines`.

Create `/home/meeh/prism/tests/e2e/testdata/config/prism.toml`:

```toml
[storage]
data_dir = "/data"

[server]
bind_addr = "0.0.0.0:3000"

[server.cors]
enabled = true
origins = ["*"]

[server.tls]
enabled = false

[security]
enabled = false

[security.audit]
enabled = false
```

**Step 6: Commit**

```bash
git add tests/e2e/testdata/
git commit -m "feat(e2e): add test data for E2E integration tests"
```

---

### Task 3: Create docker-compose.e2e.yml

**Files:**
- Create: `/home/meeh/prism/docker-compose.e2e.yml`

**Step 1: Create the compose file**

Create `/home/meeh/prism/docker-compose.e2e.yml`:

```yaml
services:
  prism:
    build: .
    ports:
      - "${PRISM_PORT:-3080}:3000"
    volumes:
      - ./tests/e2e/testdata/schemas:/data/schemas:ro
      - ./tests/e2e/testdata/config/prism.toml:/config/prism.toml:ro
      - ./tests/e2e/testdata/pipelines:/config/conf/pipelines:ro
    environment:
      - RUST_LOG=info
    command: ["--config", "/config/prism.toml", "--host", "0.0.0.0", "--port", "3000"]

  ollama:
    image: ollama/ollama:latest
    ports:
      - "11434:11434"
    volumes:
      - ollama-data:/root/.ollama
    entrypoint: ["/bin/sh", "-c"]
    command:
      - |
        /bin/ollama serve &
        sleep 5
        /bin/ollama pull nomic-embed-text
        wait
    profiles: ["full"]

volumes:
  ollama-data:
```

Key points:
- Schemas mounted at `/data/schemas` (where `schemas_dir()` looks given `data_dir = /data`)
- Config mounted at `/config/prism.toml`, passed via `--config`
- Pipelines mounted at `/config/conf/pipelines` (the pipeline loader resolves relative to config parent dir)
- Ollama behind `profiles: ["full"]` so smoke test doesn't start it
- `PRISM_PORT` env var defaults to 3080

**Step 2: Commit**

```bash
git add docker-compose.e2e.yml
git commit -m "feat(e2e): add docker-compose.e2e.yml for E2E tests"
```

---

### Task 4: Create shared test helper library

**Files:**
- Create: `/home/meeh/prism/tests/e2e/lib.sh`

**Step 1: Write the helper library**

Create `/home/meeh/prism/tests/e2e/lib.sh`:

```bash
#!/usr/bin/env bash
# E2E test helper library
# Source this file: source "$(dirname "$0")/lib.sh"

set -euo pipefail

# --- Config ---
PRISM_URL="http://localhost:${PRISM_PORT:-3080}"
E2E_TIMEOUT="${E2E_TIMEOUT:-60}"
COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.e2e.yml}"

# --- Counters ---
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_RUN=0

# --- Colors ---
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

# --- Logging ---
log_pass() {
    TESTS_PASSED=$((TESTS_PASSED + 1))
    TESTS_RUN=$((TESTS_RUN + 1))
    echo -e "${GREEN}  PASS${NC} $1"
}

log_fail() {
    TESTS_FAILED=$((TESTS_FAILED + 1))
    TESTS_RUN=$((TESTS_RUN + 1))
    echo -e "${RED}  FAIL${NC} $1"
    if [ -n "${2:-}" ]; then
        echo -e "${RED}       ${2}${NC}"
    fi
}

log_info() {
    echo -e "${YELLOW}  INFO${NC} $1"
}

# --- Summary ---
print_summary() {
    echo ""
    echo "========================================"
    echo "  Results: ${TESTS_PASSED}/${TESTS_RUN} passed, ${TESTS_FAILED} failed"
    echo "========================================"
    if [ "$TESTS_FAILED" -gt 0 ]; then
        echo -e "${RED}  FAILED${NC}"
        return 1
    else
        echo -e "${GREEN}  ALL PASSED${NC}"
        return 0
    fi
}

# --- Health check ---
wait_for_healthy() {
    local url="${1:-$PRISM_URL/health}"
    local timeout="${2:-$E2E_TIMEOUT}"
    local elapsed=0

    log_info "Waiting for $url (timeout: ${timeout}s)..."
    while [ "$elapsed" -lt "$timeout" ]; do
        if curl -sf "$url" > /dev/null 2>&1; then
            log_info "Server healthy after ${elapsed}s"
            return 0
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done
    log_fail "Server did not become healthy within ${timeout}s"
    return 1
}

# --- HTTP helpers ---
# Usage: http_get "/path" -> sets $HTTP_STATUS and $HTTP_BODY
http_get() {
    local path="$1"
    local tmpfile
    tmpfile=$(mktemp)
    HTTP_STATUS=$(curl -s -o "$tmpfile" -w "%{http_code}" "${PRISM_URL}${path}")
    HTTP_BODY=$(cat "$tmpfile")
    rm -f "$tmpfile"
}

# Usage: http_post "/path" '{"json":"body"}' -> sets $HTTP_STATUS and $HTTP_BODY
http_post() {
    local path="$1"
    local data="$2"
    local tmpfile
    tmpfile=$(mktemp)
    HTTP_STATUS=$(curl -s -o "$tmpfile" -w "%{http_code}" \
        -X POST -H "Content-Type: application/json" \
        -d "$data" "${PRISM_URL}${path}")
    HTTP_BODY=$(cat "$tmpfile")
    rm -f "$tmpfile"
}

# Usage: http_post_file "/path" "/path/to/file.json" -> sets $HTTP_STATUS and $HTTP_BODY
http_post_file() {
    local path="$1"
    local file="$2"
    local tmpfile
    tmpfile=$(mktemp)
    HTTP_STATUS=$(curl -s -o "$tmpfile" -w "%{http_code}" \
        -X POST -H "Content-Type: application/json" \
        -d "@${file}" "${PRISM_URL}${path}")
    HTTP_BODY=$(cat "$tmpfile")
    rm -f "$tmpfile"
}

# --- Assertions ---
# Usage: assert_status "test name" expected_status
assert_status() {
    local test_name="$1"
    local expected="$2"
    if [ "$HTTP_STATUS" = "$expected" ]; then
        log_pass "$test_name"
    else
        log_fail "$test_name" "Expected status $expected, got $HTTP_STATUS. Body: $HTTP_BODY"
    fi
}

# Usage: assert_json "test name" ".jq.path" "expected_value"
assert_json() {
    local test_name="$1"
    local jq_path="$2"
    local expected="$3"
    local actual
    actual=$(echo "$HTTP_BODY" | jq -r "$jq_path" 2>/dev/null || echo "JQ_ERROR")
    if [ "$actual" = "$expected" ]; then
        log_pass "$test_name"
    else
        log_fail "$test_name" "Expected $jq_path = '$expected', got '$actual'"
    fi
}

# Usage: assert_json_gt "test name" ".jq.path" min_value
assert_json_gt() {
    local test_name="$1"
    local jq_path="$2"
    local min_val="$3"
    local actual
    actual=$(echo "$HTTP_BODY" | jq -r "$jq_path" 2>/dev/null || echo "0")
    if [ "$actual" -gt "$min_val" ] 2>/dev/null; then
        log_pass "$test_name"
    else
        log_fail "$test_name" "Expected $jq_path > $min_val, got '$actual'"
    fi
}

# Usage: assert_json_contains "test name" ".jq.path" "substring"
assert_json_contains() {
    local test_name="$1"
    local jq_path="$2"
    local substring="$3"
    local actual
    actual=$(echo "$HTTP_BODY" | jq -r "$jq_path" 2>/dev/null || echo "")
    if echo "$actual" | grep -q "$substring"; then
        log_pass "$test_name"
    else
        log_fail "$test_name" "Expected $jq_path to contain '$substring', got '$actual'"
    fi
}

# --- Docker helpers ---
compose_up() {
    local profile="${1:-}"
    log_info "Starting containers..."
    if [ -n "$profile" ]; then
        docker compose -f "$COMPOSE_FILE" --profile "$profile" up -d --build 2>&1
    else
        docker compose -f "$COMPOSE_FILE" up -d --build 2>&1
    fi
}

compose_down() {
    log_info "Stopping containers..."
    docker compose -f "$COMPOSE_FILE" --profile full down -v 2>&1 || true
}

compose_logs() {
    docker compose -f "$COMPOSE_FILE" logs --no-log-prefix 2>&1
}
```

**Step 2: Make it non-executable (sourced, not run directly)**

No chmod needed — this is sourced by other scripts.

**Step 3: Commit**

```bash
git add tests/e2e/lib.sh
git commit -m "feat(e2e): add shared bash test helper library"
```

---

### Task 5: Create smoke test script

**Files:**
- Create: `/home/meeh/prism/tests/e2e/smoke.sh`

**Step 1: Write the smoke test**

Create `/home/meeh/prism/tests/e2e/smoke.sh`:

```bash
#!/usr/bin/env bash
# E2E Smoke Test — runs on every PR
# Builds Prism in Docker, indexes documents, verifies text search.
#
# Usage: ./tests/e2e/smoke.sh
# Env:   PRISM_PORT (default 3080), E2E_TIMEOUT (default 60)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

cd "$PROJECT_ROOT"
source "$SCRIPT_DIR/lib.sh"

# Ensure cleanup on exit
trap 'compose_down' EXIT

echo "========================================="
echo "  Prism E2E Smoke Test"
echo "========================================="

# --- Start ---
compose_up
wait_for_healthy

# --- Test: Health endpoint ---
http_get "/health"
assert_status "GET /health returns 200" "200"

# --- Test: Collections loaded from schema ---
http_get "/admin/collections"
assert_status "GET /admin/collections returns 200" "200"
assert_json_contains "articles collection exists" ".collections[]" "articles"

# --- Test: Pipelines loaded ---
http_get "/admin/pipelines"
assert_status "GET /admin/pipelines returns 200" "200"
assert_json "pipeline count is 1" ".pipelines | length" "1"
assert_json "normalize pipeline loaded" ".pipelines[0].name" "normalize"

# --- Test: Index documents ---
http_post_file "/collections/articles/documents" "$SCRIPT_DIR/testdata/documents/articles.json"
assert_status "POST documents returns 201" "201"

# Give indexer a moment to commit
sleep 1

# --- Test: Search finds expected document ---
http_post "/collections/articles/search" '{"query": "rust programming", "limit": 5}'
assert_status "Search returns 200" "200"
assert_json_gt "Search returns results" ".total" "0"
assert_json_contains "First result mentions rust" ".results[0].fields.title" "Rust"

# --- Test: Search with no results ---
http_post "/collections/articles/search" '{"query": "xyznonexistent123", "limit": 5}'
assert_status "Empty search returns 200" "200"
assert_json "Empty search has 0 results" ".total" "0"

# --- Test: Index with pipeline ---
http_post "/collections/articles/documents?pipeline=normalize" \
    '{"documents": [{"id": "6", "fields": {"title": "UPPERCASE TITLE", "content": "testing pipeline", "category": "test"}}]}'
assert_status "Pipeline index returns 201" "201"
assert_json "Pipeline indexed 1 doc" ".indexed" "1"
assert_json "Pipeline 0 failures" ".failed" "0"

sleep 1

# --- Test: Verify pipeline processed the document ---
http_get "/collections/articles/documents/6"
assert_status "Get pipelined doc returns 200" "200"
assert_json "Title was lowercased" ".fields.title" "uppercase title"
assert_json_contains "indexed_at was set" ".fields.indexed_at" "T"

# --- Test: Unknown pipeline returns 400 ---
http_post "/collections/articles/documents?pipeline=nonexistent" \
    '{"documents": [{"id": "99", "fields": {"title": "test"}}]}'
assert_status "Unknown pipeline returns 400" "400"

# --- Summary ---
echo ""
echo "Container logs:"
compose_logs | tail -20
echo ""
print_summary
```

**Step 2: Make executable**

```bash
chmod +x tests/e2e/smoke.sh
```

**Step 3: Run smoke test locally to verify**

Run: `./tests/e2e/smoke.sh 2>&1 | tail -30`
Expected: All tests PASS (this may take a while for the Docker build)

**Step 4: Commit**

```bash
git add tests/e2e/smoke.sh
git commit -m "feat(e2e): add smoke test script"
```

---

### Task 6: Create full E2E test script

**Files:**
- Create: `/home/meeh/prism/tests/e2e/full.sh`

**Step 1: Write the full test suite**

Create `/home/meeh/prism/tests/e2e/full.sh`:

```bash
#!/usr/bin/env bash
# E2E Full Test Suite — runs nightly or on-demand
# Includes smoke tests + vector/hybrid search via Ollama.
#
# Usage: ./tests/e2e/full.sh
# Env:   OLLAMA_URL (default: use docker-compose ollama)
#        PRISM_PORT (default 3080), E2E_TIMEOUT (default 120)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

cd "$PROJECT_ROOT"

# Increase default timeout for full suite (Ollama model pull takes time)
export E2E_TIMEOUT="${E2E_TIMEOUT:-120}"

source "$SCRIPT_DIR/lib.sh"

# Ensure cleanup on exit
trap 'compose_down' EXIT

echo "========================================="
echo "  Prism E2E Full Test Suite"
echo "========================================="

# --- Start with Ollama profile ---
compose_up "full"
wait_for_healthy

# =========================================================================
# Run all smoke tests inline (not as subprocess, so counters accumulate)
# =========================================================================

echo ""
echo "--- Smoke Tests ---"

http_get "/health"
assert_status "GET /health returns 200" "200"

http_get "/admin/collections"
assert_status "GET /admin/collections returns 200" "200"
assert_json_contains "articles collection exists" ".collections[]" "articles"

http_get "/admin/pipelines"
assert_status "GET /admin/pipelines returns 200" "200"
assert_json "normalize pipeline loaded" ".pipelines[0].name" "normalize"

http_post_file "/collections/articles/documents" "$SCRIPT_DIR/testdata/documents/articles.json"
assert_status "POST documents returns 201" "201"
sleep 1

http_post "/collections/articles/search" '{"query": "rust programming", "limit": 5}'
assert_status "Search returns 200" "200"
assert_json_gt "Search returns results" ".total" "0"

http_post "/collections/articles/search" '{"query": "xyznonexistent123", "limit": 5}'
assert_status "Empty search returns 200" "200"
assert_json "Empty search has 0 results" ".total" "0"

http_post "/collections/articles/documents?pipeline=normalize" \
    '{"documents": [{"id": "6", "fields": {"title": "UPPERCASE TITLE", "content": "testing pipeline", "category": "test"}}]}'
assert_status "Pipeline index returns 201" "201"
sleep 1

http_get "/collections/articles/documents/6"
assert_status "Get pipelined doc returns 200" "200"
assert_json "Title was lowercased" ".fields.title" "uppercase title"

http_post "/collections/articles/documents?pipeline=nonexistent" \
    '{"documents": [{"id": "99", "fields": {"title": "test"}}]}'
assert_status "Unknown pipeline returns 400" "400"

# =========================================================================
# Full suite: additional tests
# =========================================================================

echo ""
echo "--- Full Suite Tests ---"

# --- Test: Server info endpoint ---
http_get "/stats/server"
assert_status "GET /stats/server returns 200" "200"
assert_json_contains "Version is set" ".version" "0."

# --- Test: Collection schema endpoint ---
http_get "/collections/articles/schema"
assert_status "GET collection schema returns 200" "200"
assert_json "Schema collection name" ".collection" "articles"

# --- Test: Collection stats after indexing ---
http_get "/collections/articles/stats"
assert_status "GET collection stats returns 200" "200"
assert_json_gt "Doc count > 0" ".document_count" "0"

# --- Test: Bulk index more documents ---
BULK_DOCS='{"documents": ['
for i in $(seq 10 30); do
    [ "$i" -gt 10 ] && BULK_DOCS+=","
    BULK_DOCS+="{\"id\": \"bulk-${i}\", \"fields\": {\"title\": \"Bulk document ${i}\", \"content\": \"This is bulk test document number ${i} for stress testing\", \"category\": \"bulk\"}}"
done
BULK_DOCS+=']}'

http_post "/collections/articles/documents" "$BULK_DOCS"
assert_status "Bulk index 21 docs returns 201" "201"
assert_json "Bulk indexed count" ".indexed" "21"
sleep 2

# --- Test: Search after bulk index ---
http_post "/collections/articles/search" '{"query": "bulk test document", "limit": 25}'
assert_status "Bulk search returns 200" "200"
assert_json_gt "Bulk search finds documents" ".total" "5"

# --- Test: Category search ---
http_post "/collections/articles/search" '{"query": "cooking sourdough", "limit": 5}'
assert_status "Category search returns 200" "200"
assert_json_gt "Category search finds results" ".total" "0"

# --- Summary ---
echo ""
echo "Container logs (last 20 lines):"
compose_logs | tail -20
echo ""
print_summary
```

**Step 2: Make executable**

```bash
chmod +x tests/e2e/full.sh
```

**Step 3: Commit**

```bash
git add tests/e2e/full.sh
git commit -m "feat(e2e): add full E2E test suite with bulk indexing"
```

---

### Task 7: Add CI workflow integration

**Files:**
- Modify: `/home/meeh/prism/.github/workflows/ci.yml` (add e2e-smoke job)
- Create: `/home/meeh/prism/.github/workflows/e2e-full.yml`

**Step 1: Add smoke job to CI**

Append to `/home/meeh/prism/.github/workflows/ci.yml` after the `docker` job:

```yaml
  e2e-smoke:
    name: E2E Smoke Test
    needs: [test]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Run E2E smoke test
        run: ./tests/e2e/smoke.sh
```

**Step 2: Create nightly/manual full E2E workflow**

Create `/home/meeh/prism/.github/workflows/e2e-full.yml`:

```yaml
name: E2E Full Suite

on:
  workflow_dispatch:
  schedule:
    - cron: '0 3 * * *'

jobs:
  e2e-full:
    name: E2E Full Suite
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Run E2E full test suite
        run: ./tests/e2e/full.sh
        env:
          E2E_TIMEOUT: "180"
```

**Step 3: Commit**

```bash
git add .github/workflows/ci.yml .github/workflows/e2e-full.yml
git commit -m "feat(e2e): add CI workflows for smoke and full E2E tests"
```

---

### Task 8: Run smoke test end-to-end and fix issues

This is the verification task. Run the actual smoke test and fix any issues that arise.

**Step 1: Build Docker image**

Run: `docker build -t prism:e2e-test . 2>&1 | tail -5`
Expected: Build succeeds

**Step 2: Run smoke test**

Run: `./tests/e2e/smoke.sh`
Expected: All tests pass

**Step 3: If tests fail, debug and fix**

Common issues to check:
- Container not starting: `docker compose -f docker-compose.e2e.yml logs prism`
- Schema not loading: verify mount path matches `data_dir/schemas`
- Pipeline not loading: verify mount path matches `config_dir/conf/pipelines`
- Port mismatch: verify `PRISM_PORT` matches compose port mapping

**Step 4: Commit any fixes**

```bash
git add -A
git commit -m "fix(e2e): address issues found during smoke test verification"
```

---

### Task 9: Final verification and cleanup

**Step 1: Run smoke test one more time**

Run: `./tests/e2e/smoke.sh`
Expected: All tests PASS

**Step 2: Run existing unit/integration tests**

Run: `cargo test -p prism 2>&1 | tail -10`
Expected: No regressions

**Step 3: Verify CI workflow syntax**

Run: `cat .github/workflows/ci.yml | python3 -c "import sys, yaml; yaml.safe_load(sys.stdin)" && echo "Valid YAML"`
Expected: "Valid YAML"

Run: `cat .github/workflows/e2e-full.yml | python3 -c "import sys, yaml; yaml.safe_load(sys.stdin)" && echo "Valid YAML"`
Expected: "Valid YAML"
