#!/usr/bin/env bash
# xavier-test-target.sh — map old [[test]] target names to the grouped test binaries.
#
# Sourceable helper. Sourcing only defines functions (no options, no cwd change):
#   source scripts/xavier-test-target.sh
#   xavier_test_suite <old_name>               prints suite_NN, or <old_name> for a standalone target
#   xavier_test_filter <old_name>              prints "<old_name>::" for a grouped module, nothing for standalone
#   xavier_cargo_test <features> <old_name>... runs cargo test per target (CARGO env overrides "cargo")
#
# Sources of truth (read from the working tree, located via git from this file's directory):
#   tests/suite_NN.rs   `mod <name>;` declarations preceded by #[path]  -> grouped targets
#   Cargo.toml          [[test]] names other than suite_NN              -> standalone targets

_xavier_test_root() {
    local src dir root
    src="${BASH_SOURCE[0]:-$0}"
    dir="$(cd -- "$(dirname -- "$src")" 2>/dev/null && pwd -P)" || return 1
    root="$(git -C "$dir" rev-parse --show-toplevel 2>/dev/null)" || root=""
    if [ -z "$root" ]; then
        root="$(cd -- "$dir/.." 2>/dev/null && pwd -P)" || return 1
    fi
    printf '%s\n' "$root"
}

# Accepts a target name, a module name, a tests/ path or a file basename
# (tests/ci_version_gate.test.rs -> ci_version_gate). Prints the canonical module name.
_xavier_test_canonical() {
    local n="${1:-}"
    n="${n##*/}"
    n="${n%.rs}"
    n="${n%.test}"
    printf '%s\n' "$n"
}

_xavier_test_valid_name() {
    case "${1:-}" in
        '' | *[!A-Za-z0-9_]*) return 1 ;;
    esac
    return 0
}

# $1 = repo root. Prints [[test]] names from Cargo.toml that are not suite roots.
_xavier_test_standalone_names() {
    awk '
        /^\[\[test\]\]/ { in_t = 1; next }
        /^\[/ { in_t = 0 }
        in_t && /^[[:space:]]*name[[:space:]]*=/ {
            v = $0
            sub(/^[^=]*=[[:space:]]*/, "", v)
            gsub(/[" \t]/, "", v)
            print v
        }
    ' "$1/Cargo.toml" | grep -Ev '^suite_[0-9]{2}$' || true
}

# Prints the target name for a [[test]] or grouped module name.
xavier_test_suite() {
    local name root f
    name="$(_xavier_test_canonical "${1:-}")"
    if ! _xavier_test_valid_name "$name"; then
        echo "xavier_test_suite: invalid test target name: '${name}'" >&2
        return 1
    fi
    root="$(_xavier_test_root)" || return 1
    for f in "$root"/tests/suite_[0-9][0-9].rs; do
        [ -f "$f" ] || continue
        if grep -Eq "^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+${name}[[:space:]]*;" "$f"; then
            basename "$f" .rs
            return 0
        fi
    done
    if _xavier_test_standalone_names "$root" | grep -Fxq -- "$name"; then
        printf '%s\n' "$name"
        return 0
    fi
    echo "xavier_test_suite: unknown test target: ${name}" >&2
    return 1
}

# Prints the nextest/libtest name filter for a grouped module; prints nothing for a standalone target.
xavier_test_filter() {
    local name resolved
    name="$(_xavier_test_canonical "${1:-}")"
    resolved="$(xavier_test_suite "$name")" || return 1
    if [[ "$resolved" =~ ^suite_[0-9]{2}$ ]]; then
        printf '%s::\n' "$name"
    fi
    return 0
}

# Runs one cargo test per old target name, single-threaded, filtered when grouped.
# Returns 0 only if every target passed. Works from any cwd.
xavier_cargo_test() {
    local features="${1:-}" name suite filter root rc=0
    local -a cargo_cmd filter_args
    if [ -z "$features" ] || [ "$#" -lt 2 ]; then
        echo "usage: xavier_cargo_test <features> <old_name>..." >&2
        return 2
    fi
    shift
    root="$(_xavier_test_root)" || return 1
    read -r -a cargo_cmd <<< "${CARGO:-cargo}"
    for name in "$@"; do
        suite="$(xavier_test_suite "$name")" || { rc=1; continue; }
        filter="$(xavier_test_filter "$name")" || { rc=1; continue; }
        filter_args=()
        if [ -n "$filter" ]; then
            filter_args=("$filter")
        fi
        echo "+ ${cargo_cmd[*]} test --package xavier --features ${features} --test ${suite} -- --test-threads=1 ${filter}" >&2
        (cd -- "$root" && "${cargo_cmd[@]}" test --package xavier --features "$features" \
            --test "$suite" -- --test-threads=1 ${filter_args[@]+"${filter_args[@]}"}) || rc=1
    done
    return "$rc"
}
