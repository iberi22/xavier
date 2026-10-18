#!/usr/bin/env bash
# xavier-test-target-test.sh — every HEAD test target name resolves to a suite or standalone target.
#
# Names come from the base ref (default 476ba34a, the pre-grouping commit; override with XAVIER_TEST_BASE):
#   - [[test]] entries of Cargo.toml at the base ref,
#   - basenames of tests/*.rs at the base ref (explicit [[test]] paths keep their declared name),
#   - the explicit subdirectory [[test]] entries (paths outside tests/*.rs).
# 167 root files + 19 subdirectory targets = 186 targets, 3 of them standalone.
set -uo pipefail

ROOT="$(git -C "$(dirname -- "${BASH_SOURCE[0]}")" rev-parse --show-toplevel 2>/dev/null)" || ROOT=""
if [ -z "$ROOT" ]; then
    ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)"
fi
cd "$ROOT" || exit 1

PASS=0
FAIL=0
pass() { PASS=$((PASS + 1)); echo "PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "FAIL: $1"; }

BASE="${XAVIER_TEST_BASE:-476ba34a}"
if ! git rev-parse --verify -q "${BASE}^{commit}" >/dev/null 2>&1; then
    echo "SKIP: base commit '${BASE}' not available locally (fetch it or set XAVIER_TEST_BASE)"
    exit 0
fi

# shellcheck source=../xavier-test-target.sh
. "$ROOT/scripts/xavier-test-target.sh"

# --- Target names at the base ref -------------------------------------------------------
explicit_rows="$(git show "${BASE}:Cargo.toml" | awk '
    function flush() { if (in_t && name != "" && path != "") print name " " path; name = ""; path = "" }
    /^\[/ { flush(); in_t = ($0 ~ /^\[\[test\]\]/); next }
    in_t && /^name[[:space:]]*=/ { v = $0; sub(/^[^=]*=[[:space:]]*/, "", v); gsub(/[" \t]/, "", v); name = v }
    in_t && /^path[[:space:]]*=/ { v = $0; sub(/^[^=]*=[[:space:]]*/, "", v); gsub(/[" \t]/, "", v); path = v }
    END { flush() }
')"
tree_rows="$(git ls-tree --name-only "$BASE" tests/ | grep -E '^tests/[^/]+\.rs$' | grep -Ev '^tests/suite_[0-9]{2}\.rs$' || true)"

declare -A explicit_for_path=()
subdir_names=()
while IFS=' ' read -r n p; do
    [ -n "$n" ] || continue
    if [[ "$p" =~ ^tests/[^/]+\.rs$ ]]; then
        explicit_for_path["$p"]="$n"
    else
        subdir_names+=("$n")
    fi
done <<< "$explicit_rows"

root_names=()
root_count=0
while IFS= read -r p; do
    [ -n "$p" ] || continue
    root_count=$((root_count + 1))
    if [ -n "${explicit_for_path[$p]:-}" ]; then
        root_names+=("${explicit_for_path[$p]}")
    else
        base="${p#tests/}"
        root_names+=("${base%.rs}")
    fi
done <<< "$tree_rows"

for p in "${!explicit_for_path[@]}"; do
    if ! grep -Fxq -- "$p" <<< "$tree_rows"; then
        fail "explicit [[test]] path ${p} is not a tests/*.rs file at ${BASE}"
    fi
done

all_names=("${root_names[@]}" "${subdir_names[@]}")
total="${#all_names[@]}"
unique="$(printf '%s\n' "${all_names[@]}" | sort -u | wc -l)"

if [ "$total" -eq 186 ]; then
    pass "target name count is 186 (${root_count} root files + ${#subdir_names[@]} subdirectory targets)"
else
    fail "target name count is ${total}, expected 186"
fi
if [ "$unique" -eq "$total" ]; then
    pass "target names are unique"
else
    fail "duplicate target names: $((total - unique))"
fi

# --- Every target resolves to a suite or a standalone target ----------------------------
grouped=0
standalone=0
for n in "${all_names[@]}"; do
    if suite="$(xavier_test_suite "$n" 2>/dev/null)"; then
        if [[ "$suite" =~ ^suite_[0-9]{2}$ ]]; then
            grouped=$((grouped + 1))
        elif [ "$suite" = "$n" ]; then
            standalone=$((standalone + 1))
        else
            fail "${n} resolved to unexpected target '${suite}'"
        fi
    else
        fail "${n} does not resolve to a suite or standalone target"
    fi
done
if [ "$grouped" -eq 183 ] && [ "$standalone" -eq 3 ]; then
    pass "all target names resolve (${grouped} grouped, ${standalone} standalone)"
else
    fail "resolution split is ${grouped} grouped / ${standalone} standalone, expected 183 / 3"
fi

for n in mcp_session_edge_test memory_prune_test mesh_full_simulation_test; do
    if [ "$(xavier_test_suite "$n" 2>/dev/null)" = "$n" ] && [ -z "$(xavier_test_filter "$n")" ]; then
        pass "standalone ${n} resolves to itself with no filter"
    else
        fail "standalone ${n} does not resolve to itself with an empty filter"
    fi
done

# --- Function outputs for one grouped and one standalone name ----------------------------
if [ "$(xavier_test_suite access_flush_drain)" = "suite_01" ]; then
    pass "xavier_test_suite access_flush_drain -> suite_01"
else
    fail "xavier_test_suite access_flush_drain did not print suite_01"
fi
if [ "$(xavier_test_filter access_flush_drain)" = "access_flush_drain::" ]; then
    pass "xavier_test_filter access_flush_drain -> access_flush_drain::"
else
    fail "xavier_test_filter access_flush_drain did not print access_flush_drain::"
fi
if [ -z "$(xavier_test_filter mcp_session_edge_test)" ]; then
    pass "xavier_test_filter mcp_session_edge_test prints nothing"
else
    fail "xavier_test_filter mcp_session_edge_test printed a filter"
fi
if [ "$(xavier_test_suite ci_version_gate.test)" = "suite_01" ] \
    && [ "$(xavier_test_filter ci_version_gate.test)" = "ci_version_gate::" ] \
    && [ "$(xavier_test_suite tests/ci_version_gate.test.rs)" = "suite_01" ]; then
    pass "file basename ci_version_gate.test resolves to module ci_version_gate"
else
    fail "file basename ci_version_gate.test did not resolve to ci_version_gate"
fi
if ! xavier_test_suite no_such_target >/dev/null 2>&1; then
    pass "unknown target name exits non-zero"
else
    fail "unknown target name resolved"
fi
if ! xavier_test_suite 'bad name;rm' >/dev/null 2>&1; then
    pass "invalid target name is rejected"
else
    fail "invalid target name was accepted"
fi

# --- Sourcing: no side effects, works from any cwd, set -u safe --------------------------
opts_before="$-"
dir_before="$(pwd)"
. "$ROOT/scripts/xavier-test-target.sh"
if [ "$-" = "$opts_before" ] && [ "$(pwd)" = "$dir_before" ]; then
    pass "sourcing leaves shell options and cwd unchanged"
else
    fail "sourcing changed shell options or cwd"
fi
if [ "$(cd / && set -u && . "$ROOT/scripts/xavier-test-target.sh" && xavier_test_suite access_flush_drain)" = "suite_01" ]; then
    pass "sourced from / under set -u resolves names"
else
    fail "sourced from / under set -u failed"
fi

# --- Command construction without building (CARGO=echo prints instead of running cargo) --
cmd_out="$(CARGO=echo xavier_cargo_test ci-safe access_flush_drain mcp_session_edge_test 2>/dev/null)"
expected="test --package xavier --features ci-safe --test suite_01 -- --test-threads=1 access_flush_drain::
test --package xavier --features ci-safe --test mcp_session_edge_test -- --test-threads=1"
if [ "$cmd_out" = "$expected" ]; then
    pass "xavier_cargo_test builds filtered grouped and unfiltered standalone commands"
else
    fail "xavier_cargo_test command construction differs: ${cmd_out}"
fi

echo
echo "xavier-test-target-test: ${PASS} passed, ${FAIL} failed"
if [ "$FAIL" -ne 0 ]; then
    exit 1
fi
exit 0
