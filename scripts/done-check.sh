#!/usr/bin/env bash
set -euo pipefail

if [ $# -lt 1 ]; then
    echo "Usage: $0 <task-bundle.json> [--json]" >&2
    exit 2
fi

BUNDLE_FILE="$1"
OUTPUT_JSON=false
if [ "${2:-}" = "--json" ]; then
    OUTPUT_JSON=true
fi

if [ ! -f "$BUNDLE_FILE" ]; then
    echo "File not found: $BUNDLE_FILE" >&2
    exit 2
fi

if ! jq -e '
  type == "object" and
  has("task_id") and
  has("sha") and
  (.declared_files | type == "array") and
  (.changed_files | type == "array") and
  (.changed_lines | type == "number") and
  (.scans | type == "object" and has("secret_scan")) and
  (.commands | type == "array" and length > 0) and
  (.atlas | type == "object" and has("passed") and has("dod_complete") and has("evidence_count")) and
  (.review | type == "object" and has("verdict") and has("subject_sha") and has("backend") and has("executor"))
' "$BUNDLE_FILE" >/dev/null 2>&1; then
    echo "Malformed bundle JSON" >&2
    exit 2
fi

DONE_REVIEW_CMD="${DONE_REVIEW_CMD:-scripts/atlas-review-adapter.sh}"

TASK_ID=$(jq -r '.task_id' "$BUNDLE_FILE")

CHECKS="[]"
ALL_DONE=true
EXIT_CODE=0

add_check() {
    local name="$1"
    local ok="$2"
    local detail="$3"

    CHECKS=$(jq -n -c --argjson checks "$CHECKS" --arg name "$name" --argjson ok "$ok" --arg detail "$detail" '$checks + [{name: $name, ok: $ok, detail: $detail}]')
    if [ "$ok" = "false" ]; then
        ALL_DONE=false
        EXIT_CODE=1
    fi
    if [ "$OUTPUT_JSON" = "false" ]; then
        if [ "$ok" = "true" ]; then
            echo "PASS: $name - $detail"
        else
            echo "FAIL: $name - $detail" >&2
        fi
    fi
}

if jq -e '(.declared_files | sort) == (.changed_files | sort)' "$BUNDLE_FILE" >/dev/null; then
    NUM_FILES=$(jq -r '.changed_files | length' "$BUNDLE_FILE")
    CHANGED_LINES=$(jq -r '.changed_lines' "$BUNDLE_FILE")
    if [ "$NUM_FILES" -le 4 ] && [ "$CHANGED_LINES" -le 400 ]; then
        add_check "scope" "true" "Scope ok ($NUM_FILES files, $CHANGED_LINES lines)"
    else
        add_check "scope" "false" "Scope exceeds limits ($NUM_FILES files, $CHANGED_LINES lines)"
    fi
else
    add_check "scope" "false" "declared_files and changed_files mismatch"
fi

HEAD_SHA=$(git rev-parse HEAD)
BUNDLE_SHA=$(jq -r '.sha' "$BUNDLE_FILE")
if [ "$BUNDLE_SHA" = "$HEAD_SHA" ]; then
    add_check "sha" "true" "SHA matches HEAD ($BUNDLE_SHA)"
else
    add_check "sha" "false" "stale SHA ($BUNDLE_SHA != $HEAD_SHA)"
fi

if jq -e '.commands | any(contains("cargo test"))' "$BUNDLE_FILE" >/dev/null; then
    add_check "tests" "true" "Cargo test command observed"
else
    add_check "tests" "false" "Zero tests observed (missing cargo test)"
fi

SECRET_SCAN=$(jq -r '.scans.secret_scan' "$BUNDLE_FILE")
if [ "$SECRET_SCAN" = "pass" ]; then
    add_check "scans" "true" "Secret scan passed"
else
    add_check "scans" "false" "Secret scan missing or failed: $SECRET_SCAN"
fi

ATLAS_PASSED=$(jq -r '.atlas.passed' "$BUNDLE_FILE")
ATLAS_DOD=$(jq -r '.atlas.dod_complete' "$BUNDLE_FILE")
ATLAS_EV=$(jq -r '.atlas.evidence_count' "$BUNDLE_FILE")
ATLAS_VERDICT=$(jq -r '.atlas.verdict // ""' "$BUNDLE_FILE")
if [ "$ATLAS_PASSED" = "true" ] && [ "$ATLAS_DOD" = "true" ] && [ "$ATLAS_EV" -gt 0 ]; then
    if [[ "$ATLAS_VERDICT" == *"stub"* ]] || [[ "$ATLAS_VERDICT" == *"deferred"* ]]; then
        add_check "atlas" "false" "Atlas verdict is stub/deferred"
    else
        add_check "atlas" "true" "Atlas checks passed"
    fi
else
    add_check "atlas" "false" "Atlas missing criteria: passed=$ATLAS_PASSED dod=$ATLAS_DOD ev=$ATLAS_EV"
fi

if [ -x "$DONE_REVIEW_CMD" ] || command -v "$DONE_REVIEW_CMD" >/dev/null 2>&1; then
    REVIEW_OUT=$("$DONE_REVIEW_CMD" "$BUNDLE_FILE" 2>/dev/null || true)
    if [ -n "$REVIEW_OUT" ] && echo "$REVIEW_OUT" | jq -e 'type == "object" and has("verdict") and has("subject_sha") and has("backend") and has("executor")' >/dev/null 2>&1; then
        REVIEW_VERDICT=$(echo "$REVIEW_OUT" | jq -r '.verdict')
        REVIEW_SHA=$(echo "$REVIEW_OUT" | jq -r '.subject_sha')
        REVIEW_BACKEND=$(echo "$REVIEW_OUT" | jq -r '.backend')
        REVIEW_EXECUTOR=$(echo "$REVIEW_OUT" | jq -r '.executor')
    else
        REVIEW_VERDICT=$(jq -r '.review.verdict // ""' "$BUNDLE_FILE")
        REVIEW_SHA=$(jq -r '.review.subject_sha // ""' "$BUNDLE_FILE")
        REVIEW_BACKEND=$(jq -r '.review.backend // ""' "$BUNDLE_FILE")
        REVIEW_EXECUTOR=$(jq -r '.review.executor // ""' "$BUNDLE_FILE")
    fi
else
    REVIEW_VERDICT=$(jq -r '.review.verdict // ""' "$BUNDLE_FILE")
    REVIEW_SHA=$(jq -r '.review.subject_sha // ""' "$BUNDLE_FILE")
    REVIEW_BACKEND=$(jq -r '.review.backend // ""' "$BUNDLE_FILE")
    REVIEW_EXECUTOR=$(jq -r '.review.executor // ""' "$BUNDLE_FILE")
fi

if [ -z "$REVIEW_VERDICT" ] || [ -z "$REVIEW_SHA" ] || [ -z "$REVIEW_BACKEND" ] || [ -z "$REVIEW_EXECUTOR" ]; then
    add_check "review" "false" "malformed review"
elif [ "$REVIEW_VERDICT" != "approve" ]; then
    add_check "review" "false" "Reviewer verdict is not approve (PARTIAL/deferred/stub fail): $REVIEW_VERDICT"
elif [ "$REVIEW_SHA" != "$BUNDLE_SHA" ]; then
    add_check "review" "false" "Review subject_sha ($REVIEW_SHA) != bundle sha ($BUNDLE_SHA)"
elif [ "$REVIEW_BACKEND" = "$REVIEW_EXECUTOR" ]; then
    add_check "review" "false" "Review backend equals executor ($REVIEW_BACKEND)"
else
    add_check "review" "true" "Review approved"
fi

if [ "$OUTPUT_JSON" = "true" ]; then
    jq -n -c --arg id "$TASK_ID" --argjson done "$ALL_DONE" --argjson checks "$CHECKS" '{task_id: $id, "done": $done, checks: $checks}'
fi

exit "$EXIT_CODE"
