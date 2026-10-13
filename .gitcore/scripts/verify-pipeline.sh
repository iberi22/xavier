#!/usr/bin/env bash
# Deprecated entry point: delegates to scripts/verify-pipeline.sh, the
# canonical ledger pipeline that CI runs. Kept so older docs and agents work.
set -uo pipefail

echo "WARN: .gitcore/scripts/verify-pipeline.sh is deprecated; delegating to scripts/verify-pipeline.sh" >&2

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec bash "$ROOT/scripts/verify-pipeline.sh" "$@"
