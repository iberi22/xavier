#!/usr/bin/env bash
# Install (or append) the CodeGraph post-commit sync hook for this repo.
#
# Usage (from repo root):
#   bash scripts/hooks/install-post-commit-codegraph.sh
#
# Soft: never overwrites an existing post-commit that is not ours — appends
# a call instead. Idempotent.

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$ROOT" ]]; then
  echo "No git repo here" >&2
  exit 1
fi

HOOK_SRC="$ROOT/scripts/hooks/post-commit-codegraph.sh"
HOOK_DST="$ROOT/.git/hooks/post-commit"
MARKER="# xavier-codegraph-sync"

if [[ ! -x "$HOOK_SRC" && -f "$HOOK_SRC" ]]; then
  chmod +x "$HOOK_SRC"
fi

if [[ ! -f "$HOOK_SRC" ]]; then
  echo "Missing $HOOK_SRC" >&2
  exit 1
fi

if [[ ! -e "$HOOK_DST" ]]; then
  ln -sf ../../scripts/hooks/post-commit-codegraph.sh "$HOOK_DST"
  echo "Installed symlink → .git/hooks/post-commit"
  exit 0
fi

if grep -qF "$MARKER" "$HOOK_DST" 2>/dev/null; then
  echo "Hook already references CodeGraph sync (idempotent)"
  exit 0
fi

# Existing hook: append soft call
{
  echo ""
  echo "$MARKER"
  echo "bash \"$HOOK_SRC\" || true"
} >> "$HOOK_DST"
chmod +x "$HOOK_DST"
echo "Appended CodeGraph sync to existing .git/hooks/post-commit"
