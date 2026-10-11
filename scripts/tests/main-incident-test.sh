#!/bin/bash
set -euo pipefail

export HEAD_SHA="abcdef123456"
export RUN_ID="987654321"
export RUN_URL="https://github.com/iberi22/xavier/actions/runs/987654321"
export GITHUB_REPOSITORY="iberi22/xavier"

TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT
export PATH="$TEMP_DIR:$PATH"

export MOCK_GH="$TEMP_DIR/gh"
export GH_LOG="$TEMP_DIR/gh_log"

cat << 'MOCK_EOF' > "$MOCK_GH"
#!/bin/bash
echo "gh $@" >> "$GH_LOG"
if [[ "$*" == *"label create main-red"* ]]; then
  return 0 2>/dev/null || false
elif [[ "$*" == *"issue list"* && "$*" == *"$HEAD_SHA"* ]]; then
  echo "$MOCK_EXISTING_ISSUE"
elif [[ "$*" == *"api repos/iberi22/xavier/commits/"* ]]; then
  echo "testuser"
elif [[ "$*" == *"issue create"* ]]; then
  echo "https://github.com/iberi22/xavier/issues/42"
elif [[ "$*" == *"issue comment"* ]]; then
  return 0 2>/dev/null || false
elif [[ "$*" == *"issue close"* ]]; then
  return 0 2>/dev/null || false
elif [[ "$*" == *"issue list"* && "$*" != *"$HEAD_SHA"* ]]; then
  echo "$MOCK_OPEN_ISSUES"
else
  return 0 2>/dev/null || false
fi
MOCK_EOF
chmod +x "$MOCK_GH"
sed -i 's/return 0 2>\/dev\/null || false/exit 0/g' "$MOCK_GH"

SCRIPT_TO_TEST="$(pwd)/scripts/main-incident.sh"

echo "Test 1: Simulated main failure creates one incident"
rm -f "$GH_LOG"
touch "$GH_LOG"
export CONCLUSION="failure"
export MOCK_EXISTING_ISSUE="null"
bash "$SCRIPT_TO_TEST"
if ! grep -q "issue create --title CI Failure on main: abcdef123456" "$GH_LOG"; then
  echo "Test 1 failed"
  exit 1
fi
echo "Test 1 passed"

echo "Test 2: Repeated event deduplicates"
rm -f "$GH_LOG"
touch "$GH_LOG"
export CONCLUSION="failure"
export MOCK_EXISTING_ISSUE="42"
bash "$SCRIPT_TO_TEST"
if ! grep -q "issue comment 42" "$GH_LOG"; then
  echo "Test 2 failed"
  exit 1
fi
echo "Test 2 passed"

echo "Test 3: Incident closes only after linked green fix/revert SHA"
rm -f "$GH_LOG"
touch "$GH_LOG"
export CONCLUSION="success"
export MOCK_OPEN_ISSUES="42
43"
bash "$SCRIPT_TO_TEST"
if ! grep -q "issue close 42" "$GH_LOG"; then
  echo "Test 3 failed"
  exit 1
fi
echo "Test 3 passed"
echo "All tests passed."
