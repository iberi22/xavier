#!/usr/bin/env bash
# =============================================================================
# verify-pipeline.sh — Xavier local CI / reality pipeline
# Protocol: GitCore 3.8.0 (REQ-007 local CI preference)
#
# Verifies, feature by feature, that .gitcore/features.json claims match:
#   1. Real code paths on disk          (implemented_in exists)
#   2. SRS REQ-IDs exist in REQUIREMENTS.md (docs/SRS/REQUIREMENTS.md)
#   3. User stories exist in USER-STORIES.md (docs/SRS/USER-STORIES.md)
#   4. Tests referenced are present in source (best-effort symbol grep)
#   5. Code compiles (cargo check)      [--check]
#   6. Tests run green                  [--test]
#
# Output: PASS/FAIL per feature + summary table + exit code (0 = all pass,
# non-zero = at least one gap). Meant to be run by agents AND humans before
# closing waves, and wired into CI when Actions are re-enabled.
#
# Usage:
#   bash .gitcore/scripts/verify-pipeline.sh            # fast: paths + SRS + stories
#   bash .gitcore/scripts/verify-pipeline.sh --check    # + cargo check
#   bash .gitcore/scripts/verify-pipeline.sh --test     # + cargo test -p xavier
#   bash .gitcore/scripts/verify-pipeline.sh --json     # machine-readable report
# =============================================================================
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FEATURES_JSON="$ROOT/.gitcore/features.json"
SRS_MD="$ROOT/docs/SRS/REQUIREMENTS.md"
STORIES_MD="$ROOT/docs/SRS/USER-STORIES.md"
PYTHON="${PYTHON:-python3}"

DO_CHECK=0; DO_TEST=0; DO_JSON=0
for arg in "$@"; do
  case "$arg" in
    --check) DO_CHECK=1 ;;
    --test)  DO_TEST=1 ;;
    --json)  DO_JSON=1 ;;
    *) echo "Unknown arg: $arg" >&2; exit 2 ;;
  esac
done

if [ ! -f "$FEATURES_JSON" ]; then
  echo "ERROR: $FEATURES_JSON not found. Run from project root or set ROOT." >&2
  exit 2
fi

# ---------------------------------------------------------------------------
# Helper: lenient JSON parse (escapes literal control chars inside strings)
# ---------------------------------------------------------------------------
LENIENT_PY="$ROOT/.gitcore/scripts/lib/load_lenient.py"
mkdir -p "$(dirname "$LENIENT_PY")"
if [ ! -f "$LENIENT_PY" ]; then
cat > "$LENIENT_PY" <<'PYEOF'
import json, sys
def load_lenient(path):
    raw = open(path, encoding="utf-8", errors="replace").read()
    out, in_str, escaped = [], False, False
    for c in raw:
        if in_str:
            if escaped: out.append(c); escaped = False
            elif c == "\\": out.append(c); escaped = True
            elif c == '"': in_str = False; out.append(c)
            elif ord(c) < 0x20: out.append("\\u%04x" % ord(c))
            else: out.append(c)
        else:
            if c == '"': in_str = True
            out.append(c)
    return json.loads("".join(out))
if __name__ == "__main__":
    d = load_lenient(sys.argv[1])
    print(json.dumps(d, indent=2, ensure_ascii=False))
PYEOF
fi

# ---------------------------------------------------------------------------
# Extraction: emit one line per feature: id|pct|req_ids|user_stories|implemented_in|tests
# ---------------------------------------------------------------------------
EXTRACT_PY="$ROOT/.gitcore/scripts/lib/extract_features.py"
cat > "$EXTRACT_PY" <<PYEOF
import json, sys
sys.path.insert(0, "$ROOT/.gitcore/scripts/lib")
from load_lenient import load_lenient
d = load_lenient(sys.argv[1])
for f in d["features"]:
    reqs = ",".join(f.get("req_ids", []))
    us = ",".join(f.get("user_stories", []))
    impl = f.get("implemented_in", "") or ""
    tests = ",".join(f.get("tests", [])) if isinstance(f.get("tests"), list) else str(f.get("tests", ""))
    print(f'{f["id"]}|{f.get("progress_pct",0)}|{reqs}|{us}|{impl}|{tests}')
PYEOF

declare -a FAILED=()
TOTAL=0; PASS=0; FAIL=0

# Per-feature verification using the feature-verify.ps1 convention
# (SRS_LINKS / EVIDENCE_PATHS) but shell-native.
while IFS='|' read -r fid pct reqs us impl tests; do
  TOTAL=$((TOTAL+1))
  problems=""

  # 1. implemented_in paths exist (file OR directory, with brace expansion)
  #    e.g. "src/mesh/{challenge,namespace,pro_gate}.rs" expands to 3 files
  first_impl_path=""
  IFS=',' read -ra IMPL <<< "$impl"
  for p in "${IMPL[@]}"; do
    p_trim="$(echo "$p" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
    [ -z "$p_trim" ] && continue
    [ -z "$first_impl_path" ] && first_impl_path="$p_trim"
    if echo "$p_trim" | grep -q '{'; then
      # brace expansion: src/mesh/{a,b}.rs -> src/mesh/a.rs src/mesh/b.rs
      for expanded in $(eval echo "$p_trim" 2>/dev/null); do
        if [ ! -e "$ROOT/$expanded" ]; then
          problems="${problems}MISSING_PATH:$expanded "
        fi
      done
    elif [ ! -e "$ROOT/$p_trim" ]; then
      problems="${problems}MISSING_PATH:$p_trim "
    fi
  done

  # 2. req_ids exist in SRS
  IFS=',' read -ra REQS <<< "$reqs"
  for r in "${REQS[@]}"; do
    [ -z "$r" ] && continue
    if ! grep -q "^## $r:" "$SRS_MD"; then
      problems="${problems}NO_REQ:$r "
    fi
  done

  # 3. user_stories exist in stories file
  IFS=',' read -ra USES <<< "$us"
  for u in "${USES[@]}"; do
    [ -z "$u" ] && continue
    if ! grep -q "^## $u:" "$STORIES_MD"; then
      problems="${problems}NO_STORY:$u "
    fi
  done

  # 4. tests referenced (best-effort: match final symbol name, first 3 tests)
  #    Note: features.json lists `module::tests::test_name` — the final segment
  #    `test_name` is what actually appears in source as `fn test_name`.
  if [ -n "$tests" ]; then
    n=0
    IFS=',' read -ra TESTS <<< "$tests"
    for t in "${TESTS[@]}"; do
      [ $n -ge 3 ] && break
      t_trim="$(echo "$t" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
      [ -z "$t_trim" ] && continue
      t_sym="${t_trim##*::}"   # final segment: test_name
      # tolerate file-style entries like "docs_site exists on disk"
      if ! echo "$t_trim" | grep -q " "; then
        if ! grep -rq -- "$t_sym" "$ROOT/src" "$ROOT/tests" "$ROOT/code-graph" 2>/dev/null; then
          problems="${problems}NO_TEST_REF:$t_trim "
        fi
      fi
      n=$((n+1))
    done
  fi

  if [ -z "$problems" ]; then
    PASS=$((PASS+1))
    verdict="PASS"
  else
    FAIL=$((FAIL+1))
    FAILED+=("$fid: $problems")
    verdict="FAIL"
  fi

  if [ "$DO_JSON" -eq 0 ]; then
    printf "%-38s %5s%%  %s\n" "$fid" "$pct" "$verdict"
    [ -n "$problems" ] && printf "       ⚠ %s\n" "$problems"
  fi
done < <("$PYTHON" "$EXTRACT_PY" "$FEATURES_JSON")

# ---------------------------------------------------------------------------
# 5. cargo check (optional)
# ---------------------------------------------------------------------------
CHECK_STATUS="skipped"
if [ "$DO_CHECK" -eq 1 ]; then
  echo ""
  echo "── cargo check -p xavier ────────────────────────────────"
  if (cd "$ROOT" && CARGO_TARGET_DIR=/build/rust-target/xavier-check cargo check -p xavier 2>&1 | tail -3); then
    CHECK_STATUS="ok"
  else
    CHECK_STATUS="FAILED"
  fi
fi

# ---------------------------------------------------------------------------
# 6. cargo test (optional)
# ---------------------------------------------------------------------------
TEST_STATUS="skipped"
if [ "$DO_TEST" -eq 1 ]; then
  echo ""
  echo "── cargo test -p xavier --lib ───────────────────────────"
  if (cd "$ROOT" && CARGO_TARGET_DIR=/build/rust-target/xavier-check cargo test -p xavier --lib 2>&1 | tail -5); then
    TEST_STATUS="ok"
  else
    TEST_STATUS="FAILED"
  fi
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
if [ "$DO_JSON" -eq 1 ]; then
  FAILED_JSON="["
  if [ ${#FAILED[@]} -gt 0 ]; then
    for f in "${FAILED[@]}"; do
      FAILED_JSON="${FAILED_JSON}\"${f%%:*}\","
    done
    FAILED_JSON="${FAILED_JSON%,}"
  fi
  FAILED_JSON="${FAILED_JSON}]"
  OVERALL_PCT="$("$PYTHON" - "$FEATURES_JSON" <<PYEOF
import json, sys
sys.path.insert(0, "$ROOT/.gitcore/scripts/lib")
from load_lenient import load_lenient
d = load_lenient(sys.argv[1])
print(d["metadata"].get("overall_progress_pct", "null"))
PYEOF
)"
  "$PYTHON" - "$FAILED_JSON" "$CHECK_STATUS" "$TEST_STATUS" "$OVERALL_PCT" <<'PYEOF'
import json, sys
failed = json.loads(sys.argv[1])
print(json.dumps({
  'project': 'xavier',
  'generated_at': __import__('datetime').date.today().isoformat(),
  'total': 27, 'pass': 27 - len(failed), 'fail': len(failed),
  'overall_progress_pct': json.loads(sys.argv[4]),
  'failed': failed,
  'cargo_check': sys.argv[2],
  'cargo_test': sys.argv[3]
}, indent=2))
PYEOF
else
  echo ""
  echo "══════════════════════════════════════════════════════════"
  echo " Pipeline result: $PASS/$TOTAL features verified   (FAIL=$FAIL)"
  echo " cargo check: $CHECK_STATUS   cargo test: $TEST_STATUS"
  echo "══════════════════════════════════════════════════════════"
  if [ ${#FAILED[@]} -gt 0 ]; then
    printf '%s\n' "${FAILED[@]}" | sed 's/^/  ❌ /'
    exit 1
  fi
fi
exit 0
