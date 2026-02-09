# Export & Import Guide

Prism supports exporting and importing collections for backup, migration, and disaster recovery.

## Export Formats

| Format | File | Use Case |
|--------|------|----------|
| **Portable** | `.prism.jsonl` | Cross-version, human-readable, smaller collections |
| **Snapshot** | `.tar.zst` | Fast backup/restore, same-version, large collections |
| **Encrypted** | `.enc` | Secure offloading to untrusted storage |

---

## CLI Export/Import

### Export Collection (Portable)

```bash
# Export to JSON Lines format
prism-cli collection export products -o products-backup.prism.jsonl

# With progress
prism-cli collection export products -o products-backup.prism.jsonl --progress
```

### Export Collection (Snapshot)

```bash
# Binary snapshot (faster, includes index files)
prism-cli collection export products -o products-backup.tar.zst --format snapshot
```

### Import Collection

```bash
# Import from portable format
prism-cli collection restore -f products-backup.prism.jsonl

# Import with new name
prism-cli collection restore -f products-backup.prism.jsonl --target-collection products-restored

# Import snapshot
prism-cli collection restore -f products-backup.tar.zst --format snapshot
```

---

## API Export/Import

### Standard Export

```bash
# Portable format
curl -X POST http://localhost:3080/_admin/export \
  -H "Content-Type: application/json" \
  -d '{
    "collection": "products",
    "format": "portable",
    "output_path": "/backup/products.prism.jsonl"
  }'
```

### Encrypted Export

For sensitive data or untrusted storage:

```bash
# Generate key first
KEY=$(curl -s -X POST http://localhost:3080/_admin/encryption/generate-key | jq -r .key)

# Export encrypted
curl -X POST http://localhost:3080/_admin/export/encrypted \
  -H "Content-Type: application/json" \
  -d "{
    \"collection\": \"sensitive-data\",
    \"key\": \"$KEY\",
    \"output_path\": \"/backup/sensitive.enc\"
  }"

# Save key securely!
echo "$KEY" > /secure/backup-key.txt
```

### Encrypted Import

```bash
curl -X POST http://localhost:3080/_admin/import/encrypted \
  -H "Content-Type: application/json" \
  -d '{
    "input_path": "/backup/sensitive.enc",
    "key": "your-64-char-hex-key",
    "target_collection": "sensitive-data-restored"
  }'
```

---

## Format Comparison

### Portable Format (`.prism.jsonl`)

```
Line 1: {"format": "prism-portable-v1", "metadata": {...}, "schema_b64": "..."}
Line 2+: {"id": "doc-1", "fields": {...}, "vector": [...]}
```

**Pros:**
- Human-readable
- Cross-version compatible
- Can be processed with `jq`, grep, etc.
- Includes schema for full restoration

**Cons:**
- Slower for large collections
- Larger file size
- Re-indexing required on import

### Snapshot Format (`.tar.zst`)

```
metadata.json    - Export metadata
schema.yaml      - Collection schema
text/            - Tantivy index files
vector/          - HNSW index files
graph/           - Graph backend files (if enabled)
```

**Pros:**
- Fast backup/restore
- Preserves index structures
- Smaller file size (zstd compression)

**Cons:**
- Same Prism version required
- Binary format, not human-readable

### Encrypted Format (`.enc`)

Same as snapshot, but wrapped in AES-256-GCM encryption.

**Pros:**
- Secure for untrusted storage
- Runtime key configuration
- No server restart needed

**Cons:**
- Requires key management
- Slightly larger (encryption overhead)

---

## Backup Strategy

### Daily Incremental + Weekly Full

```bash
#!/bin/bash
# backup.sh

DATA_DIR="/var/lib/prism/data"
BACKUP_DIR="/backup/prism"
DATE=$(date +%Y-%m-%d)
DAY=$(date +%u)  # 1-7

# Weekly full backup on Sunday
if [ "$DAY" -eq 7 ]; then
  for collection in $(prism-cli collection list --json | jq -r '.[].name'); do
    prism-cli collection export "$collection" \
      -o "$BACKUP_DIR/full/$DATE-$collection.tar.zst" \
      --format snapshot
  done
fi

# Daily portable export (smaller, for quick restore)
for collection in $(prism-cli collection list --json | jq -r '.[].name'); do
  prism-cli collection export "$collection" \
    -o "$BACKUP_DIR/daily/$DATE-$collection.prism.jsonl"
done

# Cleanup old backups (keep 7 daily, 4 weekly)
find "$BACKUP_DIR/daily" -mtime +7 -delete
find "$BACKUP_DIR/full" -mtime +28 -delete
```

### Encrypted Cloud Backup

```bash
#!/bin/bash
# encrypted-backup.sh

KEY_FILE="/secure/prism-backup-key.txt"
CLOUD_BUCKET="s3://my-backups/prism"

# Read or generate key
if [ ! -f "$KEY_FILE" ]; then
  curl -s -X POST http://localhost:3080/_admin/encryption/generate-key | jq -r .key > "$KEY_FILE"
  chmod 600 "$KEY_FILE"
fi

KEY=$(cat "$KEY_FILE")
DATE=$(date +%Y-%m-%d)

# Export each collection encrypted
for collection in $(prism-cli collection list --json | jq -r '.[].name'); do
  curl -s -X POST http://localhost:3080/_admin/export/encrypted \
    -H "Content-Type: application/json" \
    -d "{
      \"collection\": \"$collection\",
      \"key\": \"$KEY\",
      \"output_path\": \"/tmp/$collection.enc\"
    }"

  # Upload to S3
  aws s3 cp "/tmp/$collection.enc" "$CLOUD_BUCKET/$DATE/$collection.enc"
  rm "/tmp/$collection.enc"
done
```

---

## Migration Between Servers

### Same Version

Use snapshot format for fastest migration:

```bash
# On source server
prism-cli collection export myindex -o myindex.tar.zst --format snapshot
scp myindex.tar.zst target-server:/backup/

# On target server
prism-cli collection restore -f /backup/myindex.tar.zst --format snapshot
```

### Different Versions

Use portable format for compatibility:

```bash
# On source server (v0.3.x)
prism-cli collection export myindex -o myindex.prism.jsonl

# On target server (v0.4.x)
prism-cli collection restore -f myindex.prism.jsonl
```

---

## Troubleshooting

### Import Fails with Schema Error

The portable format includes the schema. If the target has a conflicting schema:

```bash
# Delete existing collection first
prism-cli collection delete myindex

# Then import
prism-cli collection restore -f backup.prism.jsonl
```

### Snapshot Version Mismatch

```
Error: Incompatible snapshot version
```

Use portable format instead, which re-indexes documents.

### Checksum Verification Failed

```
Error: Checksum verification failed
```

The file was corrupted during transfer. Re-download and verify:

```bash
sha256sum backup.prism.jsonl
```

---

## Live Detach & Attach

Detach and attach allow you to safely snapshot and unload a collection from a running server, then re-attach it later — all without restarting. This is useful for data lifecycle management, archiving old indices, and moving collections between servers.

### How It Works

**Detach** creates a snapshot archive and then unloads the collection from the running server. Optionally deletes the on-disk data.

**Attach** imports a snapshot archive to disk and hot-loads the collection into the running server.

!!! warning "Safe ordering"
    Detach always exports the snapshot **before** unloading. If the export fails, the collection remains loaded and untouched.

### CLI Detach/Attach

```bash
# Detach: snapshot + unload from running server
prism collection detach --name logs-2025 --output /backups/logs-2025.tar.zst

# Detach and delete on-disk data
prism collection detach --name logs-2025 --output /backups/logs-2025.tar.zst --delete-data

# Attach: import snapshot + hot-load into running server
prism collection attach --input /backups/logs-2025.tar.zst

# Attach with a different collection name
prism collection attach --input /backups/logs-2025.tar.zst --target logs-restored

# Custom API URL
prism collection detach --name logs-2025 --output /backups/logs-2025.tar.zst \
  --api-url http://prism.internal:3080
```

### API Detach/Attach

```bash
# Detach a collection
curl -X POST http://localhost:3080/_admin/collections/logs-2025/detach \
  -H "Content-Type: application/json" \
  -d '{
    "destination": {"type": "file", "path": "/backups/logs-2025.tar.zst"},
    "delete_data": false
  }'
```

Response:

```json
{
  "collection": "logs-2025",
  "destination": {"type": "file", "path": "/backups/logs-2025.tar.zst"},
  "metadata": { "version": "1.0", "collection": "logs-2025", ... },
  "data_deleted": false
}
```

```bash
# Attach a collection
curl -X POST http://localhost:3080/_admin/collections/attach \
  -H "Content-Type: application/json" \
  -d '{
    "source": {"type": "file", "path": "/backups/logs-2025.tar.zst"},
    "target_collection": null
  }'
```

Response:

```json
{
  "collection": "logs-2025",
  "source": {"type": "file", "path": "/backups/logs-2025.tar.zst"},
  "files_extracted": 42,
  "bytes_extracted": 104857600
}
```

### Use Cases

**Archive old data:**

```bash
# Monthly log rotation — detach last month, keep snapshot
prism collection detach --name logs-2025-01 \
  --output /archive/logs-2025-01.tar.zst --delete-data
```

**Move collection between servers:**

```bash
# Source server
prism collection detach --name products \
  --output /tmp/products.tar.zst --api-url http://source:3080

scp /tmp/products.tar.zst target-server:/tmp/

# Target server
prism collection attach --input /tmp/products.tar.zst \
  --api-url http://target:3080
```

**Emergency unload (free memory):**

```bash
# Unload a large collection to free memory, keep data on disk
prism collection detach --name big-index \
  --output /backups/big-index.tar.zst
```

---

## See Also

- [Encryption Guide](encryption.md) — Encrypted export/import
- [Storage Backends](../admin/storage-backends.md) — Storage configuration
- [CLI Reference](../cli/prism-cli.md) — Full CLI documentation
