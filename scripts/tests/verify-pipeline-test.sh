#!/usr/bin/env bash
set -euo pipefail

REAL_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_DIR="$(mktemp -d)"
trap 'rm -rf "$TEST_DIR"' EXIT

# Setup mock repo root
MOCK_ROOT="$TEST_DIR/repo"
mkdir -p "$MOCK_ROOT/.gitcore"
mkdir -p "$MOCK_ROOT/crates"
mkdir -p "$MOCK_ROOT/src"
mkdir -p "$MOCK_ROOT/code-graph"
cat << 'TOML' > "$MOCK_ROOT/Cargo.toml"
[[test]]
name = "dummy"
TOML

cat << 'JSON' > "$MOCK_ROOT/.gitcore/features.json"
{
  "features": {
    "feat-1": {
      "id": "feat-1",
      "status": "stable",
      "implemented_in": [],
      "tests": ["dummy::test1"]
    },
    "feat-2": {
      "id": "feat-2",
      "status": "beta",
      "implemented_in": [],
      "tests": ["dummy::test2"]
    }
  }
}
JSON

# Modify verify-pipeline.sh to allow overriding ROOT, or we can just copy it and change ROOT
cp "$REAL_ROOT/scripts/verify-pipeline.sh" "$TEST_DIR/verify-pipeline.sh"
sed -i 's|ROOT="$(cd "$(dirname "${BASH_SOURCE\[0\]}")/.." && pwd)"|ROOT="'"$MOCK_ROOT"'"|' "$TEST_DIR/verify-pipeline.sh"
# Also need to fix the mode parsing if necessary? No.
# Also fix python calls that need the real root for scripts? No, python is just parsing json.

echo "=========================================================="
echo "Test Case 1: Fail-fast on -p xavier --lib"
mkdir -p "$TEST_DIR/bin1"
cat << 'BIN' > "$TEST_DIR/bin1/cargo"
#!/usr/bin/env bash
if [[ "$*" == *"test -p xavier --lib --no-run"* ]]; then
  echo "error: could not compile \`xavier\` due to previous error"
  echo "  = note: some linker error occurred"
  return 101 2>/dev/null || false
fi
return 0 2>/dev/null || true
BIN
chmod +x "$TEST_DIR/bin1/cargo"
sed -i 's/return 101 2>\/dev\/null || false/exit 101/' "$TEST_DIR/bin1/cargo"
sed -i 's/return 0 2>\/dev\/null || true/exit 0/' "$TEST_DIR/bin1/cargo"

(
  export PATH="$TEST_DIR/bin1:$PATH"
  export XAVIER_VERIFY_PER_ATTEMPT_SECS=1
  OUT="$(bash "$TEST_DIR/verify-pipeline.sh" 2>&1)" || RC=$?
  if [ "${RC:-0}" -ne 1 ]; then
    echo "FAIL: Expected exit code 1, got ${RC:-0}"
    exit 1
  fi
  if ! echo "$OUT" | grep -q 'build failed before any declared test could run'; then
    echo "FAIL: Missing fail-fast message"
    echo "$OUT"
    exit 1
  fi
  if ! echo "$OUT" | grep -q 'error: could not compile `xavier`'; then
    echo "FAIL: Missing error line in output"
    echo "$OUT"
    exit 1
  fi
  echo "PASS: Test Case 1"
)

echo "=========================================================="
echo "Test Case 2: Success path intact"
mkdir -p "$TEST_DIR/bin2"
cat << 'BIN' > "$TEST_DIR/bin2/cargo"
#!/usr/bin/env bash
if [[ "$*" == *"--no-run"* ]]; then
  return 0 2>/dev/null || true
fi
# For try_cargo testing, we return 1 test passed
echo "test result: ok. 1 passed; 0 failed"
return 0 2>/dev/null || true
BIN
chmod +x "$TEST_DIR/bin2/cargo"
sed -i 's/return 0 2>\/dev\/null || true/exit 0/g' "$TEST_DIR/bin2/cargo"

(
  export PATH="$TEST_DIR/bin2:$PATH"
  export XAVIER_VERIFY_PER_ATTEMPT_SECS=1
  OUT="$(bash "$TEST_DIR/verify-pipeline.sh" 2>&1)" || RC=$?
  if [ "${RC:-0}" -ne 0 ]; then
    echo "FAIL: Expected exit code 0, got ${RC:-0}"
    echo "$OUT"
    exit 1
  fi
  echo "PASS: Test Case 2"
)

echo "=========================================================="
echo "Test Case 3: try_cargo deduplication"
mkdir -p "$TEST_DIR/bin3"
cat << 'BIN' > "$TEST_DIR/bin3/cargo"
#!/usr/bin/env bash
if [[ "$*" == *"--no-run"* ]]; then
  return 0 2>/dev/null || true
fi
# Always fail the test runs with a consistent error
echo "error: [E0308] mismatched types"
echo "  = note: expected type \`i32\`"
echo "  = note: found type \`u32\`"
return 101 2>/dev/null || false
BIN
chmod +x "$TEST_DIR/bin3/cargo"
sed -i 's/return 0 2>\/dev\/null || true/exit 0/g' "$TEST_DIR/bin3/cargo"
sed -i 's/return 101 2>\/dev\/null || false/exit 101/g' "$TEST_DIR/bin3/cargo"

(
  export PATH="$TEST_DIR/bin3:$PATH"
  export XAVIER_VERIFY_PER_ATTEMPT_SECS=1
  # This run should fail the pipeline because tests fail
  OUT="$(bash "$TEST_DIR/verify-pipeline.sh" 2>&1)" || RC=$?
  if [ "${RC:-0}" -eq 0 ]; then
    echo "FAIL: Expected pipeline to fail, got exit code 0"
    exit 1
  fi
  # The error message should appear exactly once in OUT
  ERR_COUNT="$(echo "$OUT" | grep -c 'error: \[E0308\]' || true)"
  if [ "$ERR_COUNT" -ne 1 ]; then
    echo "FAIL: Expected deduplicated error to appear exactly once, found $ERR_COUNT"
    echo "$OUT"
    exit 1
  fi
  echo "PASS: Test Case 3"
)

echo "=========================================================="
echo "Test Case 4: check-only mode"
(
  # No need for fake cargo here as it shouldn't execute tests
  OUT="$(bash "$TEST_DIR/verify-pipeline.sh" --check-only 2>&1)" || RC=$?
  if [ "${RC:-0}" -ne 0 ]; then
    echo "FAIL: Expected check-only to succeed, got ${RC:-0}"
    echo "$OUT"
    exit 1
  fi
  if echo "$OUT" | grep -q 'warming test binaries'; then
    echo "FAIL: Should not warm binaries in check-only mode"
    echo "$OUT"
    exit 1
  fi
  if ! echo "$OUT" | grep -q 'Computing implementation score'; then
    echo "FAIL: Should run steps 1-3"
    echo "$OUT"
    exit 1
  fi
  echo "PASS: Test Case 4"
)

echo "=========================================================="
echo "Test Case 5: Warmup failure with no matching lines continues anyway"
mkdir -p "$TEST_DIR/bin5"
cat << 'BIN' > "$TEST_DIR/bin5/cargo"
#!/usr/bin/env bash
if [[ "$*" == *"test -p code-graph --lib --no-run"* ]]; then
  echo "warning: arbitrary failure with no matching keywords"
  exit 1
fi
if [[ "$*" == *"--no-run"* ]]; then
  exit 0
fi
echo "test result: ok. 1 passed; 0 failed"
exit 0
BIN
chmod +x "$TEST_DIR/bin5/cargo"

(
  export PATH="$TEST_DIR/bin5:$PATH"
  export XAVIER_VERIFY_PER_ATTEMPT_SECS=1
  OUT="$(bash "$TEST_DIR/verify-pipeline.sh" 2>&1)" || RC=$?
  if [ "${RC:-0}" -ne 0 ]; then
    echo "FAIL: Expected exit code 0, got ${RC:-0}"
    echo "$OUT"
    exit 1
  fi
  if ! echo "$OUT" | grep -q 'warmup failed for: -p code-graph --lib (continuing anyway)'; then
    echo "FAIL: Missing continuation warning"
    echo "$OUT"
    exit 1
  fi
  if ! echo "$OUT" | grep -q 'warning: arbitrary failure with no matching keywords'; then
    echo "FAIL: Missing fallback tail output"
    echo "$OUT"
    exit 1
  fi
  echo "PASS: Test Case 5"
)

echo "=========================================================="
echo "Test Case 6: Nested integration files use the integration target"
echo "Test Case 7: Code-graph integration and unit tests use their owning targets"
mkdir -p "$MOCK_ROOT/tests/integration" "$MOCK_ROOT/code-graph/tests" "$MOCK_ROOT/code-graph/src" "$TEST_DIR/bin6"
cp "$REAL_ROOT/tests/integration/agents_test.rs" "$MOCK_ROOT/tests/integration/"
cp "$REAL_ROOT/tests/integration.rs" "$MOCK_ROOT/tests/"
cp "$REAL_ROOT/code-graph/tests/fingerprint.rs" "$MOCK_ROOT/code-graph/tests/"
cp "$REAL_ROOT/code-graph/src/fingerprint.rs" "$MOCK_ROOT/code-graph/src/"
python3 - "$REAL_ROOT" "$MOCK_ROOT" "$TEST_DIR/expected-invocations" << 'PY'
import json, sys
from pathlib import Path

real, mock, expected = map(Path, sys.argv[1:])
feature = json.loads((real / ".gitcore/features.json").read_text())["features"]["feat-code-graph-fingerprint"]
tests = ["tests/integration/agents_test.rs::test_agent_creation", *feature["tests"]]
(mock / ".gitcore/features.json").write_text(json.dumps({"features": [{
    "id": "routing", "status": "in_progress", "tests": tests,
}]}))
commands = ["test -p xavier --features test-utils --test integration test_agent_creation"]
for test in feature["tests"]:
    if test.startswith("code-graph/tests/fingerprint.rs::"):
        commands.append("test -p code-graph --test fingerprint " + test.split("::")[-1])
    else:
        commands.append("test -p code-graph --lib " + test)
expected.write_text("\n".join(commands) + "\n")
PY
cat << 'BIN' > "$TEST_DIR/bin6/cargo"
#!/usr/bin/env bash
if [[ "$*" == *"--no-run"* ]]; then
  exit 0
fi
printf '%s\n' "$*" >> "$INVOCATIONS"
if ! grep -Fxq -- "$*" "$EXPECTED_INVOCATIONS"; then
  echo "error: unexpected invocation: $*"
  exit 1
fi
echo "test result: ok. 1 passed; 0 failed"
BIN
chmod +x "$TEST_DIR/bin6/cargo"
(
  export PATH="$TEST_DIR/bin6:$PATH"
  export INVOCATIONS="$TEST_DIR/invocations"
  export EXPECTED_INVOCATIONS="$TEST_DIR/expected-invocations"
  export XAVIER_VERIFY_PER_ATTEMPT_SECS=1
  OUT="$(bash "$TEST_DIR/verify-pipeline.sh" 2>&1)" || RC=$?
  if [ "${RC:-0}" -ne 0 ]; then
    echo "FAIL: Expected routing cases to succeed, got ${RC:-0}"
    echo "$OUT"
    exit 1
  fi
  if ! diff -u "$EXPECTED_INVOCATIONS" "$INVOCATIONS"; then
    echo "FAIL: Expected exact target and filter invocations without fallback sweeps"
    exit 1
  fi
  echo "PASS: Test Case 6"
  echo "PASS: Test Case 7"
)

echo "All verify-pipeline tests passed"
