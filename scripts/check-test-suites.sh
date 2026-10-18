#!/usr/bin/env bash
# check-test-suites.sh — guard for the grouped integration test binaries (tests/suite_NN.rs).
#
# Exit 1 with one message per violation when:
#   1. a tests/*.rs root file is not a suite root, not a standalone [[test]] path, and is not
#      #[path]-included exactly once (orphan or duplicated), or a #[path] include points to a missing file;
#   2. inside one suite, a module filter "name::" is a substring of another module's "name::"
#      (libtest/nextest substring filters would select the wrong tests);
#   3. a module (or standalone target) whose file uses serial_test is not covered by a
#      test-group = "serial" override for its binary in .config/nextest.toml.
# Exit 0 when clean.
set -uo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]:-$0}")" && pwd -P)"
ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null)" || ROOT=""
if [ -z "$ROOT" ]; then
    ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd -P)"
fi
cd "$ROOT" || exit 1

violations=()
add() { violations+=("$1"); }

shopt -s nullglob
suite_files=(tests/suite_[0-9][0-9].rs)
root_files=()
for f in tests/*.rs; do
    base="$(basename "$f")"
    case "$base" in
        suite_[0-9][0-9].rs) ;;
        *) root_files+=("$base") ;;
    esac
done
shopt -u nullglob

# Standalone [[test]] entries (name and path) that are not suite roots.
standalone_rows="$(awk '
    function flush() { if (in_t && name != "" && path != "") print name " " path; name = ""; path = "" }
    /^\[/ { flush(); in_t = ($0 ~ /^\[\[test\]\]/); next }
    in_t && /^[[:space:]]*name[[:space:]]*=/ { v = $0; sub(/^[^=]*=[[:space:]]*/, "", v); gsub(/[" \t]/, "", v); name = v }
    in_t && /^[[:space:]]*path[[:space:]]*=/ { v = $0; sub(/^[^=]*=[[:space:]]*/, "", v); gsub(/[" \t]/, "", v); path = v }
    END { flush() }
' Cargo.toml | grep -Ev '^suite_[0-9]{2} ' || true)"

# #[path] values across all suite roots, one per line: "<suite> <value>".
path_rows=""
# Module declarations per suite: "<suite> <mod_name> <path_value_or_->".
mod_rows=""
for sf in "${suite_files[@]}"; do
    suite="$(basename "$sf" .rs)"
    while IFS= read -r value; do
        path_rows+="${suite} ${value}"$'\n'
    done < <(sed -n 's/.*#\[path[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$sf")
    mod_rows+="$(awk -v s="$suite" '
        /#\[path[[:space:]]*=/ { p = $0; sub(/.*#\[path[[:space:]]*=[[:space:]]*"/, "", p); sub(/".*/, "", p); pending = p; next }
        /^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+[A-Za-z0-9_]+[[:space:]]*;/ {
            m = $0; sub(/^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+/, "", m); sub(/[[:space:]]*;.*/, "", m)
            print s, m, (pending == "" ? "-" : pending); pending = ""; next
        }
        /^[[:space:]]*$/ || /^[[:space:]]*\/\// || /^[[:space:]]*#\[/ { next }
        { pending = "" }
    ' "$sf")"$'\n'
done

# 1. Orphans, duplicates and missing include targets.
while IFS= read -r value; do
    [ -n "$value" ] || continue
    if [ ! -f "tests/$value" ]; then
        add "suite #[path] includes missing file: tests/${value}"
    fi
done < <(printf '%s' "$path_rows" | awk '{print $2}' | sort -u)

for base in "${root_files[@]}"; do
    includes=$(printf '%s' "$path_rows" | awk -v b="$base" '$2 == b { n++ } END { print n + 0 }')
    standalone=$(printf '%s\n' "$standalone_rows" | awk -v p="tests/$base" '$2 == p { n++ } END { print n + 0 }')
    routes=$((includes + standalone))
    if [ "$routes" -ne 1 ]; then
        add "tests/${base}: routed ${routes} times (suite #[path] includes: ${includes}, standalone [[test]]: ${standalone}); expected exactly 1 (suite root, [[test]], or #[path]-included once)"
    fi
done

# 2. Substring collisions of "name::" filters inside each suite.
for sf in "${suite_files[@]}"; do
    suite="$(basename "$sf" .rs)"
    names=$(printf '%s' "$mod_rows" | awk -v s="$suite" '$1 == s { print $2 }')
    for a in $names; do
        for b in $names; do
            if [ "$a" = "$b" ]; then
                continue
            fi
            if [[ "${b}::" == *"${a}::"* ]]; then
                add "${suite}: filter '${a}::' is a substring of module '${b}' (collision)"
            fi
        done
    done
    dups=$(printf '%s\n' "$names" | sort | uniq -d)
    for d in $dups; do
        add "${suite}: module '${d}' declared more than once"
    done
done

# 3. serial_test files must be in a test-group = "serial" override for their binary.
serial_filters="$(awk '
    /^\[/ { f = "" }
    /^filter[[:space:]]*=/ { f = $0; next }
    /^test-group[[:space:]]*=[[:space:]]*"serial"/ { if (f != "") print f; f = ""; next }
' .config/nextest.toml)"

# Targets to inspect: "<file> <binary> <module_or_empty>".
targets="$(
    while IFS=' ' read -r suite mod value; do
        [ -n "$suite" ] || continue
        [ "$value" = "-" ] && continue
        printf 'tests/%s %s %s\n' "$value" "$suite" "$mod"
    done < <(printf '%s' "$mod_rows" | awk 'NF == 3 { print $1, $2, $3 }')
    while IFS=' ' read -r name path; do
        [ -n "$name" ] || continue
        printf '%s %s %s\n' "$path" "$name" ""
    done <<< "$standalone_rows"
)"

serial_files=0
while IFS=' ' read -r file binary mod; do
    [ -n "$file" ] || continue
    [ -f "$file" ] || continue
    if ! grep -Eq 'serial_test|#\[serial' "$file"; then
        continue
    fi
    serial_files=$((serial_files + 1))
    covered=0
    while IFS= read -r line; do
        case "$line" in
            *"binary(=${binary})"*) ;;
            *) continue ;;
        esac
        if [ -z "$mod" ]; then
            covered=1
            break
        fi
        alt="$(printf '%s' "$line" | sed -n 's/.*test(\/\^(\([^)]*\))::\/).*/\1/p')"
        case "|${alt}|" in
            *"|${mod}|"*) covered=1; break ;;
        esac
    done <<< "$serial_filters"
    if [ "$covered" -eq 0 ]; then
        add "${file} (binary ${binary}${mod:+, module ${mod}}) uses serial_test but is not in a test-group = \"serial\" override in .config/nextest.toml"
    fi
done <<< "$targets"

if [ "${#violations[@]}" -gt 0 ]; then
    for v in "${violations[@]}"; do
        echo "check-test-suites: $v" >&2
    done
    echo "check-test-suites: FAILED (${#violations[@]} violation(s))" >&2
    exit 1
fi

echo "check-test-suites: OK (suites: ${#suite_files[@]}, root test files: ${#root_files[@]}, serial files: ${serial_files})"
exit 0
