#!/usr/bin/env bash
# Tests for scripts/delivery-pipeline.sh: temp repo only, no network, no real state.
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PIPELINE="$DIR/../delivery-pipeline.sh"
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }

REPO="$T/repo"
mkdir -p "$REPO/src"
git -C "$REPO" init -q -b main
echo "fn a() {}" > "$REPO/src/a.rs"
git -C "$REPO" add src/a.rs
git -C "$REPO" commit -q -m base
git -C "$REPO" tag base
echo "fn a() { let _x = 1; }" > "$REPO/src/a.rs"
git -C "$REPO" commit -q -am change
SHA=$(git -C "$REPO" rev-parse HEAD)

cat > "$T/reviewer.sh" <<'INNER'
#!/usr/bin/env bash
cat >/dev/null
echo '{"verdict":"approve","notes":"ok"}'
INNER
chmod +x "$T/reviewer.sh"

export DONE_REVIEW_CMD="$T/reviewer.sh"
export PIPELINE_REPO="$REPO"
export PIPELINE_EXECUTOR="exec-a"
export PIPELINE_REVIEWER="rev-b"
for n in 1 2 3 4 5 6 7 8; do
    export "PIPELINE_STAGE_${n}_CMD=echo stage-$n ok"
done
export PIPELINE_STAGE_6_CMD="echo cargo test ok"

run() { # run <out-dir> [args...]
    local out="$1"; shift
    PIPELINE_ARTIFACT_DIR="$out/art" bash "$PIPELINE" run --sha "$SHA" --base base --task T-1 --out "$out/bundle.json" "$@"
}

# 1. happy path
mkdir -p "$T/h1" "$T/h2"
run "$T/h1" >"$T/h1/out.txt" || { cat "$T/h1/out.txt"; fail "happy path exited nonzero"; }
B="$T/h1/bundle.json"
[ "$(jq '.stages | length' "$B")" = 8 ] || fail "expected 8 stages"
[ "$(jq -r '[.stages[].exit] | all(. == 0)' "$B")" = true ] || fail "stage exits"
[ "$(jq -r '.class' "$B")" = show ] || fail "class should be show, got $(jq -r '.class' "$B")"
[ "$(jq -r '.sha' "$B")" = "$SHA" ] || fail "sha"
grep -q '"done":true' "$T/h1/out.txt" || fail "verifier did not report done"
bash "$PIPELINE" verify "$B" >/dev/null || fail "verify of untouched bundle"

# 2. second run: identical bundle (re-runnable, deterministic)
run "$T/h1" >/dev/null || fail "rerun in place"
cp "$B" "$T/first.json"
run "$T/h1" >/dev/null
cmp -s "$B" "$T/first.json" || fail "rerun changed the bundle"
PIPELINE_ARTIFACT_DIR="$T/h1/art" bash "$PIPELINE" run --sha "$SHA" --base base --task T-1 --out "$T/h2/bundle.json" >/dev/null
cmp -s "$B" "$T/h2/bundle.json" || fail "independent run differs"

# 3. a failing stage stops the chain
mkdir -p "$T/f"
set +e
PIPELINE_STAGE_3_CMD="echo boom; exit 7" run "$T/f" >"$T/f/out.txt" 2>&1
rc=$?
set -e
[ "$rc" -ne 0 ] || fail "failing stage must fail the run"
[ "$(jq '.stages | length' "$T/f/bundle.json")" = 3 ] || fail "chain should stop at stage 3"
[ "$(jq '.stages[2].exit' "$T/f/bundle.json")" = 7 ] || fail "stage 3 exit not recorded"
[ ! -e "$T/f/art/stage-4.log" ] || fail "stage 4 must not run"
if bash "$PIPELINE" verify "$T/f/bundle.json" >/dev/null 2>&1; then fail "failed bundle verified"; fi

# 4. tampering after the fact is rejected
tamper() { # tamper <name> <jq filter>
    jq "$2" "$B" > "$T/$1.json"
    if bash "$PIPELINE" verify "$T/$1.json" >/dev/null 2>&1; then fail "tamper '$1' accepted"; fi
}
tamper sha '.sha = "0000000000000000000000000000000000000000"'
tamper exit '.stages[2].exit = 1'
tamper class '.class = "ship"'
tamper cmd '.commands[0] = "true"'
cp "$B" "$T/art-tamper.json"
echo tampered >> "$T/h1/art/stage-2.log"
if bash "$PIPELINE" verify "$T/art-tamper.json" >/dev/null 2>&1; then fail "artifact tamper accepted"; fi

# 5. a sha that is not the checked-out commit is refused
if bash "$PIPELINE" run --sha 0000000000000000000000000000000000000000 --base base --task T-1 --out "$T/x.json" >/dev/null 2>&1; then
    fail "unknown sha accepted"
fi

echo "delivery-pipeline tests passed"
