#!/bin/bash
# test-e2e-rag.sh
# End-to-End Test for Xavier RAG Flow

PORT=8990
TOKEN="e2e-test-token"
XAVIER_BIN="./target/debug/xavier"

if [ ! -f "$XAVIER_BIN" ]; then
    echo "❌ Xavier binary not found at $XAVIER_BIN. Build it first."
    exit 1
fi

export XAVIER_TOKEN=$TOKEN
export XAVIER_CONFIG_DIR=$(mktemp -d)

echo "🚀 Starting Xavier on port $PORT..."
$XAVIER_BIN http $PORT > e2e_test.log 2>&1 &
SERVER_PID=$!

cleanup() {
    echo "🧹 Cleaning up..."
    kill $SERVER_PID
    rm -rf $XAVIER_CONFIG_DIR
}

trap cleanup EXIT

# 1. Wait for readiness
echo "⏳ Waiting for readiness..."
READY=false
for i in {1..30}; do
    STATUS=$(curl -s http://localhost:$PORT/v1/health/ready | jq -r .status)
    if [ "$STATUS" == "ok" ]; then
        READY=true
        break
    fi
    sleep 1
done

if [ "$READY" != "true" ]; then
    echo "❌ Server failed to become ready."
    cat e2e_test.log
    exit 1
fi
echo "✅ Server ready!"

# 2. Add Memory
echo "📝 Adding memory..."
ADD_RESP=$(curl -s -X POST http://localhost:$PORT/v1/memories \
    -H "Content-Type: application/json" \
    -H "X-Xavier-Token: $TOKEN" \
    -d '{
        "text": "The secret code for today is XAVIER-2026",
        "content": "The secret code for today is XAVIER-2026",
        "user_id": "tester"
    }')

if [[ "$ADD_RESP" != *"ok"* ]]; then
    echo "❌ Failed to add memory: $ADD_RESP"
    exit 1
fi
echo "✅ Memory added!"

# 3. Search Memory
echo "🔍 Searching memory..."
SEARCH_RESP=$(curl -s -X POST http://localhost:$PORT/v1/memories/search \
    -H "Content-Type: application/json" \
    -H "X-Xavier-Token: $TOKEN" \
    -d '{
        "query": "secret code",
        "limit": 1
    }')

if [[ "$SEARCH_RESP" == *"XAVIER-2026"* ]]; then
    echo "✅ Search successful! Found the secret code."
else
    echo "❌ Search failed or result missing: $SEARCH_RESP"
    exit 1
fi

echo "🎉 E2E RAG Test PASSED!"
exit 0
