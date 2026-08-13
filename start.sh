#!/bin/bash
# Start prism-server detached, surviving the parent shell.
cd /home/mikalv/prism
exec ./target/release/prism-server \
  --host 0.0.0.0 --port 3080 \
  --data-dir ./prism-data \
  --schemas-dir ./test-data/schemas \
  --config ./prism.toml
