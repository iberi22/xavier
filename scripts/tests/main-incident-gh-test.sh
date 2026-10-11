#!/bin/bash
set -euo pipefail

export HEAD_SHA="abcdef1234567890abcdef1234567890abcdef12"
export RUN_ID="987654321"
export RUN_URL="https://github.com/iberi22/xavier/actions/runs/987654321"
export GITHUB_REPOSITORY="iberi22/xavier"
export WORKFLOW_NAME="Test Workflow"

TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT
export PATH="$TEMP_DIR:$PATH"

export MOCK_GH="$TEMP_DIR/gh"
export GH_LOG="$TEMP_DIR/gh_log"

export MOCK_GIT="$TEMP_DIR/git"
export GIT_LOG="$TEMP_DIR/git_log"

cat << 'MOCK_EOF' > "$MOCK_GH"
#!/bin/bash
echo "gh $@" >> "$GH_LOG"
if [[ "$*" == *"label create tbd-incident"* ]]; then
  return 0 2>/dev/null || false
elif [[ "$*" == *"issue list"* ]]; then
  echo "$MOCK_OPEN_ISSUES"
elif [[ "$*" == *"issue create"* ]]; then
  echo "https://github.com/iberi22/xavier/issues/42"
elif [[ "$*" == *"issue comment"* ]]; then
  return 0 2>/dev/null || false
elif [[ "$*" == *"issue close"* ]]; then
  return 0 2>/dev/null || false
elif [[ "$*" == *"issue list"* && "$*" != *"in:body"* ]]; then
  echo "$MOCK_OPEN_ISSUES"
else
  return 0 2>/dev/null || false
fi
MOCK_EOF
chmod +x "$MOCK_GH"
sed -i 's/return 0 2>\/dev\/null || false/exit 0/g' "$MOCK_GH"

cat << 'MOCK_EOF' > "$MOCK_GIT"
#!/bin/bash
echo "git $@" >> "$GIT_LOG"
if [[ "$*" == *"log -1"* ]]; then
  echo "testuser"
elif [[ "$*" == *"merge-base --is-ancestor"* ]]; then
  if [[ "$MOCK_IS_ANCESTOR" == "true" ]]; then
    return 0 2>/dev/null || false
  else
    exit 1
  fi
else
  return 0 2>/dev/null || false
fi
MOCK_EOF
chmod +x "$MOCK_GIT"
sed -i 's/return 0 2>\/dev\/null || false/exit 0/g' "$MOCK_GIT"
sed -i 's/return 1 2>\/dev\/null || false/exit 1/g' "$MOCK_GIT"

SCRIPT_TO_TEST="$(pwd)/scripts/main-incident-gh.sh"

echo "Test 1: Simulated main failure creates one incident"
rm -f "$GH_LOG" "$GIT_LOG"
touch "$GH_LOG" "$GIT_LOG"
export CONCLUSION="failure"
export MOCK_EXISTING_ISSUE="null"
bash "$SCRIPT_TO_TEST"
if ! grep -q "issue create" "$GH_LOG"; then
  echo "Test 1 failed: Issue was not created"
  cat "$GH_LOG"
  exit 1
fi
if ! grep -q "main red: Test Workflow @ abcdef1" "$GH_LOG"; then
  echo "Test 1 failed: Title was incorrect"
  cat "$GH_LOG"
  exit 1
fi
echo "Test 1 passed"

echo "Test 2: Repeated event deduplicates (same SHA)"
rm -f "$GH_LOG" "$GIT_LOG"
touch "$GH_LOG" "$GIT_LOG"
export CONCLUSION="failure"
export MOCK_OPEN_ISSUES='[{"number": 42, "body": "<!-- tbd-incident sha=abcdef1234567890abcdef1234567890abcdef12 workflow=Test Workflow -->"}]'
bash "$SCRIPT_TO_TEST"
if ! grep -q "issue comment 42" "$GH_LOG"; then
  echo "Test 2 failed: Did not comment on existing issue"
  cat "$GH_LOG"
  exit 1
fi
if grep -q "issue create" "$GH_LOG"; then
  echo "Test 2 failed: Created issue instead of deduplicating"
  cat "$GH_LOG"
  exit 1
fi
echo "Test 2 passed"

echo "Test 3: Green of the SAME workflow on a descendant SHA -> close"
rm -f "$GH_LOG" "$GIT_LOG"
touch "$GH_LOG" "$GIT_LOG"
export CONCLUSION="success"
export MOCK_OPEN_ISSUES='[{"number": 42, "body": "CI failed on main in run 987654321.\nOwner: testuser\nRun URL: https://github.com/iberi22/xavier/actions/runs/987654321\nFix/revert tracking: Please link a green SHA or PR to fix this.\n<!-- tbd-incident sha=abcdef1234567890abcdef1234567890abcdef12 workflow=Test Workflow -->"}]'
export MOCK_IS_ANCESTOR="true"
bash "$SCRIPT_TO_TEST"
if ! grep -q "issue close 42" "$GH_LOG"; then
  echo "Test 3 failed: Did not close descendant SHA issue"
  cat "$GH_LOG"
  exit 1
fi
echo "Test 3 passed"

echo "Test 4: Green of a DIFFERENT workflow -> nothing closed"
rm -f "$GH_LOG" "$GIT_LOG"
touch "$GH_LOG" "$GIT_LOG"
export CONCLUSION="success"
export MOCK_OPEN_ISSUES='[{"number": 42, "body": "CI failed on main in run 987654321.\nOwner: testuser\nRun URL: https://github.com/iberi22/xavier/actions/runs/987654321\nFix/revert tracking: Please link a green SHA or PR to fix this.\n<!-- tbd-incident sha=abcdef1234567890abcdef1234567890abcdef12 workflow=Other Workflow -->"}]'
export MOCK_IS_ANCESTOR="true"
bash "$SCRIPT_TO_TEST"
if grep -q "issue close 42" "$GH_LOG"; then
  echo "Test 4 failed: Closed issue of a different workflow"
  cat "$GH_LOG"
  exit 1
fi
echo "Test 4 passed"

echo "Test 5: Green on a non-descendant SHA -> nothing closed"
rm -f "$GH_LOG" "$GIT_LOG"
touch "$GH_LOG" "$GIT_LOG"
export CONCLUSION="success"
export MOCK_OPEN_ISSUES='[{"number": 42, "body": "CI failed on main in run 987654321.\nOwner: testuser\nRun URL: https://github.com/iberi22/xavier/actions/runs/987654321\nFix/revert tracking: Please link a green SHA or PR to fix this.\n<!-- tbd-incident sha=abcdef1234567890abcdef1234567890abcdef12 workflow=Test Workflow -->"}]'
export MOCK_IS_ANCESTOR="false"
bash "$SCRIPT_TO_TEST"
if grep -q "issue close 42" "$GH_LOG"; then
  echo "Test 5 failed: Closed issue despite non-descendant SHA"
  cat "$GH_LOG"
  exit 1
fi
echo "Test 5 passed"


echo "Test 6: Two open incidents for different workflows on the same SHA -> comment goes to matching"
rm -f "$GH_LOG" "$GIT_LOG"
touch "$GH_LOG" "$GIT_LOG"
export CONCLUSION="failure"
export MOCK_OPEN_ISSUES='[{"number": 42, "body": "<!-- tbd-incident sha=abcdef1234567890abcdef1234567890abcdef12 workflow=Test Workflow -->"}, {"number": 43, "body": "<!-- tbd-incident sha=abcdef1234567890abcdef1234567890abcdef12 workflow=Other Workflow -->"}]'
bash "$SCRIPT_TO_TEST"
if ! grep -q "issue comment 42" "$GH_LOG"; then
  echo "Test 6 failed: Did not comment on correct issue"
  cat "$GH_LOG"
  exit 1
fi
if grep -q "issue comment 43" "$GH_LOG"; then
  echo "Test 6 failed: Commented on incorrect workflow issue"
  cat "$GH_LOG"
  exit 1
fi
echo "Test 6 passed"

echo "Test 7: YAML shell check - RUN_EVENT=pull_request -> no action"
rm -f "$GH_LOG" "$GIT_LOG"
touch "$GH_LOG" "$GIT_LOG"
export CONCLUSION="failure"
export RUN_EVENT="pull_request"
export HEAD_REPO="$GITHUB_REPOSITORY"
export HEAD_BRANCH="main"
export MOCK_OPEN_ISSUES="[]"
# Simulate the YAML shell check
if [ "$HEAD_BRANCH" = "main" ] && [ "$RUN_EVENT" = "push" ] && [ "$HEAD_REPO" = "$GITHUB_REPOSITORY" ]; then
  bash "$SCRIPT_TO_TEST"
fi
if grep -q "issue create" "$GH_LOG"; then
  echo "Test 7 failed: Script was called"
  cat "$GH_LOG"
  exit 1
fi
echo "Test 7 passed"

echo "Test 8: YAML shell check - HEAD_REPO is a fork -> no action"
rm -f "$GH_LOG" "$GIT_LOG"
touch "$GH_LOG" "$GIT_LOG"
export CONCLUSION="failure"
export RUN_EVENT="push"
export HEAD_REPO="someone/fork"
export HEAD_BRANCH="main"
export MOCK_OPEN_ISSUES="[]"
if [ "$HEAD_BRANCH" = "main" ] && [ "$RUN_EVENT" = "push" ] && [ "$HEAD_REPO" = "$GITHUB_REPOSITORY" ]; then
  bash "$SCRIPT_TO_TEST"
fi
if grep -q "issue create" "$GH_LOG"; then
  echo "Test 8 failed: Script was called"
  cat "$GH_LOG"
  exit 1
fi
echo "Test 8 passed"

echo "All tests passed."
