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
    WARMUP_OUT="$(mktemp)"
    # shellcheck disable=SC2086
    if ! (cd "$ROOT" && CARGO_TARGET_DIR="$TARGET_DIR" cargo test $spec --no-run >"$WARMUP_OUT" 2>&1); then
      echo "  ⚠ warmup failed for: $spec (continuing anyway)"
      echo "::group::warmup $spec"
      grep -E '^error|error\[|could not compile|linker|signal' "$WARMUP_OUT" | head -40 > "$WARMUP_OUT.filtered" || true
      if [ -s "$WARMUP_OUT.filtered" ]; then
        cat "$WARMUP_OUT.filtered"
      else
        tail -40 "$WARMUP_OUT"
      fi
      rm -f "$WARMUP_OUT.filtered"
      echo "::endgroup::"
      if [ "$spec" = "-p xavier --lib" ]; then
        echo "❌ build failed before any declared test could run"
        rm -f "$WARMUP_OUT"
        exit 1
      fi
    fi
    rm -f "$WARMUP_OUT"
  done
  # Old target names (modules of tests/suite_NN.rs + standalone [[test]] names) that the
  # resolver maps to a cargo target. Used for path/:: filters pointing at test targets.
  # shellcheck source=xavier-test-target.sh
  source "$ROOT/scripts/xavier-test-target.sh"
  TEST_TARGETS=""
  while IFS= read -r name; do
    [ -n "$name" ] || continue
    if resolved="$(xavier_test_suite "$name" 2>/dev/null)"; then
      TEST_TARGETS="$TEST_TARGETS $name"
    fi
  done < <( { grep -hE '^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+[A-Za-z0-9_]+[[:space:]]*;' "$ROOT"/tests/suite_[0-9][0-9].rs 2>/dev/null \
                | sed -E 's/^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+([A-Za-z0-9_]+).*/\2/';
              grep -A1 '^\[\[test\]\]' "$ROOT/Cargo.toml" | grep 'name =' | sed -E 's/.*name = "([^"]+)".*/\1/'; } || true )
  TEST_TARGETS=" $TEST_TARGETS "
  LIST_CACHE_DIR="$(mktemp -d)"
  SEEN_ERRORS_FILE="$(mktemp)"
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

    if [ "$RC" -ne 0 ] && [ "$PASSED" -eq 0 ]; then
      ERR_SIG="$(printf '%s' "$OUT" | grep -m1 'error' || true)"
      if ! grep -F -x -q "$ERR_SIG" "$SEEN_ERRORS_FILE" 2>/dev/null; then
        echo "$ERR_SIG" >> "$SEEN_ERRORS_FILE"
        printf '%s\n' "$OUT" | head -20
      fi
    fi

    FAIL_OUT="${FAIL_OUT:-}${FAIL_OUT:+$'\n'}--- $TRY_LABEL (rc=$RC) ---"$'\n'"$(printf '%s' "$OUT" | tail -12)"
    return 1
  }
  # target_features <old_name>: cargo features the xavier test target is built with.
  target_features() {
    case "$1" in
      market_v2_router_restart) echo "ci-safe,market-v2" ;;
      mesh_integration) echo "ci-safe,mesh" ;;
      *)                echo "ci-safe" ;;
    esac
  }
  # list_suite_tests <suite> <features>: libtest names of a suite, one "name: test" per line.
  # Cached per suite+features in LIST_CACHE_DIR (a listing must use the same features as the run).
  list_suite_tests() {
    local suite="$1" features="$2" cache
    cache="$LIST_CACHE_DIR/$suite-${features//,/_}"
    if [ ! -f "$cache" ]; then
      # shellcheck disable=SC2086: $TIMEOUT_BIN is an optional "timeout N" prefix
      if (cd "$ROOT" && CARGO_TARGET_DIR="$TARGET_DIR" $TIMEOUT_BIN cargo test -p xavier --features "$features" --test "$suite" -- --list --format terse) > "$cache.tmp"; then
        mv "$cache.tmp" "$cache"
      else
        rm -f "$cache.tmp"
        echo "    ⚠ cannot list tests of $suite (features: $features)" >&2
        return 1
      fi
    fi
    cat "$cache"
  }
  # suite_fn_args <features> <old_name> <fn>: sets CARGO_ARGS to run the tests of <old_name>
  # named <fn> at any module depth (empty <fn> = every test of the target), selected from the
  # suite listing and passed by exact name. Returns 1 when nothing matches.
  suite_fn_args() {
    local features="$1" base="$2" fn="$3" suite prefix pat names n
    suite="$(xavier_test_suite "$base" 2>/dev/null)" || return 1
    prefix="$(xavier_test_filter "$base" 2>/dev/null)" || return 1
    if [ -n "$fn" ]; then
      pat="^${prefix}(.*::)?${fn}: test\$"
    else
      pat="^${prefix}.*: test\$"
    fi
    names="$(list_suite_tests "$suite" "$features" | grep -E -- "$pat" | sed 's/: test$//' || true)"
    [ -n "$names" ] || return 1
    CARGO_ARGS=(test -p xavier --features "$features" --test "$suite" -- --test-threads=1 --exact)
    while IFS= read -r n; do CARGO_ARGS+=("$n"); done <<< "$names"
  }
  # file_to_invocation <file>: sets INV_LABEL (the "--test <old_name>" label printed for the
  # test; empty when the file is not compiled as a test target), INV_BASE (old target name
  # for targets selected by function filter, else empty), INV_FEATURES, INV_ARGS (cargo args
  # for targets that are not suite-grouped).
  file_to_invocation() {
    INV_LABEL=""; INV_BASE=""; INV_FEATURES=""; INV_ARGS=()
    # NOTE: specific prefixes first — bash case takes the first match,
    # so src/telegram/* must precede the generic src/* (same for mesh).
    case "$1" in
      src/market_v2/*)
        INV_LABEL="xavier market_v2"; INV_ARGS=(test -p xavier --features ci-safe,market-v2 --lib) ;;
      src/telegram/*)
        INV_LABEL="xavier telegram"; INV_ARGS=(test -p xavier --features telegram --lib) ;;
      tests/mesh_integration.rs)
        INV_LABEL="--test mesh_integration (mesh)"; INV_BASE="mesh_integration"
        INV_FEATURES="$(target_features mesh_integration)" ;;
      src/*)
        INV_LABEL="xavier --lib"; INV_ARGS=(test -p xavier --lib) ;;
      code-graph/tests/*.rs)
        local base; base="$(basename "$1" .rs)"
        INV_LABEL="code-graph --test $base"; INV_ARGS=(test -p code-graph --test "$base") ;;
      code-graph/*)
        INV_LABEL="code-graph --lib"; INV_ARGS=(test -p code-graph --lib) ;;
      crates/xavier-core-logic/*)
        INV_LABEL="xavier-core-logic"; INV_ARGS=(test -p xavier-core-logic --lib) ;;
      crates/xavier-wasm/*)
        INV_LABEL="xavier-wasm"; INV_ARGS=(test -p xavier-wasm --lib) ;;
      crates/*/tests/*.rs)
        local crate base; crate="$(echo "$1" | cut -d/ -f2)"; base="$(basename "$1" .rs)"
        INV_LABEL="crate $crate --test $base"; INV_ARGS=(test -p "$crate" --all-features --test "$base") ;;
      crates/*)
        local crate; crate="$(echo "$1" | cut -d/ -f2)"
        # Crates with cargo features run with all of them so gated tests execute.
        if [ -f "$ROOT/crates/$crate/Cargo.toml" ] && grep -q '^\[features\]' "$ROOT/crates/$crate/Cargo.toml"; then
          INV_LABEL="crate $crate (all features)"; INV_ARGS=(test -p "$crate" --all-features --lib)
        else
          INV_LABEL="crate $crate"; INV_ARGS=(test -p "$crate" --lib)
        fi ;;
      tests/integration/*.rs)
        # Modules of tests/integration.rs run in the `integration` suite (function filter
        # `integration::`); files that are their own [[test]] target run as such; anything
        # else is not compiled at all.
        local base; base="$(basename "$1" .rs)"
        if grep -qF "#[path = \"integration/$base.rs\"]" "$ROOT/tests/integration.rs" 2>/dev/null; then
          INV_LABEL="--test integration"; INV_BASE="integration"; INV_FEATURES="$(target_features integration)"
        elif [[ " $TEST_TARGETS " == *" $base "* ]]; then
          INV_LABEL="--test $base"; INV_BASE="$base"; INV_FEATURES="$(target_features "$base")"
        fi ;;
      tests/e2e/*|tests/*.rs)
        local base; base="$(basename "$1" .rs)"
        if [[ " $TEST_TARGETS " == *" $base "* ]]; then
          INV_LABEL="--test $base"; INV_BASE="$base"; INV_FEATURES="$(target_features "$base")"
        fi ;;
      *) ;;
    esac
    return 0
  }
  # invocation_args <testname> <fnname>: sets CARGO_ARGS for one test under the current INV_*
  # target. Returns 1 when a suite-grouped target has no test with that name.
  invocation_args() {
    if [ -n "$INV_BASE" ]; then
      if ! suite_fn_args "$INV_FEATURES" "$INV_BASE" "$2"; then
        echo "    … no test '$2' listed under $INV_LABEL" >&2
        return 1
      fi
      return 0
    fi
    CARGO_ARGS=("${INV_ARGS[@]}")
    case " ${INV_ARGS[*]} " in
      # --test targets match on the bare fn name: a full `a::b::fn`
      # path never substrings-matches the flat integration test name.
      *" --test "*) CARGO_ARGS+=("$2") ;;
      *)            CARGO_ARGS+=("$1") ;;
    esac
    return 0
  }
  # run_filter: locate `fn <name>` definitions with grep (no compile), run the
  # owning target(s); fall back to lib sweeps for macro-generated tests.
  # Returns 0 iff at least one invocation executed >=1 test successfully.
  # On failure, the last cargo output is kept under ./target-verify-failures/
  # (git-ignored scratch) for diagnosis instead of being swallowed.
  run_filter() {
    local testname="$1" fnname files f
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
        file_to_invocation "$f"
        [ -n "$INV_LABEL" ] || continue
        TRY_LABEL="$INV_LABEL"
        invocation_args "$testname" "$fnname" || continue
        try_cargo "${CARGO_ARGS[@]}" && return 0
      done
    fi
    # Fallback sweeps (macro-generated or oddly located tests).
    TRY_LABEL="xavier --lib";        try_cargo test -p xavier --lib "$testname" && return 0
    TRY_LABEL="code-graph --lib";    try_cargo test -p code-graph --lib "$testname" && return 0
    TRY_LABEL="xavier-core-logic";   try_cargo test -p xavier-core-logic --lib "$testname" && return 0
    # Whole-target names (or paths to one): every test of that target, by exact name.
    local tgt="$testname"
    case "$testname" in */*.rs) tgt="$(basename "$testname" .rs)" ;; esac
    if [[ " $TEST_TARGETS " == *" $tgt "* ]] && suite_fn_args "$(target_features "$tgt")" "$tgt" ""; then
      TRY_LABEL="--test $tgt"
      try_cargo "${CARGO_ARGS[@]}" && return 0
    fi
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
  # Candidate lane (planned only, NON-GATING): execute the declared tests of
  # `planned` features so the promoter has evidence for planned->beta. The
  # lane runs AFTER the gating result is decided and never changes it:
  # CAND_FAILED never touches FAILED and never changes the exit code. Lines
  # carry a `[candidate]` marker so ledger-promote-evidence can tell the
  # lanes apart. Names already in the gating TEST_LIST are skipped (already
  # evidenced by the gating lane). Each candidate gets ONE cargo attempt
  # (no retries) under $CANDIDATE_TEST_TIMEOUT (default 300s); the whole
  # lane has a $CANDIDATE_LANE_BUDGET budget (default 900s) after which the
  # remaining names report "skipped (budget)".
  if [ "$FAILED" -ne 0 ]; then
    echo "==> [4b/5] Skipping candidate lane (gating failed)..."
  else
  echo "==> [4b/5] Executing candidate tests (planned, non-gating)..."
  CAND_TIMEOUT="${CANDIDATE_TEST_TIMEOUT:-300}"
  CAND_BUDGET="${CANDIDATE_LANE_BUDGET:-900}"
  if command -v timeout >/dev/null 2>&1; then CAND_TIMEOUT_BIN="timeout $CAND_TIMEOUT"; else CAND_TIMEOUT_BIN=""; fi
  CAND_START="$SECONDS"
  # run_candidate_once: exactly ONE cargo invocation for the test (no
  # fallback sweeps/retries), under the candidate per-test timeout.
  run_candidate_once() {
    local testname="$1" fnname files f
    FAIL_OUT=""
    case "$testname" in
      *\ *)
        echo "  ⚠ candidate (non-gating) prose, not a test filter: '$testname'"
        return 1 ;;
    esac
    fnname="${testname##*::}"
    files="$(cd "$ROOT" && grep -rl --include='*.rs' -e "fn $fnname" src code-graph crates tests 2>/dev/null | head -1)"
    if [ -n "$files" ]; then
      for f in $files; do
        file_to_invocation "$f"
        [ -n "$INV_LABEL" ] || continue
        TRY_LABEL="${INV_LABEL} (candidate)"
        invocation_args "$testname" "$fnname" || return 1
        OUT="$(cd "$ROOT" && CARGO_TARGET_DIR="$TARGET_DIR" $CAND_TIMEOUT_BIN cargo "${CARGO_ARGS[@]}" 2>&1)"
        RC=$?
        PASSED="$(printf '%s' "$OUT" | grep -oE '[0-9]+ passed' | grep -oE '[0-9]+' | head -1)"
        PASSED="${PASSED:-0}"
        if [ "$RC" -eq 0 ] && [ "$PASSED" -ge 1 ]; then
          echo "    ✅ $TRY_LABEL (passed=$PASSED)"
          return 0
        fi
        return 1
      done
    fi
    TRY_LABEL="xavier --lib (candidate)"
    OUT="$(cd "$ROOT" && CARGO_TARGET_DIR="$TARGET_DIR" $CAND_TIMEOUT_BIN cargo test -p xavier --lib "$testname" 2>&1)"
    RC=$?
    PASSED="$(printf '%s' "$OUT" | grep -oE '[0-9]+ passed' | grep -oE '[0-9]+' | head -1)"
    PASSED="${PASSED:-0}"
    if [ "$RC" -eq 0 ] && [ "$PASSED" -ge 1 ]; then
      echo "    ✅ $TRY_LABEL (passed=$PASSED)"
      return 0
    fi
    return 1
  }
  CAND_LIST="$(mktemp)"
  CAND_FAILED=0
  if python3 - "$LEDGER" << 'PY' > "$CAND_LIST"
import json, sys
ledger = json.load(open(sys.argv[1]))
raw = ledger.get("features", [])
feats = list(raw.values()) if isinstance(raw, dict) else raw
for f in feats:
    if f.get("status") == "planned":
        tests = f.get("tests")
        tests = tests if isinstance(tests, list) else ([tests] if tests else [])
        for t in tests:
            print(t)
PY
  then
    while IFS= read -r testname; do
      [ -z "$testname" ] && continue
      if grep -Fxq -- "$testname" "$TEST_LIST" 2>/dev/null; then
        echo "  ▶ $testname [candidate] skipped (already in gating lane)"
        continue
      fi
      ELAPSED=$((SECONDS - CAND_START))
      if [ "$ELAPSED" -ge "$CAND_BUDGET" ]; then
        echo "  ▶ $testname [candidate] skipped (budget)"
        continue
      fi
      echo "  ▶ $testname [candidate]"
      if ! run_candidate_once "$testname"; then
        echo "  ⚠ candidate (non-gating) produced no evidence: $testname"
        CAND_FAILED=1
      fi
    done < "$CAND_LIST"
  else
    echo "  ⚠ candidate test list generation failed (non-gating, continuing)"
  fi
  rm -f "$CAND_LIST"
  if [ "$CAND_FAILED" -ne 0 ]; then
    echo "  ⚠ candidate lane had failures (non-gating — pipeline result unchanged)"
  else
    echo "  ✅ candidate lane complete"
  fi
  fi
  rm -f "$TEST_LIST"
  rm -f "$SEEN_ERRORS_FILE"
  rm -rf "$LIST_CACHE_DIR"
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
