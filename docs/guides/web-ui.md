# Web UI

Prism includes an optional embedded web interface for searching and exploring your collections.

## Enabling the Web UI

The web UI is built as a separate feature flag. To include it in your build:

```bash
# Build with UI enabled
cargo build -p prism-server --features ui --release

# Or include in full build
cargo build -p prism-server --features "full,ui" --release
```

Once enabled, the UI is available at `http://localhost:3080/ui/`.

## Features

The web UI provides:

- **Search interface** — Full-text and hybrid search across collections
- **Collection browser** — View and explore indexed documents
- **Query builder** — Build complex queries with filters and aggregations
- **Results visualization** — Highlighted matches and relevance scores

## Development Mode

For rapid frontend development, Prism supports serving UI assets from disk instead of the embedded bundle:

```bash
# Option 1: Symlink the built UI
cd /path/to/prism
ln -s websearch-ui/dist webui

# Option 2: Run Vite in watch mode
cd websearch-ui
npm run build -- --watch
```

When a `webui/` directory exists in the current working directory, Prism serves files from there with no-cache headers, enabling hot reload workflows.

## Building the UI from Source

The websearch-ui is a Vite + React application:

```bash
# Navigate to UI source
cd websearch-ui

# Install dependencies
npm install

# Development server (standalone)
npm run dev

# Production build
npm run build
```

The build output goes to `websearch-ui/dist/` and is automatically embedded during `cargo build`.

## Docker with UI

### Dockerfile

```dockerfile
FROM rust:1.75-slim as builder
WORKDIR /app

# Install Node.js for UI build
RUN apt-get update && apt-get install -y nodejs npm

COPY . .

# Build with UI feature
RUN cargo build --release -p prism-server --features ui

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/prism-server /usr/local/bin/
EXPOSE 3080
CMD ["prism-server"]
```

### Multi-stage with Cached UI

For faster rebuilds, build the UI separately:

```dockerfile
# Stage 1: Build UI
FROM node:20-slim as ui-builder
WORKDIR /app/websearch-ui
COPY websearch-ui/package*.json ./
RUN npm ci
COPY websearch-ui/ ./
RUN npm run build

# Stage 2: Build Rust
FROM rust:1.75-slim as builder
WORKDIR /app
COPY . .
COPY --from=ui-builder /app/websearch-ui/dist ./websearch-ui/dist
RUN cargo build --release -p prism-server --features ui

# Stage 3: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/prism-server /usr/local/bin/
EXPOSE 3080
CMD ["prism-server"]
```

## Disabling the UI

The UI is optional and excluded by default. To build without it:

```bash
# Default build (no UI)
cargo build -p prism-server --release
```

This produces a smaller binary suitable for headless/API-only deployments.

## Configuration

The UI requires CORS to be enabled if accessing from a different origin:

```toml
[server.cors]
enabled = true
origins = ["http://localhost:5173"]  # Vite dev server
```

For production behind a reverse proxy, the UI and API share the same origin, so CORS is typically not needed.

## Troubleshooting

### UI shows "UI not built" placeholder

The npm build failed during cargo build. Check the build warnings:

```bash
cargo build -p prism-server --features ui 2>&1 | grep "cargo:warning"
```

Common fixes:
- Ensure Node.js and npm are installed
- Run `cd websearch-ui && npm install && npm run build` manually
- Check for TypeScript errors in the UI source

### UI assets not updating

In development mode with the `webui/` symlink, ensure you're running Vite in watch mode or rebuilding after changes.

### 404 on UI routes

The UI uses client-side routing. All non-file paths under `/ui/` return `index.html` for the SPA to handle. If you see 404s, ensure you're accessing `/ui/` (with trailing slash) or a valid asset path.

## See Also

- [Getting Started](getting-started.md) — Quick start guide
- [API Reference](api-reference.md) — REST API documentation
- [Deployment](../admin/deployment.md) — Production deployment
