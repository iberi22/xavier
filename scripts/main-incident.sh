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

export GUARDIAN_STATE="$STATE_FILE"

if [[ -f "$STATE_FILE" ]]; then
  if jq -e --arg sha "$SHA" 'if type == "array" then any(.[]; .failing_sha == $sha) else .failing_sha == $sha end' "$STATE_FILE" >/dev/null 2>&1; then
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
