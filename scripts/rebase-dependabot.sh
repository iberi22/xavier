#!/usr/bin/env bash
# scripts/rebase-dependabot.sh — Comment '@dependabot rebase' on open dependabot PRs to align with new main
set -euo pipefail

PRS=$(gh pr list --search "is:pr is:open author:app/dependabot" --json number -q '.[].number')

for PR in $PRS; do
  echo "Triggering rebase for Dependabot PR #$PR..."
  gh pr comment "$PR" --body "@dependabot rebase" || true
  sleep 1
done

echo "✅ All Dependabot PRs rebase triggered against latest main."
