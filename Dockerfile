# =============================================================================
# Rust MMO Gateway — Multi-stage Dockerfile (Dual Crate)
# =============================================================================
# Stage 1: Builder — compile gateway + logic-lib in release mode with LTO
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

# Copy gateway manifest files first for Docker layer caching
COPY Cargo.toml Cargo.lock build.rs rust-toolchain.toml ./
COPY proto/ ./proto/

# Copy gateway source code (10 core modules, zero game logic)
COPY src/ ./src/
COPY tests/ ./tests/
COPY benches/ ./benches/

# Copy logic-lib (game logic crate)
COPY logic-lib/Cargo.toml ./logic-lib/
COPY logic-lib/src/ ./logic-lib/src/
COPY logic-lib/tests/ ./logic-lib/tests/

# Build all release binaries
# Gateway
RUN cargo build --release --bin rust-mmo-gate && \
    cargo build --release --bin login-server && \
    cargo build --release --bin throughput-bench
# Logic servers (built from logic-lib crate)
RUN cd logic-lib && cargo build --release --bin logic-server && \
    cargo build --release --bin scene-server && \
    cargo build --release --bin chat-server && \
    cargo build --release --bin combat-server

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
RUN mkdir -p /app/logs && \
    chown -R gateway:gateway /app

# Copy gateway binaries from builder
COPY --from=builder /build/target/release/rust-mmo-gate /app/rust-mmo-gate
COPY --from=builder /build/target/release/login-server /app/login-server
COPY --from=builder /build/target/release/throughput-bench /app/throughput-bench

# Copy logic server binaries from builder
COPY --from=builder /build/logic-lib/target/release/logic-server /app/logic-server
COPY --from=builder /build/logic-lib/target/release/scene-server /app/scene-server
COPY --from=builder /build/logic-lib/target/release/chat-server /app/chat-server
COPY --from=builder /build/logic-lib/target/release/combat-server /app/combat-server

# Copy configuration
COPY .env.dev /app/.env
COPY .env.prod /app/.env.prod

# Switch to non-root user
USER gateway

# Expose ports: TCP game traffic + HTTP admin
EXPOSE 7888 9090

# gRPC internal ports (logic servers)
EXPOSE 50051 50052 50053 50054

# Health check via HTTP admin endpoint
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://127.0.0.1:9090/health || exit 1

# Default environment
ENV RUST_LOG=info
ENV APP_ENV=prod

# Start gateway
ENTRYPOINT ["/app/rust-mmo-gate"]
