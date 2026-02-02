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
