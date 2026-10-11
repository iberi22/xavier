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

echo "All verify-pipeline tests passed"
