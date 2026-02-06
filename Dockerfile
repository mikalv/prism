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
COPY prism-importer/Cargo.toml prism-importer/
COPY xtask/Cargo.toml xtask/

# Create dummy src files for dependency caching
RUN mkdir -p prism/src prism-server/src prism-cli/src prism-storage/src prism-importer/src xtask/src && \
    echo "pub fn main() {}" > prism/src/lib.rs && \
    echo "fn main() {}" > prism-server/src/main.rs && \
    echo "fn main() {}" > prism-cli/src/main.rs && \
    echo "" > prism-storage/src/lib.rs && \
    echo "fn main() {}" > prism-importer/src/main.rs && \
    echo "fn main() {}" > xtask/src/main.rs

# Build dependencies only
RUN cargo build --release --workspace && \
    rm -rf prism/src prism-server/src prism-cli/src prism-storage/src xtask/src

# Copy actual source
COPY prism/src prism/src
COPY prism-server/src prism-server/src
COPY prism-cli/src prism-cli/src
COPY prism-storage/src prism-storage/src
COPY prism-importer/src prism-importer/src
COPY xtask/src xtask/src
COPY prism/tests prism/tests

# Touch source files to invalidate cache and rebuild
RUN touch prism/src/lib.rs prism-server/src/main.rs prism-cli/src/main.rs prism-storage/src/lib.rs prism-importer/src/main.rs xtask/src/main.rs

# Build release binaries
RUN cargo build --release --workspace

# Runtime stage - distroless for minimal attack surface
FROM gcr.io/distroless/cc-debian12:nonroot

# Copy binaries
COPY --from=builder /app/target/release/prism-server /usr/local/bin/prism-server
COPY --from=builder /app/target/release/prism /usr/local/bin/prism
COPY --from=builder /app/target/release/prism-import /usr/local/bin/prism-import

# Create data directory
WORKDIR /data

# Expose default port
EXPOSE 3000

# Default command
ENTRYPOINT ["/usr/local/bin/prism-server"]
CMD ["--host", "0.0.0.0", "--port", "3000"]
