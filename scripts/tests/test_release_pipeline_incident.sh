#!/bin/bash
# Offline contract checks for scripts/release-pipeline-incident.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPT="$ROOT/scripts/release-pipeline-incident.sh"
WF="$ROOT/.github/workflows/release-watch.yml"
WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT

assert_file() { [ -e "$1" ] || { echo "missing $1"; exit 1; }; }
assert_contains() { grep -q "$2" "$1" || { echo "expected '$2' in $1"; exit 1; }; }
assert_not_contains() { if grep -q "$2" "$1"; then echo "unexpected '$2' in $1"; exit 1; fi; }
assert_eq() { [ "$1" = "$2" ] || { echo "expected '$2' got '$1'"; exit 1; }; }

assert_file "$SCRIPT"
assert_file "$WF"
[ -x "$SCRIPT" ] || { echo "script not executable"; exit 1; }

assert_contains "$WF" 'Release & Multi-Platform Binaries'
assert_contains "$WF" 'release-pipeline-incident.sh'
assert_contains "$WF" 'workflow_run:'
assert_contains "$WF" 'issues: write'

MARKER_LINE=$(grep -E 'MARKER=' "$SCRIPT" | head -1)
assert_contains <(echo "$MARKER_LINE") 'release-pipeline-failure:'
assert_contains <(echo "$MARKER_LINE") 'WORKFLOW_NAME'
assert_not_contains <(echo "$MARKER_LINE") 'RUN_ID'
assert_not_contains <(echo "$MARKER_LINE") 'HEAD_SHA'

# Must capture lookup status explicitly (bash assignment + set -e pitfall)
assert_contains "$SCRIPT" 'LOOKUP_RC'
assert_contains "$SCRIPT" 'aborting without mutations'
if grep -q 'find_open_issue || true' "$SCRIPT"; then
  echo "found fail-open suppression"; exit 1
fi

# Missing env fails closed
set +e
CONCLUSION= HEAD_SHA= RUN_ID= RUN_URL= WORKFLOW_NAME= bash "$SCRIPT" >"$WORKDIR/out0.txt" 2>"$WORKDIR/err0.txt"
RC0=$?
set -e
[ "$RC0" -ne 0 ] || { echo "expected missing-env failure"; exit 1; }
assert_contains "$WORKDIR/err0.txt" 'CONCLUSION'

# API lookup failure must abort BEFORE create/comment/close.
# Mock records attempted mutations but allows them so a lookup bug would show
# up as a create/close happening after a failed list.
cat > "$WORKDIR/gh" <<'GH'
#!/bin/bash
echo "gh $*" >> "$(dirname "$0")/gh.log"
case "$1 $2" in
  "label create") exit 0 ;;
  "issue list")
    echo "simulated GitHub list failure" >&2
    exit 1
    ;;
  "issue create"|"issue comment"|"issue close")
    echo "MUTATION:$1 $2" >> "$(dirname "$0")/mutations.log"
    exit 0
    ;;
  *)
    echo "unexpected gh invocation: $*" >&2
    exit 99
    ;;
esac
GH
chmod +x "$WORKDIR/gh"
: > "$WORKDIR/gh.log"
: > "$WORKDIR/mutations.log"

set +e
env PATH="$WORKDIR:$PATH" \
  CONCLUSION=failure HEAD_SHA=abc1234 RUN_ID=1 RUN_URL=https://example.test \
  WORKFLOW_NAME='Release & Multi-Platform Binaries' \
  bash "$SCRIPT" >"$WORKDIR/out1.txt" 2>"$WORKDIR/err1.txt"
RC1=$?
set -e
[ "$RC1" -ne 0 ] || { echo "expected lookup failure to abort"; cat "$WORKDIR/out1.txt" "$WORKDIR/err1.txt"; exit 1; }
assert_contains "$WORKDIR/err1.txt" 'simulated GitHub list failure'
assert_contains "$WORKDIR/err1.txt" 'aborting without mutations'
assert_eq "$(wc -l < "$WORKDIR/mutations.log" | tr -d ' ')" "0"

set +e
env PATH="$WORKDIR:$PATH" \
  CONCLUSION=success HEAD_SHA=abc1234 RUN_ID=2 RUN_URL=https://example.test \
  WORKFLOW_NAME='Release & Multi-Platform Binaries' \
  bash "$SCRIPT" >"$WORKDIR/out2.txt" 2>"$WORKDIR/err2.txt"
RC2=$?
set -e
[ "$RC2" -ne 0 ] || { echo "expected lookup failure on success path too"; exit 1; }
assert_eq "$(wc -l < "$WORKDIR/mutations.log" | tr -d ' ')" "0"

echo "OK: release-pipeline-incident contract"
