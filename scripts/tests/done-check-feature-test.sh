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

cat << JSON > green.json
{
  "feature_id": "feat-1",
  "dag_closed": true,
  "ci": {"passed": true, "sha": "$HEAD_SHA"},
  "ledger": {"full_verifier_run": true, "promoted_by": "robot"}
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

cat << JSON > dag_open.json
{
  "feature_id": "feat-1",
  "dag_closed": false,
  "ci": {"passed": true, "sha": "$HEAD_SHA"},
  "ledger": {"full_verifier_run": true, "promoted_by": "robot"}
}
JSON
if bash "$DONE_CHECK" dag_open.json --json >/dev/null 2>&1; then
    echo "Test 2 failed: script exited 0 but should fail (open dag)"
    exit 1
fi

cat << JSON > stale_ci.json
{
  "feature_id": "feat-1",
  "dag_closed": true,
  "ci": {"passed": true, "sha": "badbadbadbadbadbadbadbadbadbadbadbadbadb"},
  "ledger": {"full_verifier_run": true, "promoted_by": "robot"}
}
JSON
if bash "$DONE_CHECK" stale_ci.json --json >/dev/null 2>&1; then
    echo "Test 3 failed: script exited 0 but should fail (stale ci sha)"
    exit 1
fi

cat << JSON > failed_ci.json
{
  "feature_id": "feat-1",
  "dag_closed": true,
  "ci": {"passed": false, "sha": "$HEAD_SHA"},
  "ledger": {"full_verifier_run": true, "promoted_by": "robot"}
}
JSON
if bash "$DONE_CHECK" failed_ci.json --json >/dev/null 2>&1; then
    echo "Test 4 failed: script exited 0 but should fail (failed ci)"
    exit 1
fi

cat << JSON > no_full_run.json
{
  "feature_id": "feat-1",
  "dag_closed": true,
  "ci": {"passed": true, "sha": "$HEAD_SHA"},
  "ledger": {"full_verifier_run": false, "promoted_by": "robot"}
}
JSON
if bash "$DONE_CHECK" no_full_run.json --json >/dev/null 2>&1; then
    echo "Test 5 failed: script exited 0 but should fail (no full verifier run)"
    exit 1
fi

cat << JSON > hand_promoted.json
{
  "feature_id": "feat-1",
  "dag_closed": true,
  "ci": {"passed": true, "sha": "$HEAD_SHA"},
  "ledger": {"full_verifier_run": true, "promoted_by": "human"}
}
JSON
if bash "$DONE_CHECK" hand_promoted.json --json >/dev/null 2>&1; then
    echo "Test 6 failed: script exited 0 but should fail (hand promoted)"
    exit 1
fi

echo "All feature tests passed"
