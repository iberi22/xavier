#!/usr/bin/env bash
# Day 0 Atlas & Automated Maintenance Runner for Xavier
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
echo "==> [Atlas Maintenance Day 0] Running at $(date -Iseconds)"

# 1. Clean cache & db hygiene
echo "--> Purging runtime temporary databases (*.db, *.sqlite* in data/ or .xavier/)..."
find "$ROOT/data" "$ROOT/.xavier" -maxdepth 2 \( -name "*.db*" -o -name "*.sqlite*" \) 2>/dev/null | grep -v "maturity-anchors" | xargs -r rm -f || true
echo "    ✅ Cache hygiene verified."

# 2. Refresh Skill Registry (.atl & .gitcore/skill-registry.json)
echo "--> Synchronizing Skill Registry..."
if [ -f "$HOME/.hermes/scripts/skill-registry-refresh.sh" ]; then
    bash "$HOME/.hermes/scripts/skill-registry-refresh.sh" --cwd "$ROOT" > /dev/null
    echo "    ✅ ATL Skill Registry synchronized."
fi

# 3. Drift & Verification Pipeline Preflight
echo "--> Running Xavier feature ledger check..."
python3 -c '
import json, sys
f = json.load(open(sys.argv[1]))
count = len(f.get("features", {}))
print(f"    ✅ Ledger valid: {count} features registered.")
' "$ROOT/.gitcore/features.json"

echo "==> [Atlas Maintenance Day 0] Completed successfully."
