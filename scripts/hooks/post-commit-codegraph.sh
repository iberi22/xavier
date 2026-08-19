#!/usr/bin/env bash
# Optional post-commit hook: incremental CodeGraph sync from git deltas.
#
# NOT installed automatically. To enable:
#   ln -sf ../../scripts/hooks/post-commit-codegraph.sh .git/hooks/post-commit
#   # or append:  bash scripts/hooks/post-commit-codegraph.sh
#
# Prefers `xavier` on PATH; falls back to ~/.local/bin/xavier.
# Soft-fails so a sync error never blocks the commit.

set -u

XAVIER_BIN="${XAVIER_BIN:-}"
if [[ -z "$XAVIER_BIN" ]]; then
  if command -v xavier >/dev/null 2>&1; then
    XAVIER_BIN="$(command -v xavier)"
  elif [[ -x "${HOME}/.local/bin/xavier" ]]; then
    XAVIER_BIN="${HOME}/.local/bin/xavier"
  else
    echo "[codegraph] xavier no encontrado en PATH; skip sync" >&2
    exit 0
  fi
fi

echo "[codegraph] sync --git …" >&2
if ! "$XAVIER_BIN" code sync --git; then
  echo "[codegraph] sync falló (no bloquea el commit)" >&2
fi
exit 0
