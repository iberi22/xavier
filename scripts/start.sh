#!/usr/bin/env bash
# Xavier - Fast Vector Memory for AI Agents
# Startup script for NixOS
set -e

cd "$(dirname "$0")"

# Load environment
if [ -f .env ]; then
    set -a
    source .env
    set +a
fi

# NixOS-specific OpenSSL paths
export PKG_CONFIG_PATH=/nix/store/9f6v723cic8d86fszmd44vybijysb8gr-openssl-3.6.1-dev/lib/pkgconfig
export OPENSSL_LIB_DIR=/nix/store/jn166h76cwg4aqq04dq5g0z88zm1znyx-openssl-3.6.1/lib
export OPENSSL_INCLUDE_DIR=/nix/store/9f6v723cic8d86fszmd44vybijysb8gr-openssl-3.6.1-dev/include

# Ensure token is set (runtime requirement)
if [ -z "$XAVIER_TOKEN" ]; then
    echo "ERROR: XAVIER_TOKEN is not set. Create a .env file with XAVIER_TOKEN=<your-token>"
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Try release first, fall back to debug
XAVIER_BIN="${XAVIER_BIN:-"$SCRIPT_DIR/target/release/xavier"}"
if [ ! -f "$XAVIER_BIN" ]; then
    XAVIER_BIN="$SCRIPT_DIR/target/debug/xavier"
fi

if [ ! -f "$XAVIER_BIN" ]; then
    echo "ERROR: Xavier binary not found. Compile first:"
    echo "  cd $SCRIPT_DIR && CARGO_TARGET_DIR=./target cargo build --features local-gllm"
    exit 1
fi

# Ensure gllm embedding mode is set (native GGML, no external server needed)
export XAVIER_EMBEDDER="${XAVIER_EMBEDDER:-gllm}"
export XAVIER_EMBEDDING_PROVIDER_MODE="${XAVIER_EMBEDDING_PROVIDER_MODE:-gllm}"
export XAVIER_GLLM_MODEL="${XAVIER_GLLM_MODEL:-bge-small-en}"

exec "$XAVIER_BIN" http "$@"
