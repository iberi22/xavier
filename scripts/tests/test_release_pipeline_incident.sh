#!/bin/bash
# Offline contract checks for scripts/release-pipeline-incident.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPT="$ROOT/scripts/release-pipeline-incident.sh"
WF="$ROOT/.github/workflows/release-watch.yml"
WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT

test -x "$SCRIPT"
test -f "$WF"

grep -q 'Release & Multi-Platform Binaries' "$WF"
grep -q 'release-pipeline-incident.sh' "$WF"
grep -q 'workflow_run:' "$WF"
grep -q 'issues: write' "$WF"

MARKER_LINE=$(grep -E 'MARKER=' "$SCRIPT" | head -1)
echo "$MARKER_LINE" | grep -q 'release-pipeline-failure:'
echo "$MARKER_LINE" | grep -q 'WORKFLOW_NAME'
! echo "$MARKER_LINE" | grep -q 'RUN_ID'
! echo "$MARKER_LINE" | grep -q 'HEAD_SHA'

# Must NOT suppress find_open_issue failures with || true
! grep -n 'find_open_issue || true' "$SCRIPT"

# Missing env fails closed
if CONCLUSION= HEAD_SHA= RUN_ID= RUN_URL= WORKFLOW_NAME= bash "$SCRIPT" 2>"$WORKDIR/err.txt"; then
  echo "expected missing-env failure"; exit 1
fi
grep -q 'CONCLUSION' "$WORKDIR/err.txt"

# API lookup failure must abort before create/comment (fail-closed)
cat > "$WORKDIR/gh" <<'GH'
#!/bin/bash
echo "simulated GitHub list failure" >&2
exit 1
GH
chmod +x "$WORKDIR/gh"
if env PATH="$WORKDIR:$PATH" \
  CONCLUSION=failure HEAD_SHA=abc1234 RUN_ID=1 RUN_URL=https://example.test \
  WORKFLOW_NAME='Release & Multi-Platform Binaries' \
  bash "$SCRIPT" >"$WORKDIR/out.txt" 2>"$WORKDIR/err2.txt"; then
  echo "expected gh list failure to abort"; exit 1
fi
grep -q 'simulated GitHub list failure' "$WORKDIR/err2.txt"
! grep -q 'Creating release-pipeline incident' "$WORKDIR/out.txt"
! grep -q 'Updating existing' "$WORKDIR/out.txt"

echo "OK: release-pipeline-incident contract"
