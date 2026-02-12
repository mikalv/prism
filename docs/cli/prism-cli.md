# prism-cli

Command-line tools for managing Prism collections and data.

## Synopsis

```bash
prism [OPTIONS] <COMMAND>
```

## Global Options

| Option | Default | Description |
|--------|---------|-------------|
| `-d, --data-dir <DIR>` | `./data` | Data directory |
| `-h, --help` | — | Print help |
| `-V, --version` | — | Print version |

## Commands

- [collection](#collection) — Manage collections
- [document](#document) — Import/export documents
- [index](#index) — Index maintenance
- [benchmark](#benchmark) — Run search benchmarks
- [cache-stats](#cache-stats) — View cache statistics
- [cache-clear](#cache-clear) — Clear cache

---

## collection

Manage collections.

### collection list

List all collections.

```bash
prism collection list
```

Output:

```
Collections:
  - articles (125,430 documents)
  - products (45,230 documents)
  - logs-2025-01 (1,245,000 documents)
```

### collection inspect

Inspect collection index structure and statistics.

```bash
prism collection inspect --name <NAME> [--verbose]
```

| Option | Description |
|--------|-------------|
| `-n, --name <NAME>` | Collection name (required) |
| `-v, --verbose` | Show detailed segment info |

Example:

```bash
prism collection inspect -n articles -v
```

Output:

```
Collection: articles
Documents: 125,430
Segments: 5
  - Segment 0: 50,000 docs, 128 MB
  - Segment 1: 40,000 docs, 102 MB
  - Segment 2: 25,000 docs, 64 MB
  - Segment 3: 8,000 docs, 20 MB
  - Segment 4: 2,430 docs, 6 MB

Text Backend:
  Fields: title (text), content (text), author (string)
  Total size: 320 MB

Vector Backend:
  Dimension: 384
  Index size: 48 MB
  HNSW layers: 4
```

### collection detach

Detach a collection from a running server (snapshot + unload).

```bash
prism collection detach --name <NAME> --output <FILE> [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `-n, --name <NAME>` | required | Collection name |
| `-o, --output <FILE>` | required | Output snapshot path |
| `--api-url <URL>` | `http://localhost:3080` | Prism API URL |
| `--delete-data` | false | Delete on-disk data after detaching |

Examples:

```bash
# Detach and keep data on disk
prism collection detach -n logs-2025 -o /backups/logs-2025.tar.zst

# Detach and delete data
prism collection detach -n logs-2025 -o /backups/logs-2025.tar.zst --delete-data
```

### collection attach

Attach a collection from a snapshot into a running server.

```bash
prism collection attach --input <FILE> [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `-i, --input <FILE>` | required | Input snapshot path |
| `-t, --target <NAME>` | from snapshot | Override collection name |
| `--api-url <URL>` | `http://localhost:3080` | Prism API URL |

Examples:

```bash
# Attach from snapshot
prism collection attach -i /backups/logs-2025.tar.zst

# Attach with a different name
prism collection attach -i /backups/logs-2025.tar.zst -t logs-restored
```

### collection graph-merge

Merge all graph shards into shard 0 for full graph traversal.

```bash
prism collection graph-merge --name <NAME> [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `-n, --name <NAME>` | required | Collection name |
| `--schemas-dir <DIR>` | `schemas` | Schemas directory path |

Examples:

```bash
# Merge all graph shards in a collection
prism collection graph-merge -n knowledge-base

# With custom schemas directory
prism collection graph-merge -n knowledge-base --schemas-dir /etc/prism/schemas
```

### collection merge

Merge multiple collections into a new target collection.

```bash
prism collection merge --source <NAME> --source <NAME> --target <NAME> [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `-s, --source <NAME>` | required (2+) | Source collection names |
| `-t, --target <NAME>` | required | Target collection name |
| `--schemas-dir <DIR>` | `schemas` | Schemas directory path |

Examples:

```bash
# Merge two collections into a new one
prism collection merge -s col_a -s col_b -t combined

# Merge three collections
prism collection merge -s tenant_1 -s tenant_2 -s tenant_3 -t all_tenants
```

---

## document

Import and export documents.

### document import

Import documents from JSONL file.

```bash
prism document import --collection <NAME> [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `-c, --collection <NAME>` | required | Target collection |
| `-f, --file <FILE>` | stdin | Input JSONL file |
| `--api-url <URL>` | `http://localhost:3080` | Prism API URL |
| `--batch-size <N>` | `100` | Documents per batch |
| `--no-progress` | false | Disable progress bar |

Examples:

```bash
# Import from file
prism document import -c articles -f articles.jsonl

# Import from stdin
cat articles.jsonl | prism document import -c articles

# Custom batch size and API URL
prism document import -c articles -f articles.jsonl \
  --api-url http://prism.internal:3080 \
  --batch-size 500
```

JSONL format (one JSON object per line):

```json
{"id": "1", "title": "Hello", "content": "World"}
{"id": "2", "title": "Foo", "content": "Bar"}
```

### document export

Export collection to JSONL file.

```bash
prism document export --collection <NAME> [--output <FILE>]
```

| Option | Default | Description |
|--------|---------|-------------|
| `-c, --collection <NAME>` | required | Source collection |
| `-o, --output <FILE>` | stdout | Output file |

Examples:

```bash
# Export to file
prism document export -c articles -o articles-backup.jsonl

# Export to stdout (pipe to other tools)
prism document export -c articles | gzip > articles.jsonl.gz
```

---

## index

Index maintenance operations.

### index optimize

Merge segments and garbage collect deleted documents.

```bash
prism index optimize --collection <NAME> [--gc-only]
```

| Option | Description |
|--------|-------------|
| `-c, --collection <NAME>` | Collection name (required) |
| `--gc-only` | Only garbage collect, don't merge |

Examples:

```bash
# Full optimization
prism index optimize -c articles

# Just garbage collect
prism index optimize -c articles --gc-only
```

When to optimize:

- After bulk imports
- After many deletes
- When search performance degrades
- Scheduled maintenance (weekly/monthly)

---

## benchmark

Run search benchmarks.

```bash
prism benchmark --collection <NAME> --queries <FILE> [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `-c, --collection <NAME>` | required | Collection to benchmark |
| `-q, --queries <FILE>` | required | File with queries (one per line) |
| `-r, --repeat <N>` | `10` | Repeat each query N times |
| `-w, --warmup <N>` | `3` | Warmup iterations |
| `-k, --top-k <N>` | `10` | Results to fetch |

Example:

```bash
# Create query file
cat > queries.txt << EOF
hybrid search
vector embeddings
full text search
EOF

# Run benchmark
prism benchmark -c articles -q queries.txt -r 50
```

Output:

```
Benchmark: articles (50 iterations, 3 warmup)
  Query                  p50      p95      p99      QPS
  hybrid search          12ms     18ms     24ms     83.3
  vector embeddings      15ms     22ms     31ms     66.7
  full text search        8ms     12ms     15ms    125.0

  Overall: 91.7 QPS (avg), 11.7ms p50
```

---

## cache-stats

View embedding cache statistics.

```bash
prism cache-stats --path <PATH>
```

| Option | Description |
|--------|-------------|
| `-p, --path <PATH>` | Cache directory path (required) |

Example:

```bash
prism cache-stats -p ~/.prism/cache
```

Output:

```
Embedding Cache: ~/.prism/cache
  Entries: 45,230
  Size: 128 MB
  Hit rate: 94.2%
  Oldest: 2025-01-01 10:00:00
  Newest: 2025-01-15 14:30:00
```

---

## cache-clear

Clear embedding cache.

```bash
prism cache-clear --path <PATH> [--older-than-days <N>]
```

| Option | Description |
|--------|-------------|
| `-p, --path <PATH>` | Cache directory path (required) |
| `--older-than-days <N>` | Only clear entries older than N days |

Examples:

```bash
# Clear all cache
prism cache-clear -p ~/.prism/cache

# Clear entries older than 30 days
prism cache-clear -p ~/.prism/cache --older-than-days 30
```

---

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Error (see stderr for details) |

## See Also

- [prism-server](prism-server.md)
- [prism-import](prism-import.md)
- [Configuration Reference](../admin/configuration.md)
