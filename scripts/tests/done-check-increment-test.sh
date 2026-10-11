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
  "increment_id": "inc-1",
  "dag_closed": true,
  "ci": {"passed": true, "sha": "$HEAD_SHA"},
  "ledger": {"full_verifier_run": true, "promoted_by": "robot"},
  "incidents": [],
  "package_links": ["https://example.com/pkg"]
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

cat << JSON > incident_open.json
{
  "increment_id": "inc-1",
  "dag_closed": true,
  "ci": {"passed": true, "sha": "$HEAD_SHA"},
  "ledger": {"full_verifier_run": true, "promoted_by": "robot"},
  "incidents": ["inc-123"],
  "package_links": ["https://example.com/pkg"]
}
JSON
if bash "$DONE_CHECK" incident_open.json --json >/dev/null 2>&1; then
    echo "Test 2 failed: script exited 0 but should fail (open incident)"
    exit 1
fi

cat << JSON > no_package.json
{
  "increment_id": "inc-1",
  "dag_closed": true,
  "ci": {"passed": true, "sha": "$HEAD_SHA"},
  "ledger": {"full_verifier_run": true, "promoted_by": "robot"},
  "incidents": [],
  "package_links": []
}
JSON
if bash "$DONE_CHECK" no_package.json --json >/dev/null 2>&1; then
    echo "Test 3 failed: script exited 0 but should fail (no package links)"
    exit 1
fi

cat << JSON > missing_feature_fields.json
{
  "increment_id": "inc-1",
  "dag_closed": false,
  "ci": {"passed": true, "sha": "$HEAD_SHA"},
  "ledger": {"full_verifier_run": true, "promoted_by": "robot"},
  "incidents": [],
  "package_links": ["https://example.com/pkg"]
}
JSON
if bash "$DONE_CHECK" missing_feature_fields.json --json >/dev/null 2>&1; then
    echo "Test 4 failed: script exited 0 but should fail (open dag)"
    exit 1
fi


echo "All increment tests passed"
