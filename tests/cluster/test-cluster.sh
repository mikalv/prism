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
#   2. Index documents on node 1 (local HTTP API)
#   3. Search on node 1 returns results (local)
#   4. FEDERATED search from node 3 finds docs indexed on node 1 (cross-node RPC)
#   5. FEDERATED search from node 2 also finds docs from node 1
#   6. Stats, schema, and metrics on each node
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

# ── Test 5: Local search on node 1 ────────────────────────────────
echo "[5] Local search on node 1"
RESULTS=$(curl -s -X POST "$NODE1/collections/docs/search" \
    -H "Content-Type: application/json" \
    -d '{"query": "distributed systems", "limit": 10}' 2>/dev/null || echo "ERROR")

if echo "$RESULTS" | grep -q "doc1"; then
    pass "Local search found 'doc1' (Distributed Systems)"
else
    fail "Local search on node 1 did not find doc1 (got: $RESULTS)"
fi

TOTAL=$(echo "$RESULTS" | grep -o '"total":[0-9]*' | head -1 | cut -d: -f2)
if [ -n "$TOTAL" ] && [ "$TOTAL" -gt 0 ] 2>/dev/null; then
    pass "Local search returned $TOTAL results"
else
    fail "Local search returned 0 or unknown total (got: $RESULTS)"
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

# ── Test 8: Cluster RPC server status ─────────────────────────────
echo "[8] Cluster RPC server status (via logs)"
for i in 1 2 3; do
    LOG=$(docker logs "prism-node$i" 2>&1 | grep -c "cluster RPC server" || echo "0")
    if [ "$LOG" -gt 0 ]; then
        pass "Node $i cluster RPC server started"
    else
        # Also check for the new log message format
        LOG2=$(docker logs "prism-node$i" 2>&1 | grep -c "Cluster initialized" || echo "0")
        if [ "$LOG2" -gt 0 ]; then
            pass "Node $i cluster initialized"
        else
            fail "Node $i cluster not found in logs"
        fi
    fi
done
echo ""

# ── Test 9: Cluster health endpoint ───────────────────────────────
echo "[9] Cluster health endpoint"
for i in 1 2 3; do
    port=$((3080 + i))
    CHEALTH=$(curl -s "http://127.0.0.1:$port/cluster/health" 2>/dev/null || echo "ERROR")
    if echo "$CHEALTH" | grep -q '"federated":true'; then
        pass "Node $i cluster health reports federated=true"
    else
        fail "Node $i cluster health unexpected: $CHEALTH"
    fi
done
echo ""

# ══════════════════════════════════════════════════════════════════
# CRITICAL E2E TEST: Cross-node federated search
# Index on node 1, search via federation from node 3
# This proves actual inter-node QUIC RPC communication works
# ══════════════════════════════════════════════════════════════════

echo "========================================"
echo "FEDERATED SEARCH E2E (cross-node RPC)"
echo "========================================"
echo ""

# ── Test 10: Federated search from node 3 finds node 1's docs ────
echo "[10] Federated search from NODE 3 for docs indexed on NODE 1"
FED_RESULTS=$(curl -s -X POST "$NODE3/cluster/collections/docs/search" \
    -H "Content-Type: application/json" \
    -d '{"query": "distributed systems", "limit": 10}' 2>/dev/null || echo "ERROR")

echo "    Response: $FED_RESULTS"

if echo "$FED_RESULTS" | grep -q "doc1"; then
    pass "FEDERATED: Node 3 found 'doc1' from node 1 via cross-node RPC!"
else
    fail "FEDERATED: Node 3 did NOT find doc1 (indexed on node 1). Response: $FED_RESULTS"
fi

# Check the federated search returned results from node 1's data
if echo "$FED_RESULTS" | grep -q "Distributed Systems"; then
    pass "FEDERATED: Node 3 got full document content from node 1"
else
    fail "FEDERATED: Content mismatch. Response: $FED_RESULTS"
fi

# Check is_partial field exists (shows federation layer is active)
if echo "$FED_RESULTS" | grep -q '"is_partial"'; then
    pass "FEDERATED: Response includes shard status metadata"
else
    fail "FEDERATED: Missing shard status metadata"
fi
echo ""

# ── Test 11: Federated search from node 2 also finds node 1's docs
echo "[11] Federated search from NODE 2 for docs indexed on NODE 1"
FED_RESULTS2=$(curl -s -X POST "$NODE2/cluster/collections/docs/search" \
    -H "Content-Type: application/json" \
    -d '{"query": "HNSW vector search", "limit": 10}' 2>/dev/null || echo "ERROR")

echo "    Response: $FED_RESULTS2"

if echo "$FED_RESULTS2" | grep -q "doc2"; then
    pass "FEDERATED: Node 2 found 'doc2' from node 1 via cross-node RPC!"
else
    fail "FEDERATED: Node 2 did NOT find doc2 (indexed on node 1). Response: $FED_RESULTS2"
fi
echo ""

# ── Test 12: Federated index via node 3, then search from node 1 ──
echo "[12] Federated index via NODE 3, then search from NODE 1"
FED_INDEX=$(curl -s -w '\n%{http_code}' -X POST "$NODE3/cluster/collections/docs/documents" \
    -H "Content-Type: application/json" \
    -d '{"documents": [
        {"id": "fed_doc1", "fields": {"title": "Federated Document", "body": "This doc was indexed via federation from node 3", "category": "test"}}
    ]}' 2>/dev/null || echo "ERROR")

FED_INDEX_CODE=$(echo "$FED_INDEX" | tail -1)
FED_INDEX_BODY=$(echo "$FED_INDEX" | sed '$d')
echo "    Index response (HTTP $FED_INDEX_CODE): $FED_INDEX_BODY"

if [ "$FED_INDEX_CODE" = "200" ] || [ "$FED_INDEX_CODE" = "201" ]; then
    pass "Federated index via node 3 accepted"
else
    fail "Federated index via node 3 failed (HTTP $FED_INDEX_CODE)"
fi

sleep 1

# Search from node 1's federation endpoint
FED_SEARCH_BACK=$(curl -s -X POST "$NODE1/cluster/collections/docs/search" \
    -H "Content-Type: application/json" \
    -d '{"query": "federated document", "limit": 10}' 2>/dev/null || echo "ERROR")

echo "    Search from node 1: $FED_SEARCH_BACK"

if echo "$FED_SEARCH_BACK" | grep -q "fed_doc1"; then
    pass "FEDERATED: Node 1 found fed_doc1 (indexed via node 3 federation)"
else
    fail "FEDERATED: Node 1 did NOT find fed_doc1. Response: $FED_SEARCH_BACK"
fi
echo ""

# ── Test 13: Collection schema ────────────────────────────────────
echo "[13] Collection schema"
SCHEMA=$(curl -s "$NODE1/collections/docs/schema" 2>/dev/null || echo "ERROR")
if echo "$SCHEMA" | grep -q "title"; then
    pass "Schema endpoint returns field definitions"
else
    fail "Schema endpoint unexpected response: $SCHEMA"
fi
echo ""

# ── Test 14: Metrics endpoint ────────────────────────────────────
echo "[14] Metrics endpoint"
for i in 1 2 3; do
    port=$((3080 + i))
    check_status "http://127.0.0.1:$port/metrics" "200" "Node $i metrics"
done
echo ""

# ── Summary ────────────────────────────────────────────────────────
echo "========================================"
echo "Results: $PASSED passed, $FAILED failed"
echo "========================================"

if [ "$FAILED" -gt 0 ]; then
    exit 1
fi
