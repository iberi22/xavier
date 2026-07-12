#!/bin/bash
# start-xavier.sh - Xavier en WSL (vía systemd o manual)
# Panel UI:   http://127.0.0.1:8006/panel
# MCP SSE:    http://127.0.0.1:8100
# Health:     http://127.0.0.1:8006/health

set -e
DATA_DIR="/home/belal/.xavier/data"
BINARY="/mnt/e/scripts-python/xavier/target/release/xavier"
PORT="${1:-8006}"

mkdir -p "$DATA_DIR/workspaces"

export XAVIER_DATA_DIR="$DATA_DIR"
export XAVIER_TOKEN="dev-token"
export XAVIER_LOG_LEVEL="info"
export XAVIER_EMBEDDING_PROVIDER_MODE="cloud"
export XAVIER_EMBEDDING_URL="https://openrouter.ai/api/v1/embeddings"
export XAVIER_EMBEDDING_MODEL="text-embedding-3-small"
export XAVIER_EMBEDDING_DIMENSIONS="1536"
export XAVIER_EMBEDDING_API_FLAVOR="openai"
export XAVIER_EMBEDDING_CACHE_ENABLED="true"
export XAVIER_EMBEDDER="cloud"

# Leer API key del vault cifrado (nunca en texto plano)
# Almacenar con: xavier vault set embedding_api_key <key>
API_KEY=$("$BINARY" vault get embedding_api_key 2>/dev/null | grep -o 'sk-or-.*' || true)
if [ -n "$API_KEY" ]; then
    export XAVIER_EMBEDDING_API_KEY="$API_KEY"
fi

echo ""
echo "╔══════════════════════════════════════════╗"
echo "║         XAVIER MEMORY RUNTIME            ║"
echo "║        v0.12.0 — Cognitive Memory        ║"
echo "╠══════════════════════════════════════════╣"
echo "║ HTTP:  http://127.0.0.1:$PORT           ║"
echo "║ Panel: http://127.0.0.1:$PORT/panel     ║"
echo "║ MCP:   http://127.0.0.1:8100            ║"
echo "║ Token: dev-token                         ║"
echo "║ Data:  $DATA_DIR                        ║"
if [ -n "$API_KEY" ]; then
    echo "║ API:   ✅ From encrypted vault          ║"
else
    echo "║ API:   ⚠️  Not in vault (xavier vault set)║"
fi
echo "╚══════════════════════════════════════════╝"
echo ""

cd "$DATA_DIR"
exec "$BINARY" http "$PORT"
