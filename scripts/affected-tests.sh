#!/usr/bin/env bash
# affected-tests.sh — check only what a change touches: clippy diagnostics in the
# changed files, the tests of their modules, and a warning for modules that have none.
#
# Maps each changed file to its module path and runs:
#   - clippy on the crate and fails ONLY on diagnostics located in changed files
#     (cargo caches the build, so this is the cost of one incremental check);
#   - lib unit tests whose path starts with that module (`memory::access`);
#   - integration targets in tests/ that import that module (`xavier::memory::access`)
#     or that were themselves changed;
#   - a warning for changed Rust modules with no #[test] at all (add real tests).
# A change to Cargo.toml / Cargo.lock only runs `cargo check`. Docs, panel-ui,
# workflows and other non-Rust files run nothing.
#
# Usage:
#   scripts/affected-tests.sh                      # files staged in git (pre-commit)
#   scripts/affected-tests.sh --base origin/main   # files changed vs a base (pre-push / PR)
#   scripts/affected-tests.sh --files a.rs b.rs    # explicit list (Jules bodies, gates)
#   scripts/affected-tests.sh --dry-run            # print the commands only
#   scripts/affected-tests.sh --no-clippy          # tests only
#
# TMPDIR is forced to /tmp; cargo runs under the shared build lock
# (/tmp/orch-1008/cargo.lock) when flock is present, so parallel builds serialize.
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT" || exit 1

MODE="staged"; BASE=""; DRY=0; CLIPPY=1; FILES=()
while [ $# -gt 0 ]; do
    case "$1" in
        --base)      MODE="base"; BASE="$2"; shift 2 ;;
        --files)     MODE="files"; shift; while [ $# -gt 0 ] && [ "${1#--}" = "$1" ]; do FILES+=("$1"); shift; done ;;
        --dry-run)   DRY=1; shift ;;
        --no-clippy) CLIPPY=0; shift ;;
        *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
done

case "$MODE" in
    staged) mapfile -t FILES < <(git diff --cached --name-only --diff-filter=ACMR) ;;
    base)   mapfile -t FILES < <(git diff --name-only --diff-filter=ACMR "$BASE"...HEAD) ;;
esac
[ "${#FILES[@]}" -eq 0 ] && { echo "affected-tests: no changed files"; exit 0; }

declare -A LIB_FILTERS=() TEST_TARGETS=() RS_CHANGED=()
CHECK_ONLY=0

add_targets_for_module() {
    local mod="$1"
    LIB_FILTERS["$mod"]=1
    while IFS= read -r t; do
        [ -n "$t" ] && TEST_TARGETS["$(basename "$t" .rs)"]=1
    done < <(grep -l -E "xavier::${mod}\b" tests/*.rs 2>/dev/null)
}

for f in "${FILES[@]}"; do
    case "$f" in
        Cargo.toml|Cargo.lock|*/Cargo.toml) CHECK_ONLY=1 ;;
        tests/*.rs)
            TEST_TARGETS["$(basename "$f" .rs)"]=1
            RS_CHANGED+=("$f") ;;
        src/*.rs)
            rel="${f#src/}"; rel="${rel%.rs}"; rel="${rel%/mod}"
            case "$rel" in lib|main|main_tui) continue ;; esac
            mod="${rel//\//::}"
            add_targets_for_module "$mod"
            RS_CHANGED+=("$f")
            ;;
        *) : ;;   # docs, panel-ui, workflows, scripts: nothing to run
    esac
done

run() {
    local cmd=(env TMPDIR=/tmp cargo "$@")
    if [ "$DRY" -eq 1 ]; then printf 'DRY: %s\n' "${cmd[*]}"; return 0; fi
    if command -v flock >/dev/null 2>&1; then
        mkdir -p /tmp/orch-1008 && flock -o /tmp/orch-1008/cargo.lock "${cmd[@]}"
    else
        "${cmd[@]}"
    fi
}

RC=0

# 1. Clippy: whole-crate analysis (cached), failing only on changed files.
if [ "$CLIPPY" -eq 1 ] && [ "${#RS_CHANGED[@]}" -gt 0 ]; then
    echo "affected-tests: clippy on changed files (${#RS_CHANGED[@]})"
    if [ "$DRY" -eq 1 ]; then
        printf 'DRY: env TMPDIR=/tmp cargo clippy -p xavier --features ci-safe --all-targets --message-format=short\n'
    else
        CLOG="$(mktemp -t affected-clippy.XXXXXX)"
        run clippy -p xavier --features ci-safe --all-targets --message-format=short >"$CLOG" 2>&1 || true
        PAT="$(printf '%s|' "${RS_CHANGED[@]}" | sed 's/|$//; s/\./\\./g')"
        HITS="$(grep -E "^(${PAT}):[0-9]+:[0-9]+: (warning|error)" "$CLOG" || true)"
        if [ -n "$HITS" ]; then
            echo "❌ clippy: diagnostics in changed files:"
            printf '%s\n' "$HITS" | head -40
            RC=1
        else
            echo "✅ clippy: no diagnostics in changed files"
        fi
        rm -f "$CLOG"
    fi
fi

# 2. Tests for the touched modules.
if [ "$CHECK_ONLY" -eq 1 ]; then
    echo "affected-tests: manifest changed → cargo check only"
    run check -p xavier --features ci-safe --all-targets || RC=1
fi
for m in "${!LIB_FILTERS[@]}"; do
    echo "affected-tests: lib filter ${m}"
    run test -p xavier --features ci-safe --lib -- --test-threads=1 "$m" || RC=1
done
for t in "${!TEST_TARGETS[@]}"; do
    echo "affected-tests: integration target ${t}"
    run test -p xavier --features ci-safe --test "$t" -- --test-threads=1 || RC=1
done

# 3. Changed production modules with no tests at all: say so (add real tests).
for f in "${RS_CHANGED[@]}"; do
    case "$f" in src/*.rs)
        if ! grep -q '#\[test\]\|#\[tokio::test\]' "$f" 2>/dev/null; then
            rel="${f#src/}"; rel="${rel%.rs}"; mod="${rel//\//::}"
            if ! grep -l -E "xavier::${mod}\b" tests/*.rs >/dev/null 2>&1; then
                echo "⚠  sin tests: $f (añade un #[cfg(test)] mod tests con casos reales para lo que cambias)"
            fi
        fi ;;
    esac
done

if [ "${#LIB_FILTERS[@]}" -eq 0 ] && [ "${#TEST_TARGETS[@]}" -eq 0 ] && [ "$CHECK_ONLY" -eq 0 ]; then
    echo "affected-tests: no Rust module or test target affected"
fi
exit "$RC"
