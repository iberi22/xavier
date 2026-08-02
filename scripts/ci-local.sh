#!/usr/bin/env bash
# ci-local.sh — Local CI pipeline for Xavier
# Usage:
#   ./scripts/ci-local.sh          # Full: check + test + clippy + fmt
#   ./scripts/ci-local.sh --quick  # Quick: check only
#   ./scripts/ci-local.sh --fix    # Auto-fix fmt + clippy
set -euo pipefail

cd "$(dirname "$0")/.."
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
PASS=0; FAIL=0

run_check() {
    local name="$1"; shift
    echo -e "${YELLOW}▶ $name${NC}"
    if "$@"; then
        echo -e "${GREEN}✅ $name passed${NC}"
        ((PASS++))
    else
        echo -e "${RED}❌ $name failed${NC}"
        ((FAIL++))
    fi
    echo
}

MODE="${1:-full}"

# Always run: cargo check
run_check "Cargo Check" cargo check -p xavier --lib

if [[ "$MODE" == "--quick" ]]; then
    echo -e "${GREEN}Quick mode: skipping test, clippy, fmt${NC}"
    exit $FAIL
fi

# Tests
run_check "Cargo Test" cargo test -p xavier --lib -- --test-threads=1

if [[ "$MODE" == "--fix" ]]; then
    echo -e "${YELLOW}▶ Auto-fixing formatting and clippy...${NC}"
    cargo fmt --all
    cargo clippy -p xavier --all-targets --fix --allow-dirty 2>/dev/null || true
    echo -e "${GREEN}✅ Auto-fix applied${NC}"
    echo
fi

# Clippy
run_check "Cargo Clippy" cargo clippy -p xavier --all-targets -- -D warnings

# Format check
run_check "Cargo Fmt" cargo fmt -p xavier -- --check

# Summary
echo "═══════════════════════════════════════"
echo -e "Results: ${GREEN}$PASS passed${NC}, ${RED}$FAIL failed${NC}"
echo "═══════════════════════════════════════"

exit $FAIL
