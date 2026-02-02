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
