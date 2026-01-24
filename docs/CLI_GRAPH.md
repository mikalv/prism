engraph-migrate: Graph CLI

This documents the new graph-related subcommands added to engraf-migrate CLI.

Commands

- engraf-migrate graph add-node --collection <name> --node_id <id> --node_type <type> --title <title> [--payload '{...}'] [--api_url http://localhost:8080]
  - Calls POST /collections/:collection/graph/add_node with JSON body { node_id, node_type, title, payload }

- engraf-migrate graph add-edge --collection <name> --from <id> --to <id> --edge_type <type> [--weight <f32>] [--api_url http://localhost:8080]
  - Calls POST /collections/:collection/graph/add_edge with JSON body { from, to, edge_type, weight }

- engraf-migrate graph bfs --collection <name> --start <id> --edge_type <type> [--max_depth 3] [--api_url http://localhost:8080]
  - Calls POST /collections/:collection/graph/bfs with JSON body { start, edge_type, max_depth } and prints the returned node id list.

- engraf-migrate graph shortest-path --collection <name> --start <id> --target <id> [--edge_types "rel,type2"] [--api_url http://localhost:8080]
  - Calls POST /collections/:collection/graph/shortest_path with JSON body { start, target, edge_types } and prints the shortest path (weighted) between start and target.

Notes

- The CLI uses the HTTP API; ensure the engraph-core server is running and accessible at --api_url.
- Payload should be a JSON string; if invalid JSON, an empty object is sent.
