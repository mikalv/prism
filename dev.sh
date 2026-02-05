#!/bin/bash

set -e

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

cleanup() {
  echo "Shutting down..."
  if [ ! -z "$SERVER_PID" ]; then
    kill $SERVER_PID 2>/dev/null || true
  fi
  exit
}

trap cleanup INT TERM

echo "Starting prism-server..."
cd prism-server
cargo run &
SERVER_PID=$!
cd ..

echo "Waiting for server to start..."
sleep 3

echo "Starting websearch-ui..."
cd websearch-ui
npm run dev &
FRONTEND_PID=$!
cd ..

echo "Services running:"
echo "  Backend: http://localhost:3080"
echo "  Frontend: http://localhost:5173"

wait $FRONTEND_PID
