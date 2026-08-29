#!/usr/bin/env bash
# =============================================================================
# ci-local.sh — Canonical local CI parity for Xavier (issue #1639).
#
# Runs the exact gate set the SWAL standard requires, in order:
#   1. fmt     : cargo fmt --all --check
#   2. clippy  : RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
#                --features ci-safe --exclude app
#   3. check   : RUSTFLAGS="-D warnings" cargo check --workspace
#                --features ci-safe --exclude app --all-targets
#   4. test    : cargo test -p xavier --lib --features ci-safe
#   5. secrets : scripts/check-secrets.sh
#
# Fail-fast: exits non-zero on the first failing gate; a summary table is
# always printed. Honors CARGO_TARGET_DIR if exported (RAM-disk convention).
#
# Usage:
#   scripts/ci-local.sh              # run all gates in order
#   scripts/ci-local.sh <gate>       # run a single gate:
#                                    #   fmt | clippy | check | test | secrets
#
# See docs/protocol/LOCAL_CI.md for the full guide.
# =============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

GATES=("fmt" "clippy" "check" "test" "secrets")
declare -A STATUS=()
declare -A SECONDS_ELAPSED=()

if [ -n "${CARGO_TARGET_DIR:-}" ]; then
  echo "==> CARGO_TARGET_DIR=${CARGO_TARGET_DIR} (honored)"
else
  echo "==> CARGO_TARGET_DIR not set — using default target/ directory"
fi

run_gate() {
  local name="$1"
  shift
  echo ""
  echo "──────────────────────────────────────────────────────────────"
  echo "▶ [$name] $*"
  echo "──────────────────────────────────────────────────────────────"
  local start
  start=$(date +%s)
  local rc=0
  "$@" || rc=$?
  local end
  end=$(date +%s)
  SECONDS_ELAPSED[$name]=$((end - start))
  if [ "$rc" -eq 0 ]; then
    STATUS[$name]="PASS"
    echo "✅ [$name] passed (${SECONDS_ELAPSED[$name]}s)"
  else
    STATUS[$name]="FAIL"
    echo "❌ [$name] FAILED with exit code $rc (${SECONDS_ELAPSED[$name]}s)"
  fi
  return "$rc"
}

gate_cmd() {
  case "$1" in
    fmt)
      run_gate fmt cargo fmt --all --check
      ;;
    clippy)
      RUSTFLAGS="-D warnings" run_gate clippy \
        cargo clippy --workspace --all-targets --features ci-safe --exclude app
      ;;
    check)
      RUSTFLAGS="-D warnings" run_gate check \
        cargo check --workspace --features ci-safe --exclude app --all-targets
      ;;
    test)
      run_gate test cargo test -p xavier --lib --features ci-safe
      ;;
    secrets)
      run_gate secrets bash "$ROOT/scripts/check-secrets.sh"
      ;;
    *)
      echo "ERROR: unknown gate '$1' (valid: ${GATES[*]})" >&2
      exit 2
      ;;
  esac
}

print_summary() {
  echo ""
  echo "═══════════════════════════ CI LOCAL SUMMARY ═══════════════════════════"
  printf '%-10s | %-8s | %-8s\n' "GATE" "RESULT" "TIME(s)"
  printf -- '-----------+----------+--------\n'
  for g in "${GATES[@]}"; do
    local res="${STATUS[$g]:-SKIP}"
    local t="${SECONDS_ELAPSED[$g]:-—}"
    printf '%-10s | %-8s | %-8s\n' "$g" "$res" "$t"
  done
  echo "═════════════════════════════════════════════════════════════════════"
}

MODE="${1:-all}"

trap 'print_summary' EXIT

if [ "$MODE" = "all" ]; then
  for gate in "${GATES[@]}"; do
    if ! gate_cmd "$gate"; then
      exit 1
    fi
  done
else
  gate_cmd "$MODE"
fi

exit 0
