#!/usr/bin/env bash
# =============================================================================
# Xavier Feature Verification Pipeline (public harness)
#
# Validates docs/features/features.json and EXECUTES the tests declared by each
# feature. The ledger is the source of truth; this pipeline is the judge.
#
#   Usage:  scripts/verify-pipeline.sh [--strict] [--check-only]
#   Exit:   0 = all green, 1 = any failure, 2 = preflight/deps missing
#
#   --check-only : validate structure + file existence only (skip test runs)
#   --strict     : also require zero TODO/FIXME stubs in implemented files
#
# Supports both ledger formats: list (gitcore 3.8 sample) and dict
# (xavier native: { "features": { "feat-id": {...} } }).
#
# Environment: all paths derive from the repo root (this script's parent dir).
# No personal configuration required.
# =============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LEDGER="$ROOT/.gitcore/features.json"
# Fallback for legacy layout (docs/features)
if [ ! -f "$LEDGER" ] && [ -f "$ROOT/docs/features/features.json" ]; then
  LEDGER="$ROOT/docs/features/features.json"
fi
MODE="full"
STRICT=0

for arg in "$@"; do
  case "$arg" in
    --check-only) MODE="check" ;;
    --strict)     STRICT=1 ;;
  esac
done

# ---------------------------------------------------------------- preflight
fail() { echo "❌ $*" >&2; exit 2; }
for tool in python3 git; do
  command -v "$tool" >/dev/null 2>&1 || fail "missing required tool: $tool"
done
[ -f "$LEDGER" ] || fail "ledger not found: $LEDGER"
command -v cargo >/dev/null 2>&1 || echo "⚠ cargo not found — test execution will be skipped (structure only)"

echo "==> Xavier verify pipeline (mode: $MODE, root: $ROOT)"

# ---------------------------------------------------------------- structure
echo "==> [1/5] Validating ledger schema..."
python3 - "$LEDGER" << 'PY'
import json, sys
ledger = json.load(open(sys.argv[1]))
raw = ledger.get("features", [])
# normalize: list of entries OR dict {id: entry}
if isinstance(raw, dict):
    feats = list(raw.values())
else:
    feats = raw
assert isinstance(feats, list), "features must be a list or dict"
ids = set()
def norm_list(v):
    """accept list or single string"""
    if v is None:
        return []
    return v if isinstance(v, list) else [v]
for f in feats:
    fid = f.get("id") or (f.get("name") if isinstance(f, dict) else None)
    assert fid, f"feature missing id: {f}"
    assert fid not in ids, f"duplicate id: {fid}"
    ids.add(fid)
    assert f.get("status") in ("planned", "beta", "stable", "active", "implemented", "in_progress"), \
        f"{fid}: bad status {f.get('status')}"
    assert isinstance(norm_list(f.get("tests")), list), f"{fid}: tests must be a list or string"
    assert isinstance(norm_list(f.get("implemented_in")), list), f"{fid}: implemented_in must be a list or string"
print(f"  ✅ ledger OK: {len(feats)} features")
PY

# ---------------------------------------------------------------- existence
echo "==> [2/5] Checking implemented_in[] paths..."
python3 - "$LEDGER" "$ROOT" << 'PY' || exit 1
import json, os, sys
ledger, root = json.load(open(sys.argv[1])), sys.argv[2]
raw = ledger.get("features", [])
feats = list(raw.values()) if isinstance(raw, dict) else raw
bad = 0
def norm_paths(v):
    """accept list, string, or comma-separated string of paths"""
    if v is None:
        return []
    if isinstance(v, str):
        return [p.strip() for p in v.split(",") if p.strip()]
    return v
for f in feats:
    for p in norm_paths(f.get("implemented_in")):
        if not os.path.exists(os.path.join(root, p)):
            print(f"  ❌ {f.get('id')}: missing file {p}")
            bad += 1
if bad:
    print(f"  ❌ {bad} missing paths — failing pipeline")
    sys.exit(1)
print("  ✅ all paths exist")
PY

# ---------------------------------------------------------------- score
echo "==> [3/5] Computing implementation score..."
python3 - "$LEDGER" << 'PY'
import json, sys
ledger = json.load(open(sys.argv[1]))
raw = ledger.get("features", [])
feats = list(raw.values()) if isinstance(raw, dict) else raw
n = len(feats)
stable = sum(1 for f in feats if f.get("status") == "stable")
beta = sum(1 for f in feats if f.get("status") in ("beta", "implemented", "active"))
pct = round(sum(f.get("progress_pct", 0) for f in feats) / n, 1) if n else 0
print(f"  stable={stable} beta={beta} planned={n-stable-beta} total={n} real%={pct}")
PY

# ---------------------------------------------------------------- tests
if [ "$MODE" = "full" ] && command -v cargo >/dev/null 2>&1; then
  echo "==> [4/5] Executing declared tests (stable + beta)..."
  FAILED=0
  ZERO_MATCH=0
  TEST_LIST="$(mktemp)"
  python3 - "$LEDGER" << 'PY' > "$TEST_LIST" || { echo "❌ test list generation failed"; rm -f "$TEST_LIST"; exit 1; }
import json, sys
ledger = json.load(open(sys.argv[1]))
raw = ledger.get("features", [])
feats = list(raw.values()) if isinstance(raw, dict) else raw
for f in feats:
    if f.get("status") in ("stable", "beta", "implemented", "active", "in_progress"):
        tests = f.get("tests")
        tests = tests if isinstance(tests, list) else ([tests] if tests else [])
        for t in tests:
            print(t)
PY
  TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/xavier-verify-target}"
  # NOTE: never default to /build (small tmpfs ramdisk, fills up); /tmp lives
  # on the big disk. Override with CARGO_TARGET_DIR if you know better.
  PER_ATTEMPT="${XAVIER_VERIFY_PER_ATTEMPT_SECS:-600}"
  if command -v timeout >/dev/null 2>&1; then TIMEOUT_BIN="timeout $PER_ATTEMPT"; else TIMEOUT_BIN=""; fi
  # Warmup: compile lib test binaries once WITHOUT the per-attempt timeout,
  # so the first filters don't burn their budget on a cold target dir.
  echo "  … warming test binaries (one-time compile, may take minutes) …"
  for spec in "-p xavier --lib" "-p code-graph --lib" "-p xavier-core-logic --lib" "-p xavier --features telegram --lib" "-p xavier --features mesh --lib"; do
    # shellcheck disable=SC2086
    (cd "$ROOT" && CARGO_TARGET_DIR="$TARGET_DIR" cargo test $spec --no-run >/dev/null 2>&1) \
      || echo "  ⚠ warmup failed for: $spec (continuing anyway)"
  done
  # Declared [[test]] target names (for path/:: filters pointing at integration targets).
  TEST_TARGETS="$(grep -A1 '^\[\[test\]\]' "$ROOT/Cargo.toml" | grep 'name =' | sed -E 's/.*name = "([^"]+)".*/\1/' | tr '\n' ' ')"
  # try_cargo: run one cargo invocation, set OUT/RC/PASSED. Echoes the label on success.
  try_cargo() {
    OUT="$(cd "$ROOT" && CARGO_TARGET_DIR="$TARGET_DIR" $TIMEOUT_BIN cargo "$@" 2>&1)"
    RC=$?
    PASSED="$(printf '%s' "$OUT" | grep -oE '[0-9]+ passed' | grep -oE '[0-9]+' | head -1)"
    PASSED="${PASSED:-0}"
    echo "    … attempt [$TRY_LABEL]: rc=$RC passed=$PASSED" >&2
    if [ "$RC" -eq 0 ] && [ "$PASSED" -ge 1 ]; then
      echo "    ✅ $TRY_LABEL (passed=$PASSED)"
      return 0
    fi
    FAIL_OUT="${FAIL_OUT:-}${FAIL_OUT:+$'\n'}--- $TRY_LABEL (rc=$RC) ---"$'\n'"$(printf '%s' "$OUT" | tail -12)"
    return 1
  }
  # file_to_invocation: map a defining source file to "label|cargo args".
  file_to_invocation() {
    # NOTE: specific prefixes first — bash case takes the first match,
    # so src/telegram/* must precede the generic src/* (same for mesh).
    case "$1" in
      src/telegram/*)
        echo "xavier telegram|test -p xavier --features telegram --lib" ;;
      tests/mesh_integration.rs)
        echo "--test mesh_integration (mesh)|test --features mesh --test mesh_integration" ;;
      src/*)            echo "xavier --lib|test -p xavier --lib" ;;
      code-graph/*)     echo "code-graph --lib|test -p code-graph --lib" ;;
      crates/xavier-core-logic/*) echo "xavier-core-logic|test -p xavier-core-logic --lib" ;;
      crates/xavier-wasm/*)       echo "xavier-wasm|test -p xavier-wasm --lib" ;;
      crates/*)         echo "crate $(echo "$1" | cut -d/ -f2)|test -p $(echo "$1" | cut -d/ -f2) --lib" ;;
      tests/e2e/*|tests/*.rs)
        local base; base="$(basename "$1" .rs)"
        echo "--test $base|test --test $base" ;;
      *)                echo "" ;;
    esac
  }
  # run_filter: locate `fn <name>` definitions with grep (no compile), run the
  # owning target(s); fall back to lib sweeps for macro-generated tests.
  # Returns 0 iff at least one invocation executed >=1 test successfully.
  # On failure, the last cargo output is kept under ./target-verify-failures/
  # (git-ignored scratch) for diagnosis instead of being swallowed.
  run_filter() {
    local testname="$1" fnname files f inv filter_arg
    FAIL_OUT=""
    case "$testname" in
      *\ *)
        echo "  ❌ LEDGER-FIX NEEDED: '$testname' is prose, not a test filter"
        return 1 ;;
    esac
    fnname="${testname##*::}"
    files="$(cd "$ROOT" && grep -rl --include='*.rs' -e "fn $fnname" src code-graph crates tests 2>/dev/null | head -5)"
    if [ -n "$files" ]; then
      for f in $files; do
        inv="$(file_to_invocation "$f")"
        [ -z "$inv" ] && continue
        TRY_LABEL="${inv%%|*}"
        # --test targets match on the bare fn name: a full `a::b::fn`
        # path never substrings-matches the flat integration test name.
        case "$inv" in
          "--test "*) filter_arg="$fnname" ;;
          *) filter_arg="$testname" ;;
        esac
        # shellcheck disable=SC2086: intentional word-splitting of cargo args
        try_cargo ${inv#*|} "$filter_arg" && return 0
      done
    fi
    # Fallback sweeps (macro-generated or oddly located tests).
    TRY_LABEL="xavier --lib";        try_cargo test -p xavier --lib "$testname" && return 0
    TRY_LABEL="code-graph --lib";    try_cargo test -p code-graph --lib "$testname" && return 0
    TRY_LABEL="xavier-core-logic";   try_cargo test -p xavier-core-logic --lib "$testname" && return 0
    TRY_LABEL="--test $testname";    try_cargo test --test "$testname" && return 0
    case "$testname" in
      */*.rs)
        local base; base="$(basename "$testname" .rs)"
        TRY_LABEL="--test $base (path)"; try_cargo test --test "$base" && return 0 ;;
    esac
    return 1
  }
  while IFS= read -r testname; do
    [ -z "$testname" ] && continue
    echo "  ▶ $testname"
    # Wrap: features.json declares test NAMES (filter), not commands.
    if ! run_filter "$testname"; then
      echo "  ❌ FAILED: $testname (no invocation executed >=1 test)"
      faildir="$ROOT/target-verify-failures"
      mkdir -p "$faildir"
      printf '%s\n' "$FAIL_OUT" | tail -60 > "$faildir/$(printf '%s' "$testname" | tr -c 'A-Za-z0-9_-' '_').log"
      echo "    (last output kept in target-verify-failures/)"
      FAILED=1
      ZERO_MATCH=1
    fi
  done < "$TEST_LIST"
  rm -f "$TEST_LIST"
  if [ "$FAILED" -ne 0 ]; then
    echo "  ❌ declared tests failed (zero-match=$ZERO_MATCH) — failing pipeline"
    exit 1
  fi
  echo "  ✅ all declared tests passed"
fi

# ---------------------------------------------------------------- strict
if [ "$STRICT" -eq 1 ]; then
  echo "==> [5/5] Strict: scanning implemented files for stubs..."
  python3 - "$LEDGER" "$ROOT" << 'PY'
import json, os, re, sys
ledger, root = json.load(open(sys.argv[1])), sys.argv[2]
raw = ledger.get("features", [])
feats = list(raw.values()) if isinstance(raw, dict) else raw
pat = re.compile(r"TODO|FIXME|UnimplementedError|unimplemented!|todo!|placeholder")
hits = 0
def norm_paths(v):
    if v is None:
        return []
    if isinstance(v, str):
        return [p.strip() for p in v.split(",") if p.strip()]
    return v
for f in feats:
    for p in norm_paths(f.get("implemented_in")):
        fp = os.path.join(root, p)
        if not os.path.isfile(fp):
            continue
        for i, line in enumerate(open(fp, errors="ignore"), 1):
            if pat.search(line):
                print(f"  ⚠ {f.get('id')} {p}:{i}: {line.strip()[:70]}")
                hits += 1
print(f"  {'✅ no stubs found' if hits == 0 else f'⚠ {hits} stub markers'}")
PY
fi

echo ""
echo "==> Pipeline complete. ✅"
exit 0
