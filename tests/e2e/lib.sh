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
