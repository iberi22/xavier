#!/bin/bash
set -e

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

STATE_FILE="$TMP_DIR/incidents.json"

MOCK_DIR="$TMP_DIR/bin"
mkdir -p "$MOCK_DIR"
cat <<'EOF' > "$MOCK_DIR/python3"
#!/bin/bash
if [[ "$1" == "scripts/guardian/notify.py" ]]; then
  EVENT_JSON=$(cat)
  if [[ ! -f "$GUARDIAN_STATE" ]]; then
    echo "[$EVENT_JSON]" > "$GUARDIAN_STATE"
  else
    # parse the array, add the new event, and write back
    jq --argjson event "$EVENT_JSON" '. + [$event]' "$GUARDIAN_STATE" > "$GUARDIAN_STATE.tmp" && mv "$GUARDIAN_STATE.tmp" "$GUARDIAN_STATE"
  fi
  exit 0
fi
exec /usr/bin/python3 "$@"
EOF
chmod +x "$MOCK_DIR/python3"

export PATH="$MOCK_DIR:$PATH"
export GUARDIAN_STATE="$STATE_FILE"

# Test 1: Record one incident
bash scripts/main-incident.sh --sha abcdef --run-url http://ci.local --state "$STATE_FILE"

# Test 2: The same SHA twice records one incident (dedupe)
bash scripts/main-incident.sh --sha abcdef --run-url http://ci.local --state "$STATE_FILE"

# Test 3: A second distinct SHA adds one
bash scripts/main-incident.sh --sha 123456 --run-url http://ci.local --state "$STATE_FILE"

# We check length of the array to assert dedupe
COUNT=$(jq 'length' "$STATE_FILE" || echo 0)
if [[ "$COUNT" -ne 2 ]]; then
  echo "Expected 2 incidents, got $COUNT"
  exit 1
fi

# Test 4: Malformed arguments exit 2 with no traceback
set +e
OUTPUT=$(bash scripts/main-incident.sh --invalid 2>&1)
EXIT_CODE=$?
set -e

if [[ $EXIT_CODE -ne 2 ]]; then
  echo "Expected exit code 2 for malformed args, got $EXIT_CODE"
  exit 1
fi

if [[ "$OUTPUT" != "Malformed arguments" ]]; then
  echo "Expected exact string 'Malformed arguments', got '$OUTPUT'"
  exit 1
fi

# Also test without required args
set +e
OUTPUT=$(bash scripts/main-incident.sh 2>&1)
EXIT_CODE=$?
set -e

if [[ $EXIT_CODE -ne 2 ]]; then
  echo "Expected exit code 2 for missing args, got $EXIT_CODE"
  exit 1
fi

if grep -q -i "secret" "$STATE_FILE"; then
  echo "State file contains secret"
  exit 1
fi

if echo "$OUTPUT" | grep -q "Traceback"; then
  echo "Output contains traceback"
  exit 1
fi

echo "All tests passed"
exit 0
