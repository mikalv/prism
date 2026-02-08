---
name: prism-backup
description: Use when backing up, restoring, or migrating Prism collections. Covers export formats, encrypted backups, disaster recovery scripts, cross-version migration, and automated backup strategies.
---

# Prism Backup & Recovery

Complete guide for backing up, restoring, and migrating Prism data.

## Export Formats

| Format | Extension | Cross-Version | Speed | Use Case |
|--------|-----------|---------------|-------|----------|
| Portable | `.prism.jsonl` | Yes | Slow | Migration, debugging |
| Snapshot | `.tar.zst` | No | Fast | Daily backups |
| Encrypted | `.enc` | No | Fast | Cloud storage, compliance |

---

## CLI Export/Import

### Portable Format (Cross-Version Safe)

```bash
# Export
prism-cli collection export articles -o articles.prism.jsonl

# With progress
prism-cli collection export articles -o articles.prism.jsonl --progress

# Import
prism-cli collection restore -f articles.prism.jsonl

# Import with new name
prism-cli collection restore -f articles.prism.jsonl --target-collection articles-v2
```

### Snapshot Format (Fast, Same-Version)

```bash
# Export
prism-cli collection export articles -o articles.tar.zst --format snapshot

# Import
prism-cli collection restore -f articles.tar.zst --format snapshot
```

### Document-Level Export (JSONL)

```bash
# Export documents only (no schema)
prism-cli document export -c articles -o articles.jsonl

# Import documents
prism-cli document import -c articles -f articles.jsonl --batch-size 500
```

---

## Encrypted Backup (API)

### Generate Encryption Key

```bash
curl -X POST http://localhost:3080/_admin/encryption/generate-key
```

Response:
```json
{
  "key": "a1b2c3d4e5f6...64 hex characters",
  "key_bytes": 32,
  "algorithm": "AES-256-GCM"
}
```

**Save this key securely!** Prism never stores it.

### Encrypted Export

```bash
curl -X POST http://localhost:3080/_admin/export/encrypted \
  -H "Content-Type: application/json" \
  -d '{
    "collection": "sensitive-data",
    "key": "a1b2c3d4e5f6...64 hex chars",
    "output_path": "/backup/sensitive.enc"
  }'
```

### Encrypted Import

```bash
curl -X POST http://localhost:3080/_admin/import/encrypted \
  -H "Content-Type: application/json" \
  -d '{
    "input_path": "/backup/sensitive.enc",
    "key": "a1b2c3d4e5f6...64 hex chars",
    "target_collection": "sensitive-restored"
  }'
```

---

## Backup Scripts

### Daily Snapshot Backup

```bash
#!/bin/bash
# daily-backup.sh

BACKUP_DIR="/backup/prism/daily"
DATE=$(date +%Y-%m-%d)
API="http://localhost:3080"

mkdir -p "$BACKUP_DIR"

# Get all collections
collections=$(curl -s "$API/admin/collections" | jq -r '.[]')

for collection in $collections; do
  echo "Backing up: $collection"
  prism-cli collection export "$collection" \
    -o "$BACKUP_DIR/$DATE-$collection.tar.zst" \
    --format snapshot
done

# Cleanup backups older than 7 days
find "$BACKUP_DIR" -name "*.tar.zst" -mtime +7 -delete

echo "Backup complete: $BACKUP_DIR"
```

### Weekly Full Backup (Portable)

```bash
#!/bin/bash
# weekly-backup.sh

BACKUP_DIR="/backup/prism/weekly"
DATE=$(date +%Y-%m-%d)
API="http://localhost:3080"

mkdir -p "$BACKUP_DIR"

for collection in $(curl -s "$API/admin/collections" | jq -r '.[]'); do
  echo "Full backup: $collection"
  prism-cli collection export "$collection" \
    -o "$BACKUP_DIR/$DATE-$collection.prism.jsonl"
done

# Cleanup backups older than 28 days
find "$BACKUP_DIR" -name "*.prism.jsonl" -mtime +28 -delete
```

### Encrypted Cloud Backup

```bash
#!/bin/bash
# encrypted-cloud-backup.sh

KEY_FILE="/secure/prism-backup-key.txt"
S3_BUCKET="s3://my-company-backups/prism"
API="http://localhost:3080"
DATE=$(date +%Y-%m-%d)

# Generate or read key
if [ ! -f "$KEY_FILE" ]; then
  echo "Generating new encryption key..."
  curl -s -X POST "$API/_admin/encryption/generate-key" | jq -r .key > "$KEY_FILE"
  chmod 600 "$KEY_FILE"
  echo "Key saved to $KEY_FILE - STORE SECURELY!"
fi

KEY=$(cat "$KEY_FILE")

# Export each collection encrypted
for collection in $(curl -s "$API/admin/collections" | jq -r '.[]'); do
  echo "Encrypting: $collection"

  curl -s -X POST "$API/_admin/export/encrypted" \
    -H "Content-Type: application/json" \
    -d "{
      \"collection\": \"$collection\",
      \"key\": \"$KEY\",
      \"output_path\": \"/tmp/$collection.enc\"
    }"

  # Upload to S3
  aws s3 cp "/tmp/$collection.enc" "$S3_BUCKET/$DATE/$collection.enc"
  rm "/tmp/$collection.enc"

  echo "Uploaded: $S3_BUCKET/$DATE/$collection.enc"
done

echo "Encrypted backup complete"
```

### Restore from Encrypted Cloud Backup

```bash
#!/bin/bash
# restore-from-cloud.sh

KEY_FILE="/secure/prism-backup-key.txt"
S3_BUCKET="s3://my-company-backups/prism"
API="http://localhost:3080"
BACKUP_DATE="${1:-$(date +%Y-%m-%d)}"  # Use arg or today

KEY=$(cat "$KEY_FILE")

# List available backups
echo "Available backups for $BACKUP_DATE:"
aws s3 ls "$S3_BUCKET/$BACKUP_DATE/"

# Restore each collection
for file in $(aws s3 ls "$S3_BUCKET/$BACKUP_DATE/" | awk '{print $4}'); do
  collection="${file%.enc}"
  echo "Restoring: $collection"

  # Download
  aws s3 cp "$S3_BUCKET/$BACKUP_DATE/$file" "/tmp/$file"

  # Import
  curl -X POST "$API/_admin/import/encrypted" \
    -H "Content-Type: application/json" \
    -d "{
      \"input_path\": \"/tmp/$file\",
      \"key\": \"$KEY\"
    }"

  rm "/tmp/$file"
done

echo "Restore complete"
```

---

## Disaster Recovery

### Emergency: Disk Full

When disk unexpectedly fills and you need to offload data immediately:

```bash
#!/bin/bash
# emergency-offload.sh

API="http://localhost:3080"
EXTERNAL_MOUNT="/mnt/emergency-storage"  # NFS, USB, etc.
KEY_FILE="/tmp/emergency-key.json"

# 1. Generate emergency key
echo "Generating emergency encryption key..."
curl -X POST "$API/_admin/encryption/generate-key" > "$KEY_FILE"
KEY=$(jq -r .key "$KEY_FILE")
echo "KEY SAVED TO $KEY_FILE - COPY THIS FILE TO SAFE LOCATION!"

# 2. Identify largest collections
echo "Collection sizes:"
for c in $(curl -s "$API/admin/collections" | jq -r '.[]'); do
  size=$(curl -s "$API/collections/$c/stats" | jq -r '.size_bytes')
  echo "  $c: $(numfmt --to=iec $size)"
done

# 3. Export largest collections to external storage
read -p "Enter collections to offload (space-separated): " collections

for c in $collections; do
  echo "Exporting $c..."
  curl -X POST "$API/_admin/export/encrypted" \
    -H "Content-Type: application/json" \
    -d "{
      \"collection\": \"$c\",
      \"key\": \"$KEY\",
      \"output_path\": \"$EXTERNAL_MOUNT/$c.enc\"
    }"
done

# 4. Verify exports
echo "Verifying exports..."
ls -lh "$EXTERNAL_MOUNT"/*.enc

# 5. Delete local collections (CONFIRM!)
read -p "Delete local collections? (yes/no): " confirm
if [ "$confirm" = "yes" ]; then
  for c in $collections; do
    echo "Deleting $c locally..."
    prism-cli collection delete "$c"
  done
  echo "Local collections deleted. Free space:"
  df -h /var/lib/prism
fi

echo "
IMPORTANT: Save these for recovery:
1. Key file: $KEY_FILE
2. Exports: $EXTERNAL_MOUNT/*.enc
"
```

### Restore After Emergency

```bash
#!/bin/bash
# emergency-restore.sh

API="http://localhost:3080"
EXTERNAL_MOUNT="/mnt/emergency-storage"
KEY_FILE="/secure/emergency-key.json"

KEY=$(jq -r .key "$KEY_FILE")

for file in "$EXTERNAL_MOUNT"/*.enc; do
  collection=$(basename "$file" .enc)
  echo "Restoring $collection..."

  curl -X POST "$API/_admin/import/encrypted" \
    -H "Content-Type: application/json" \
    -d "{
      \"input_path\": \"$file\",
      \"key\": \"$KEY\"
    }"
done

echo "Restore complete"
```

---

## Migration

### Local → S3 Storage Backend

```bash
#!/bin/bash
# migrate-to-s3.sh

API="http://localhost:3080"
BACKUP_DIR="/tmp/prism-migration"

mkdir -p "$BACKUP_DIR"

# 1. Export all collections
echo "Exporting all collections..."
for c in $(curl -s "$API/admin/collections" | jq -r '.[]'); do
  prism-cli document export -c "$c" -o "$BACKUP_DIR/$c.jsonl"
done

# 2. Stop server
echo "Stop prism-server and update prism.toml to use S3 backend"
read -p "Press enter when ready to continue..."

# 3. Re-import all collections
echo "Re-importing collections..."
for file in "$BACKUP_DIR"/*.jsonl; do
  c=$(basename "$file" .jsonl)
  prism-cli document import -c "$c" -f "$file"
done

echo "Migration complete"
```

### Cross-Version Migration

```bash
# On old Prism version (v0.4.x)
prism-cli collection export myindex -o myindex.prism.jsonl

# Transfer file to new server
scp myindex.prism.jsonl new-server:/backup/

# On new Prism version (v0.5.x)
prism-cli collection restore -f /backup/myindex.prism.jsonl
```

### Same-Version Fast Migration

```bash
# On source server
prism-cli collection export myindex -o myindex.tar.zst --format snapshot
scp myindex.tar.zst target-server:/backup/

# On target server
prism-cli collection restore -f /backup/myindex.tar.zst --format snapshot
```

### Elasticsearch → Prism

```bash
prism-import \
  --source http://elasticsearch:9200 \
  --index "logs-*" \
  --target http://localhost:3080 \
  --batch-size 1000
```

---

## Backup Verification

### Verify Backup Integrity

```bash
#!/bin/bash
# verify-backup.sh

BACKUP_FILE="$1"

if [[ "$BACKUP_FILE" == *.tar.zst ]]; then
  echo "Verifying snapshot..."
  zstd -t "$BACKUP_FILE" && tar -tzf "$BACKUP_FILE" > /dev/null
  echo "Snapshot valid"

elif [[ "$BACKUP_FILE" == *.prism.jsonl ]]; then
  echo "Verifying portable format..."
  head -1 "$BACKUP_FILE" | jq -e '.format == "prism-portable-v1"' > /dev/null
  line_count=$(wc -l < "$BACKUP_FILE")
  echo "Portable format valid: $((line_count - 1)) documents"

elif [[ "$BACKUP_FILE" == *.enc ]]; then
  echo "Encrypted file detected"
  # Check magic bytes
  head -c 4 "$BACKUP_FILE" | grep -q "PENC" && echo "Encrypted format valid"
fi
```

### Test Restore (Dry Run)

```bash
# Restore to temporary collection
prism-cli collection restore -f backup.prism.jsonl --target-collection test-restore

# Verify document count
curl http://localhost:3080/collections/test-restore/stats

# Cleanup
prism-cli collection delete test-restore
```

---

## Cron Schedule Examples

```bash
# Daily snapshots at 2 AM
0 2 * * * /opt/scripts/daily-backup.sh >> /var/log/prism-backup.log 2>&1

# Weekly full backup on Sunday at 3 AM
0 3 * * 0 /opt/scripts/weekly-backup.sh >> /var/log/prism-backup.log 2>&1

# Encrypted cloud backup at 4 AM
0 4 * * * /opt/scripts/encrypted-cloud-backup.sh >> /var/log/prism-backup.log 2>&1
```

---

## Key Management Best Practices

1. **Never commit keys to git**
2. **Use secrets manager** (AWS Secrets Manager, Vault, etc.)
3. **Rotate keys periodically:**
   ```bash
   # Export with old key
   # Generate new key
   # Import (re-encrypts with new key)
   ```
4. **Store key separate from backup** (different S3 bucket, different cloud provider)
5. **Test restores regularly** with actual keys
