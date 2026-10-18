#!/usr/bin/env bash
# affected-tests.sh — check only what a change touches: clippy diagnostics in the
# changed files, the tests of their modules, and a warning for modules that have none.
#
# Maps each changed file to its module path and runs:
#   - clippy on the crate and fails ONLY on diagnostics located in changed files
#     (cargo caches the build, so this is the cost of one incremental check);
#   - a batched test run for all touched lib modules and affected integration targets
#     (using cargo-nextest if available, or a batched cargo test fallback). Integration
#     tests are grouped in tests/suite_NN.rs; scripts/xavier-test-target.sh resolves an
#     old target name to its suite, and a change to a suite file runs that whole suite;
#   - a warning for changed Rust modules with no #[test] at all (add real tests).
# A change to Cargo.toml / Cargo.lock only runs `cargo check`. Docs, panel-ui,
# workflows and other non-Rust files run nothing.
#
# Usage:
#   scripts/affected-tests.sh                      # files staged in git (pre-commit)
#   scripts/affected-tests.sh --quick              # clippy-on-changed-files only (no tests)
#   scripts/affected-tests.sh --base origin/main   # files changed vs a base (pre-push / PR)
#   scripts/affected-tests.sh --files a.rs b.rs    # explicit list (Jules bodies, gates)
#   scripts/affected-tests.sh --dry-run            # print the commands only
#   scripts/affected-tests.sh --no-clippy          # tests only
#   scripts/affected-tests.sh --no-integration     # skip integration targets (deferred to pre-push/CI)
#
# Concurrency and build locks:
#   TMPDIR is forced to /tmp. Compile steps (clippy, check, test --no-run) acquire a
#   build-slot semaphore from $SWAL_BUILD_LOCK_DIR (default: /tmp/orch-1008) with
#   SWAL_BUILD_SLOTS (default: 1 slot, where slot 0 is cargo.lock). If GameMode is
#   active (ClientCount > 0), SLOTS is forced to 1 and jobs are capped to 2. Test
#   executions run without holding the build-slot lock.
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT" || exit 1

# Target resolver: old [[test]] name -> suite_NN (grouped) or itself (standalone).
if [ ! -f "$ROOT/scripts/xavier-test-target.sh" ]; then
    echo "affected-tests: missing scripts/xavier-test-target.sh" >&2
    exit 2
fi
# shellcheck source=xavier-test-target.sh
source "$ROOT/scripts/xavier-test-target.sh"

MODE="staged"; BASE=""; DRY=0; CLIPPY=1; QUICK=0; NO_INTEGRATION=0; FILES=()
[ "${WI_BATCH_VALIDATE:-0}" = "1" ] && QUICK=1

while [ $# -gt 0 ]; do
    case "$1" in
        --base)            MODE="base"; BASE="$2"; shift 2 ;;
        --files)           MODE="files"; shift; while [ $# -gt 0 ] && [ "${1#--}" = "$1" ]; do FILES+=("$1"); shift; done ;;
        --dry-run)         DRY=1; shift ;;
        --no-clippy)       CLIPPY=0; shift ;;
        --quick)           QUICK=1; shift ;;
        --no-integration)  NO_INTEGRATION=1; shift ;;
        *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
done

case "$MODE" in
    staged) mapfile -t FILES < <(git diff --cached --name-only --diff-filter=ACMR) ;;
    base)   mapfile -t FILES < <(git diff --name-only --diff-filter=ACMR "$BASE"...HEAD) ;;
esac
[ "${#FILES[@]}" -eq 0 ] && { echo "affected-tests: no changed files"; exit 0; }

declare -A LIB_FILTERS=() TEST_TARGETS=(); RS_CHANGED=()
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

# Concurrency & slot configuration
is_gamemode_active() {
    if command -v busctl >/dev/null 2>&1; then
        local out
        out="$(busctl --user get-property com.feralinteractive.GameMode /com/feralinteractive/GameMode com.feralinteractive.GameMode ClientCount 2>/dev/null || true)"
        if [[ "$out" =~ i[[:space:]]+([1-9][0-9]*) ]]; then
            return 0
        fi
    fi
    return 1
}

NPROC=$(nproc 2>/dev/null || echo 2)
[ "$NPROC" -lt 1 ] && NPROC=2

# Default SWAL_BUILD_SLOTS=1, ensure it is a whole number of at least 1
SLOTS="${SWAL_BUILD_SLOTS:-1}"
case "$SLOTS" in
    ""|*[!0-9]*|0) SLOTS=1 ;;
esac

GAMEMODE_ACTIVE=0
if is_gamemode_active; then
    GAMEMODE_ACTIVE=1
    SLOTS=1
fi

LOCKDIR="${SWAL_BUILD_LOCK_DIR:-/tmp/orch-1008}"

if [ -z "${CARGO_BUILD_JOBS:-}" ]; then
    if [ "$GAMEMODE_ACTIVE" -eq 1 ]; then
        export CARGO_BUILD_JOBS=2
    else
        calc_jobs=$(( NPROC / (SLOTS > 0 ? SLOTS : 1) ))
        [ "$calc_jobs" -lt 2 ] && calc_jobs=2
        export CARGO_BUILD_JOBS="$calc_jobs"
    fi
fi

if [ -z "${NEXTEST_TEST_THREADS:-}" ]; then
    if [ "$GAMEMODE_ACTIVE" -eq 1 ]; then
        export NEXTEST_TEST_THREADS=2
    else
        calc_threads=$(( NPROC / 2 ))
        [ "$calc_threads" -lt 2 ] && calc_threads=2
        export NEXTEST_TEST_THREADS="$calc_threads"
    fi
fi

get_slot_file() {
    local idx="$1"
    if [ "$idx" -eq 0 ]; then
        echo "$LOCKDIR/cargo.lock"
    else
        echo "$LOCKDIR/cargo.lock.$idx"
    fi
}

run_build() {
    local cmd=(env TMPDIR=/tmp cargo "$@")
    if [ "$DRY" -eq 1 ]; then
        printf 'DRY: %s\n' "${cmd[*]}"
        return 0
    fi

    if ! command -v flock >/dev/null 2>&1; then
        "${cmd[@]}"
        return $?
    fi

    mkdir -p "$LOCKDIR" || true

    local acquired=0
    local slot_fd=""

    # Try non-blocking acquisition across available slots
    for (( i=0; i<SLOTS; i++ )); do
        local slot_file
        slot_file="$(get_slot_file "$i")"
        exec {slot_fd}>"$slot_file"
        if flock -n "$slot_fd" 2>/dev/null; then
            acquired=1
            break
        else
            exec {slot_fd}>&-
        fi
    done

    # If no slot was immediately free, block on slot-0 (cargo.lock)
    if [ "$acquired" -eq 0 ]; then
        local slot_file
        slot_file="$(get_slot_file 0)"
        exec {slot_fd}>"$slot_file"
        flock "$slot_fd"
    fi

    local status=0
    ( exec {slot_fd}>&-; exec "${cmd[@]}" ) || status=$?

    # Release slot
    exec {slot_fd}>&-
    return "$status"
}

run_test_unlocked() {
    if [ "$DRY" -eq 1 ]; then
        printf 'DRY: env TMPDIR=/tmp'; printf ' %s' "$@"; printf '\n'
        return 0
    fi
    env TMPDIR=/tmp "$@"
}

RC=0

# 1. Clippy: whole-crate analysis (cached), failing only on changed files.
if [ "$CLIPPY" -eq 1 ] && [ "${#RS_CHANGED[@]}" -gt 0 ]; then
    echo "affected-tests: clippy on changed files (${#RS_CHANGED[@]})"
    if [ "$DRY" -eq 1 ]; then
        printf 'DRY: env TMPDIR=/tmp cargo clippy -p xavier --features ci-safe --all-targets --message-format=short\n'
    else
        CLOG="$(mktemp -t affected-clippy.XXXXXX)"
        run_build clippy -p xavier --features ci-safe --all-targets --message-format=short >"$CLOG" 2>&1 || true
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

# Quick mode handling
if [ "$QUICK" -eq 1 ]; then
    if [ "${#RS_CHANGED[@]}" -eq 0 ] && [ "$CHECK_ONLY" -eq 1 ]; then
        echo "affected-tests: manifest changed → cargo check only"
        run_build check -p xavier --features ci-safe --all-targets || RC=1
    fi
    exit "$RC"
fi

# 2. Tests for the touched modules.
if [ "$CHECK_ONLY" -eq 1 ]; then
    echo "affected-tests: manifest changed → cargo check only"
    run_build check -p xavier --features ci-safe --all-targets || RC=1
fi

# Collect sorted unique target and lib filter lists
SORTED_TARGETS=()
if [ "${#TEST_TARGETS[@]}" -gt 0 ]; then
    mapfile -t SORTED_TARGETS < <(printf '%s\n' "${!TEST_TARGETS[@]}" | sort)
fi

SORTED_LIB_FILTERS=()
if [ "${#LIB_FILTERS[@]}" -gt 0 ]; then
    mapfile -t SORTED_LIB_FILTERS < <(printf '%s\n' "${!LIB_FILTERS[@]}" | sort)
fi

if [ "$NO_INTEGRATION" -eq 1 ] && [ "${#SORTED_TARGETS[@]}" -gt 0 ]; then
    echo "affected-tests: deferred to pre-push/CI: ${SORTED_TARGETS[*]}"
fi

# Resolve each changed target name to the unit that actually runs it:
#   - tests/suite_NN.rs changed            -> SUITE_ALL[suite_NN] (whole suite binary)
#   - old name grouped into suite_NN       -> SUITE_FILTERS[suite_NN] += "<name>::"
#   - old name that is a standalone [[test]] -> STANDALONE[<name>]
declare -A SUITE_ALL=() SUITE_FILTERS=() STANDALONE=()
UNRESOLVED=()
for t in "${SORTED_TARGETS[@]}"; do
    if [[ "$t" =~ ^suite_[0-9]+$ ]] && [ -f "tests/$t.rs" ]; then
        SUITE_ALL["$t"]=1
        continue
    fi
    suite="$(xavier_test_suite "$t" 2>/dev/null || true)"
    if [ -z "$suite" ]; then
        UNRESOLVED+=("$t")
        continue
    fi
    filter="$(xavier_test_filter "$t" 2>/dev/null || true)"
    if [ -z "$filter" ]; then
        STANDALONE["$suite"]=1
    elif [[ " ${SUITE_FILTERS[$suite]-} " != *" $filter "* ]]; then
        SUITE_FILTERS["$suite"]+=" $filter"
    fi
done
if [ "${#UNRESOLVED[@]}" -gt 0 ]; then
    echo "❌ affected-tests: no suite or [[test]] target for: ${UNRESOLVED[*]} (add the file to a tests/suite_NN.rs #[path] module or a [[test]] entry)"
    RC=1
fi

ACTIVE_SUITES=()
ACTIVE_STANDALONE=()
if [ "$NO_INTEGRATION" -eq 0 ]; then
    map_keys() { local -n _map="$1"; local _k; for _k in "${!_map[@]}"; do printf '%s\n' "$_k"; done; }
    if [ "${#SUITE_ALL[@]}" -gt 0 ] || [ "${#SUITE_FILTERS[@]}" -gt 0 ]; then
        mapfile -t ACTIVE_SUITES < <({ map_keys SUITE_ALL; map_keys SUITE_FILTERS; } | sort -u)
    fi
    if [ "${#STANDALONE[@]}" -gt 0 ]; then
        mapfile -t ACTIVE_STANDALONE < <(map_keys STANDALONE | sort -u)
    fi
fi
# Units that get a --test argument (deduplicated: each suite once, each standalone once)
ACTIVE_UNITS=("${ACTIVE_SUITES[@]}" "${ACTIVE_STANDALONE[@]}")

# Compute the union of required-features across selected test units
declare -A FEAT_SET=( ["ci-safe"]=1 )
for t in "${ACTIVE_UNITS[@]}"; do
    rf=$(awk -v t="$t" '/^\[\[/{intest=($0=="[[test]]"); name=""} intest && /^name *=/{n=$0; gsub(/^name *= *"|".*$/,"",n); name=n} intest && name==t && /^required-features/{v=$0; gsub(/^[^[]*\[|\].*$|"| /,"",v); print v; exit}' Cargo.toml)
    if [ -n "$rf" ]; then
        IFS=',' read -ra ADDFEATS <<< "$rf"
        for feat in "${ADDFEATS[@]}"; do
            [ -n "$feat" ] && FEAT_SET["$feat"]=1
        done
    fi
done
# FEATURES="ci-safe" plus every rf, comma-joined, de-duplicated (ci-safe first)
FEATURES="ci-safe"
for feat in "${!FEAT_SET[@]}"; do
    if [ "$feat" != "ci-safe" ]; then
        FEATURES="${FEATURES},${feat}"
    fi
done

HAS_TESTS=0
if [ "${#SORTED_LIB_FILTERS[@]}" -gt 0 ] || [ "${#ACTIVE_UNITS[@]}" -gt 0 ]; then
    HAS_TESTS=1
fi

if [ "$HAS_TESTS" -eq 1 ]; then
    # Construct build args: -p xavier --features "$FEATURES" [--lib] [--test <suite|standalone> ...]
    BUILD_ARGS=("-p" "xavier" "--features" "$FEATURES")
    if [ "${#SORTED_LIB_FILTERS[@]}" -gt 0 ]; then
        BUILD_ARGS+=("--lib")
    fi
    for u in "${ACTIVE_UNITS[@]}"; do
        BUILD_ARGS+=("--test" "$u")
    done

    # Step 1 (BUILD, under the build slot): cargo test --no-run <build args>
    if ! run_build test --no-run "${BUILD_ARGS[@]}"; then
        RC=1
    else
        # Step 2 (RUN, NOT under any lock)
        if cargo nextest --version >/dev/null 2>&1; then
            # Construct nextest filter expression
            EXPR_PARTS=()
            if [ "${#SORTED_LIB_FILTERS[@]}" -gt 0 ]; then
                LIB_PARTS=()
                for m in "${SORTED_LIB_FILTERS[@]}"; do
                    LIB_PARTS+=("test(/^${m}::/)")
                done
                LIB_EXPR=$(IFS='|'; echo "${LIB_PARTS[*]}")
                EXPR_PARTS+=("(kind(lib) & (${LIB_EXPR}))")
            fi
            for s in "${ACTIVE_SUITES[@]}"; do
                if [ -n "${SUITE_ALL[$s]-}" ]; then
                    EXPR_PARTS+=("binary(=${s})")
                else
                    read -ra SUITE_NAMES <<< "${SUITE_FILTERS[$s]-}"
                    for n in "${SUITE_NAMES[@]}"; do
                        EXPR_PARTS+=("(binary(=${s}) & test(/^${n}/))")
                    done
                fi
            done
            for s in "${ACTIVE_STANDALONE[@]}"; do
                EXPR_PARTS+=("binary(=${s})")
            done
            FULL_EXPR=$(IFS='|'; echo "${EXPR_PARTS[*]}")

            run_test_unlocked cargo nextest run --profile ci --no-fail-fast --no-tests=warn "${BUILD_ARGS[@]}" -E "$FULL_EXPR" || RC=1
        else
            # Fallback without nextest: at most TWO run invocations, not under any lock
            if [ "${#SORTED_LIB_FILTERS[@]}" -gt 0 ]; then
                run_test_unlocked cargo test -p xavier --features "$FEATURES" --lib -- --test-threads=1 "${SORTED_LIB_FILTERS[@]}" || RC=1
            fi
            # One invocation per suite: whole suite if its file changed, else only the named modules.
            for s in "${ACTIVE_SUITES[@]}"; do
                FALLBACK_CMD=(cargo test -p xavier --features "$FEATURES" --test "$s" -- --test-threads=1)
                if [ -z "${SUITE_ALL[$s]-}" ]; then
                    read -ra SUITE_NAMES <<< "${SUITE_FILTERS[$s]-}"
                    FALLBACK_CMD+=("${SUITE_NAMES[@]}")
                fi
                run_test_unlocked "${FALLBACK_CMD[@]}" || RC=1
            done
            # Standalone [[test]] targets share one invocation.
            if [ "${#ACTIVE_STANDALONE[@]}" -gt 0 ]; then
                FALLBACK_TARGET_ARGS=()
                for s in "${ACTIVE_STANDALONE[@]}"; do
                    FALLBACK_TARGET_ARGS+=("--test" "$s")
                done
                run_test_unlocked cargo test -p xavier --features "$FEATURES" "${FALLBACK_TARGET_ARGS[@]}" -- --test-threads=1 || RC=1
            fi
        fi
    fi
fi

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
