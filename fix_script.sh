#!/bin/bash
set -e
# Replace the review checking section in scripts/done-check.sh

sed -i '/if \[ -x "$DONE_REVIEW_CMD" \] || command -v "$DONE_REVIEW_CMD" >\/dev\/null 2>&1; then/,/fi/c\
if [ -z "${DONE_REVIEW_CMD:-}" ] || ! command -v "$DONE_REVIEW_CMD" >/dev/null 2>&1; then\
    add_check "review" "false" "DONE_REVIEW_CMD missing or unusable"\
    REVIEW_VERDICT=""\
    REVIEW_SHA=""\
    REVIEW_BACKEND=""\
    REVIEW_EXECUTOR=""\
else\
    REVIEW_OUT=$("$DONE_REVIEW_CMD" < "$BUNDLE_FILE" 2>/dev/null || true)\
    if [ -n "$REVIEW_OUT" ] && echo "$REVIEW_OUT" | jq -e '"'"'type == "object" and has("verdict")'"'"' >/dev/null 2>&1; then\
        REVIEW_VERDICT=$(echo "$REVIEW_OUT" | jq -r '"'"'.verdict'"'"')\
        REVIEW_SHA=$(jq -r '"'"'.review.subject_sha // ""'"'"' "$BUNDLE_FILE")\
        REVIEW_BACKEND=$(jq -r '"'"'.review.backend // ""'"'"' "$BUNDLE_FILE")\
        REVIEW_EXECUTOR=$(jq -r '"'"'.review.executor // ""'"'"' "$BUNDLE_FILE")\
    else\
        add_check "review" "false" "malformed review output from DONE_REVIEW_CMD"\
        REVIEW_VERDICT=""\
        REVIEW_SHA=""\
        REVIEW_BACKEND=""\
        REVIEW_EXECUTOR=""\
    fi\
fi\
\
if [ -n "$REVIEW_VERDICT" ]; then\
    if [ "$REVIEW_VERDICT" != "approve" ]; then\
        add_check "review" "false" "Reviewer verdict is not approve (PARTIAL/deferred/stub fail): $REVIEW_VERDICT"\
    elif [ "$REVIEW_SHA" != "$BUNDLE_SHA" ]; then\
        add_check "review" "false" "Review subject_sha ($REVIEW_SHA) != bundle sha ($BUNDLE_SHA)"\
    elif [ "$REVIEW_BACKEND" = "$REVIEW_EXECUTOR" ]; then\
        add_check "review" "false" "Review backend equals executor ($REVIEW_BACKEND)"\
    else\
        add_check "review" "true" "Review approved"\
    fi\
fi' scripts/done-check.sh
