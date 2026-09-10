#!/usr/bin/env bash
# scripts/coverage.sh — WAVE-10.01 cargo-tarpaulin baseline
# Usage: bash scripts/coverage.sh [--html] [--module mesh|crypto|session|all]
set -euo pipefail

MODULE="${1:-all}"
OUTPUT_DIR="coverage"
mkdir -p "$OUTPUT_DIR"

# Args to tarpaulin
# NOTE 2026-09-09: scoped to --package xavier --lib (same gate as CI test job).
# --workspace pulls panel-ui/src-tauri -> glib-sys, which needs system
# libglib2.0-dev absent on ubuntu-latest (see tarpaulin infra failure).
ARGS=(
  --package xavier
  --lib
  --features ci-safe
  --timeout 180
  --exclude-files "tests/*"
  --exclude-files "benches/*"
  --exclude-files "*/target/*"
  --out Html
  --out Xml
  --output-dir "$OUTPUT_DIR"
)

# Engine override: TARPAULIN_ENGINE=llvm (nightly + llvm-tools) works around
# ptrace-engine const-eval failures on SIMD crates (pulp via qrcode).
# MUST come before the `--` test-args separator below.
if [ -n "${TARPAULIN_ENGINE:-}" ]; then
  ARGS+=(--engine "$TARPAULIN_ENGINE")
fi

# Serial like the CI test gate: env-var tests (PLANKKA_*) race in parallel.
ARGS+=(-- --test-threads=1)

case "$MODULE" in
  mesh)
    ARGS+=(--include-files "src/mesh/*")
    ;;
  crypto)
    ARGS+=(--include-files "src/crypto/*")
    ;;
  session)
    ARGS+=(--include-files "src/session/*")
    ;;
  all|*)
    # full workspace coverage
    ;;
esac

echo "==> Running cargo-tarpaulin (module=$MODULE)"
cargo tarpaulin "${ARGS[@]}"

echo ""
echo "==> Coverage report written to $OUTPUT_DIR/"
echo "    HTML: $OUTPUT_DIR/tarpaulin-report.html"
echo "    XML:  $OUTPUT_DIR/cobertura.xml"

# Quick summary from XML
if [[ -f "$OUTPUT_DIR/cobertura.xml" ]]; then
  total=$(grep -oP 'line-rate="[0-9.]+"' "$OUTPUT_DIR/cobertura.xml" | head -1 | grep -oP '[0-9.]+')
  if [[ -n "$total" ]]; then
    pct=$(python3 -c "print(round(float('$total') * 100, 2))")
    echo "    Total line coverage: ${pct}%"
  fi
fi
