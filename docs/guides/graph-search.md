# Graph Search

Prism includes a sharded graph backend for storing relationships between documents and performing traversal queries like BFS and shortest-path.

## Overview

The graph backend stores **nodes** (entities) and **edges** (relationships) in memory with optional persistence to disk or S3. Nodes are distributed across shards using the same hash function as the vector backend, so a document's graph node lives on the same shard as its vector embedding.

## Configuration

Add a `graph` section to your collection schema:

```yaml
collection: knowledge-base
backends:
  text:
    fields:
      - name: title
        type: text
        stored: true
        indexed: true
  graph:
    edges:
      - edge_type: references
        from_field: id
        to_field: referenced_id
      - edge_type: authored_by
        from_field: id
        to_field: author_id
    num_shards: 4
    scope: shard
```

### Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `edges` | required | List of edge type definitions |
| `num_shards` | `1` | Number of graph shards |
| `scope` | `shard` | Edge scope: `shard` or `collection` |

### Edge type definition

Each edge type specifies:

| Field | Description |
|-------|-------------|
| `edge_type` | Name of the relationship (e.g., "references", "authored_by") |
| `from_field` | Source field in the document |
| `to_field` | Target field in the document |

## Graph sharding

When `num_shards > 1`, graph nodes are distributed across shards using the same consistent hash function as vector embeddings. This co-location means:

- BFS traversals stay within a single shard (zero cross-shard overhead)
- Shortest-path queries are shard-local
- Edges can only connect nodes on the same shard (`scope: shard`)

Attempting to add an edge between nodes on different shards returns a `400 Bad Request` error.

With `num_shards: 1` (default), there are no restrictions on edges.

## API

### Add a node

```bash
curl -X POST http://localhost:3080/collections/knowledge-base/graph/nodes \
  -H 'Content-Type: application/json' \
  -d '{
    "id": "doc-1",
    "node_type": "article",
    "title": "Introduction to Search",
    "payload": {"tags": ["search", "tutorial"]}
  }'
```

**Response:** `201 Created`

### Get a node

```bash
curl http://localhost:3080/collections/knowledge-base/graph/nodes/doc-1
```

**Response:**

```json
{
  "id": "doc-1",
  "node_type": "article",
  "title": "Introduction to Search",
  "payload": {"tags": ["search", "tutorial"]}
}
```

### Delete a node

Removes the node and all its edges:

```bash
curl -X DELETE http://localhost:3080/collections/knowledge-base/graph/nodes/doc-1
```

**Response:** `204 No Content`

### Add an edge

```bash
curl -X POST http://localhost:3080/collections/knowledge-base/graph/edges \
  -H 'Content-Type: application/json' \
  -d '{
    "from": "doc-1",
    "to": "doc-2",
    "edge_type": "references",
    "weight": 1.0
  }'
```

**Response:** `201 Created`

The `weight` field defaults to `1.0` and is used by the shortest-path algorithm (Dijkstra).

### Get edges from a node

```bash
curl http://localhost:3080/collections/knowledge-base/graph/nodes/doc-1/edges
```

**Response:**

```json
[
  {"from": "doc-1", "to": "doc-2", "edge_type": "references", "weight": 1.0},
  {"from": "doc-1", "to": "doc-3", "edge_type": "references", "weight": 0.5}
]
```

### BFS traversal

Breadth-first search discovers all reachable nodes from a starting point, following edges of a specific type up to a maximum depth:

```bash
curl -X POST http://localhost:3080/collections/knowledge-base/graph/bfs \
  -H 'Content-Type: application/json' \
  -d '{
    "start": "doc-1",
    "edge_type": "references",
    "max_depth": 3
  }'
```

**Response:**

```json
{
  "nodes": ["doc-1", "doc-2", "doc-3", "doc-5"],
  "count": 4
}
```

### Shortest path

Find the shortest path between two nodes using Dijkstra's algorithm (weighted by edge `weight`):

```bash
curl -X POST http://localhost:3080/collections/knowledge-base/graph/shortest-path \
  -H 'Content-Type: application/json' \
  -d '{
    "start": "doc-1",
    "target": "doc-5",
    "edge_types": ["references"]
  }'
```

**Response:**

```json
{
  "path": ["doc-1", "doc-3", "doc-5"],
  "length": 2
}
```

If no path exists (or nodes are on different shards), `path` and `length` are `null`.

The `edge_types` field is optional — when omitted, all edge types are considered.

### Graph statistics

```bash
curl http://localhost:3080/collections/knowledge-base/graph/stats
```

**Response:**

```json
{
  "node_count": 42,
  "edge_count": 128
}
```

## Use cases

### Knowledge graphs

Build relationships between documents for RAG applications:

```yaml
backends:
  text:
    fields:
      - {name: title, type: text, stored: true, indexed: true}
      - {name: content, type: text, stored: true, indexed: true}
  vector:
    dimension: 384
    distance: cosine
  graph:
    edges:
      - {edge_type: cites, from_field: id, to_field: citation_id}
      - {edge_type: related_to, from_field: id, to_field: related_id}
    num_shards: 4
```

Combine vector similarity search with graph traversal: find semantically similar documents, then explore their citation graph.

### Dependency tracking

Track dependencies between software packages, services, or infrastructure components:

```yaml
backends:
  text:
    fields:
      - {name: name, type: string, stored: true, indexed: true}
      - {name: version, type: string, stored: true, indexed: true}
  graph:
    edges:
      - {edge_type: depends_on, from_field: id, to_field: dependency_id}
      - {edge_type: maintained_by, from_field: id, to_field: maintainer_id}
```

Use BFS to find all transitive dependencies, or shortest-path to trace dependency chains.

## Merge operations

### Merge shards within a collection

Consolidate all graph shards into shard 0. This enables full graph traversal across all data (BFS and shortest-path work across the entire graph instead of being shard-local):

```bash
prism collection graph-merge --name knowledge-base --schemas-dir schemas
```

Output:

```
Merging graph shards for 'knowledge-base'
  Before: 1000 nodes, 4500 edges across 4 shards
  After:  1000 nodes, 4500 edges in shard 0
  Time:   0.12s
```

After merging, the graph data persists in shard 0. The collection continues to work normally — new nodes added after the merge will still be routed by hash, but all existing data is in shard 0.

### Merge collections

Combine graph data from multiple collections into a new target collection:

```bash
prism collection merge \
  --source col_a --source col_b \
  --target combined \
  --schemas-dir schemas
```

This creates a new schema for the target collection (copied from the first source, with `num_shards: 1`), then imports all nodes and edges from each source. The source collections are not modified.

Requirements:

- At least two source collections
- All sources must have a graph backend configured
- Target collection must not already exist
