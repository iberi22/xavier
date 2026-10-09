#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NOTIFY_PY="${GUARDIAN_NOTIFY_PY:-$SCRIPT_DIR/guardian/notify.py}"

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

# incident_key is "<target_id>:<deployment_id>", so the failing SHA is the
# deployment_id and each SHA dedupes to one IncidentRecord. severity must be a
# contract value ("critical"|"warning"|"info"), never "incident".
EVENT_JSON=$(jq -n \
  --arg target "main-ci" \
  --arg sha "$SHA" \
  --arg url "$RUN_URL" \
  --arg at "$OPENED_AT" \
  '{
    target_id: $target,
    deployment_id: $sha,
    kind: "release_tag",
    severity: "critical",
    state: "open",
    opened_at: $at,
    evidence: {failing_sha: $sha, run_url: $url, opened_at: $at}
  }')

export GUARDIAN_STATE="$STATE_FILE"

INCIDENT_KEY="main-ci:$SHA"
if [[ -f "$STATE_FILE" ]]; then
  if jq -e --arg key "$INCIDENT_KEY" \
      'if type == "array" then any(.[]; .incident_key == $key) else (.incident_key == $key) end' \
      "$STATE_FILE" >/dev/null 2>&1; then
    if [[ $JSON_OUT -eq 1 ]]; then
      echo '{"status": "deduplicated"}'
    fi
    exit 0
  fi
fi

EVENT_FILE=$(mktemp)
trap 'rm -f "${EVENT_FILE:-}"' EXIT
printf '%s' "$EVENT_JSON" > "$EVENT_FILE"

# notify.py takes the event as a file positional and --state; it never reads stdin.
if [[ $JSON_OUT -eq 1 ]]; then
  python3 "$NOTIFY_PY" "$EVENT_FILE" --state "$STATE_FILE" --json
  echo '{"status": "recorded"}'
else
  python3 "$NOTIFY_PY" "$EVENT_FILE" --state "$STATE_FILE"
fi

exit 0
