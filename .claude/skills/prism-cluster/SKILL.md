---
name: prism-cluster
description: Use when configuring Prism clustering, federation, node discovery, replication, or multi-node deployments. Covers static/DNS/gossip discovery, QUIC transport, zone-aware placement, and split-brain handling.
---

# Prism Clustering & Federation

Guide for distributed Prism deployments with horizontal scaling and high availability.

## Deployment Modes

| Mode | Nodes | Consensus | Use Case |
|------|-------|-----------|----------|
| Single | 1 | None | Development, small datasets |
| Federation | 2+ | None | Read scaling, simple HA, multi-DC |
| Cluster | 3+ | Raft | Full distributed, auto-failover |

**Architecture layers:**
```
Layer 3: Cluster  (Raft consensus, auto-rebalancing, leader election)
Layer 2: Federation  (Query routing, result merging, health checks)
Layer 1: Single Node  (Indexing, search, aggregations)
```

---

## Node Discovery

### Static (Development/Fixed Deployments)

```toml
[federation.discovery]
backend = "static"
nodes = ["node1:3000", "node2:3000", "node3:3000"]
```

### DNS-Based (Kubernetes)

```toml
[federation.discovery]
backend = "dns"
dns_name = "prism-headless.default.svc.cluster.local"
dns_refresh_interval_secs = 30
```

**Kubernetes headless service:**
```yaml
apiVersion: v1
kind: Service
metadata:
  name: prism-headless
spec:
  clusterIP: None
  selector:
    app: prism
  ports:
    - port: 7000
      name: cluster
```

### Gossip (Dynamic Environments)

```toml
[federation.discovery]
backend = "gossip"
gossip_port = 7946
gossip_seeds = ["seed1:7946", "seed2:7946"]
```

---

## Inter-Node Transport

### QUIC (Recommended)

```toml
[cluster.transport]
transport = "quic"
bind_port = 7000
quic_idle_timeout_ms = 30000
quic_max_streams = 100

# TLS required for QUIC
cert_path = "/etc/prism/cluster-cert.pem"
key_path = "/etc/prism/cluster-key.pem"
```

### TCP (Alternative)

```toml
[cluster.transport]
transport = "tcp"
bind_port = 7000
```

**Generate cluster certificates:**
```bash
# Self-signed for internal cluster
openssl req -x509 -newkey rsa:4096 -keyout cluster-key.pem -out cluster-cert.pem \
  -days 365 -nodes -subj "/CN=prism-cluster"
```

---

## Node Identity

```toml
[node]
id = "node-1"              # Unique identifier
zone = "eu-west-1a"        # Availability zone
rack = "rack-42"           # Physical rack (optional)
region = "eu-west-1"       # Cloud region (optional)

[node.attributes]
disk_type = "ssd"
storage_gb = 500
```

---

## Replication

### Replication Factor

```toml
[collection.replication]
factor = 2                    # Number of replicas
min_replicas_for_write = 1    # Minimum for write success
```

| Mode | Recommended RF | Reason |
|------|----------------|--------|
| Federation | 2 | Survives 1 node failure |
| Cluster (Raft) | 3 | Odd number for quorum |

### Zone-Aware Placement

Replicas automatically spread across zones:

```toml
[cluster.placement]
spread_across = "zone"  # zone | rack | region | none
```

**Constraint:** Never two replicas of same shard in same zone.

---

## Read Consistency

```toml
[federation.consistency]
read = "eventual"              # Default: fastest
# read = "read-your-writes"    # Client sees own writes
# read = "bounded"             # Max staleness
# read = "strong"              # Always fresh

bounded_staleness_ms = 5000    # For bounded mode
```

| Level | Latency | Freshness |
|-------|---------|-----------|
| eventual | Lowest | May be stale |
| read-your-writes | Low | Own writes visible |
| bounded | Medium | Max N seconds old |
| strong | Highest | Always current |

---

## Query Execution

### Partial Results on Failure

```toml
[federation.query]
allow_partial_results = true
partial_results_timeout_ms = 5000
min_successful_shards = 1
```

When some nodes fail, search returns partial results with warning:
```json
{
  "results": [...],
  "shards": {
    "total": 3,
    "successful": 2,
    "failed": 1,
    "failures": [{"node": "node-3", "reason": "timeout"}]
  }
}
```

---

## Health Checks

```toml
[cluster.health]
heartbeat_interval_ms = 1000
failure_threshold = 3          # Missed heartbeats before suspect
suspect_timeout_ms = 5000      # Suspect → dead timeout
on_node_failure = "rebalance"  # rebalance | none
```

**Node states:**
```
alive → suspect → dead → removed
```

| Transition | Trigger |
|------------|---------|
| alive → suspect | Missed heartbeats |
| suspect → dead | Timeout without recovery |
| suspect → alive | Heartbeat received |
| dead → removed | Admin action or auto-cleanup |

---

## Rebalancing

```toml
[cluster.rebalancing]
imbalance_threshold_percent = 15
max_concurrent_moves = 2
max_bytes_per_sec = "100MB"
continuous_optimization = false
pause_schedule = "0 9-17 * * MON-FRI"  # Pause during business hours
```

**Rebalancing priority:**
1. Under-replicated shards (critical)
2. Unassigned shards
3. Imbalanced nodes (soft)

---

## Split-Brain Handling

### Federation Mode

Low risk - nodes are independent, eventual consistency. Partitioned nodes continue serving reads.

### Cluster Mode (Raft)

Minority partition becomes read-only:

```toml
[cluster.consistency]
min_nodes_for_write = "quorum"   # Majority required
partition_behavior = "read_only"
allow_stale_reads = true
```

---

## Distributed Embedding Cache

Two-tier cache: L1 local (fast) + L2 distributed (shared):

```toml
[embedding.cache]
# L1 - local per node
l1_enabled = true
l1_max_entries = 10000

# L2 - distributed
l2_backend = "redis"  # redis | sqlite | s3
l2_url = "redis://cache-cluster:6379"
l2_ttl_secs = 86400
```

---

## Rolling Upgrades

1. Deploy new version to one node
2. Node restarts, rejoins cluster
3. Wait for health check pass
4. Wait for replication catch-up
5. Repeat for next node

**Protocol versioning:**
```toml
[cluster]
protocol_version = 1
min_supported_version = 1
```

---

## Cluster Metrics

| Metric | Description |
|--------|-------------|
| `prism_node_state{node_id, state}` | Node health state |
| `prism_shard_status{shard, state}` | Shard assignment |
| `prism_replication_lag_seconds{shard, replica}` | Replication delay |
| `prism_query_shard_latency_seconds{node, shard}` | Per-shard latency |

**Prometheus alerting:**
```yaml
- alert: PrismNodeDown
  expr: prism_node_state{state="dead"} == 1
  for: 1m
  labels:
    severity: critical

- alert: PrismReplicationLag
  expr: prism_replication_lag_seconds > 30
  for: 5m
  labels:
    severity: warning
```

---

## Full Cluster Config Example

```toml
# Node identity
[node]
id = "prism-node-1"
zone = "eu-west-1a"

# Discovery
[federation.discovery]
backend = "dns"
dns_name = "prism-headless.prism.svc.cluster.local"

# Transport
[cluster.transport]
transport = "quic"
bind_port = 7000
cert_path = "/etc/prism/certs/cluster.pem"
key_path = "/etc/prism/certs/cluster-key.pem"

# Replication
[collection.replication]
factor = 3
min_replicas_for_write = 2

# Consistency
[federation.consistency]
read = "eventual"

# Health
[cluster.health]
heartbeat_interval_ms = 1000
failure_threshold = 3

# Rebalancing
[cluster.rebalancing]
imbalance_threshold_percent = 15
max_concurrent_moves = 2

# Distributed cache
[embedding.cache]
l1_enabled = true
l1_max_entries = 10000
l2_backend = "redis"
l2_url = "redis://redis:6379"
```

---

## Kubernetes StatefulSet

```yaml
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: prism
spec:
  serviceName: prism-headless
  replicas: 3
  selector:
    matchLabels:
      app: prism
  template:
    metadata:
      labels:
        app: prism
    spec:
      containers:
        - name: prism
          image: prism:latest
          ports:
            - containerPort: 3080
              name: http
            - containerPort: 7000
              name: cluster
          env:
            - name: NODE_ID
              valueFrom:
                fieldRef:
                  fieldPath: metadata.name
            - name: NODE_ZONE
              valueFrom:
                fieldRef:
                  fieldPath: metadata.labels['topology.kubernetes.io/zone']
          volumeMounts:
            - name: data
              mountPath: /data
            - name: certs
              mountPath: /etc/prism/certs
  volumeClaimTemplates:
    - metadata:
        name: data
      spec:
        accessModes: ["ReadWriteOnce"]
        resources:
          requests:
            storage: 100Gi
```
