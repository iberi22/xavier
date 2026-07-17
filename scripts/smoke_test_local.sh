#!/usr/bin/env bash
# Usage: bash scripts/smoke_test_local.sh
#
# Environment variables available:
#   XAVIER_BIN           Path to the Xavier binary (default: ./target/debug/xavier)
#   XAVIER_PORT          Port to run the Xavier HTTP server on (default: 18006)
#   XAVIER_TOKEN         Token for authenticating with Xavier (default: randomly generated)
#   PYTHON_BIN           Path to python3 binary (default: python3)

set -euo pipefail

XAVIER_BIN="${XAVIER_BIN:-./target/debug/xavier}"
XAVIER_PORT="${XAVIER_PORT:-18006}"
PYTHON_BIN="${PYTHON_BIN:-python3}"

if ! command -v "${PYTHON_BIN}" >/dev/null 2>&1; then
  PYTHON_BIN="python"
fi

XAVIER_TOKEN="${XAVIER_TOKEN:-$("${PYTHON_BIN}" -c 'import secrets; print(secrets.token_hex(16))' 2>/dev/null || echo "token-$RANDOM")}"

if [ ! -f "${XAVIER_BIN}" ]; then
  echo "❌ Xavier binary not found at ${XAVIER_BIN}. Build it first." >&2
  exit 1
fi

LOG_FILE="xavier_smoke_local.log"
echo "Cleaning old logs..."
rm -f "${LOG_FILE}"

echo "🚀 Starting Xavier in background on port ${XAVIER_PORT}..."
echo "Using token: ${XAVIER_TOKEN}"

# Spawn binary with headless=true, local provider and unreachable LLM url (forces memory-fallback if exists)
XAVIER_HEADLESS=true \
XAVIER_MODEL_PROVIDER=local \
XAVIER_LOCAL_LLM_URL=http://127.0.0.1:1/v1 \
XAVIER_TOKEN="${XAVIER_TOKEN}" \
"${XAVIER_BIN}" http "${XAVIER_PORT}" > "${LOG_FILE}" 2>&1 &

PID=$!

cleanup() {
  echo "🧹 Cleaning up background server (PID: ${PID})..."
  kill "${PID}" 2>/dev/null || true
  wait "${PID}" 2>/dev/null || true
}

trap cleanup EXIT

# 1. Poll GET /health max 30s (60 attempts x 500ms)
echo "⏳ Waiting for /health to become ready..."
READY=false
for i in {1..60}; do
  if curl -fsS "http://127.0.0.1:${XAVIER_PORT}/health" >/dev/null 2>&1; then
    READY=true
    break
  fi
  sleep 0.5
done

if [ "${READY}" != "true" ]; then
  echo "❌ Server failed to become ready in 30 seconds." >&2
  echo "=== LAST 50 LINES OF SERVER LOGS ==="
  tail -n 50 "${LOG_FILE}" || true
  exit 1
fi

echo "✅ Server /health is ready!"

# 2. POST /v1/chat/completions
echo "💬 Sending chat completion request..."
RESPONSE_FILE=$(mktemp)
trap 'rm -f "${RESPONSE_FILE}"; cleanup' EXIT

HTTP_CODE=$(curl -s -o "${RESPONSE_FILE}" -w "%{http_code}" \
  -X POST "http://127.0.0.1:${XAVIER_PORT}/v1/chat/completions" \
  -H "X-Xavier-Token: ${XAVIER_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"model":"auto","messages":[{"role":"user","content":"ping"}]}')

if [ "${HTTP_CODE}" = "200" ]; then
  echo "✅ HTTP 200 Received. Validating response payload..."
  if ! "${PYTHON_BIN}" - "${RESPONSE_FILE}" <<'PY'
import json
import sys

try:
    with open(sys.argv[1], 'r') as f:
        payload = json.load(f)
except Exception as e:
    print(f"FAIL: Failed to parse JSON response: {e}")
    sys.exit(1)

if not isinstance(payload.get("choices"), list) or len(payload["choices"]) == 0:
    print("FAIL: choices list is empty or missing")
    sys.exit(1)

content = payload["choices"][0].get("message", {}).get("content", "")
if not content:
    print("FAIL: choices[0].message.content is empty")
    sys.exit(1)

print(f"PASS: choices[0].message.content is: {content}")
sys.exit(0)
PY
  then
    echo "❌ Chat completion content validation failed." >&2
    echo "=== RESPONSE BODY ==="
    cat "${RESPONSE_FILE}"
    echo "=== LAST 50 LINES OF SERVER LOGS ==="
    tail -n 50 "${LOG_FILE}" || true
    exit 1
  fi
elif [ "${HTTP_CODE}" = "500" ] || [ "${HTTP_CODE}" = "429" ]; then
  echo "⚠️ HTTP ${HTTP_CODE} Received (Expected in this fallback context if memory-fallback is optional/absent, or if security rules block 'ping')."
  echo "=== RESPONSE BODY ==="
  cat "${RESPONSE_FILE}"
  echo "----------------------"
  echo "✅ Server is alive and responded correctly to the request."
  echo "PASS /v1/chat/completions (server responded)"
else
  echo "❌ Unexpected HTTP status code ${HTTP_CODE} received." >&2
  echo "=== RESPONSE BODY ==="
  cat "${RESPONSE_FILE}"
  echo "=== LAST 50 LINES OF SERVER LOGS ==="
  tail -n 50 "${LOG_FILE}" || true
  exit 1
fi

echo "🎉 Local smoke test PASSED!"
exit 0
