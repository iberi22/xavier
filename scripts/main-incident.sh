#!/bin/bash
set -euo pipefail

REPO=${GITHUB_REPOSITORY:-iberi22/xavier}

gh label create main-red --color B60205 --description "CI failing on main" 2>/dev/null || true

if [ "$CONCLUSION" = "failure" ]; then
  # Check if there is already an open incident for this SHA
  EXISTING_ISSUE=$(gh issue list --label "main-red" --state open --search "in:title \"$HEAD_SHA\"" --json number -q '.[0].number')

  if [ -n "$EXISTING_ISSUE" ] && [ "$EXISTING_ISSUE" != "null" ]; then
    echo "Found existing incident #$EXISTING_ISSUE. Adding comment..."
    gh issue comment "$EXISTING_ISSUE" --body "Repeated failure on main in run $RUN_ID. See: $RUN_URL"
  else
    echo "Creating new incident..."
    OWNER=$(gh api repos/$REPO/commits/$HEAD_SHA --jq '.author.login' 2>/dev/null || echo "Unknown")

    BODY="CI failed on main in run $RUN_ID.
Owner: @$OWNER
Run URL: $RUN_URL
Fix/revert tracking: Please link a green SHA or PR to fix this."

    gh issue create --title "CI Failure on main: $HEAD_SHA" --body "$BODY" --label "main-red"
  fi
elif [ "$CONCLUSION" = "success" ]; then
  OPEN_ISSUES=$(gh issue list --label "main-red" --state open --json number -q '.[].number')

  if [ -n "$OPEN_ISSUES" ] && [ "$OPEN_ISSUES" != "null" ]; then
    for ISSUE in $OPEN_ISSUES; do
      if [ -n "$ISSUE" ] && [ "$ISSUE" != "null" ]; then
        echo "Closing incident #$ISSUE with green run from SHA $HEAD_SHA..."
        gh issue comment "$ISSUE" --body "Fixed by green run $RUN_ID at $HEAD_SHA. See: $RUN_URL"
        gh issue close "$ISSUE"
      fi
    done
  fi
fi
