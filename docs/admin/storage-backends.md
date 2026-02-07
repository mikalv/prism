# Storage Backends

Prism supports multiple storage backends for flexibility in deployment scenarios.

## Backend Types

| Backend | Use Case |
|---------|----------|
| `local` | Single-node, local disk storage (default) |
| `s3` | Cloud storage, shared access, durability |
| `cached` | Hybrid: local cache (L1) + S3 backend (L2) |
| `compressed` | Reduce storage with LZ4/Zstd compression |
| `encrypted` | AES-256-GCM encryption at rest |

## Local Storage

Default backend for single-node deployments.

```toml
[unified_storage]
backend = "local"
data_dir = "~/.prism/data"
buffer_dir = "~/.prism/buffer"
```

| Option | Default | Description |
|--------|---------|-------------|
| `data_dir` | `~/.prism/data` | Directory for index data |
| `buffer_dir` | `~/.prism/buffer` | Directory for write buffers |

## S3 Storage

Store indexes in Amazon S3 or S3-compatible storage.

```toml
[unified_storage]
backend = "s3"

[unified_storage.s3]
bucket = "my-prism-bucket"
region = "us-east-1"
prefix = "collections/"
```

| Option | Default | Description |
|--------|---------|-------------|
| `bucket` | required | S3 bucket name |
| `region` | `us-east-1` | AWS region |
| `prefix` | `""` | Key prefix for all objects |
| `endpoint` | — | Custom endpoint URL (for MinIO, etc.) |
| `force_path_style` | `false` | Use path-style URLs |
| `access_key_id` | — | AWS access key (or use IAM/env) |
| `secret_access_key` | — | AWS secret key |

### AWS Credentials

Credentials are resolved in order:

1. Explicit config (`access_key_id`, `secret_access_key`)
2. Environment variables (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`)
3. AWS credentials file (`~/.aws/credentials`)
4. IAM instance role (EC2, ECS, Lambda)

### IAM Policy

Minimum required permissions:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:PutObject",
        "s3:DeleteObject",
        "s3:ListBucket"
      ],
      "Resource": [
        "arn:aws:s3:::my-prism-bucket",
        "arn:aws:s3:::my-prism-bucket/*"
      ]
    }
  ]
}
```

## MinIO (S3-Compatible)

Use MinIO for self-hosted S3-compatible storage.

```toml
[unified_storage]
backend = "s3"

[unified_storage.s3]
bucket = "prism-data"
region = "us-east-1"
endpoint = "http://localhost:9000"
force_path_style = true
access_key_id = "minioadmin"
secret_access_key = "minioadmin"
```

### Docker Compose with MinIO

```yaml
version: '3.8'
services:
  minio:
    image: minio/minio
    ports:
      - "9000:9000"
      - "9001:9001"
    environment:
      MINIO_ROOT_USER: minioadmin
      MINIO_ROOT_PASSWORD: minioadmin
    command: server /data --console-address ":9001"
    volumes:
      - minio-data:/data

  prism:
    build: .
    depends_on:
      - minio
    environment:
      - RUST_LOG=info
    volumes:
      - ./prism.toml:/etc/prism/prism.toml

volumes:
  minio-data:
```

Create the bucket on first run:

```bash
# Using MinIO client
mc alias set local http://localhost:9000 minioadmin minioadmin
mc mb local/prism-data
```

## Cached Storage

Two-tier caching for performance with durability.

```toml
[unified_storage]
backend = "cached"

# L2 backend (S3)
[unified_storage.s3]
bucket = "my-prism-bucket"
region = "us-east-1"

# L1 cache (local)
[unified_storage.cache]
l1_path = "~/.prism/cache"
l1_max_size_gb = 10
write_through = true
```

| Option | Default | Description |
|--------|---------|-------------|
| `l1_path` | required | Local cache directory |
| `l1_max_size_gb` | `10` | Maximum cache size in GB |
| `write_through` | `true` | Write to S3 immediately |

### How Caching Works

```
┌─────────────────────────────────────────────────┐
│                   Read Path                      │
│  Request → L1 Cache → (hit) → Return            │
│              ↓                                   │
│           (miss)                                 │
│              ↓                                   │
│           S3 (L2) → Populate L1 → Return        │
└─────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────┐
│                  Write Path                      │
│  (write_through = true)                         │
│  Write → L1 Cache + S3 (parallel) → Confirm    │
│                                                  │
│  (write_through = false)                        │
│  Write → L1 Cache → Confirm → Async to S3      │
└─────────────────────────────────────────────────┘
```

### Cache Eviction

When L1 cache exceeds `l1_max_size_gb`, oldest files are evicted using LRU policy.

## Per-Collection Storage

Override storage backend per collection in the schema:

```yaml
collection: large-archive
backends:
  text:
    fields:
      - name: content
        type: text

# This collection uses S3 directly
storage:
  backend: s3
  s3:
    bucket: archive-bucket
    prefix: "archive/"
```

## Migration Between Backends

### Local to S3

```bash
# Export all documents
prism-cli document export -c myindex -o backup.jsonl

# Update config to S3 backend
# Restart server

# Re-import documents
prism-cli document import -c myindex -f backup.jsonl
```

### Using AWS CLI

```bash
# Sync local data to S3
aws s3 sync ~/.prism/data/ s3://my-bucket/prism/

# Update config and restart
```

## Compressed Storage

Reduce storage requirements with transparent compression.

```toml
[storage]
backend = "compressed"

[storage.compressed]
algorithm = "zstd"  # or "lz4", "none"
min_size = 1024     # Only compress files > 1KB

[storage.compressed.inner]
backend = "local"
data_dir = "~/.prism/data"
```

| Option | Default | Description |
|--------|---------|-------------|
| `algorithm` | `zstd` | Compression: `lz4` (fast), `zstd` (balanced), `zstd:9` (high), `none` |
| `min_size` | `1024` | Skip compression for files smaller than this (bytes) |

### Algorithm Comparison

| Algorithm | Speed | Ratio | Use Case |
|-----------|-------|-------|----------|
| `lz4` | Fastest | ~2x | Real-time, high throughput |
| `zstd` | Fast | ~3x | Balanced (default) |
| `zstd:9` | Slower | ~4x | Archival, cold storage |

## Encrypted Storage

AES-256-GCM encryption at rest for sensitive data.

```toml
[storage]
backend = "encrypted"

[storage.encrypted]
key_source = "env"
key_env_var = "PRISM_ENCRYPTION_KEY"

[storage.encrypted.inner]
backend = "local"
data_dir = "~/.prism/data"
```

| Option | Description |
|--------|-------------|
| `key_source` | `env` (environment variable), `hex` (inline), or `base64` |
| `key_env_var` | Environment variable name (when `key_source = "env"`) |
| `key` | Hex or base64 encoded key (when inline) |
| `key_id` | Identifier for logging (not the key itself) |

### Key Sources

**Environment Variable (Recommended):**

```toml
[storage.encrypted]
key_source = "env"
key_env_var = "PRISM_ENCRYPTION_KEY"
```

```bash
export PRISM_ENCRYPTION_KEY="a1b2c3...64 hex chars"
```

**Hex Key (Development):**

```toml
[storage.encrypted]
key_source = "hex"
key = "a1b2c3d4e5f6..."
key_id = "dev-key"
```

**Base64 Key:**

```toml
[storage.encrypted]
key_source = "base64"
key = "oWLDnNT1..."
key_id = "prod-key"
```

### Generate a Key

```bash
# Via API (requires running server)
curl -X POST http://localhost:3080/_admin/encryption/generate-key

# Via OpenSSL
openssl rand -hex 32
```

### Layered Encryption + Compression

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
region = "us-east-1"
```

Data flow: `Data → Encrypt → Compress → S3`

## See Also

- [Encryption Guide](../guides/encryption.md) — Detailed encryption documentation
- [Export & Import](../guides/export-import.md) — Backup and migration
- [Configuration](configuration.md) — Full config reference
- [Deployment](deployment.md) — Production setup
