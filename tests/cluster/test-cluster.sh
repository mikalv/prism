#!/usr/bin/env bash
# Integration test for a 3-node Prism cluster.
#
# Prerequisites:
#   ./generate-certs.sh
#   docker compose up --build -d
#   (wait for nodes to become healthy)
#
# This script tests:
#   1. All 3 nodes respond to health checks
#   2. Index documents on node 1
#   3. Search on node 1 returns results
#   4. Each node operates independently (separate collections)
#   5. Stats are correct per node
set -euo pipefail

NODE1="http://127.0.0.1:3081"
NODE2="http://127.0.0.1:3082"
NODE3="http://127.0.0.1:3083"

PASSED=0
FAILED=0

pass() {
    PASSED=$((PASSED + 1))
    echo "  PASS: $1"
}

fail() {
    FAILED=$((FAILED + 1))
    echo "  FAIL: $1"
}

check_status() {
    local url="$1"
    local expected="$2"
    local desc="$3"
    local status
    status=$(curl -s -o /dev/null -w '%{http_code}' "$url" 2>/dev/null || echo "000")
    if [ "$status" = "$expected" ]; then
        pass "$desc (HTTP $status)"
    else
        fail "$desc (expected HTTP $expected, got $status)"
    fi
}

echo "========================================"
echo "Prism 3-Node Cluster Integration Test"
echo "========================================"
echo ""

# ── Test 1: Health checks ──────────────────────────────────────────
echo "[1] Health checks"
for i in 1 2 3; do
    port=$((3080 + i))
    check_status "http://127.0.0.1:$port/health" "200" "Node $i health"
done
echo ""

# ── Test 2: Root info on each node ─────────────────────────────────
echo "[2] Root info"
for i in 1 2 3; do
    port=$((3080 + i))
    root=$(curl -s "http://127.0.0.1:$port/" 2>/dev/null || echo "ERROR")
    if echo "$root" | grep -q '"status":"ok"'; then
        pass "Node $i root responds with status ok"
    else
        fail "Node $i root unexpected: $root"
    fi
done
echo ""

# ── Test 3: List collections on each node ──────────────────────────
echo "[3] List collections"
for i in 1 2 3; do
    port=$((3080 + i))
    collections=$(curl -s "http://127.0.0.1:$port/admin/collections" 2>/dev/null || echo "ERROR")
    if echo "$collections" | grep -q "docs"; then
        pass "Node $i has 'docs' collection"
    else
        fail "Node $i missing 'docs' collection (got: $collections)"
    fi
done
echo ""

# ── Test 4: Index documents on node 1 ─────────────────────────────
echo "[4] Index documents on node 1"
RESPONSE=$(curl -s -w '\n%{http_code}' -X POST "$NODE1/collections/docs/documents" \
    -H "Content-Type: application/json" \
    -d '{"documents": [
        {"id": "doc1", "fields": {"title": "Distributed Systems", "body": "A guide to building distributed search engines", "category": "engineering"}},
        {"id": "doc2", "fields": {"title": "HNSW Algorithm", "body": "Hierarchical navigable small world graphs for vector search", "category": "algorithms"}},
        {"id": "doc3", "fields": {"title": "Raft Consensus", "body": "Understanding consensus protocols for distributed databases", "category": "engineering"}},
        {"id": "doc4", "fields": {"title": "Vector Sharding", "body": "How to shard HNSW indexes across multiple nodes", "category": "engineering"}},
        {"id": "doc5", "fields": {"title": "QUIC Transport", "body": "Modern transport protocol for low-latency connections", "category": "networking"}}
    ]}' 2>/dev/null || echo "ERROR")

HTTP_CODE=$(echo "$RESPONSE" | tail -1)
BODY=$(echo "$RESPONSE" | sed '$d')
if [ "$HTTP_CODE" = "200" ] || [ "$HTTP_CODE" = "201" ]; then
    pass "Indexed 5 documents on node 1 (HTTP $HTTP_CODE)"
else
    fail "Index on node 1 failed (HTTP $HTTP_CODE): $BODY"
fi

# Small delay for indexing to settle
sleep 1
echo ""

# ── Test 5: Search on node 1 (POST) ──────────────────────────────
echo "[5] Search on node 1"
RESULTS=$(curl -s -X POST "$NODE1/collections/docs/search" \
    -H "Content-Type: application/json" \
    -d '{"query": "distributed systems", "limit": 10}' 2>/dev/null || echo "ERROR")

if echo "$RESULTS" | grep -q "doc1"; then
    pass "Search found 'doc1' (Distributed Systems)"
else
    fail "Search on node 1 did not find doc1 (got: $RESULTS)"
fi

# Check total count
TOTAL=$(echo "$RESULTS" | grep -o '"total":[0-9]*' | head -1 | cut -d: -f2)
if [ -n "$TOTAL" ] && [ "$TOTAL" -gt 0 ] 2>/dev/null; then
    pass "Search returned $TOTAL results"
else
    fail "Search returned 0 or unknown total (got: $RESULTS)"
fi
echo ""

# ── Test 6: Get document by ID on node 1 ──────────────────────────
echo "[6] Get document by ID on node 1"
DOC=$(curl -s "$NODE1/collections/docs/documents/doc2" 2>/dev/null || echo "ERROR")
if echo "$DOC" | grep -q "HNSW"; then
    pass "Got doc2 with correct content"
else
    fail "Get doc2 failed (got: $DOC)"
fi
echo ""

# ── Test 7: Stats on node 1 ───────────────────────────────────────
echo "[7] Stats on node 1"
STATS=$(curl -s "$NODE1/collections/docs/stats" 2>/dev/null || echo "ERROR")
if echo "$STATS" | grep -q '"document_count"'; then
    pass "Stats endpoint returns document_count"
    DOC_COUNT=$(echo "$STATS" | grep -o '"document_count":[0-9]*' | head -1 | cut -d: -f2)
    if [ "$DOC_COUNT" = "5" ]; then
        pass "Document count is 5"
    else
        fail "Expected document_count=5, got $DOC_COUNT"
    fi
else
    fail "Stats endpoint unexpected response: $STATS"
fi
echo ""

# ── Test 8: Index on node 2 (independent data) ────────────────────
echo "[8] Index documents on node 2 (independent)"
RESPONSE2=$(curl -s -w '\n%{http_code}' -X POST "$NODE2/collections/docs/documents" \
    -H "Content-Type: application/json" \
    -d '{"documents": [
        {"id": "n2_doc1", "fields": {"title": "Node 2 Document", "body": "This document lives on node 2 only", "category": "test"}}
    ]}' 2>/dev/null || echo "ERROR")

HTTP_CODE2=$(echo "$RESPONSE2" | tail -1)
if [ "$HTTP_CODE2" = "200" ] || [ "$HTTP_CODE2" = "201" ]; then
    pass "Indexed 1 document on node 2"
else
    fail "Index on node 2 failed (HTTP $HTTP_CODE2)"
fi

sleep 1

# Verify node 2 has the doc
DOC_N2=$(curl -s "$NODE2/collections/docs/documents/n2_doc1" 2>/dev/null || echo "ERROR")
if echo "$DOC_N2" | grep -q "Node 2 Document"; then
    pass "Node 2 has its own document"
else
    fail "Node 2 missing its own document: $DOC_N2"
fi
echo ""

# ── Test 9: Index on node 3 ───────────────────────────────────────
echo "[9] Index documents on node 3 (independent)"
RESPONSE3=$(curl -s -w '\n%{http_code}' -X POST "$NODE3/collections/docs/documents" \
    -H "Content-Type: application/json" \
    -d '{"documents": [
        {"id": "n3_doc1", "fields": {"title": "Node 3 First", "body": "Content on node three", "category": "test"}},
        {"id": "n3_doc2", "fields": {"title": "Node 3 Second", "body": "More content on node three", "category": "test"}}
    ]}' 2>/dev/null || echo "ERROR")

HTTP_CODE3=$(echo "$RESPONSE3" | tail -1)
if [ "$HTTP_CODE3" = "200" ] || [ "$HTTP_CODE3" = "201" ]; then
    pass "Indexed 2 documents on node 3"
else
    fail "Index on node 3 failed (HTTP $HTTP_CODE3)"
fi

sleep 1

# Search on node 3
RESULTS3=$(curl -s -X POST "$NODE3/collections/docs/search" \
    -H "Content-Type: application/json" \
    -d '{"query": "node three", "limit": 10}' 2>/dev/null || echo "ERROR")

if echo "$RESULTS3" | grep -q "n3_doc"; then
    pass "Node 3 search returns its own documents"
else
    fail "Node 3 search failed (got: $RESULTS3)"
fi
echo ""

# ── Test 10: Collection schema ─────────────────────────────────────
echo "[10] Collection schema"
SCHEMA=$(curl -s "$NODE1/collections/docs/schema" 2>/dev/null || echo "ERROR")
if echo "$SCHEMA" | grep -q "title"; then
    pass "Schema endpoint returns field definitions"
else
    fail "Schema endpoint unexpected response: $SCHEMA"
fi
echo ""

# ── Test 11: Metrics endpoint ─────────────────────────────────────
echo "[11] Metrics endpoint"
for i in 1 2 3; do
    port=$((3080 + i))
    check_status "http://127.0.0.1:$port/metrics" "200" "Node $i metrics"
done
echo ""

# ── Test 12: Cluster RPC server running ───────────────────────────
echo "[12] Cluster RPC server status (via logs)"
for i in 1 2 3; do
    LOG=$(docker logs "prism-node$i" 2>&1 | grep -c "Cluster server started" || echo "0")
    if [ "$LOG" -gt 0 ]; then
        pass "Node $i cluster RPC server started"
    else
        fail "Node $i cluster RPC server not found in logs"
    fi
done
echo ""

# ── Summary ────────────────────────────────────────────────────────
echo "========================================"
echo "Results: $PASSED passed, $FAILED failed"
echo "========================================"

if [ "$FAILED" -gt 0 ]; then
    exit 1
fi
