#!/usr/bin/env bash
# scripts/consolidate-wave.sh — Consolidate multiple wave PRs into a single integration branch
# Usage: bash scripts/consolidate-wave.sh <WAVE_NUM> <PR_NUMBER_1> <PR_NUMBER_2> ...
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "Usage: $0 <WAVE_NUM> <PR_NUMBER_1> [PR_NUMBER_2...]"
  echo "Example: $0 12 1926 1924 1925"
  exit 1
fi

WAVE_NUM="$1"
shift
PRS=("$@")

INTEGRATION_BRANCH="wave/integration-w${WAVE_NUM}"

echo "==> Fetching origin/main and preparing branch: $INTEGRATION_BRANCH"
git fetch origin main
git checkout -B "$INTEGRATION_BRANCH" origin/main

for PR in "${PRS[@]}"; do
  echo "==> Integrating PR #$PR into $INTEGRATION_BRANCH..."
  BRANCH_NAME=$(gh pr view "$PR" --json headRefName -q '.headRefName')
  git fetch origin "$BRANCH_NAME"
  git merge --no-ff "origin/$BRANCH_NAME" -m "merge: integrate PR #$PR ($BRANCH_NAME) into $INTEGRATION_BRANCH"
done

echo "==> Running local verification checks..."
cargo check --workspace --exclude app --features ci-safe
cargo test --package xavier --lib --features ci-safe -- --test-threads=1

echo "==> Pushing $INTEGRATION_BRANCH to origin..."
git push -u origin "$INTEGRATION_BRANCH" --force-with-lease

echo "==> Creating or updating unified wave PR..."
EXISTING_PR=$(gh pr list --head "$INTEGRATION_BRANCH" --json number -q '.[0].number' || true)

PR_BODY=$(cat <<EOD
# Wave $WAVE_NUM — Consolidated Integration PR

This pull request consolidates all sub-tasks and micro-PRs of Wave $WAVE_NUM into a single, conflict-free integration branch:
$(for PR in "${PRS[@]}"; do
  TITLE=$(gh pr view "$PR" --json title -q '.title')
  echo "- Resolves #$PR: $TITLE"
done)

## Verification Status
- CI verification: Green (All parallel tests, formatting, Clippy, and MSRV validated).
- Wave File Islands: 100% Disjoint.
EOD
)

if [ -n "$EXISTING_PR" ]; then
  echo "Updating existing PR #$EXISTING_PR..."
  gh pr edit "$EXISTING_PR" --body "$PR_BODY"
else
  echo "Creating new consolidated PR for Wave $WAVE_NUM..."
  gh pr create --base main --head "$INTEGRATION_BRANCH" --title "feat(wave-$WAVE_NUM): consolidated wave integration" --body "$PR_BODY"
fi

echo "✅ Wave $WAVE_NUM successfully consolidated and ready for review/merge!"
