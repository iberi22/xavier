#!/usr/bin/env bash
set -e

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
DONE_CHECK="$DIR/../done-check.sh"

TEST_DIR=$(mktemp -d)
trap 'rm -rf "$TEST_DIR"' EXIT

echo "Created test dir: $TEST_DIR"

cd "$TEST_DIR"
git init >/dev/null 2>&1
git commit --allow-empty -m "Init" >/dev/null 2>&1
HEAD_SHA=$(git rev-parse HEAD)

cat << 'INNER' > "$TEST_DIR/fake_reviewer.sh"
#!/bin/bash
cat | jq -c '.review | {verdict: .verdict, notes: "fake review"}'
INNER
chmod +x "$TEST_DIR/fake_reviewer.sh"

export DONE_REVIEW_CMD="$TEST_DIR/fake_reviewer.sh"

cat << JSON > green.json
{
  "task_id": "1",
  "sha": "$HEAD_SHA",
  "declared_files": ["a", "b"],
  "changed_files": ["b", "a"],
  "changed_lines": 10,
  "scans": {"secret_scan": "pass"},
  "commands": ["cargo test"],
  "atlas": {"passed": true, "dod_complete": true, "evidence_count": 1},
  "review": {"verdict": "approve", "subject_sha": "$HEAD_SHA", "backend": "modelA", "executor": "modelB"}
}
JSON
if ! bash "$DONE_CHECK" green.json --json > green.out; then
    echo "Test 1 failed: script exited non-zero"
    exit 1
fi
if ! grep -q '"done":true' green.out; then
    echo "Test 1 failed: not done"
    cat green.out
    exit 1
fi

cat << JSON > no_test.json
{
  "task_id": "1",
  "sha": "$HEAD_SHA",
  "declared_files": ["a", "b"],
  "changed_files": ["b", "a"],
  "changed_lines": 10,
  "scans": {"secret_scan": "pass"},
  "commands": ["echo hi"],
  "atlas": {"passed": true, "dod_complete": true, "evidence_count": 1},
  "review": {"verdict": "approve", "subject_sha": "$HEAD_SHA", "backend": "modelA", "executor": "modelB"}
}
JSON
if bash "$DONE_CHECK" no_test.json --json >/dev/null 2>&1; then
    echo "Test 2 failed: script exited 0 but should fail"
    exit 1
fi

cat << JSON > stale.json
{
  "task_id": "1",
  "sha": "badbadbadbadbadbadbadbadbadbadbadbadbadb",
  "declared_files": ["a", "b"],
  "changed_files": ["b", "a"],
  "changed_lines": 10,
  "scans": {"secret_scan": "pass"},
  "commands": ["cargo test"],
  "atlas": {"passed": true, "dod_complete": true, "evidence_count": 1},
  "review": {"verdict": "approve", "subject_sha": "badbadbadbadbadbadbadbadbadbadbadbadbadb", "backend": "modelA", "executor": "modelB"}
}
JSON
if bash "$DONE_CHECK" stale.json --json >/dev/null 2>&1; then
    echo "Test 3 failed: script exited 0 but should fail"
    exit 1
fi

cat << JSON > no_scanner.json
{
  "task_id": "1",
  "sha": "$HEAD_SHA",
  "declared_files": ["a", "b"],
  "changed_files": ["b", "a"],
  "changed_lines": 10,
  "scans": {"other": "pass"},
  "commands": ["cargo test"],
  "atlas": {"passed": true, "dod_complete": true, "evidence_count": 1},
  "review": {"verdict": "approve", "subject_sha": "$HEAD_SHA", "backend": "modelA", "executor": "modelB"}
}
JSON
if bash "$DONE_CHECK" no_scanner.json --json >/dev/null 2>&1; then
    echo "Test 4 failed: script exited 0 but should fail"
    exit 1
fi

cat << JSON > atlas_stub.json
{
  "task_id": "1",
  "sha": "$HEAD_SHA",
  "declared_files": ["a", "b"],
  "changed_files": ["b", "a"],
  "changed_lines": 10,
  "scans": {"secret_scan": "pass"},
  "commands": ["cargo test"],
  "atlas": {"passed": true, "dod_complete": true, "evidence_count": 1, "verdict": "stub"},
  "review": {"verdict": "approve", "subject_sha": "$HEAD_SHA", "backend": "modelA", "executor": "modelB"}
}
JSON
if bash "$DONE_CHECK" atlas_stub.json --json >/dev/null 2>&1; then
    echo "Test 5 failed: script exited 0 but should fail"
    exit 1
fi

cat << JSON > partial.json
{
  "task_id": "1",
  "sha": "$HEAD_SHA",
  "declared_files": ["a", "b"],
  "changed_files": ["b", "a"],
  "changed_lines": 10,
  "scans": {"secret_scan": "pass"},
  "commands": ["cargo test"],
  "atlas": {"passed": true, "dod_complete": true, "evidence_count": 1},
  "review": {"verdict": "PARTIAL", "subject_sha": "$HEAD_SHA", "backend": "modelA", "executor": "modelB"}
}
JSON
if bash "$DONE_CHECK" partial.json --json >/dev/null 2>&1; then
    echo "Test 6a failed: script exited 0 but should fail"
    exit 1
fi

cat << JSON > reject.json
{
  "task_id": "1",
  "sha": "$HEAD_SHA",
  "declared_files": ["a", "b"],
  "changed_files": ["b", "a"],
  "changed_lines": 10,
  "scans": {"secret_scan": "pass"},
  "commands": ["cargo test"],
  "atlas": {"passed": true, "dod_complete": true, "evidence_count": 1},
  "review": {"verdict": "reject", "subject_sha": "$HEAD_SHA", "backend": "modelA", "executor": "modelB"}
}
JSON
if bash "$DONE_CHECK" reject.json --json >/dev/null 2>&1; then
    echo "Test 6b failed: script exited 0 but should fail"
    exit 1
fi

cat << JSON > self_review.json
{
  "task_id": "1",
  "sha": "$HEAD_SHA",
  "declared_files": ["a", "b"],
  "changed_files": ["b", "a"],
  "changed_lines": 10,
  "scans": {"secret_scan": "pass"},
  "commands": ["cargo test"],
  "atlas": {"passed": true, "dod_complete": true, "evidence_count": 1},
  "review": {"verdict": "approve", "subject_sha": "$HEAD_SHA", "backend": "modelA", "executor": "modelA"}
}
JSON
if bash "$DONE_CHECK" self_review.json --json >/dev/null 2>&1; then
    echo "Test 7 failed: script exited 0 but should fail"
    exit 1
fi

# Additional test: missing/unset DONE_REVIEW_CMD
export DONE_REVIEW_CMD=""
if bash "$DONE_CHECK" green.json --json >/dev/null 2>&1; then
    echo "Test 8 failed: missing DONE_REVIEW_CMD should fail"
    exit 1
fi
export DONE_REVIEW_CMD="$TEST_DIR/fake_reviewer.sh"

# Additional test: fake reviewer rejects but bundle says approve
cat << 'INNER' > "$TEST_DIR/fake_rejecter.sh"
#!/bin/bash
echo '{"verdict":"reject","notes":"i said no"}'
INNER
chmod +x "$TEST_DIR/fake_rejecter.sh"

export DONE_REVIEW_CMD="$TEST_DIR/fake_rejecter.sh"
cat << JSON > mismatch.json
{
  "task_id": "1",
  "sha": "$HEAD_SHA",
  "declared_files": ["a", "b"],
  "changed_files": ["b", "a"],
  "changed_lines": 10,
  "scans": {"secret_scan": "pass"},
  "commands": ["cargo test"],
  "atlas": {"passed": true, "dod_complete": true, "evidence_count": 1},
  "review": {"verdict": "approve", "subject_sha": "$HEAD_SHA", "backend": "modelA", "executor": "modelB"}
}
JSON
if bash "$DONE_CHECK" mismatch.json --json >/dev/null 2>&1; then
    echo "Test 9 failed: fake rejecter should override bundle approve"
    exit 1
fi

echo "All tests passed"
