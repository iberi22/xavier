#!/bin/bash
set -euo pipefail

# Inputs from env:
# CONCLUSION
# HEAD_SHA
# RUN_ID
# RUN_URL
# WORKFLOW_NAME

gh label create tbd-incident --color B60205 --description "CI failing on main" 2>/dev/null || true

SHORT_SHA=${HEAD_SHA:0:7}
TITLE="main red: $WORKFLOW_NAME @ $SHORT_SHA"
MARKER="<!-- tbd-incident sha=$HEAD_SHA workflow=$WORKFLOW_NAME -->"

if [ "$CONCLUSION" = "failure" ]; then
  # Dedupe on the full SHA embedded in the body marker (search body)
  EXISTING_ISSUE=$(gh issue list --label "tbd-incident" --state open --search "in:body \"$MARKER\"" --json number -q '.[0].number')

  if [ -n "$EXISTING_ISSUE" ] && [ "$EXISTING_ISSUE" != "null" ]; then
    echo "Found existing incident #$EXISTING_ISSUE. Adding comment..."
    gh issue comment "$EXISTING_ISSUE" --body "Repeated failure on main in run $RUN_ID. See: $RUN_URL"
  else
    echo "Creating new incident..."
    # Retrieve author without ping
    OWNER=$(git log -1 --format='%an' "$HEAD_SHA" 2>/dev/null || echo "Unknown")

    BODY="CI failed on main in run $RUN_ID.
Owner: $OWNER
Run URL: $RUN_URL
Fix/revert tracking: Please link a green SHA or PR to fix this.
$MARKER"

    gh issue create --title "$TITLE" --body "$BODY" --label "tbd-incident"
  fi
elif [ "$CONCLUSION" = "success" ]; then
  OPEN_ISSUES=$(gh issue list --label "tbd-incident" --state open --json number,body)

  if [ "$OPEN_ISSUES" != "[]" ] && [ -n "$OPEN_ISSUES" ]; then
    NUMBERS=$(echo "$OPEN_ISSUES" | jq -r '.[].number')
    for NUMBER in $NUMBERS; do
      BODY=$(echo "$OPEN_ISSUES" | jq -r ".[] | select(.number == $NUMBER) | .body")
      if [[ "$BODY" =~ \<!--\ tbd-incident\ sha=([a-zA-Z0-9]+)\ workflow=(.+)\ --\> ]]; then
        ISSUE_SHA="${BASH_REMATCH[1]}"
        ISSUE_WF="${BASH_REMATCH[2]}"

        if [[ "$ISSUE_WF" == "$WORKFLOW_NAME" ]]; then
          if git merge-base --is-ancestor "$ISSUE_SHA" "$HEAD_SHA" 2>/dev/null; then
            echo "Closing incident #$NUMBER with green run from SHA $HEAD_SHA..."
            gh issue comment "$NUMBER" --body "Fixed by green run $RUN_ID at $HEAD_SHA. See: $RUN_URL"
            gh issue close "$NUMBER"
          fi
        fi
      fi
    done
  fi
fi
