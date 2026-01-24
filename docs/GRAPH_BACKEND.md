Graph Backend (Phase 3) — engraph-core

Overview

This document describes the graph backend implemented in Phase 3: schema types, in-memory storage, persistence options, CozoDB persistence, API and CLI.

Schema

- GraphBackendConfig:
  - path: string (unused for in-memory mode)
  - edges: list of EdgeTypeConfig { edge_type, from_field, to_field }

Runtime

- GraphSchema: derived from GraphBackendConfig; contains EdgeType entries and helpers to render Datalog.
- GraphBackend: in-memory node and edge storage with optional on-disk persistence and optional CozoDB HTTP persistence.
  - Nodes are stored as a map id -> title
  - Edges are stored as map from_id -> list of (to_id, edge_type, weight)
  - Persistence: set ENGRAPH_GRAPH_DATA_DIR environment variable to enable on-disk persistence under that directory (nodes.json and edges.json).
  - CozoDB: when compiled with --features reqwest_client, GraphBackend will POST nodes and edges to CozoDB at configured CozoConfig.base_url (/nodes and /edges endpoints). The CozoClient is a thin HTTP wrapper.

API

- POST /collections/:collection/graph/add_node
  - body: { node_id, node_type, title, payload }
  - returns 201 on success

- POST /collections/:collection/graph/add_edge
  - body: { from, to, edge_type, weight }
  - returns 201 on success

- POST /collections/:collection/graph/bfs
  - body: { start, edge_type, max_depth }
  - returns list of discovered node ids

- POST /collections/:collection/graph/shortest_path
  - body: { start, target, edge_types } where edge_types is optional comma-separated list
  - returns shortest path (by summed weight) as list of node ids between start and target or null

CLI

- engraf-migrate graph add-node ...  (calls API; prints HTTP status and response)
- engraf-migrate graph add-edge ...
- engraf-migrate graph bfs ...
- engraf-migrate graph shortest-path ...

Examples

Enable on-disk persistence (per process):

export ENGRAPH_GRAPH_DATA_DIR=/var/lib/engraph/graphs/mycollection

Add node via CLI:
engraf-migrate graph add-node --collection mycollection --node_id n1 --node_type t --title "Node 1" --payload '{"x":1}'

Add edge and compute shortest path:
engraf-migrate graph add-edge --collection mycollection --from n1 --to n2 --edge_type rel --weight 1.5
engraf-migrate graph shortest-path --collection mycollection --start n1 --target n3 --edge_types "rel"

Notes & Next steps

- CozoDB persistence assumes simple /nodes and /edges endpoints. Adjust CozoClient when a concrete API is known.
- Consider adding authentication headers and retries for CozoClient.
- Consider background persistence and snapshotting for performance with large graphs.
