#!/usr/bin/env bash
set -euo pipefail

if [ $# -lt 1 ]; then
    echo "Usage: $0 <bundle.json> [--json]" >&2
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

BUNDLE_TYPE=$(jq -r 'if type == "object" then if has("task_id") then "task" elif has("feature_id") then "feature" elif has("increment_id") then "increment" else "unknown" end else "unknown" end' "$BUNDLE_FILE")

if [ "$BUNDLE_TYPE" = "unknown" ]; then
    echo "Malformed bundle JSON" >&2
    exit 2
fi

if [ "$BUNDLE_TYPE" = "task" ]; then
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
      (.review | type == "object" and has("verdict") and has("subject_sha") and has("backend") and has("executor")) and
      has("worktree") and
      has("claim_file")
    ' "$BUNDLE_FILE" >/dev/null 2>&1; then
        echo "Malformed bundle JSON" >&2
        exit 2
    fi
    ID=$(jq -r '.task_id' "$BUNDLE_FILE")
elif [ "$BUNDLE_TYPE" = "feature" ]; then
    if ! jq -e '
      type == "object" and
      has("feature_id") and
      (.dag_closed | type == "boolean") and
      (.ci | type == "object" and has("passed") and has("sha")) and
      (.ledger | type == "object" and has("full_verifier_run") and has("promoted_by"))
    ' "$BUNDLE_FILE" >/dev/null 2>&1; then
        echo "Malformed bundle JSON" >&2
        exit 2
    fi
    ID=$(jq -r '.feature_id' "$BUNDLE_FILE")
elif [ "$BUNDLE_TYPE" = "increment" ]; then
    if ! jq -e '
      type == "object" and
      has("increment_id") and
      (.dag_closed | type == "boolean") and
      (.ci | type == "object" and has("passed") and has("sha")) and
      (.ledger | type == "object" and has("full_verifier_run") and has("promoted_by")) and
      (.incidents | type == "array") and
      (.package_links | type == "array")
    ' "$BUNDLE_FILE" >/dev/null 2>&1; then
        echo "Malformed bundle JSON" >&2
        exit 2
    fi
    ID=$(jq -r '.increment_id' "$BUNDLE_FILE")
fi

# Resolve the default reviewer relative to this script so the seam works from
# any CWD. An explicitly empty DONE_REVIEW_CMD stays empty (fail-closed).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DONE_REVIEW_CMD="${DONE_REVIEW_CMD:-$SCRIPT_DIR/atlas-review-adapter.sh}"

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

HEAD_SHA=$(git rev-parse HEAD)

if [ "$BUNDLE_TYPE" = "task" ]; then
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

    if [ -z "${DONE_REVIEW_CMD:-}" ] || ! command -v "$DONE_REVIEW_CMD" >/dev/null 2>&1; then
        add_check "review" "false" "DONE_REVIEW_CMD missing or unusable"
    else
        # The bundle already carries the worktree/claim_file the reviewer needs
        # (validated above), so it is forwarded verbatim on stdin.
        REVIEW_OUT=$("$DONE_REVIEW_CMD" < "$BUNDLE_FILE" 2>/dev/null || true)
        if [ -n "$REVIEW_OUT" ] && echo "$REVIEW_OUT" | jq -e 'type == "object" and has("verdict")' >/dev/null 2>&1; then
            REVIEW_VERDICT=$(echo "$REVIEW_OUT" | jq -r '.verdict')
            REVIEW_SHA=$(jq -r '.review.subject_sha // ""' "$BUNDLE_FILE")
            REVIEW_BACKEND=$(jq -r '.review.backend // ""' "$BUNDLE_FILE")
            REVIEW_EXECUTOR=$(jq -r '.review.executor // ""' "$BUNDLE_FILE")

            if [ "$REVIEW_VERDICT" != "approve" ]; then
                add_check "review" "false" "Reviewer verdict is not approve (PARTIAL/deferred/stub fail): $REVIEW_VERDICT"
            elif [ "$REVIEW_SHA" != "$BUNDLE_SHA" ]; then
                add_check "review" "false" "Review subject_sha ($REVIEW_SHA) != bundle sha ($BUNDLE_SHA)"
            elif [ "$REVIEW_BACKEND" = "$REVIEW_EXECUTOR" ]; then
                add_check "review" "false" "Review backend equals executor ($REVIEW_BACKEND)"
            else
                add_check "review" "true" "Review approved"
            fi
        else
            add_check "review" "false" "malformed review output from DONE_REVIEW_CMD"
        fi
    fi
elif [ "$BUNDLE_TYPE" = "feature" ] || [ "$BUNDLE_TYPE" = "increment" ]; then
    DAG_CLOSED=$(jq -r '.dag_closed' "$BUNDLE_FILE")
    if [ "$DAG_CLOSED" = "true" ]; then
        add_check "dag" "true" "DAG is closed"
    else
        add_check "dag" "false" "DAG is not closed"
    fi

    CI_PASSED=$(jq -r '.ci.passed' "$BUNDLE_FILE")
    CI_SHA=$(jq -r '.ci.sha' "$BUNDLE_FILE")
    if [ "$CI_PASSED" = "true" ] && [ "$CI_SHA" = "$HEAD_SHA" ]; then
        add_check "ci" "true" "CI passed on current SHA"
    else
        add_check "ci" "false" "CI failed or stale SHA ($CI_SHA != $HEAD_SHA)"
    fi

    LEDGER_FULL=$(jq -r '.ledger.full_verifier_run' "$BUNDLE_FILE")
    LEDGER_PROMOTED=$(jq -r '.ledger.promoted_by' "$BUNDLE_FILE")
    if [ "$LEDGER_FULL" = "true" ] && [ "$LEDGER_PROMOTED" != "human" ]; then
        add_check "ledger" "true" "Full ledger run without hand promotion"
    else
        add_check "ledger" "false" "Ledger incomplete or hand promoted ($LEDGER_PROMOTED)"
    fi

    if [ "$BUNDLE_TYPE" = "increment" ]; then
        INCIDENTS_COUNT=$(jq -r '.incidents | length' "$BUNDLE_FILE")
        if [ "$INCIDENTS_COUNT" -eq 0 ]; then
            add_check "incidents" "true" "No open incidents"
        else
            add_check "incidents" "false" "Open incidents exist ($INCIDENTS_COUNT)"
        fi

        PKG_LINKS_COUNT=$(jq -r '.package_links | length' "$BUNDLE_FILE")
        if [ "$PKG_LINKS_COUNT" -gt 0 ]; then
            add_check "package_links" "true" "Package links present"
        else
            add_check "package_links" "false" "No package links"
        fi
    fi
fi

if [ "$OUTPUT_JSON" = "true" ]; then
    if [ "$BUNDLE_TYPE" = "task" ]; then
        jq -n -c --arg id "$ID" --argjson done "$ALL_DONE" --argjson checks "$CHECKS" '{task_id: $id, "done": $done, checks: $checks}'
    elif [ "$BUNDLE_TYPE" = "feature" ]; then
        jq -n -c --arg id "$ID" --argjson done "$ALL_DONE" --argjson checks "$CHECKS" '{feature_id: $id, "done": $done, checks: $checks}'
    elif [ "$BUNDLE_TYPE" = "increment" ]; then
        jq -n -c --arg id "$ID" --argjson done "$ALL_DONE" --argjson checks "$CHECKS" '{increment_id: $id, "done": $done, checks: $checks}'
    fi
fi

exit "$EXIT_CODE"
