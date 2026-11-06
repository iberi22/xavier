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
# Identity is the HTML marker only (label is advisory and may be removed).
# Must exit non-zero on gh/jq failure (callers check explicitly; bash
# assignments do not honor errexit for command substitutions).
#
# Search is a HINT only. Declaring "no incident" requires an exhaustive
# paginated open-issue scan so a marker omitted by search cannot create
# duplicates or skip closure.
find_open_issue() {
  local open_issues="[]" number body page per_page=100 max_pages=50 count chunk search_hits

  # Fast path: if search returns the marker, use it (still marker-based).
  if search_hits=$(gh issue list --state open --limit 100 --search "$MARKER" --json number,body 2>/dev/null); then
    if [ -n "$search_hits" ] && [ "$search_hits" != "[]" ]; then
      echo "$search_hits" | jq -e 'type == "array"' >/dev/null || return $?
      while IFS= read -r number; do
        [ -n "$number" ] || continue
        body=$(echo "$search_hits" | jq -r --argjson n "$number" '.[] | select(.number == $n) | .body // empty') || return $?
        if [[ "$body" == *"$MARKER"* ]]; then
          echo "$number"
          return 0
        fi
      done < <(echo "$search_hits" | jq -r '.[].number')
    fi
  fi

  # Exhaustive scan before declaring absence (or when search missed the marker).
  page=1
  while :; do
    chunk=$(gh api "repos/${GITHUB_REPOSITORY}/issues?state=open&per_page=${per_page}&page=${page}") || return $?
    echo "$chunk" | jq -e 'type == "array"' >/dev/null || return $?
    count=$(echo "$chunk" | jq 'length') || return $?
    [ "$count" -gt 0 ] || break
    open_issues=$(jq -s 'add' <(echo "$open_issues") <(echo "$chunk" | jq '[.[] | select(.pull_request == null) | {number, body}]')) || return $?
    while IFS= read -r number; do
      [ -n "$number" ] || continue
      body=$(echo "$chunk" | jq -r --argjson n "$number" '.[] | select(.pull_request == null and .number == $n) | .body // empty') || return $?
      if [[ "$body" == *"$MARKER"* ]]; then
        echo "$number"
        return 0
      fi
    done < <(echo "$chunk" | jq -r '.[] | select(.pull_request == null) | .number')
    if [ "$count" -lt "$per_page" ]; then
      break
    fi
    page=$((page + 1))
    if [ "$page" -gt "$max_pages" ]; then
      echo "release-pipeline-incident: open-issue scan exceeded ${max_pages} pages; aborting to avoid incomplete lookup" >&2
      return 2
    fi
  done
  return 0
}

# Close duplicate open incidents that share the same marker, keep the oldest.
dedupe_marker_issues() {
  local keep="$1" page=1 per_page=100 max_pages=50 chunk count number body
  [ -n "$keep" ] || return 0
  while :; do
    chunk=$(gh api "repos/${GITHUB_REPOSITORY}/issues?state=open&per_page=${per_page}&page=${page}") || return $?
    echo "$chunk" | jq -e 'type == "array"' >/dev/null || return $?
    count=$(echo "$chunk" | jq 'length') || return $?
    [ "$count" -gt 0 ] || break
    while IFS= read -r number; do
      [ -n "$number" ] || continue
      [ "$number" = "$keep" ] && continue
      body=$(echo "$chunk" | jq -r --argjson n "$number" '.[] | select(.pull_request == null and .number == $n) | .body // empty') || return $?
      if [[ "$body" == *"$MARKER"* ]]; then
        echo "Closing duplicate release-pipeline incident #$number (keeping #$keep)"
        gh issue comment "$number" --body "Duplicate of #$keep for the same release-pipeline marker; closing."
        gh issue close "$number" --reason "not planned"
      fi
    done < <(echo "$chunk" | jq -r '.[] | select(.pull_request == null) | .number')
    [ "$count" -lt "$per_page" ] && break
    page=$((page + 1))
    [ "$page" -gt "$max_pages" ] && break
  done
  return 0
}

# Comment on recent unprocessed release failures.
# Belt-and-suspenders with concurrency.queue:max so a cancelled/skipped
# watcher cannot permanently drop a failure comment.
reconcile_recent_runs() {
  local keep="${1:-}" runs run_id conclusion url sha body already
  [ -n "$keep" ] || return 0
  runs=$(gh run list --workflow "Release & Multi-Platform Binaries" --limit 15 --json databaseId,conclusion,url,headSha,status) || return $?
  echo "$runs" | jq -e 'type == "array"' >/dev/null || return $?
  body=$(gh issue view "$keep" --json body,comments --jq '{body:.body, comments:[.comments[].body]}') || return $?
  while IFS= read -r run_id; do
    [ -n "$run_id" ] || continue
    conclusion=$(echo "$runs" | jq -r --argjson id "$run_id" '.[] | select(.databaseId == $id) | .conclusion // empty') || return $?
    url=$(echo "$runs" | jq -r --argjson id "$run_id" '.[] | select(.databaseId == $id) | .url // empty') || return $?
    sha=$(echo "$runs" | jq -r --argjson id "$run_id" '.[] | select(.databaseId == $id) | .headSha // empty') || return $?
    already=$(echo "$body" | jq -r --arg id "$run_id" --arg url "$url" '[.body, (.comments // [])[]] | join("\n") | (contains($id) or contains($url))') || return $?
    if [ "$already" = "true" ]; then
      continue
    fi
    case "$conclusion" in
      failure|cancelled|timed_out)
        echo "Reconcile: commenting missed $conclusion run $run_id on #$keep"
        {
          echo "Reconciled missed watcher event ($conclusion) for run $run_id @ \`${sha:0:7}\`."
          echo "Run: $url"
        } > /tmp/release-pipeline-reconcile.md
        gh issue comment "$keep" --body-file /tmp/release-pipeline-reconcile.md || return $?
        ;;
    esac
  done < <(echo "$runs" | jq -r '.[].databaseId')
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
    CREATED_URL=$(gh issue create --title "$TITLE" --body-file /tmp/release-pipeline-body.md --label "$LABEL")
    echo "Created: $CREATED_URL"
    # Race with a parallel watcher: re-scan and close newer duplicates.
    set +e
    KEEP=$(find_open_issue)
    DEDUPE_RC=$?
    set -e
    if [ "$DEDUPE_RC" -ne 0 ]; then
      echo "release-pipeline-incident: post-create lookup failed (rc=$DEDUPE_RC)" >&2
      exit "$DEDUPE_RC"
    fi
    if [ -n "$KEEP" ]; then
      dedupe_marker_issues "$KEEP"
    fi
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

# Reconcile recent release runs against the open incident (if any).
if [ "${RELEASE_WATCH_SKIP_RECONCILE:-}" = "1" ]; then
  echo "reconcile skipped (RELEASE_WATCH_SKIP_RECONCILE=1)"
else
  set +e
  RECON_KEEP=${KEEP:-$EXISTING}
  if [ -z "${RECON_KEEP:-}" ]; then
    RECON_KEEP=$(find_open_issue)
    RECON_RC=$?
  else
    RECON_RC=0
  fi
  set -e
  if [ "${RECON_RC:-0}" -ne 0 ]; then
    echo "release-pipeline-incident: reconcile lookup failed (rc=$RECON_RC)" >&2
    exit "$RECON_RC"
  fi
  if [ -n "${RECON_KEEP:-}" ]; then
    reconcile_recent_runs "$RECON_KEEP"
  fi
fi
