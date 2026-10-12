#!/usr/bin/env bash
set -euo pipefail

KEEP=0
BINARY=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --keep)
            KEEP=1
            shift
            ;;
        -*)
            echo "Unknown flag: $1" >&2
            exit 1
            ;;
        *)
            if [[ -z "$BINARY" ]]; then
                BINARY="$1"
            else
                echo "Too many arguments. Expected only one binary." >&2
                exit 1
            fi
            shift
            ;;
    esac
done

if [[ -z "$BINARY" ]]; then
    echo "Usage: $0 BINARY [--keep]" >&2
    exit 1
fi

if [[ ! -x "$BINARY" ]]; then
    echo "Error: BINARY '$BINARY' is not executable or not found." >&2
    exit 1
fi

TEMP_DIR=$(mktemp -d)

cleanup() {
    local exit_code=$?
    if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
        kill "$SERVER_PID" || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    if [[ "$KEEP" -eq 1 ]]; then
        echo "Keeping temp dir: $TEMP_DIR"
    else
        rm -rf "$TEMP_DIR"
    fi
    exit $exit_code
}
trap cleanup EXIT

export HOME="$TEMP_DIR"
export XAVIER_DATA_DIR="$TEMP_DIR"
export XAVIER_STATE_DIR="$TEMP_DIR"
export XAVIER_CONFIG_DIR="$TEMP_DIR"
export XAVIER_WORKSPACE_DIR="$TEMP_DIR"
export XAVIER_CONFIG_PATH="$TEMP_DIR/config.json"

RESOLVED_DATA_DIR=$(python3 -c "import os, sys; print(os.path.realpath(sys.argv[1]))" "$XAVIER_DATA_DIR")
RESOLVED_TEMP_DIR=$(python3 -c "import os, sys; print(os.path.realpath(sys.argv[1]))" "$TEMP_DIR")

if [[ "$RESOLVED_DATA_DIR" != "$RESOLVED_TEMP_DIR"* ]]; then
    echo "Error: resolved data dir is not under temp root" >&2
    exit 1
fi

XAVIER_TOKEN=$(python3 -c "import os, binascii; print(binascii.hexlify(os.urandom(16)).decode('utf-8'))")
export XAVIER_TOKEN

HTTP_PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
MCP_PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
export XAVIER_PORT="$HTTP_PORT"
export XAVIER_MCP_PORT="$MCP_PORT"
export XAVIER_HOST="127.0.0.1"

echo "Starting binary: $BINARY http"
"$BINARY" http > "$TEMP_DIR/server.log" 2>&1 &
SERVER_PID=$!

BASE_URL="http://127.0.0.1:$HTTP_PORT"
MCP_URL="http://127.0.0.1:$MCP_PORT/mcp"

echo "Waiting for health check (max 30s)..."
MAX_TRIES=30
HEALTHY=0
for ((i=1; i<=MAX_TRIES; i++)); do
    if curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/health" | grep -q "200"; then
        HEALTHY=1
        break
    fi
    sleep 1
done

if [[ "$HEALTHY" -eq 0 ]]; then
    echo "SMOKE: FAIL health"
    exit 1
fi
echo "SMOKE: PASS 1/5 (health 200)"

PAYLOAD=$(cat <<JSON
{
    "content": "smoke memory test",
    "metadata": {}
}
JSON
)

ADD_RESP=$(curl -s -w "\n%{http_code}" -X POST "$BASE_URL/v1/memory/add" \
    -H "X-Xavier-Token: $XAVIER_TOKEN" \
    -H "Content-Type: application/json" \
    -d "$PAYLOAD")

ADD_BODY=$(echo "$ADD_RESP" | head -n -1)
ADD_CODE=$(echo "$ADD_RESP" | tail -n 1)

if [[ "$ADD_CODE" != "200" ]]; then
    echo "SMOKE: FAIL add"
    echo "Body: $ADD_BODY"
    exit 1
fi
echo "SMOKE: PASS 2/5 (add memory)"

SEARCH_PAYLOAD=$(cat <<JSON
{
    "query": "smoke memory",
    "limit": 10
}
JSON
)

SEARCH_RESP=$(curl -s -w "\n%{http_code}" -X POST "$BASE_URL/v1/memory/search" \
    -H "X-Xavier-Token: $XAVIER_TOKEN" \
    -H "Content-Type: application/json" \
    -d "$SEARCH_PAYLOAD")

SEARCH_BODY=$(echo "$SEARCH_RESP" | head -n -1)
SEARCH_CODE=$(echo "$SEARCH_RESP" | tail -n 1)

if [[ "$SEARCH_CODE" != "200" ]] || ! echo "$SEARCH_BODY" | grep -q "smoke memory test"; then
    echo "SMOKE: FAIL search"
    echo "Body: $SEARCH_BODY"
    exit 1
fi
echo "SMOKE: PASS 3/5 (search memory)"

UNAUTH_RESP=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE_URL/v1/memory/add" \
    -H "Content-Type: application/json" \
    -d "$PAYLOAD")

if [[ "$UNAUTH_RESP" != "401" && "$UNAUTH_RESP" != "403" ]]; then
    echo "SMOKE: FAIL unauthenticated write"
    echo "Code: $UNAUTH_RESP"
    exit 1
fi
echo "SMOKE: PASS 4/5 (unauth write rejected)"

MCP_PAYLOAD=$(cat <<JSON
{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tools/list"
}
JSON
)

MCP_RESP=$(curl -s -w "\n%{http_code}" -X POST "$MCP_URL" \
    -H "X-Xavier-Token: $XAVIER_TOKEN" \
    -H "Content-Type: application/json" \
    -d "$MCP_PAYLOAD")

MCP_BODY=$(echo "$MCP_RESP" | head -n -1)
MCP_CODE=$(echo "$MCP_RESP" | tail -n 1)

if [[ "$MCP_CODE" != "200" ]] || ! echo "$MCP_BODY" | grep -q "tools"; then
    echo "SMOKE: FAIL mcp"
    echo "Code: $MCP_CODE"
    echo "Body: $MCP_BODY"
    exit 1
fi
echo "SMOKE: PASS 5/5 (mcp tools/list)"
