#!/usr/bin/env bash
set -euo pipefail

# Helper function to emit reject
emit_reject() {
    local note="$1"
    # Make sure we emit exactly what is expected and nothing more on stdout
    jq -n -c --arg notes "$note" '{verdict: "reject", notes: $notes}'
    exit 0
}

# 1. Read one Atlas JSON object on stdin
input_json=$(cat)

if ! echo "$input_json" | jq -e . >/dev/null 2>&1; then
    emit_reject "Malformed stdin: not a valid JSON object"
fi

task_id=$(echo "$input_json" | jq -r -e '.task_id // empty') || emit_reject "Missing task_id"
worktree=$(echo "$input_json" | jq -r -e '.worktree // empty') || emit_reject "Missing worktree"
claim_file=$(echo "$input_json" | jq -r -e '.claim_file // empty') || emit_reject "Missing claim_file"

# 2. Reviewer command resolution
reviewer_cmd="${ATLAS_REVIEW_CMD:-$HOME/.hermes/scripts/co-agent-review.sh}"

# Check if reviewer_cmd exists and is executable. Since it could be a command in PATH or a direct path
# We check command -v or -x file
if ! command -v "$reviewer_cmd" >/dev/null 2>&1 && [[ ! -x "$reviewer_cmd" ]]; then
    emit_reject "Reviewer binary missing: $reviewer_cmd"
fi

# 3. Setup timeout and report dir
timeout_ms="${ATLAS_REVIEW_TIMEOUT_MS:-300000}"
timeout_s=$(awk "BEGIN {print $timeout_ms / 1000}")

report_dir="${ATLAS_REPORT_DIR:-$(mktemp -d)}"
report_path="$report_dir/$task_id.report"

# 4. Invoke the reviewer
set +e
timeout "$timeout_s" "$reviewer_cmd" verify "$worktree" "$claim_file" > "$report_path" 2>&1
reviewer_exit_code=$?
set -e

# Calculate sha256 of report and print to stderr
if [[ -f "$report_path" ]]; then
    if command -v sha256sum >/dev/null 2>&1; then
        report_sha256=$(sha256sum "$report_path" | awk '{print $1}')
    else
        report_sha256=$(shasum -a 256 "$report_path" | awk '{print $1}')
    fi
    echo "Report saved to: $report_path" >&2
    echo "Report sha256: $report_sha256" >&2
else
    emit_reject "Report file was not created"
fi

# 5. Verdict mapping
if [[ $reviewer_exit_code -eq 124 ]]; then
    emit_reject "Reviewer timeout"
fi

if [[ $reviewer_exit_code -ne 0 ]]; then
    emit_reject "Reviewer exited with nonzero status $reviewer_exit_code"
fi

if [[ ! -s "$report_path" ]]; then
    emit_reject "Reviewer output is empty"
fi

# Need to reject if output contains PARTIAL
if grep -q "PARTIAL" "$report_path"; then
    emit_reject "Reviewer output contains PARTIAL"
fi

# Need to reject if VERDICT: APPROVE is missing
if ! grep -q "VERDICT: APPROVE" "$report_path"; then
    emit_reject "Reviewer output missing VERDICT: APPROVE"
fi

# All checks passed
jq -n -c '{verdict: "approve", notes: "Reviewer approved"}'
