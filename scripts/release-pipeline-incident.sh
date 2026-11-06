#!/bin/bash
# Upsert a GitHub issue when the Release & Multi-Platform Binaries workflow
# fails (or close it when the same pipeline goes green again).
#
# Deterministic marker (one open issue per watched workflow name):
#   <!-- release-pipeline-failure:<WORKFLOW_NAME> -->
#
# Required env (same contract as scripts/main-incident-gh.sh):
#   CONCLUSION, HEAD_SHA, RUN_ID, RUN_URL, WORKFLOW_NAME
# Optional:
#   HEAD_BRANCH  (tag or branch that triggered the release run)
#   LABEL        (default: release-pipeline)
set -euo pipefail

: "${CONCLUSION:?CONCLUSION required}"
: "${HEAD_SHA:?HEAD_SHA required}"
: "${RUN_ID:?RUN_ID required}"
: "${RUN_URL:?RUN_URL required}"
: "${WORKFLOW_NAME:?WORKFLOW_NAME required}"

LABEL="${LABEL:-release-pipeline}"
HEAD_BRANCH="${HEAD_BRANCH:-}"
SHORT_SHA=${HEAD_SHA:0:7}
MARKER="<!-- release-pipeline-failure:${WORKFLOW_NAME} -->"
TITLE="release pipeline red: ${WORKFLOW_NAME}"

gh label create "$LABEL" --color B60205 --description "Release/binary pipeline failure" 2>/dev/null || true

# Prints the matching open issue number, or empty if none.
# Must exit non-zero on gh/jq failure (callers check explicitly; bash
# assignments do not honor errexit for command substitutions).
find_open_issue() {
  local open_issues number body
  open_issues=$(gh issue list --label "$LABEL" --state open --limit 200 --json number,body) || return $?
  if [ -z "$open_issues" ] || [ "$open_issues" = "[]" ]; then
    return 0
  fi
  echo "$open_issues" | jq -e 'type == "array"' >/dev/null || return $?
  while IFS= read -r number; do
    [ -n "$number" ] || continue
    body=$(echo "$open_issues" | jq -r --argjson n "$number" '.[] | select(.number == $n) | .body // empty') || return $?
    if [[ "$body" == *"$MARKER"* ]]; then
      echo "$number"
      return 0
    fi
  done < <(echo "$open_issues" | jq -r '.[].number')
  return 0
}

# Bash does not trip set -e on failed command substitutions in assignments.
# Capture status explicitly so lookup failures abort before any mutation.
set +e
EXISTING=$(find_open_issue)
LOOKUP_RC=$?
set -e
if [ "$LOOKUP_RC" -ne 0 ]; then
  echo "release-pipeline-incident: issue lookup failed (rc=$LOOKUP_RC); aborting without mutations" >&2
  exit "$LOOKUP_RC"
fi

if [ "$CONCLUSION" = "failure" ] || [ "$CONCLUSION" = "cancelled" ] || [ "$CONCLUSION" = "timed_out" ]; then
  REF_LINE=""
  if [ -n "$HEAD_BRANCH" ]; then
    REF_LINE="Ref: \`$HEAD_BRANCH\`"
  fi
  if [ -n "$EXISTING" ]; then
    echo "Updating existing release-pipeline incident #$EXISTING"
    {
      echo "Still failing ($CONCLUSION) in run $RUN_ID @ \`$SHORT_SHA\`."
      [ -n "$REF_LINE" ] && echo "$REF_LINE"
      echo "Run: $RUN_URL"
    } > /tmp/release-pipeline-comment.md
    gh issue comment "$EXISTING" --body-file /tmp/release-pipeline-comment.md
  else
    echo "Creating release-pipeline incident"
    {
      echo "Release/binary pipeline **$WORKFLOW_NAME** concluded \`$CONCLUSION\`."
      echo
      echo "- Run: $RUN_ID"
      echo "- SHA: \`$HEAD_SHA\`"
      [ -n "$REF_LINE" ] && echo "- $REF_LINE"
      echo "- URL: $RUN_URL"
      echo
      echo "This issue is upserted by \`scripts/release-pipeline-incident.sh\`."
      echo "It will be commented on for repeated failures and closed when the same workflow succeeds."
      echo
      echo "$MARKER"
    } > /tmp/release-pipeline-body.md
    gh issue create --title "$TITLE" --body-file /tmp/release-pipeline-body.md --label "$LABEL"
  fi
elif [ "$CONCLUSION" = "success" ]; then
  if [ -n "$EXISTING" ]; then
    echo "Closing release-pipeline incident #$EXISTING after green run"
    gh issue comment "$EXISTING" --body "Pipeline green again in run $RUN_ID @ \`$HEAD_SHA\`. See: $RUN_URL"
    gh issue close "$EXISTING"
  else
    echo "No open release-pipeline incident to close"
  fi
else
  echo "Ignoring conclusion=$CONCLUSION"
fi
