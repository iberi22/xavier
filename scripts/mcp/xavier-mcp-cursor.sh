#!/usr/bin/env bash
# Cursor/Claude MCP launcher for Xavier (Linux).
# Loads secrets from .env — do not hardcode tokens in mcp.json.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ENV_FILE="${XAVIER_ENV_FILE:-$REPO_ROOT/.env}"

# Prefer the systemd-backed env if the workspace .env cannot auth to :8006
# and a sibling install exists (common on this machine).
FALLBACK_ENV="/home/belal/projects/xavier/.env"

if [[ -f "$ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$ENV_FILE"
  set +a
fi

# Linux data dir for this checkout (override Windows paths from migrated .env)
export XAVIER_DATA_DIR="${XAVIER_DATA_DIR_OVERRIDE:-$REPO_ROOT/data}"

# If token looks wrong / empty, try fallback used by xavier.service
if [[ -z "${XAVIER_TOKEN:-}" && -f "$FALLBACK_ENV" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$FALLBACK_ENV"
  set +a
  export XAVIER_DATA_DIR="${XAVIER_DATA_DIR_OVERRIDE:-$REPO_ROOT/data}"
fi

# Prefer PATH binary; fall back to known install locations
if command -v xavier >/dev/null 2>&1; then
  exec xavier mcp
elif [[ -x "$HOME/.local/bin/xavier" ]]; then
  exec "$HOME/.local/bin/xavier" mcp
elif [[ -x "$REPO_ROOT/target/release/xavier" ]]; then
  exec "$REPO_ROOT/target/release/xavier" mcp
else
  echo "xavier binary not found on PATH or known locations" >&2
  exit 127
fi
