#!/bin/bash
# Offline contract checks for scripts/release-pipeline-incident.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPT="$ROOT/scripts/release-pipeline-incident.sh"
WF="$ROOT/.github/workflows/release-watch.yml"

test -x "$SCRIPT"
test -f "$WF"

grep -q 'Release & Multi-Platform Binaries' "$WF"
grep -q 'release-pipeline-incident.sh' "$WF"
grep -q 'workflow_run:' "$WF"
grep -q 'issues: write' "$WF"

# Marker must be stable on workflow name (not run id / sha) so upserts update.
MARKER_LINE=$(grep -E 'MARKER=' "$SCRIPT" | head -1)
echo "$MARKER_LINE" | grep -q 'release-pipeline-failure:'
echo "$MARKER_LINE" | grep -q 'WORKFLOW_NAME'
! echo "$MARKER_LINE" | grep -q 'RUN_ID'
! echo "$MARKER_LINE" | grep -q 'HEAD_SHA'

# Script must refuse missing env (determinism / fail-closed).
if CONCLUSION= HEAD_SHA= RUN_ID= RUN_URL= WORKFLOW_NAME= bash "$SCRIPT" 2>/tmp/rpi-err.txt; then
  echo "expected missing-env failure"; exit 1
fi
grep -q 'required' /tmp/rpi-err.txt || grep -q 'CONCLUSION' /tmp/rpi-err.txt

echo "OK: release-pipeline-incident contract"
