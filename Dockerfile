# Build stage
FROM rust:1.93-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy workspace manifests first for better layer caching
COPY Cargo.toml Cargo.lock ./
COPY prism/Cargo.toml prism/
COPY prism-server/Cargo.toml prism-server/
COPY prism-cli/Cargo.toml prism-cli/
COPY prism-storage/Cargo.toml prism-storage/
COPY prism-cluster/Cargo.toml prism-cluster/
COPY prism-importer/Cargo.toml prism-importer/
COPY prism-es-compat/Cargo.toml prism-es-compat/
COPY prism-ui/Cargo.toml prism-ui/
COPY prism-ui/build.rs prism-ui/
COPY prism-treesitter/Cargo.toml prism-treesitter/
COPY xtask/Cargo.toml xtask/

# Create placeholder for websearch-ui (rust_embed needs the folder at compile time)
RUN mkdir -p websearch-ui/dist && \
    echo '<!DOCTYPE html><html><body>Prism UI placeholder</body></html>' > websearch-ui/dist/index.html

# Create dummy src files for dependency caching
RUN mkdir -p prism/src prism-server/src prism-cli/src prism-storage/src prism-cluster/src prism-importer/src prism-es-compat/src prism-ui/src prism-treesitter/src xtask/src && \
    echo "pub fn main() {}" > prism/src/lib.rs && \
    echo "fn main() {}" > prism-server/src/main.rs && \
    echo "fn main() {}" > prism-cli/src/main.rs && \
    echo "" > prism-storage/src/lib.rs && \
    echo "" > prism-cluster/src/lib.rs && \
    echo "" > prism-importer/src/lib.rs && \
    echo "fn main() {}" > prism-importer/src/main.rs && \
    echo "" > prism-es-compat/src/lib.rs && \
    echo "" > prism-ui/src/lib.rs && \
    echo "" > prism-treesitter/src/lib.rs && \
    echo "fn main() {}" > xtask/src/main.rs

# Build arg: enable extra features for prism-server
ARG FEATURES=""

# Build dependencies only
RUN cargo build --release --workspace && \
    rm -rf prism/src prism-server/src prism-cli/src prism-storage/src prism-cluster/src prism-importer/src prism-es-compat/src prism-ui/src prism-treesitter/src xtask/src

# Copy actual source
COPY prism/src prism/src
COPY prism-server/src prism-server/src
COPY prism-cli/src prism-cli/src
COPY prism-storage/src prism-storage/src
COPY prism-cluster/src prism-cluster/src
COPY prism-importer/src prism-importer/src
COPY prism-es-compat/src prism-es-compat/src
COPY prism-ui/src prism-ui/src
COPY prism-treesitter/src prism-treesitter/src
COPY xtask/src xtask/src
COPY prism/tests prism/tests

# Touch source files to invalidate cache and rebuild
RUN touch prism/src/lib.rs prism-server/src/main.rs prism-cli/src/main.rs prism-storage/src/lib.rs prism-cluster/src/lib.rs prism-importer/src/lib.rs prism-importer/src/main.rs prism-es-compat/src/lib.rs prism-ui/src/lib.rs prism-treesitter/src/lib.rs xtask/src/main.rs

# Build release binaries (with optional features)
RUN if [ -n "$FEATURES" ]; then \
      cargo build --release -p prism-server --features "$FEATURES" && \
      cargo build --release -p prism -p prism-cli -p prism-importer; \
    else \
      cargo build --release --workspace; \
    fi

# Runtime stage - distroless for minimal attack surface
FROM gcr.io/distroless/cc-debian12:nonroot

# Copy binaries
COPY --from=builder /app/target/release/prism-server /usr/local/bin/prism-server
COPY --from=builder /app/target/release/prism /usr/local/bin/prism
COPY --from=builder /app/target/release/prism-import /usr/local/bin/prism-import

# Create data directory
WORKDIR /data

# Expose default port (HTTP) and cluster port (QUIC)
EXPOSE 3000 9080

# Default command
ENTRYPOINT ["/usr/local/bin/prism-server"]
CMD ["--host", "0.0.0.0", "--port", "3000"]
