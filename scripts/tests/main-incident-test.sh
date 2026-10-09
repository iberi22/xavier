#!/bin/bash
set -e

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

STATE_FILE="$TMP_DIR/incidents.json"

# Exercise the real notify.py CLI boundary (event file positional + --state).
# python3 is NEVER mocked. notify.py is owned by PR 2893, so on this branch it
# may be absent; when absent we use a real Python fixture that implements the
# same documented CLI, so the argument shape passed by main-incident.sh is what
# gets tested. In the integrated tree the real notify.py is used.
NOTIFY_PY="$(pwd)/scripts/guardian/notify.py"
if [[ ! -f "$NOTIFY_PY" ]]; then
  NOTIFY_PY="$TMP_DIR/notify_fixture.py"
  cat > "$NOTIFY_PY" <<'PY'
"""Real-Python stand-in for scripts/guardian/notify.py (PR 2893).

Implements the same CLI contract: notify.py <event.json> --state <state.json> [--json].
Never reads stdin, so main-incident.sh must pass the event as a file.
"""
import argparse
import json
import os

parser = argparse.ArgumentParser()
parser.add_argument("event")
parser.add_argument("--state", required=True)
parser.add_argument("--json", action="store_true")
args = parser.parse_args()

with open(args.event, "r", encoding="utf-8") as fh:
    event = json.load(fh)

records = []
if os.path.exists(args.state):
    with open(args.state, "r", encoding="utf-8") as fh:
        records = json.load(fh)

key = "{}:{}".format(
    event.get("target_id", "unknown_target"),
    event.get("deployment_id", "unknown_deployment"),
)
if not any(r.get("incident_key") == key for r in records):
    record = dict(event)
    record["incident_key"] = key
    records.append(record)

tmp = args.state + ".tmp"
with open(tmp, "w", encoding="utf-8") as fh:
    json.dump(records, fh)
os.replace(tmp, args.state)

if args.json:
    print(json.dumps(records[-1]))
PY
fi

export GUARDIAN_NOTIFY_PY="$NOTIFY_PY"
export GUARDIAN_STATE="$STATE_FILE"

# Test 1: Record one incident through the real notify.py CLI.
bash scripts/main-incident.sh --sha abcdef --run-url http://ci.local --state "$STATE_FILE"

if [[ ! -s "$STATE_FILE" ]]; then
  echo "Expected a non-empty state file after recording an incident"
  exit 1
fi

# The event must reach notify.py as a file (not stdin) and the record must be a
# contract IncidentRecord with a severity inside the enum.
if ! jq -e 'type == "array" and length == 1' "$STATE_FILE" >/dev/null; then
  echo "Expected exactly one recorded incident"
  exit 1
fi
if ! jq -e 'all(.[]; .severity == "critical" or .severity == "warning" or .severity == "info")' "$STATE_FILE" >/dev/null; then
  echo "severity outside the contract enum {critical,warning,info}"
  exit 1
fi
if ! jq -e 'all(.[]; .opened_at != null and (.opened_at | test("Z$")))' "$STATE_FILE" >/dev/null; then
  echo "opened_at must be an ISO-8601 UTC timestamp ending in Z"
  exit 1
fi

# Test 2: The same SHA twice records one incident (dedupe).
bash scripts/main-incident.sh --sha abcdef --run-url http://ci.local --state "$STATE_FILE"

# Test 3: A second distinct SHA adds one.
bash scripts/main-incident.sh --sha 123456 --run-url http://ci.local --state "$STATE_FILE"

COUNT=$(jq 'length' "$STATE_FILE" || echo 0)
if [[ "$COUNT" -ne 2 ]]; then
  echo "Expected 2 incidents, got $COUNT"
  exit 1
fi

# Test 4: Malformed arguments exit 2 with no traceback.
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
