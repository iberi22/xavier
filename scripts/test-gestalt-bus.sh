#!/usr/bin/env bash
# Gestalt Event Bus Test Script (Cross-Platform POSIX)
set -eo pipefail

BUS_URL="${1:-http://127.0.0.1:8081}"

echo "Testing Gestalt Event Bus on $BUS_URL..."

# Test 1: Connectivity
echo "[Test 1] Event Bus Connectivity"
HTTP_STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$BUS_URL/healthz" || curl -s -o /dev/null -w "%{http_code}" "$BUS_URL/health" || true)

if [ "$HTTP_STATUS" = "200" ]; then
    echo "[PASS] Event Bus is running at $BUS_URL"
else
    echo "[FAIL] Event Bus is not running at $BUS_URL (HTTP status: $HTTP_STATUS)"
    echo "Start it with: cargo run --bin gestalt_cli -- bus serve --port 8081"
    exit 1
fi

# Test 2: Send test event
echo "[Test 2] Send Test Event"
EVENT_PAYLOAD=$(cat <<EOF
{
  "agent": "test-script",
  "event_type": "test_event",
  "state": "Testing",
  "summary": "Test event from POSIX shell",
  "ts": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF
)

RESPONSE=$(curl -s -X POST "$BUS_URL/api/event" \
    -H "Content-Type: application/json" \
    -d "$EVENT_PAYLOAD")

if echo "$RESPONSE" | grep -q '"status":"ok"'; then
    echo "[PASS] Test event sent successfully"
    echo "Gestalt Event Bus tests completed successfully"
    exit 0
else
    echo "[FAIL] Failed to send test event: $RESPONSE"
    exit 1
fi
