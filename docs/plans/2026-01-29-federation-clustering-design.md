# Prism Federation & Clustering Design

**Date:** 2026-01-29
**Status:** Draft
**Author:** mikalv
**Epic:** [#34](https://github.com/mikalv/prism/issues/34)

## Executive Summary

This document describes the architecture for distributed Prism deployments, enabling horizontal scaling, high availability, and multi-region deployments. The design uses a layered approach where **Federation** (simpler) and **Cluster** (full distributed) share common primitives.

## Motivation

| Priority | Goal | Description |
|----------|------|-------------|
| 1 | **Data scaling** | Datasets too large for single node (sharding) |
| 2 | **High availability** | Redundancy, failover, no single point of failure |
| 3 | **Multi-tenancy** | Independent instances searchable together |
| 4 | **Geo-distribution** | Data closer to users in different regions |

## Architecture

### Layered Design

```
┌─────────────────────────────────────────────┐
│  Layer 3: Cluster                           │
│  - Raft consensus                           │
│  - Automatic shard allocation               │
│  - Leader election                          │
│  - Automatic rebalancing                    │
├─────────────────────────────────────────────┤
│  Layer 2: Federation Primitives             │
│  - Query routing                            │
│  - Result merging                           │
│  - Node discovery                           │
│  - Health checks                            │
├─────────────────────────────────────────────┤
│  Layer 1: Single Node (current Prism)       │
│  - Indexing                                 │
│  - Search (text, vector, hybrid)            │
│  - Aggregations                             │
│  - Schema management                        │
└─────────────────────────────────────────────┘
```

- **Federation mode** = Layer 1 + Layer 2
- **Cluster mode** = Layer 1 + Layer 2 + Layer 3

This layered approach means:
- No code duplication between modes
- Clear separation of concerns
- Federation is literally "cluster without consensus"
- Progressive upgrade path from federation to cluster

### Deployment Modes

| Mode | Use Case | Complexity |
|------|----------|------------|
| **Single node** | Development, small datasets | Minimal |
| **Federation** | Read scaling, simple HA, multi-DC | Low |
| **Cluster** | Full distributed, auto-failover | High |

## Node Discovery

Pluggable discovery mechanism allowing users to choose based on their infrastructure.

### Backends

| Backend | Description | Best For |
|---------|-------------|----------|
| **Static** | List of nodes in config file | Development, fixed deployments |
| **DNS** | SRV record lookup | Kubernetes, cloud-native |
| **Gossip** | SWIM-style protocol | Dynamic environments |

### API

```rust
#[async_trait]
pub trait NodeDiscovery: Send + Sync {
    async fn get_nodes(&self) -> Result<Vec<NodeInfo>>;
    async fn watch(&self) -> impl Stream<Item = ClusterEvent>;
}

pub struct NodeInfo {
    pub id: String,
    pub address: SocketAddr,
    pub zone: Option<String>,
    pub rack: Option<String>,
    pub region: Option<String>,
    pub attributes: HashMap<String, String>,
}

pub enum ClusterEvent {
    NodeJoined(NodeInfo),
    NodeLeft(String),
    NodeUpdated(NodeInfo),
}
```

### Configuration

```toml
[federation.discovery]
backend = "static"  # static | dns | gossip

# Static
nodes = ["node1:3000", "node2:3000"]

# DNS
dns_name = "prism-headless.default.svc.cluster.local"
dns_refresh_interval_secs = 30

# Gossip
gossip_port = 7946
gossip_seeds = ["seed1:7946"]
```

## Data Distribution

Users decide distribution strategy per collection.

### Strategies

| Strategy | Description | Use Case |
|----------|-------------|----------|
| **Collection-per-node** | Entire collection on one node | Small collections |
| **Sharded** | Collection split across nodes | Large datasets |
| **Full replication** | All nodes have all data | Read-heavy, small data |

### Collection as Portable Unit

A collection contains all its components:

```
Collection
├── Tantivy index (text search)
├── Vector store (embeddings)
├── Graph backend (relationships)
└── Schema + config
```

### Export Formats

| Format | Contents | Use Case |
|--------|----------|----------|
| **Portable** | JSON docs + schema + vectors | Migration, debugging, cross-version |
| **Snapshot** | Binary files from all backends | Fast backup/restore, same version |

## Write Handling

### Federation Mode: Single Writer

Each shard has one owner node that accepts writes. Replicas are read-only.

```
Client → Shard owner (direct)
```

### Cluster Mode: Any-Node Routing

Any node accepts writes and routes to the shard leader (Raft).

```
Client → Any node → Shard leader (via Raft)
```

## Read Consistency

| Level | Description | Default |
|-------|-------------|---------|
| **Eventual** | Reads may hit stale replicas | ✅ Yes |
| **Read-your-writes** | Client sees own writes immediately | No |
| **Bounded staleness** | Max N seconds old | No |
| **Strong** | Always fresh, higher latency | No |

```toml
[federation.consistency]
read = "eventual"  # eventual | read-your-writes | bounded | strong
bounded_staleness_ms = 5000
```

## Query Execution

### Adaptive Merge Strategy

Different query types require different merge strategies:

| Query Type | Strategy | Reason |
|------------|----------|--------|
| Filter/exact match | Simple merge | Scores are binary |
| BM25 text search | Score normalization | IDF varies per shard |
| Vector search | Simple top-K | Cosine already normalized |
| Hybrid | RRF (Reciprocal Rank Fusion) | Combines different score types |
| Aggregations | Two-phase | Partial → merge |

```rust
pub enum MergeStrategy {
    Simple,
    ScoreNormalized,
    ReciprocalRankFusion,
    TwoPhase(Box<MergeStrategy>),
}
```

### Failure Handling

Default: Return partial results with warning.

```rust
pub struct SearchResponse {
    pub hits: Vec<Hit>,
    pub total: u64,
    pub shards: ShardStatus,
}

pub struct ShardStatus {
    pub total: u32,
    pub successful: u32,
    pub failed: u32,
    pub failures: Vec<ShardFailure>,
}
```

```toml
[federation.query]
allow_partial_results = true
partial_results_timeout_ms = 5000
min_successful_shards = 1
```

## Replication

### Replication Factor

| Mode | Default RF | Reason |
|------|------------|--------|
| Federation | 2 | Async replicas, survives 1 failure |
| Cluster (Raft) | 3 | Odd number for quorum |

```toml
[collection.replication]
factor = 2
min_replicas_for_write = 1
```

### Replica Placement

Zone-aware placement with load balancing.

**Constraints (prioritized):**
1. **Hard:** Never place two replicas of same shard in same failure domain
2. **Soft:** Balance load across nodes within valid placements

```rust
pub struct PlacementStrategy {
    pub spread_across: SpreadLevel,  // zone, rack, region
    pub balance_by: Vec<BalanceFactor>,
}

pub enum SpreadLevel {
    Zone,    // default
    Rack,
    Region,
    None,
}

pub enum BalanceFactor {
    ShardCount,
    DiskUsage,
    IndexSize,
}
```

### Node Topology

```toml
[node]
id = "node-1"
zone = "eu-west-1a"
rack = "rack-42"
region = "eu-west-1"

[node.attributes]
disk_type = "ssd"
storage_gb = 500
```

## Shard Rebalancing

### Triggers

| Trigger | Default | Description |
|---------|---------|-------------|
| Topology change | Always on | Node join/leave |
| Threshold-based | On (15%) | Imbalance exceeds threshold |
| Continuous | Off | Background optimization |

### Priority

1. Under-replicated shards (critical)
2. Unassigned shards
3. Imbalanced nodes (soft)

```toml
[cluster.rebalancing]
imbalance_threshold_percent = 15
max_concurrent_moves = 2
max_bytes_per_sec = "100MB"
continuous_optimization = false
pause_schedule = "0 9-17 * * MON-FRI"
```

## Inter-Node Protocol

### Decision: tarpc + bincode over QUIC

| Component | Choice | Reason |
|-----------|--------|--------|
| RPC framework | tarpc | Rust-native, simpler than gRPC |
| Serialization | bincode | Fast, native Rust types |
| Transport | QUIC | 0-RTT, multiplexing, connection migration |

**Why not gRPC?**
- All nodes are Rust (no cross-language needed)
- Protobuf compilation overhead
- Heavier runtime

### Service Definition

```rust
#[tarpc::service]
pub trait ClusterNode {
    async fn search(request: SearchRequest) -> SearchResponse;
    async fn replicate(docs: Vec<Document>) -> ReplicateResponse;
    async fn health() -> HealthStatus;
    async fn transfer_shard(shard_id: ShardId) -> ShardTransferStream;
    async fn forward_write(doc: Document) -> WriteResponse;
}
```

### Configuration

```toml
[cluster.transport]
transport = "quic"  # quic | tcp
bind_port = 7000

# QUIC settings
quic_idle_timeout_ms = 30000
quic_max_streams = 100

# TLS (required for QUIC)
cert_path = "/etc/prism/cert.pem"
key_path = "/etc/prism/key.pem"
```

## Distributed Embedding Cache

Two-tier architecture: L1 local + L2 distributed.

```
┌─────────────────────────────────────────┐
│           Node                          │
│  ┌─────────────────────────────────┐   │
│  │  L1: Local LRU cache            │   │  ← µs latency
│  │  (in-memory, limited size)      │   │
│  └──────────────┬──────────────────┘   │
│                 │ miss                  │
│  ┌──────────────▼──────────────────┐   │
│  │  L2: Distributed cache          │   │  ← ms latency
│  │  (Redis cluster / shared SQLite)│   │
│  └─────────────────────────────────┘   │
└─────────────────────────────────────────┘
```

```toml
[embedding.cache]
# L1 - local per node
l1_enabled = true
l1_max_entries = 10_000

# L2 - distributed
l2_backend = "redis"  # redis | sqlite | s3
l2_url = "redis://cache-cluster:6379"
l2_ttl_secs = 86400
```

## Vector Index (HNSW) Sharding

### Strategy: Independent HNSW per Shard

Each shard maintains its own HNSW graph. Recall is maintained via over-fetching.

**Why this approach:**
- Simple to implement
- Scales horizontally
- Works with existing architecture

### Shard Assignment: Vector Follows Document

Vector lives on same shard as its document.

**Why:**
- Efficient hybrid queries (text + vector on same node)
- Document is atomic unit
- Simpler backup/restore

### Over-fetching for Recall

```rust
pub struct VectorQueryConfig {
    pub k: usize,              // desired results
    pub shard_oversample: f32, // default 2.0-3.0
}
```

Research shows 2-3x over-fetch gives 95-99% of single-index recall.

### Segment-based Compaction

Immutable segments with background compaction (aligns with Tantivy).

```rust
pub struct VectorSegment {
    pub id: SegmentId,
    pub hnsw: HnswIndex,
    pub tombstones: RoaringBitmap,
    pub vector_count: u64,
    pub deleted_count: u64,
}
```

```toml
[vector.compaction]
min_segments = 3
delete_ratio_threshold = 0.2
max_concurrent_compactions = 1
```

## Health Checks & Membership

### Node States

```
alive → suspect → dead → removed
```

| Transition | Trigger |
|------------|---------|
| alive → suspect | Missed heartbeats |
| suspect → dead | Timeout without recovery |
| suspect → alive | Heartbeat received |
| dead → removed | Admin action or auto-cleanup |

```toml
[cluster.health]
heartbeat_interval_ms = 1000
failure_threshold = 3
suspect_timeout_ms = 5000
on_node_failure = "rebalance"
```

## Split-Brain Handling

### Federation Mode

Low risk - nodes are independent, eventual consistency. Partitioned nodes continue serving reads.

### Cluster Mode (Raft)

Quorum-based - minority partition becomes read-only.

```toml
[cluster.consistency]
min_nodes_for_write = "quorum"
partition_behavior = "read_only"
allow_stale_reads = true
```

## Schema Changes

### Change Types

| Type | Strategy |
|------|----------|
| Additive (new field) | Immediate propagation |
| Breaking (remove/change) | Coordinated migration |

### Versioned Schemas

```rust
pub struct SchemaVersion {
    pub version: u64,
    pub fields: Vec<FieldDef>,
    pub compatible_with: Vec<u64>,
}
```

## Observability

### Metrics (Prometheus)

```
# Query metrics
prism_query_duration_seconds{collection, query_type, shard}
prism_query_shard_latency_seconds{node, shard}

# Cluster metrics
prism_node_state{node_id, state}
prism_shard_status{shard, state}
prism_replication_lag_seconds{shard, replica}

# Cache metrics
prism_cache_hits_total{layer}
prism_embedding_requests_total{provider, status}
```

### Tracing (OpenTelemetry)

Distributed trace propagation via tarpc metadata.

### Logging (Structured JSON)

Correlation IDs for request tracing across nodes.

## Rolling Upgrades

### Protocol Versioning

```rust
pub struct NodeCapabilities {
    pub protocol_version: u32,
    pub min_supported_version: u32,
    pub features: HashSet<Feature>,
}
```

### Upgrade Process

1. Deploy new version to node
2. Node restarts, rejoins cluster
3. Health check
4. Wait for replication catch-up
5. Repeat for next node

## Implementation Phases

### Phase 1: Federation Foundation
- [ ] Node discovery (static, DNS)
- [ ] Query routing and result merging
- [ ] Collection export/import
- [ ] Basic health checks

### Phase 2: Replication
- [ ] Replica placement (zone-aware)
- [ ] Async replication
- [ ] Failure detection and recovery

### Phase 3: Full Cluster
- [ ] Raft consensus integration
- [ ] Automatic shard allocation
- [ ] Automatic rebalancing
- [ ] Leader election

### Phase 4: Operations
- [ ] Distributed embedding cache
- [ ] Observability (metrics, tracing)
- [ ] Rolling upgrades
- [ ] Schema versioning

## Related Issues

- [#26](https://github.com/mikalv/prism/issues/26) - Ranking improvements
- [#28](https://github.com/mikalv/prism/issues/28) - Federation query routing
- [#29](https://github.com/mikalv/prism/issues/29) - Node discovery
- [#30](https://github.com/mikalv/prism/issues/30) - Collection export/import
- [#31](https://github.com/mikalv/prism/issues/31) - Distributed embedding cache
- [#32](https://github.com/mikalv/prism/issues/32) - Inter-node protocol
- [#33](https://github.com/mikalv/prism/issues/33) - Replication and shard placement
- [#34](https://github.com/mikalv/prism/issues/34) - Federation/Clustering epic
- [#35](https://github.com/mikalv/prism/issues/35) - Schema changes
- [#36](https://github.com/mikalv/prism/issues/36) - Health checks
- [#37](https://github.com/mikalv/prism/issues/37) - Split-brain handling
- [#38](https://github.com/mikalv/prism/issues/38) - Observability
- [#39](https://github.com/mikalv/prism/issues/39) - Rolling upgrades
- [#40](https://github.com/mikalv/prism/issues/40) - HNSW sharding

## References

- [Elasticsearch Distributed Architecture](https://www.elastic.co/guide/en/elasticsearch/reference/current/scalability.html)
- [Raft Consensus Algorithm](https://raft.github.io/)
- [SWIM Failure Detector](https://www.cs.cornell.edu/projects/Quicksilver/public_pdfs/SWIM.pdf)
- [HNSW Algorithm](https://arxiv.org/abs/1603.09320)
- [tarpc - Rust RPC Framework](https://github.com/google/tarpc)
- [quinn - Rust QUIC Implementation](https://github.com/quinn-rs/quinn)
