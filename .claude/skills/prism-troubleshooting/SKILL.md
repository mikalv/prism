---
name: prism-troubleshooting
description: Use when debugging Prism issues. Covers startup failures, slow searches, memory problems, encryption errors, cluster issues, and diagnostic commands.
---

# Prism Troubleshooting

Diagnostic guide for common Prism issues and their solutions.

## Quick Diagnostics

```bash
# Health check
curl http://localhost:3080/health

# Server info
curl http://localhost:3080/stats/server

# List collections
curl http://localhost:3080/admin/collections

# Collection stats
curl http://localhost:3080/collections/myindex/stats

# Cache stats
curl http://localhost:3080/stats/cache

# Metrics (if enabled)
curl http://localhost:3080/metrics
```

---

## Server Won't Start

### Check Config Syntax

```bash
# Validate config
prism-server -c prism.toml --check

# Verbose startup
RUST_LOG=debug prism-server -c prism.toml
```

### Common Causes

| Error | Cause | Fix |
|-------|-------|-----|
| `Address already in use` | Port conflict | Check `lsof -i :3080`, change port |
| `TOML parse error` | Invalid config | Check TOML syntax |
| `Schema validation failed` | Bad schema YAML | Check schema files |
| `Permission denied` | File permissions | Check data_dir permissions |
| `No such file or directory` | Missing directory | Create data_dir |

### Port Conflict

```bash
# Find what's using port 3080
lsof -i :3080
# or
ss -tlnp | grep 3080

# Kill process or change port
prism-server -c prism.toml -p 3081
```

### Permission Issues

```bash
# Check data directory permissions
ls -la /var/lib/prism

# Fix permissions
chown -R prism:prism /var/lib/prism
chmod 755 /var/lib/prism
```

---

## Slow Searches

### 1. Check Segment Count

```bash
prism-cli collection inspect -n myindex -v
```

**Problem:** Many small segments = slow searches

**Fix:** Merge segments
```bash
prism-cli index optimize -c myindex
```

### 2. Check Cache Hit Rate

```bash
curl http://localhost:3080/stats/cache
```

```json
{
  "hit_rate": 0.45,  // < 0.8 is concerning
  "total_entries": 1000,
  "total_bytes": 5000000
}
```

**Problem:** Low hit rate = cold cache or no cache configured

**Fix:**
- Wait for cache warm-up
- Increase cache size in config
- Check cache backend connectivity

### 3. Benchmark Specific Queries

```bash
echo "slow query here" > queries.txt
prism-cli benchmark -c myindex -q queries.txt -r 50
```

Output shows p50/p95/p99 latencies.

### 4. Check Query Complexity

**Slow patterns:**
- Wildcard at start: `*term`
- Very long queries
- High limit values
- Large aggregations

**Fix:** Simplify queries, reduce limits

### 5. Check Vector Dimension

High dimensions = more memory and slower:
- 384d = ~2KB/doc, fast
- 768d = ~4KB/doc, moderate
- 1536d = ~8KB/doc, slower

---

## Out of Memory

### Symptoms

- Server killed by OOM killer
- `dmesg | grep -i oom` shows prism
- Slow performance before crash

### Diagnosis

```bash
# Check current memory
ps aux | grep prism-server

# Check limits
cat /proc/$(pgrep prism-server)/limits

# Check collection sizes
for c in $(curl -s http://localhost:3080/admin/collections | jq -r '.[]'); do
  echo "$c: $(curl -s http://localhost:3080/collections/$c/stats | jq -r '.size_bytes')"
done
```

### Fixes

1. **Reduce batch sizes:**
   ```bash
   prism-cli document import -c myindex -f data.jsonl --batch-size 100
   ```

2. **Enable compression:**
   ```toml
   [storage]
   backend = "compressed"
   [storage.compressed]
   algorithm = "zstd"
   ```

3. **Increase memory limits:**
   ```yaml
   # Kubernetes
   resources:
     limits:
       memory: "4Gi"
   ```

4. **Reduce vector dimensions** (if possible)

5. **Use cached storage with size limit:**
   ```toml
   [unified_storage.cache]
   l1_max_size_gb = 10
   ```

---

## Encryption Issues

### Decryption Failed

```
Error: Decryption failed (wrong key or corrupted data)
```

**Causes:**
1. Wrong key
2. Truncated/corrupted file
3. Wrong file (not encrypted)

**Debug:**
```bash
# Check file format
head -c 4 backup.enc
# Should output: PENC

# Verify key length
echo -n "$KEY" | wc -c
# Should be 64 (hex chars)
```

### Key Too Short

```
Error: Key must be 32 bytes, got 16
```

**Fix:** Key must be 64 hex characters (256 bits)
```bash
# Generate valid key
curl -X POST http://localhost:3080/_admin/encryption/generate-key
```

### Cannot Read Encrypted Storage

Server won't start with encrypted storage:

```bash
# Check environment variable is set
echo $PRISM_ENCRYPTION_KEY | wc -c
# Should be 64

# Try with explicit key source
RUST_LOG=debug prism-server -c prism.toml
```

---

## Schema Issues

### Schema Validation Failed

```bash
# Lint all schemas
curl http://localhost:3080/admin/lint-schemas
```

Response shows which schemas have errors:
```json
{
  "valid": ["articles", "products"],
  "errors": {
    "broken": "Missing required field: backends"
  }
}
```

### Schema Conflict on Import

```
Error: Schema mismatch for collection 'myindex'
```

**Fix:**
```bash
# Delete existing collection
prism-cli collection delete myindex

# Then restore
prism-cli collection restore -f backup.prism.jsonl
```

### Field Type Mismatch

```
Error: Cannot index string value for i64 field 'count'
```

**Fix:** Ensure document field types match schema

---

## Cluster Issues

### Node Not Joining

```bash
# Check cluster transport
RUST_LOG=prism::cluster=debug prism-server

# Verify connectivity
nc -zv other-node 7000

# Check certificates (QUIC)
openssl x509 -in cluster-cert.pem -text -noout
```

### Split-Brain Detected

```
Warning: Cluster partition detected
```

**Actions:**
1. Check network connectivity between nodes
2. Check if quorum exists
3. If no quorum, minority partition is read-only

```bash
# Check node states
curl http://localhost:3080/cluster/nodes
```

### Replication Lag

```bash
# Check replication status
curl http://localhost:3080/cluster/replication
```

**High lag causes:**
- Network issues
- Slow disk on replica
- High write throughput

---

## Indexing Issues

### Documents Not Appearing

```bash
# Check if commit happened
curl http://localhost:3080/collections/myindex/stats

# Force commit (if using CLI)
prism-cli index commit -c myindex
```

**Check:** Documents are committed after `commit_interval_secs` or when batch completes.

### Indexing Too Slow

1. **Increase batch size:**
   ```bash
   prism-cli document import -c myindex -f data.jsonl --batch-size 1000
   ```

2. **Disable embedding generation temporarily:**
   - Index without vectors
   - Re-index with embeddings later

3. **Check embedding provider latency:**
   ```bash
   curl http://localhost:3080/stats/embedding
   ```

---

## Search Issues

### No Results

```bash
# Check collection has documents
curl http://localhost:3080/collections/myindex/stats

# Try simple query
curl -X POST http://localhost:3080/collections/myindex/search \
  -d '{"query": "*", "limit": 1}'

# Check field is indexed
curl http://localhost:3080/collections/myindex/schema | jq '.backends.text.fields'
```

### Wrong Results (Relevance)

1. **Check merge strategy:**
   ```json
   {"query": "test", "merge_strategy": "rrf"}
   ```

2. **Adjust weights:**
   ```json
   {"query": "exact keyword", "text_weight": 0.8, "vector_weight": 0.2}
   ```

3. **Check BM25 parameters in schema:**
   ```yaml
   backends:
     text:
       bm25_k1: 1.2
       bm25_b: 0.75
   ```

### Highlighting Not Working

Check field is stored:
```yaml
fields:
  - name: content
    type: text
    stored: true   # Required for highlighting
    indexed: true
```

---

## Log Analysis

### Enable Debug Logging

```bash
RUST_LOG=debug prism-server
# or
RUST_LOG="info,prism=debug,prism::search=trace" prism-server
```

### Common Log Patterns

| Pattern | Meaning |
|---------|---------|
| `Query completed in Xms` | Normal search |
| `Slow query: Xms` | Performance issue |
| `Connection refused` | Upstream service down |
| `Rate limited` | Embedding provider throttling |
| `Segment merge started` | Background optimization |

### JSON Log Parsing

```bash
# Search for errors
cat /var/log/prism.log | jq 'select(.level == "ERROR")'

# Slow queries (> 100ms)
cat /var/log/prism.log | jq 'select(.duration_ms > 100)'

# Specific collection
cat /var/log/prism.log | jq 'select(.collection == "myindex")'
```

---

## Metrics-Based Debugging

### Enable Prometheus Metrics

```toml
[observability]
metrics_enabled = true
```

### Key Metrics to Check

```promql
# Search latency p99
histogram_quantile(0.99, sum(rate(prism_search_duration_seconds_bucket[5m])) by (le))

# Error rate
sum(rate(prism_search_total{status="error"}[5m])) / sum(rate(prism_search_total[5m]))

# Indexing rate
sum(rate(prism_index_documents_total[5m]))

# Cache efficiency
sum(rate(prism_embedding_cache_hits_total[5m])) /
(sum(rate(prism_embedding_cache_hits_total[5m])) + sum(rate(prism_embedding_cache_misses_total[5m])))
```

---

## Support Information

When reporting issues, gather:

```bash
# Version
prism-server --version

# Config (redact secrets)
cat prism.toml | grep -v key

# Schema
cat /var/lib/prism/schemas/*.yaml

# Stats
curl http://localhost:3080/stats/server
curl http://localhost:3080/stats/cache

# Recent logs
tail -100 /var/log/prism.log

# System info
uname -a
free -h
df -h /var/lib/prism
```
