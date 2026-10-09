#!/bin/bash

set -e

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
DELIVERY_RUN="$DIR/../delivery-run.sh"

TEST_DIR=$(mktemp -d)
trap 'rm -rf "$TEST_DIR"' EXIT

echo "Created test dir: $TEST_DIR"

cat << 'INNER' > "$TEST_DIR/backend_hang_stdin.sh"
#!/bin/bash
cat # Hangs on stdin
INNER
chmod +x "$TEST_DIR/backend_hang_stdin.sh"

cat << 'INNER' > "$TEST_DIR/backend_zero_bytes.sh"
#!/bin/bash
exit 0
INNER
chmod +x "$TEST_DIR/backend_zero_bytes.sh"

cat << 'INNER' > "$TEST_DIR/backend_wall_exceed.sh"
#!/bin/bash
sleep 10
INNER
chmod +x "$TEST_DIR/backend_wall_exceed.sh"

cat << 'INNER' > "$TEST_DIR/backend_fail_nonzero.sh"
#!/bin/bash
echo "failing"
exit 1
INNER
chmod +x "$TEST_DIR/backend_fail_nonzero.sh"

cat << 'INNER' > "$TEST_DIR/backend_success.sh"
#!/bin/bash
echo "success"
exit 0
INNER
chmod +x "$TEST_DIR/backend_success.sh"

echo "Running tests..."

# Test 1: Hangs on stdin, then succeeds on backend B
rm -rf "$TEST_DIR/out1"
if ! bash "$DELIVERY_RUN" --backend-a "$TEST_DIR/backend_hang_stdin.sh" --backend-b "$TEST_DIR/backend_success.sh" --wall 3 --quiet 2 --out "$TEST_DIR/out1" -- arg1 arg2; then
  echo "Test 1 failed (should exit 0)"
  exit 1
fi
if ! grep -q "backend-a" "$TEST_DIR/out1/attempts.jsonl"; then
  echo "Test 1 attempts.jsonl missing backend-a"
  exit 1
fi
if ! grep -q "backend-b" "$TEST_DIR/out1/attempts.jsonl"; then
  echo "Test 1 attempts.jsonl missing backend-b"
  exit 1
fi
echo "Test 1 passed"

# Test 2: Prints nothing and exits 0 (treated as failure), then succeeds
rm -rf "$TEST_DIR/out2"
if ! bash "$DELIVERY_RUN" --backend-a "$TEST_DIR/backend_zero_bytes.sh" --backend-b "$TEST_DIR/backend_success.sh" --wall 3 --quiet 2 --out "$TEST_DIR/out2" -- arg1; then
  echo "Test 2 failed (should exit 0)"
  exit 1
fi
if ! grep -q '"reason":"zero_bytes"' "$TEST_DIR/out2/attempts.jsonl"; then
  echo "Test 2 attempts.jsonl missing zero_bytes reason"
  exit 1
fi
echo "Test 2 passed"

# Test 3: Wall limit exceeded, fails on A and B
rm -rf "$TEST_DIR/out3"
if bash "$DELIVERY_RUN" --backend-a "$TEST_DIR/backend_wall_exceed.sh" --backend-b "$TEST_DIR/backend_wall_exceed.sh" --wall 2 --quiet 3 --out "$TEST_DIR/out3" -- arg1 2>/dev/null; then
  echo "Test 3 failed (should exit 1)"
  exit 1
fi
if ! grep -q '"reason":"wall_limit"' "$TEST_DIR/out3/attempts.jsonl"; then
  echo "Test 3 attempts.jsonl missing wall_limit reason"
  exit 1
fi
echo "Test 3 passed"

# Test 4: Fail once, succeed on backend B exits 0
rm -rf "$TEST_DIR/out4"
if ! bash "$DELIVERY_RUN" --backend-a "$TEST_DIR/backend_fail_nonzero.sh" --backend-b "$TEST_DIR/backend_success.sh" --wall 3 --quiet 2 --out "$TEST_DIR/out4" -- arg1; then
  echo "Test 4 failed (should exit 0)"
  exit 1
fi
if ! grep -q '"reason":"nonzero_exit"' "$TEST_DIR/out4/attempts.jsonl"; then
  echo "Test 4 attempts.jsonl missing nonzero_exit reason"
  exit 1
fi
echo "Test 4 passed"

echo "All tests passed!"
