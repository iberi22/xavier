#!/usr/bin/env bash
# =============================================================================
# Tests for production-target preflight
# =============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$ROOT/scripts/check-production-targets.sh"
TEMP_DIR=$(mktemp -d)
INVENTORY="$TEMP_DIR/inventory.json"

cleanup() {
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

# 1. Create a fixture inventory with one of each kind
# Health checks must not be older than 24h, so we create it dynamically.
NOW=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
cat <<EOF > "$INVENTORY"
{
  "targets": [
    {
      "id": "target1",
      "kind": "local_binary",
      "production_id": "p1",
      "source_sha": "abc",
      "artifact_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
      "health_checked_at": "${NOW}"
    },
    {
      "id": "target2",
      "kind": "release_tag",
      "production_id": "p2",
      "source_sha": "abc",
      "artifact_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
      "health_checked_at": "${NOW}"
    },
    {
      "id": "target3",
      "kind": "cf_pages",
      "production_id": "p3",
      "source_sha": "abc",
      "artifact_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
      "health_checked_at": "${NOW}"
    },
    {
      "id": "target4",
      "kind": "cf_worker",
      "production_id": "p4",
      "source_sha": "abc",
      "artifact_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
      "health_checked_at": "${NOW}"
    },
    {
      "id": "target5",
      "kind": "gh_pages",
      "production_id": "p5",
      "source_sha": "abc",
      "artifact_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
      "health_checked_at": "${NOW}"
    }
  ]
}
EOF

# Helper function
run_case() {
    local DESC="$1"
    local EXPECTED_EXIT="$2"
    local EXPECTED_GREP="$3"

    echo "Running test: $DESC"
    local OUT="$TEMP_DIR/out"

    set +e
    "$SCRIPT" "$INVENTORY" > "$OUT" 2>&1
    local ACTUAL_EXIT=$?
    set -e

    if [ "$ACTUAL_EXIT" -ne "$EXPECTED_EXIT" ]; then
        echo "FAIL: $DESC - Expected exit $EXPECTED_EXIT, got $ACTUAL_EXIT"
        cat "$OUT"
        exit 1
    fi

    if [ -n "$EXPECTED_GREP" ]; then
        if ! grep -q "$EXPECTED_GREP" "$OUT"; then
            echo "FAIL: $DESC - Output did not contain expected string '$EXPECTED_GREP'"
            cat "$OUT"
            exit 1
        fi
    fi

    if grep -q "fake-token" "$OUT"; then
        echo "FAIL: $DESC - Output contains fake-token!"
        cat "$OUT"
        exit 1
    fi

    echo "PASS: $DESC"
}

# Base setup for full credentials
export XAVIER_GUARDIAN_ACCEPTED=1
export CLOUDFLARE_API_TOKEN="fake-token-cf-api"
export CLOUDFLARE_ACCOUNT_ID="fake-token-cf-acc"
export GH_TOKEN="fake-token-gh"

# Case 1: All set
run_case "All credentials set correctly" 0 "OK XAVIER_GUARDIAN_ACCEPTED"

# Case 2: Missing CLOUDFLARE_API_TOKEN
unset CLOUDFLARE_API_TOKEN
run_case "Missing CLOUDFLARE_API_TOKEN" 1 "MISSING CLOUDFLARE_API_TOKEN"
export CLOUDFLARE_API_TOKEN="fake-token-cf-api"

# Case 3: Missing GH_TOKEN
unset GH_TOKEN
run_case "Missing GH_TOKEN" 1 "MISSING GH_TOKEN"
export GH_TOKEN="fake-token-gh"

# Case 4: Missing XAVIER_GUARDIAN_ACCEPTED
export XAVIER_GUARDIAN_ACCEPTED=0
run_case "XAVIER_GUARDIAN_ACCEPTED is not 1" 1 "MISSING XAVIER_GUARDIAN_ACCEPTED"
export XAVIER_GUARDIAN_ACCEPTED=1

# Case 5: Empty XAVIER_GUARDIAN_ACCEPTED
unset XAVIER_GUARDIAN_ACCEPTED
run_case "XAVIER_GUARDIAN_ACCEPTED is unset" 1 "MISSING XAVIER_GUARDIAN_ACCEPTED"
export XAVIER_GUARDIAN_ACCEPTED=1

echo "All tests passed!"
exit 0
