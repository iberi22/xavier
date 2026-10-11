#!/bin/bash
set -euo pipefail

TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT
export PATH="$TEMP_DIR:$PATH"

export MOCK_GH="$TEMP_DIR/gh"
export GH_LOG="$TEMP_DIR/gh_log"

cat << 'MOCK_EOF' > "$MOCK_GH"
#!/bin/bash
echo "gh $@" >> "$GH_LOG"
if [[ "$*" == *"repos/iberi22/xavier/rulesets/1"* ]]; then
  echo '{"id": 1, "name": "r1", "rules": [{"type": "deletion"}, {"type": "non_fast_forward"}]}'
elif [[ "$*" == *"repos/iberi22/xavier/rulesets/2"* ]]; then
  echo '{"id": 2, "name": "ruleone", "rules": [{"type": "deletion"}, {"type": "non_fast_forward"}]}'
elif [[ "$*" == *"repos/iberi22/xavier/rulesets/3"* ]]; then
  echo '{"id": 3, "name": "foreign", "rules": [{"type": "deletion"}, {"type": "pull_request"}]}'
elif [[ "$*" == *"repos/iberi22/xavier/rulesets"*"--jq"* ]]; then
  if [[ "$MOCK_RULESETS" == "empty" ]]; then
    echo "[]"
  elif [[ "$MOCK_RULESETS" == "duplicates" ]]; then
    echo '[{"id": 1, "name": "r1", "rules": [{"type": "deletion"}, {"type": "non_fast_forward"}]}, {"id": 2, "name": "ruleone", "rules": [{"type": "deletion"}, {"type": "non_fast_forward"}]}, {"id": 3, "name": "foreign", "rules": [{"type": "deletion"}, {"type": "pull_request"}]}]'
  elif [[ "$MOCK_RULESETS" == "idempotent" ]]; then
    echo '[{"id": 4, "name": "main-protection", "rules": [{"type": "deletion"}]}]'
  fi
else
  return 0 2>/dev/null || false
fi
MOCK_EOF
chmod +x "$MOCK_GH"
sed -i 's/return 0 2>\/dev\/null || false/exit 0/g' "$MOCK_GH"

SCRIPT_TO_TEST="$(pwd)/scripts/apply-main-ruleset.sh"

test_ 1: dry-run makes 0 POST/PUT/DELETE/PATCH calls"
rm -f "$GH_LOG"
touch "$GH_LOG"
export MOCK_RULESETS="duplicates"
bash "$SCRIPT_TO_TEST"
if grep -q -E "POST|PUT|DELETE|PATCH" "$GH_LOG"; then
  test_ 1 failed: dry-run made mutating calls"
  cat "$GH_LOG"
  exit 1
fi
test_ 1 passed"

test_ 2: --execute creates one POST and deletes r1/ruleone, ignores foreign"
rm -f "$GH_LOG"
touch "$GH_LOG"
export MOCK_RULESETS="duplicates"
bash "$SCRIPT_TO_TEST" --execute
if [ $(grep -c "POST repos/iberi22/xavier/rulesets" "$GH_LOG") -ne 1 ]; then
  test_ 2 failed: POST count != 1"
  cat "$GH_LOG"
  exit 1
fi
if [ $(grep -c "DELETE repos/iberi22/xavier/rulesets/1" "$GH_LOG") -ne 1 ]; then
  test_ 2 failed: Did not delete r1"
  cat "$GH_LOG"
  exit 1
fi
if [ $(grep -c "DELETE repos/iberi22/xavier/rulesets/2" "$GH_LOG") -ne 1 ]; then
  test_ 2 failed: Did not delete ruleone"
  cat "$GH_LOG"
  exit 1
fi
if grep -q "DELETE repos/iberi22/xavier/rulesets/3" "$GH_LOG"; then
  test_ 2 failed: Deleted foreign ruleset"
  cat "$GH_LOG"
  exit 1
fi
test_ 2 passed"

test_ 3: --execute on existing main-protection uses PUT (idempotence)"
rm -f "$GH_LOG"
touch "$GH_LOG"
export MOCK_RULESETS="idempotent"
bash "$SCRIPT_TO_TEST" --execute
if [ $(grep -c "PUT repos/iberi22/xavier/rulesets/4" "$GH_LOG") -ne 1 ]; then
  test_ 3 failed: PUT count != 1"
  cat "$GH_LOG"
  exit 1
fi
if grep -q "POST" "$GH_LOG"; then
  test_ 3 failed: Made POST instead of PUT"
  cat "$GH_LOG"
  exit 1
fi
test_ 3 passed"

test_ 4: --execute --enable-auto-merge makes a PATCH call"
rm -f "$GH_LOG"
touch "$GH_LOG"
export MOCK_RULESETS="empty"
bash "$SCRIPT_TO_TEST" --execute --enable-auto-merge
if ! grep -q "PATCH repos/iberi22/xavier" "$GH_LOG"; then
  test_ 4 failed: Missing PATCH call"
  cat "$GH_LOG"
  exit 1
fi
test_ 4 passed"

echo "All tests passed."
