# =============================================================================
# Rust MMO Gateway — Multi-stage Dockerfile
# =============================================================================
# Stage 1: Builder — compile release binary with LTO
# Stage 2: Runtime — minimal Debian slim image
# =============================================================================

# ---------- Stage 1: Builder ----------
FROM rust:1.95.0-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /build

# Copy manifest files first for Docker layer caching
COPY Cargo.toml Cargo.lock build.rs rust-toolchain.toml ./
COPY proto/ ./proto/

# Copy source code
COPY src/ ./src/
COPY tests/ ./tests/
COPY benches/ ./benches/

# Build release binary with LTO
# CARGO_BUILD_JOBS=2 to avoid OOM on limited memory
ENV CARGO_BUILD_JOBS=4
ENV CARGO_TARGET_DIR=/build/target

RUN cargo build --release --bin rust-mmo-gate && \
    cargo build --release --bin logic-server && \
    cargo build --release --bin throughput_bench

# ---------- Stage 2: Runtime ----------
FROM debian:bookworm-slim AS runtime

# Install minimal runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN groupadd -r gateway && useradd -r -g gateway -s /sbin/nologin gateway

# Create directories
WORKDIR /app
RUN mkdir -p /app/config /app/logs && \
    chown -R gateway:gateway /app

# Copy binaries from builder
COPY --from=builder /build/target/release/rust-mmo-gate /app/rust-mmo-gate
COPY --from=builder /build/target/release/logic-server /app/logic-server
COPY --from=builder /build/target/release/throughput_bench /app/throughput-bench

# Copy configuration templates
COPY .env.dev.example /app/config/.env.dev.example
COPY .env.prod.example /app/config/.env.prod.example

# Switch to non-root user
USER gateway

# Expose ports: TCP game traffic + HTTP admin
EXPOSE 7888 9090

# Health check via HTTP admin endpoint
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://127.0.0.1:9090/health || exit 1

# Default environment
ENV RUST_LOG=info
ENV APP_ENV=prod

# Start gateway
ENTRYPOINT ["/app/rust-mmo-gate"]
