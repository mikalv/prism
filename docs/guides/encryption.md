# Encryption Guide

Prism supports AES-256-GCM encryption for data at rest, enabling secure storage of sensitive index data.

## Use Cases

- **Compliance**: Healthcare (HIPAA), finance (PCI-DSS), GDPR
- **Multi-tenant**: Isolate customer data with per-tenant keys
- **Disaster Recovery**: Safely offload data to untrusted cloud storage during emergencies
- **Backup Security**: Encrypted backups to external systems

## Quick Start

### Generate an Encryption Key

```bash
# Via API
curl -X POST http://localhost:3080/_admin/encryption/generate-key
```

```json
{
  "key": "a1b2c3d4...64 hex characters",
  "key_bytes": 32,
  "algorithm": "AES-256-GCM"
}
```

**Important:** Save this key securely (secrets manager, HSM, or encrypted vault). The key is never stored by Prism.

### Encrypted Export (API)

Export a collection with encryption, key provided at runtime:

```bash
curl -X POST http://localhost:3080/_admin/export/encrypted \
  -H "Content-Type: application/json" \
  -d '{
    "collection": "sensitive-data",
    "key": "a1b2c3d4e5f6...64 hex chars",
    "output_path": "/backup/sensitive-data.enc"
  }'
```

**Response:**

```json
{
  "success": true,
  "collection": "sensitive-data",
  "output_path": "/backup/sensitive-data.enc",
  "size_bytes": 104857600
}
```

### Encrypted Import (API)

Restore from an encrypted backup:

```bash
curl -X POST http://localhost:3080/_admin/import/encrypted \
  -H "Content-Type: application/json" \
  -d '{
    "input_path": "/backup/sensitive-data.enc",
    "key": "a1b2c3d4e5f6...64 hex chars",
    "target_collection": "sensitive-data-restored"
  }'
```

---

## Storage-Level Encryption

For always-on encryption at the storage layer, configure it in `prism.toml`:

### Via Environment Variable (Recommended)

```toml
[storage]
backend = "encrypted"

[storage.encrypted]
key_source = "env"
key_env_var = "PRISM_ENCRYPTION_KEY"

[storage.encrypted.inner]
backend = "local"
data_dir = "/var/lib/prism/data"
```

Set the key:

```bash
export PRISM_ENCRYPTION_KEY="a1b2c3d4e5f6..."  # 64 hex chars
prism-server --config prism.toml
```

### Via Hex Key (Development Only)

```toml
[storage]
backend = "encrypted"

[storage.encrypted]
key_source = "hex"
key = "a1b2c3d4e5f6789..."  # 64 hex characters
key_id = "dev-key-2026"

[storage.encrypted.inner]
backend = "local"
data_dir = "/var/lib/prism/data"
```

### With S3 Backend

Encrypt data before storing in S3:

```toml
[storage]
backend = "encrypted"

[storage.encrypted]
key_source = "env"
key_env_var = "PRISM_ENCRYPTION_KEY"

[storage.encrypted.inner]
backend = "s3"

[storage.encrypted.inner.s3]
bucket = "my-prism-bucket"
region = "us-east-1"
```

### Layered Configuration

Combine encryption with compression:

```toml
[storage]
backend = "encrypted"

[storage.encrypted]
key_source = "env"
key_env_var = "PRISM_ENCRYPTION_KEY"

[storage.encrypted.inner]
backend = "compressed"
algorithm = "zstd"

[storage.encrypted.inner.inner]
backend = "s3"

[storage.encrypted.inner.inner.s3]
bucket = "secure-bucket"
```

Data flow: `Write → Encrypt → Compress → S3`

---

## Disaster Recovery Workflow

When disk fills up unexpectedly and you need to offload data quickly:

### 1. Generate or Retrieve Key

```bash
# Generate new key
curl -X POST http://localhost:3080/_admin/encryption/generate-key > /secure/backup-key.json

# Or use existing key from secrets manager
```

### 2. Export Critical Collections

```bash
# Export each collection that needs to be moved
for collection in logs metrics events; do
  curl -X POST http://localhost:3080/_admin/export/encrypted \
    -H "Content-Type: application/json" \
    -d "{
      \"collection\": \"$collection\",
      \"key\": \"$(cat /secure/backup-key.json | jq -r .key)\",
      \"output_path\": \"/mnt/emergency-storage/$collection.enc\"
    }"
done
```

### 3. Free Disk Space

```bash
# Delete exported collections locally
prism-cli collection delete logs metrics events
```

### 4. Restore When Ready

```bash
# Mount cloud storage, restore collections
for collection in logs metrics events; do
  curl -X POST http://localhost:3080/_admin/import/encrypted \
    -H "Content-Type: application/json" \
    -d "{
      \"input_path\": \"/mnt/emergency-storage/$collection.enc\",
      \"key\": \"$(cat /secure/backup-key.json | jq -r .key)\"
    }"
done
```

---

## Security Considerations

### Key Management

| Approach | Security | Convenience |
|----------|----------|-------------|
| Environment variable | Good | Easy rotation |
| Secrets manager (AWS/GCP/Vault) | Best | Automated |
| Config file | Poor | Not recommended |

### Key Rotation

1. Export all data with old key
2. Update key in secrets manager
3. Restart Prism with new key
4. Import data (re-encrypts with new key)

### Audit Logging

Enable audit logging to track encryption operations:

```toml
[security.audit]
enabled = true
index_to_collection = true
```

### TLS in Production

Always use HTTPS when sending keys via API:

```toml
[server.tls]
enabled = true
cert_file = "/etc/prism/cert.pem"
key_file = "/etc/prism/key.pem"
```

---

## File Format

Encrypted files use this structure:

```
┌─────────────────────────────────────────────────┐
│ Magic: 4 bytes "PENC"                           │
│ Version: 1 byte (0x01)                          │
│ Nonce: 12 bytes (random per file)               │
│ Ciphertext: variable (AES-256-GCM encrypted)    │
│ Auth Tag: 16 bytes (integrity verification)     │
└─────────────────────────────────────────────────┘
```

- **Algorithm**: AES-256-GCM (authenticated encryption)
- **Nonce**: Unique per write (cryptographically random)
- **Key size**: 256 bits (32 bytes, 64 hex characters)

---

## API Reference

### POST /_admin/encryption/generate-key

Generate a new AES-256 encryption key.

**Response:**

```json
{
  "key": "64 hex characters",
  "key_bytes": 32,
  "algorithm": "AES-256-GCM"
}
```

### POST /_admin/export/encrypted

Export a collection with encryption.

**Request:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `collection` | string | Yes | Collection name |
| `key` | string | Yes | Hex-encoded 256-bit key |
| `output_path` | string | Yes | Output file path |

**Response:**

```json
{
  "success": true,
  "collection": "name",
  "output_path": "/path/to/file.enc",
  "size_bytes": 12345
}
```

**Errors:**

- `400` — Invalid key format
- `404` — Collection not found
- `500` — Export failed

### POST /_admin/import/encrypted

Import a collection from encrypted backup.

**Request:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `input_path` | string | Yes | Encrypted file path |
| `key` | string | Yes | Hex-encoded 256-bit key |
| `target_collection` | string | No | Override collection name |

**Response:**

```json
{
  "success": true,
  "collection": "restored-name",
  "files_extracted": 42,
  "bytes_extracted": 12345678
}
```

**Errors:**

- `400` — Invalid key or decryption failed (wrong key)
- `500` — Import failed

---

## Troubleshooting

### Decryption Failed

```
Error: Decryption failed (wrong key or corrupted data)
```

- Verify the key matches the one used during export
- Check file integrity (not truncated or modified)

### Key Too Short

```
Error: Key must be 32 bytes, got 16
```

- Key must be 64 hex characters (256 bits)
- Use `/_admin/encryption/generate-key` to create valid key

### Permission Denied

Ensure Prism has write access to the output directory and read access to input files.

---

## See Also

- [Storage Backends](../admin/storage-backends.md) — Backend configuration
- [Security](../admin/security.md) — Authentication and authorization
- [API Reference](api-reference.md) — Full API documentation
