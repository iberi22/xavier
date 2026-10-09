#!/bin/bash
set -e

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

STATE_FILE="$TMP_DIR/incidents.json"

# We mock python3 to intercept notify.py calls
MOCK_DIR="$TMP_DIR/bin"
mkdir -p "$MOCK_DIR"
cat <<'EOF' > "$MOCK_DIR/python3"
#!/bin/bash
if [[ "$1" == "scripts/guardian/notify.py" ]]; then
  # Read stdin and append it to the state file
  EVENT_JSON=$(cat)
  # Basic array wrapping for the mock
  if [[ ! -f "$GUARDIAN_STATE" ]]; then
    echo "[$EVENT_JSON]" > "$GUARDIAN_STATE"
  else
    # Simple append for testing (not valid JSON array, but enough for grep dedupe test)
    # Actually, we can just append the raw json to the file.
    echo "$EVENT_JSON" >> "$GUARDIAN_STATE"
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

COUNT=$(grep -c failing_sha "$STATE_FILE" || true)
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

# "a --state in the temp dir contains no secret and no personal path"
if grep -q -i "secret" "$STATE_FILE"; then
  echo "State file contains secret"
  exit 1
fi

# Verify there is no traceback
if echo "$OUTPUT" | grep -q "Traceback"; then
  echo "Output contains traceback"
  exit 1
fi

echo "All tests passed"
exit 0
