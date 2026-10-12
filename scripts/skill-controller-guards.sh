#!/usr/bin/env bash
# =============================================================================
# WAVE-1012.11 Static Guards for Skill Controller invariants
# =============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail=0
# Allow overriding root for tests
TARGET_DIR="${XAVIER_TARGET_DIR:-$ROOT}"

# Strip tests: ignore separate test files entirely, or strip from the first #[cfg(test)] to EOF.
strip_tests() {
  case "$1" in
    */tests.rs|*_test.rs|*_tests.rs)
      return 0
      ;;
    *)
      awk '/#\[cfg\(test\)\]/ {exit} {print}' "$1"
      ;;
  esac
}

check_01() {
  local id="guard_01"
  local desc="No MCP tool mutates skills"
  local file="$TARGET_DIR/src/server/mcp/tools_context.rs"
  if [[ ! -f "$file" ]]; then return 0; fi

  local hits
  hits=$(strip_tests "$file" | awk '{print NR ":" $0}' | grep -E 'name:[ \t]*"xavier_.*skill.*"' | grep -vE '"xavier_skill_list"|"xavier_dispatch_skill"|"xavier_skill_usage"' || true)

  if [[ -n "$hits" ]]; then
    while IFS= read -r line; do
      local lno
      lno=$(echo "$line" | cut -d: -f1)
      echo "FAIL $id $desc [${file#$TARGET_DIR/}:$lno]"
      fail=1
    done <<< "$hits"
  else
    echo "PASS $id $desc"
  fi
}

check_02() {
  local id="guard_02"
  local desc="/api/skill/audit and /api/skill/usage handlers take no request extractors"
  local file="$TARGET_DIR/src/api/skills.rs"
  if [[ ! -f "$file" ]]; then return 0; fi

  local hits
  hits=$(strip_tests "$file" | awk '{print NR ":" $0}' | grep -E 'pub async fn (audit_skills|skill_usage)\(' | grep -vE 'pub async fn (audit_skills|skill_usage)\(\)' || true)

  if [[ -n "$hits" ]]; then
    while IFS= read -r line; do
      local lno
      lno=$(echo "$line" | cut -d: -f1)
      echo "FAIL $id $desc [${file#$TARGET_DIR/}:$lno]"
      fail=1
    done <<< "$hits"
  else
    echo "PASS $id $desc"
  fi
}

check_03() {
  local id="guard_03"
  local desc="skill-controller modules contain no .unwrap()/.expect() outside #[cfg(test)]"
  local dir="$TARGET_DIR/src/context/skill_controller"
  if [[ ! -d "$dir" ]]; then return 0; fi

  local passed=1
  while IFS= read -r file; do
    local hits
    hits=$(strip_tests "$file" | awk '{print NR ":" $0}' | grep -E '\.(unwrap|expect)\(' || true)
    if [[ -n "$hits" ]]; then
      while IFS= read -r line; do
        local lno
        lno=$(echo "$line" | cut -d: -f1)
        echo "FAIL $id $desc [${file#$TARGET_DIR/}:$lno]"
        fail=1
        passed=0
      done <<< "$hits"
    fi
  done < <(find "$dir" -type f -name "*.rs")

  if [[ $passed -eq 1 ]]; then
    echo "PASS $id $desc"
  fi
}

check_04() {
  local id="guard_04"
  local desc="no std::process::Command in src/context/skill_controller/"
  local dir="$TARGET_DIR/src/context/skill_controller"
  if [[ ! -d "$dir" ]]; then return 0; fi

  local passed=1
  while IFS= read -r file; do
    local hits
    hits=$(strip_tests "$file" | awk '{print NR ":" $0}' | grep -E 'std::process::Command' || true)
    if [[ -n "$hits" ]]; then
      while IFS= read -r line; do
        local lno
        lno=$(echo "$line" | cut -d: -f1)
        echo "FAIL $id $desc [${file#$TARGET_DIR/}:$lno]"
        fail=1
        passed=0
      done <<< "$hits"
    fi
  done < <(find "$dir" -type f -name "*.rs")

  if [[ $passed -eq 1 ]]; then
    echo "PASS $id $desc"
  fi
}

check_05() {
  local id="guard_05"
  local desc="blocking controller calls from async code go through spawn_blocking"
  local dir="$TARGET_DIR/src/context/skill_controller"
  if [[ ! -d "$dir" ]]; then return 0; fi

  local passed=1
  while IFS= read -r file; do
    local hits
    hits=$(strip_tests "$file" | awk '{print NR ":" $0}' | grep -E 'async[ \t]+fn[ \t]+' || true)
    if [[ -n "$hits" ]]; then
      while IFS= read -r line; do
        local lno
        lno=$(echo "$line" | cut -d: -f1)
        echo "FAIL $id $desc [${file#$TARGET_DIR/}:$lno]"
        fail=1
        passed=0
      done <<< "$hits"
    fi
  done < <(find "$dir" -type f -name "*.rs")

  if [[ $passed -eq 1 ]]; then
    echo "PASS $id $desc"
  fi
}

check_01
check_02
check_03
check_04
check_05

if [[ $fail -ne 0 ]]; then
  exit 1
else
  exit 0
fi
