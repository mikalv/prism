#!/usr/bin/env bash
# Run the full 3-node cluster integration test.
#
# Usage:
#   ./run.sh          # Build, start, test, and tear down
#   ./run.sh --keep   # Keep cluster running after tests
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

KEEP=false
if [ "${1:-}" = "--keep" ]; then
    KEEP=true
fi

echo "=== Prism Cluster Integration Test ==="
echo ""

# Step 1: Generate TLS certificates
echo "[Step 1] Generating TLS certificates..."
./generate-certs.sh
echo ""

# Step 2: Build and start cluster
echo "[Step 2] Building Docker images (with cluster feature)..."
docker compose build --build-arg FEATURES=cluster
echo ""

echo "[Step 3] Starting 3-node cluster..."
docker compose up -d
echo ""

# Step 3: Wait for nodes to be ready
echo "[Step 4] Waiting for nodes to be ready..."
MAX_WAIT=120
WAIT=0
READY=false

while [ $WAIT -lt $MAX_WAIT ]; do
    ALL_UP=true
    for port in 3081 3082 3083; do
        STATUS=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$port/_health" 2>/dev/null || echo "000")
        if [ "$STATUS" != "200" ]; then
            ALL_UP=false
            break
        fi
    done

    if $ALL_UP; then
        READY=true
        break
    fi

    sleep 2
    WAIT=$((WAIT + 2))
    printf "\r  Waiting... (%ds/%ds)" "$WAIT" "$MAX_WAIT"
done
echo ""

if ! $READY; then
    echo "ERROR: Cluster did not become ready within ${MAX_WAIT}s"
    echo ""
    echo "=== Container logs ==="
    docker compose logs --tail=30
    docker compose down -v
    exit 1
fi

echo "  All 3 nodes are healthy."
echo ""

# Step 4: Run integration tests
echo "[Step 5] Running integration tests..."
echo ""
TEST_EXIT=0
./test-cluster.sh || TEST_EXIT=$?
echo ""

# Step 5: Cleanup
if $KEEP; then
    echo "Cluster kept running. To stop: docker compose down -v"
else
    echo "[Step 6] Tearing down cluster..."
    docker compose down -v
fi

exit $TEST_EXIT
