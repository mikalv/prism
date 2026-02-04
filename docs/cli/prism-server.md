# prism-server

HTTP server for Prism search engine.

## Synopsis

```bash
prism-server [OPTIONS]
```

## Options

| Option | Default | Description |
|--------|---------|-------------|
| `-c, --config <FILE>` | `prism.toml` | Configuration file path |
| `--host <HOST>` | `127.0.0.1` | Host to bind to |
| `-p, --port <PORT>` | `3080` | Port to listen on |
| `-h, --help` | — | Print help |
| `-V, --version` | — | Print version |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Override log level (e.g., `info,prism=debug`) |
| `LOG_FORMAT` | Override log format (`pretty` or `json`) |

## Examples

### Start with defaults

```bash
prism-server
```

Reads `prism.toml` from current directory, listens on `127.0.0.1:3080`.

### Custom config and port

```bash
prism-server -c /etc/prism/config.toml -p 8080
```

### Bind to all interfaces

```bash
prism-server --host 0.0.0.0 -p 3080
```

### Debug logging

```bash
RUST_LOG=debug prism-server
```

### JSON logging for production

```bash
LOG_FORMAT=json prism-server
```

## Startup Sequence

1. Load configuration from file
2. Initialize storage backend
3. Load collection schemas from `<data_dir>/schemas/`
4. Build/load indexes for each collection
5. Start HTTP server
6. Log: `Prism server listening on <address>`

## Verify Server

```bash
# Health check
curl http://localhost:3080/health

# List collections
curl http://localhost:3080/admin/collections

# Get metrics
curl http://localhost:3080/metrics
```

## Signals

| Signal | Action |
|--------|--------|
| `SIGTERM` | Graceful shutdown |
| `SIGINT` | Graceful shutdown (Ctrl+C) |

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Normal shutdown |
| `1` | Configuration error |
| `2` | Startup error (port in use, etc.) |

## See Also

- [Configuration Reference](../admin/configuration.md)
- [Deployment Guide](../admin/deployment.md)
- [prism-cli](prism-cli.md)
