#!/usr/bin/env bash
# Build Xavier using Docker BuildKit (no cross-compilation needed).
#
# Builds from the repo-root `Dockerfile` (multi-stage: panel-ui build ->
# cargo release build -> minimal debian-slim runtime). `docker/Dockerfile`
# (rust:1.77-slim, missing the codegraph-types/crates/vendor path deps and a
# panel-ui build stage) was removed as obsolete/broken — nothing in CI or any
# docker-compose file referenced it; see GH #2545 cleanup.
#
# NOTE: the image tag below (`iberi22/xavier` on whatever registry your local
# `docker` is logged into) is for LOCAL smoke testing only. The only image
# this project's CI actually publishes is `ghcr.io/iberi22/xavier`, pushed by
# the `docker-publish` job in `.github/workflows/release.yml`, and only when
# a `v*` tag is pushed.
set -e

echo "=== Xavier Docker Build (Native) ==="

cd "$(dirname "$0")/.."

# Ensure Docker is running
if ! docker info > /dev/null 2>&1; then
    echo "ERROR: Docker is not running"
    exit 1
fi

# Build with BuildKit. First build is ~20 min (panel-ui + full cargo build,
# uncached); subsequent builds reuse Docker layer/BuildKit cache and are
# much faster unless Cargo.lock or src/ changed broadly.
echo "Building Docker image..."
DOCKER_BUILDKIT=1 docker build \
    --platform linux/amd64 \
    -t iberi22/xavier:latest \
    .

echo ""
echo "=== Build successful ==="
docker images iberi22/xavier --format "{{.Repository}}:{{.Tag}} - {{.Size}}"