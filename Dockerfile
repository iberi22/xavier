# syntax=docker/dockerfile:1
# Xavier - Optimized Multi-Stage Docker Build
# Target: < 100MB production image
#
# Usage: docker build --build-arg FEATURES="local-gllm,cli-interactive" -t xavier .
#        docker run -p 8006:8006 xavier

# Stage 0: Frontend Builder
FROM node:22-bookworm-slim AS frontend-builder
WORKDIR /app
COPY pnpm-workspace.yaml package.json pnpm-lock.yaml ./
COPY panel-ui/package.json panel-ui/package.json
COPY vendor/ vendor/
RUN --mount=type=cache,id=pnpm,target=/root/.local/share/pnpm/store \
    corepack enable && corepack prepare pnpm@11.24.0 --activate && pnpm install --config.dangerouslyAllowAllBuilds=true
COPY panel-ui/ panel-ui/
COPY Cargo.toml Cargo.toml
RUN pnpm --filter xavier-panel-ui run build

# Stage 1: Builder
# Using slim variant to keep final image small (~500MB vs ~800MB for full)
FROM rust:1.94-bookworm AS builder

ARG FEATURES=local-gllm,cli-interactive

WORKDIR /app

# Install ONLY what cargo/rustc need at build time:
# - protobuf-compiler: for tonic (gRPC/prost) used by surrealdb
# - libssl-dev: for OpenSSL linkage during build
# - pkg-config: for finding libraries
RUN apt-get update && apt-get install -y --no-install-recommends \
        protobuf-compiler \
        libssl-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy source (minimal build context)
# NOTE: must mirror every path dependency / workspace member used by
# Cargo.toml, or `cargo build` fails with "failed to read ... Cargo.toml":
# - codegraph-types/: used by code-graph + code-graph/parsers/*
#   (path = "../codegraph-types" / "../../../codegraph-types")
# - vendor/maloca-core/: used by root crate (maloca-core path dep)
COPY Cargo.toml Cargo.lock ./
COPY benches/ benches/
COPY src/ src/
COPY code-graph/ code-graph/
COPY codegraph-types/ codegraph-types/
COPY crates/ crates/
COPY vendor/maloca-core/ vendor/maloca-core/
COPY panel-ui/src-tauri/ panel-ui/src-tauri/

# Build only xavier binary (skip bench, gui, tui for smaller image)
# Production path: cargo build --release --bin xavier -j 1 --features "${FEATURES}"
# Heavy optional features (e.g. local-gllm, cli-interactive, enterprise) can be enabled via the FEATURES build-arg.
# Using -j 1 to avoid OOM on memory-constrained systems (Windows Docker Desktop)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --bin xavier -j 1 --features "${FEATURES}" && \
    cp /app/target/release/xavier /app/xavier && \
    strip -s /app/xavier

# Stage 2: Runtime
# Minimal Debian-based runtime with only essential libs
FROM debian:bookworm-slim

ARG XAVIER_VERSION=0.0.1
LABEL org.opencontainers.image.version=$XAVIER_VERSION

# Runtime dependencies:
# - ca-certificates: for HTTPS/TLS certificate validation
# - libssl3: required by rusqlite bundled SQLite and any OpenSSL-using deps
# - curl: for healthcheck
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 \
        curl \
    && rm -rf /var/lib/apt/lists/*

# Create data directory
RUN mkdir -p /data

WORKDIR /app

# Copy binary from builder stage
COPY --from=builder /app/xavier /usr/local/bin/xavier

# Copy frontend assets from frontend-builder stage
COPY --from=frontend-builder /app/panel-ui/build /app/panel-ui/build

EXPOSE 8006

# Healthcheck: verify the server is responding
HEALTHCHECK --interval=30s --timeout=10s --start-period=15s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8006/health || exit 1

ENV XAVIER_PORT=8006 \
    XAVIER_HOST=0.0.0.0 \
    XAVIER_URL=http://localhost:8006 \
    RUST_LOG=info \
    XAVIER_VERSION=${XAVIER_VERSION} \
    XAVIER_WORKSPACE_ID=default

# Required: set XAVIER_TOKEN to a secure random value
CMD ["/usr/local/bin/xavier", "http"]
