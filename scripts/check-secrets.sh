#!/usr/bin/env bash
# =============================================================================
# Secret scan for the Xavier public repo.
# Scans ONLY git-tracked files (what would be published), never local junk.
# Uses gitleaks when available; falls back to a grep of common secret patterns.
# Exit: 0 = clean, 1 = secrets found, 2 = tooling error.
# Usage: scripts/check-secrets.sh
# =============================================================================
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> Secret scan on tracked files ($(git ls-files | wc -l) files)"

# ---- gitleaks (preferred) ----
if command -v gitleaks >/dev/null 2>&1; then
  gitleaks detect --source "$ROOT" --config "$ROOT/gitleaks.toml" --no-banner
  exit $?
fi

# ---- fallback: grep ONLY tracked files ----
echo "⚠ gitleaks not installed — using grep fallback (less thorough)"
PATTERNS='sk-or-v1-[A-Za-z0-9]{20,}|ghp_[A-Za-z0-9]{20,}|AKIA[0-9A-Z]{16}|xox[baprs]-[A-Za-z0-9-]{10,}|AIza[0-9A-Za-z_-]{30,}|BEGIN (RSA|OPENSSH|EC|DSA) PRIVATE KEY|XAVIER_TOKEN=[a-zA-Z0-9]{8,}|XAVIER_SUPABASE_KEY=[a-zA-Z0-9]{8,}'

HITS=$(git ls-files -z | xargs -0 -I{} sh -c \
  'case "$1" in *.lock|.env.example|*.min.js|*.map|*.db|*.sqlite3*|src/security/*|tests/security/*) exit 0;; esac; grep -nHE '"'"'sk-or-v1-[A-Za-z0-9]{20,}|ghp_[A-Za-z0-9]{20,}|AKIA[0-9A-Z]{16}|xox[baprs]-[A-Za-z0-9-]{10,}|AIza[0-9A-Za-z_-]{30,}|BEGIN (RSA|OPENSSH|EC|DSA) PRIVATE KEY|XAVIER_TOKEN=[a-zA-Z0-9]{8,}|XAVIER_SUPABASE_KEY=[a-zA-Z0-9]{8,}'"'"' "$1" 2>/dev/null' _ {} | head -20)

if [ -n "$HITS" ]; then
  echo "❌ Potential secrets found in tracked files:"
  echo "$HITS"
  exit 1
fi
echo "✅ No obvious secrets in tracked files"
exit 0
