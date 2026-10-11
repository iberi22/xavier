#!/usr/bin/env bash
set -e

# Setup temp dir
TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT

export ATLAS_REPORT_DIR="$TEMP_DIR"
ADAPTER_SCRIPT="$(pwd)/scripts/atlas-review-adapter.sh"
FAKE_REVIEWER="$TEMP_DIR/fake-reviewer.sh"

export ATLAS_REVIEW_CMD="$FAKE_REVIEWER"

# Create fake reviewer that acts based on TEST_MODE env var
cat << 'EOF' > "$FAKE_REVIEWER"
#!/usr/bin/env bash

# Sleep to check timeout if mode is timeout
if [[ "$TEST_MODE" == "timeout" ]]; then
    sleep 1
    exit 0
fi

if [[ "$TEST_MODE" == "approve" ]]; then
    echo "Some logs"
    echo "VERDICT: APPROVE"
    exit 0
fi

if [[ "$TEST_MODE" == "reject" ]]; then
    echo "Some logs"
    echo "VERDICT: REJECT"
    exit 0
fi

if [[ "$TEST_MODE" == "partial" ]]; then
    echo "Some logs"
    echo "PARTIAL success"
    echo "VERDICT: APPROVE"
    exit 0
fi

if [[ "$TEST_MODE" == "malformed" ]]; then
    echo "Malfo"
    exit 0
fi

if [[ "$TEST_MODE" == "nonzero" ]]; then
    echo "VERDICT: APPROVE"
    exit 1
fi

EOF
chmod +x "$FAKE_REVIEWER"

# Helper for testing
run_adapter() {
    local task_id="$1"
    # Ensure json structure matches task
    jq -n -c --arg tid "$task_id" '{task_id: $tid, worktree: "/wt", claim_file: "/cf"}' | bash "$ADAPTER_SCRIPT" 2> "$TEMP_DIR/stderr_$task_id.log"
}

check_verdict() {
    local output="$1"
    local expected_verdict="$2"
    local actual_verdict
    actual_verdict=$(echo "$output" | jq -r '.verdict')
    if [[ "$actual_verdict" != "$expected_verdict" ]]; then
        echo "FAIL: Expected $expected_verdict, got $actual_verdict"
        echo "Output: $output"
        exit 1
    fi
}

echo "Running tests..."

# 1. approve
export TEST_MODE="approve"
out=$(run_adapter "test_approve")
check_verdict "$out" "approve"

# 2. reject
export TEST_MODE="reject"
out=$(run_adapter "test_reject")
check_verdict "$out" "reject"

# 3. PARTIAL with exit 0
export TEST_MODE="partial"
out=$(run_adapter "test_partial")
check_verdict "$out" "reject"

# 4. malformed output
export TEST_MODE="malformed"
out=$(run_adapter "test_malformed")
check_verdict "$out" "reject"

# 5. timeout
export TEST_MODE="timeout"
export ATLAS_REVIEW_TIMEOUT_MS=200
out=$(run_adapter "test_timeout")
check_verdict "$out" "reject"
unset ATLAS_REVIEW_TIMEOUT_MS

# 6. reviewer unavailable
export ATLAS_REVIEW_CMD="/nonexistent"
out=$(run_adapter "test_unavailable")
check_verdict "$out" "reject"
export ATLAS_REVIEW_CMD="$FAKE_REVIEWER"

# 7. a report file exists and its sha256 matches the stderr digest
export TEST_MODE="approve"
out=$(run_adapter "test_digest")
check_verdict "$out" "approve"

REPORT_FILE="$TEMP_DIR/test_digest.report"
if [[ ! -f "$REPORT_FILE" ]]; then
    echo "FAIL: Report file not found at $REPORT_FILE"
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL_SHA=$(sha256sum "$REPORT_FILE" | awk '{print $1}')
else
    ACTUAL_SHA=$(shasum -a 256 "$REPORT_FILE" | awk '{print $1}')
fi

STDERR_LOG="$TEMP_DIR/stderr_test_digest.log"
# Search for the sha256 output in stderr
if ! grep -q "Report sha256: $ACTUAL_SHA" "$STDERR_LOG"; then
    echo "FAIL: sha256 mismatch or not found in stderr"
    echo "Expected SHA: $ACTUAL_SHA"
    cat "$STDERR_LOG"
    exit 1
fi

echo "All tests passed"
exit 0
