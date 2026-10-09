#!/bin/bash
set -euo pipefail

SHA=""
RUN_URL=""
STATE_FILE="${GUARDIAN_STATE:-$HOME/.xavier/guardian/incidents.json}"
JSON_OUT=0

while [[ $# -gt 0 ]]; do
  case $1 in
    --sha)
      SHA="$2"
      shift 2
      ;;
    --run-url)
      RUN_URL="$2"
      shift 2
      ;;
    --state)
      STATE_FILE="$2"
      shift 2
      ;;
    --json)
      JSON_OUT=1
      shift
      ;;
    *)
      echo "Malformed arguments" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$SHA" || -z "$RUN_URL" ]]; then
  echo "Malformed arguments" >&2
  exit 2
fi

OPENED_AT=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

EVENT_JSON=$(cat <<EOF
{
  "severity": "incident",
  "failing_sha": "$SHA",
  "run_url": "$RUN_URL",
  "opened_at": "$OPENED_AT"
}
EOF
)

# Use env to set GUARDIAN_STATE explicitly for notify.py to use it as its default or something
export GUARDIAN_STATE="$STATE_FILE"

# Deduplication needs to happen.
# Actually, the instructions say: "builds one incident event ... and records it by invoking python3 scripts/guardian/notify.py with the event on stdin ... it never calls gh directly ... Exit 0 when the incident is recorded".
# Does notify.py do the deduplication or does main-incident.sh need to do it?
# Let's read the instructions again: "the same SHA twice records one incident (dedupe); a second distinct SHA adds one"
# Since notify.py is an existing script (which we aren't allowed to touch), and the offline test for main-incident-test.sh says "the same SHA twice records one incident (dedupe)", maybe we need to dedupe before calling notify.py? Wait, if we call notify.py, it records it. If we call it twice, does it record it twice? The test says: "the same SHA twice records one incident (dedupe)", so if we run main-incident.sh twice with the same SHA, it should only record it once.
# Since we don't have notify.py in the tree (or we couldn't find it), let's implement deduplication in main-incident.sh by checking the state file first.

if [[ -f "$STATE_FILE" ]]; then
  if grep -q "\"failing_sha\": \"$SHA\"" "$STATE_FILE"; then
    # Already recorded
    if [[ $JSON_OUT -eq 1 ]]; then
      echo '{"status": "deduplicated"}'
    fi
    exit 0
  fi
fi

echo "$EVENT_JSON" | python3 scripts/guardian/notify.py

if [[ $JSON_OUT -eq 1 ]]; then
  echo '{"status": "recorded"}'
fi

exit 0
