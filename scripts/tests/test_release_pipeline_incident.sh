#!/bin/bash
# Offline contract checks for scripts/release-pipeline-incident.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPT="$ROOT/scripts/release-pipeline-incident.sh"
WF="$ROOT/.github/workflows/release-watch.yml"
WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT

assert_file() { [ -e "$1" ] || { echo "missing $1"; exit 1; }; }
assert_contains() { grep -Fq -- "$2" "$1" || { echo "expected '$2' in $1"; exit 1; }; }
assert_not_contains() { if grep -Fq -- "$2" "$1"; then echo "unexpected '$2' in $1"; exit 1; fi; }
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

assert_contains "$SCRIPT" 'LOOKUP_RC'
assert_contains "$SCRIPT" 'aborting without mutations'
assert_contains "$SCRIPT" '--search'
# Must not require the advisory label for identity
if grep -n 'gh issue list --label' "$SCRIPT"; then
  echo "lookup still depends on label filter"; exit 1
fi
if grep -q 'find_open_issue || true' "$SCRIPT"; then
  echo "found fail-open suppression"; exit 1
fi

set +e
CONCLUSION= HEAD_SHA= RUN_ID= RUN_URL= WORKFLOW_NAME= bash "$SCRIPT" >"$WORKDIR/out0.txt" 2>"$WORKDIR/err0.txt"
RC0=$?
set -e
[ "$RC0" -ne 0 ] || { echo "expected missing-env failure"; exit 1; }
assert_contains "$WORKDIR/err0.txt" 'CONCLUSION'

cat > "$WORKDIR/gh" <<'GH'
#!/bin/bash
echo "gh $*" >> "$(dirname "$0")/gh.log"
case "$1 $2" in
  "label create") exit 0 ;;
  "issue list")
    # Fail closed path for API errors
    if [[ "$*" == *"--search"* ]]; then
      echo "simulated GitHub list failure" >&2
      exit 1
    fi
    echo "[]"
    exit 0
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

# When search fails, script falls back to gh api pagination — make that fail too
# by also providing a failing `gh api` via same binary.
cat > "$WORKDIR/gh" <<'GH'
#!/bin/bash
LOGDIR="$(dirname "$0")"
echo "gh $*" >> "$LOGDIR/gh.log"
if [ "$1" = "label" ] && [ "$2" = "create" ]; then exit 0; fi
if [ "$1" = "issue" ] && [ "$2" = "list" ]; then
  echo "simulated GitHub list failure" >&2
  exit 1
fi
if [ "$1" = "api" ]; then
  echo "simulated GitHub api failure" >&2
  exit 1
fi
if [ "$1" = "issue" ] && { [ "$2" = "create" ] || [ "$2" = "comment" ] || [ "$2" = "close" ]; }; then
  echo "MUTATION:$1 $2" >> "$LOGDIR/mutations.log"
  exit 0
fi
echo "unexpected gh invocation: $*" >&2
exit 99
GH
chmod +x "$WORKDIR/gh"

set +e
env PATH="$WORKDIR:$PATH" GITHUB_REPOSITORY='southwest-ai-labs/xavier' \
  CONCLUSION=failure HEAD_SHA=abc1234 RUN_ID=1 RUN_URL=https://example.test \
  WORKFLOW_NAME='Release & Multi-Platform Binaries' \
  bash "$SCRIPT" >"$WORKDIR/out1.txt" 2>"$WORKDIR/err1.txt"
RC1=$?
set -e
[ "$RC1" -ne 0 ] || { echo "expected lookup failure to abort"; cat "$WORKDIR/out1.txt" "$WORKDIR/err1.txt"; exit 1; }
assert_contains "$WORKDIR/err1.txt" 'aborting without mutations'
assert_eq "$(wc -l < "$WORKDIR/mutations.log" | tr -d ' ')" "0"

# Happy path: marker match without label, then create when absent
cat > "$WORKDIR/gh" <<'GH'
#!/bin/bash
LOGDIR="$(dirname "$0")"
echo "gh $*" >> "$LOGDIR/gh.log"
if [ "$1" = "label" ] && [ "$2" = "create" ]; then exit 0; fi
if [ "$1" = "issue" ] && [ "$2" = "list" ]; then
  # Return an unlabeled issue that still carries the marker
  cat <<'JSON'
[{"number":42,"body":"old\n<!-- release-pipeline-failure:Release & Multi-Platform Binaries -->\n","labels":[]}]
JSON
  exit 0
fi
if [ "$1" = "issue" ] && [ "$2" = "comment" ]; then
  echo "MUTATION:comment:$3" >> "$LOGDIR/mutations.log"
  exit 0
fi
if [ "$1" = "issue" ] && [ "$2" = "create" ]; then
  echo "MUTATION:create" >> "$LOGDIR/mutations.log"
  exit 0
fi
if [ "$1" = "issue" ] && [ "$2" = "close" ]; then
  echo "MUTATION:close:$3" >> "$LOGDIR/mutations.log"
  exit 0
fi
echo "unexpected gh invocation: $*" >&2
exit 99
GH
chmod +x "$WORKDIR/gh"
: > "$WORKDIR/mutations.log"

env PATH="$WORKDIR:$PATH" GITHUB_REPOSITORY='southwest-ai-labs/xavier' \
  CONCLUSION=failure HEAD_SHA=abc1234 RUN_ID=9 RUN_URL=https://example.test \
  WORKFLOW_NAME='Release & Multi-Platform Binaries' \
  bash "$SCRIPT" >"$WORKDIR/out3.txt" 2>"$WORKDIR/err3.txt"
assert_contains "$WORKDIR/mutations.log" 'MUTATION:comment:42'
assert_not_contains "$WORKDIR/mutations.log" 'MUTATION:create'

env PATH="$WORKDIR:$PATH" GITHUB_REPOSITORY='southwest-ai-labs/xavier' \
  CONCLUSION=success HEAD_SHA=abc1234 RUN_ID=10 RUN_URL=https://example.test \
  WORKFLOW_NAME='Release & Multi-Platform Binaries' \
  bash "$SCRIPT" >"$WORKDIR/out4.txt" 2>"$WORKDIR/err4.txt"
assert_contains "$WORKDIR/mutations.log" 'MUTATION:close:42'

echo "OK: release-pipeline-incident contract"
