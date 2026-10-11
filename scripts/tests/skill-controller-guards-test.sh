#!/usr/bin/env bash
# =============================================================================
# Tests for WAVE-1012.11 skill-controller-guards
# =============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$ROOT/scripts/skill-controller-guards.sh"

TMP_DIR="$(mktemp -d)"
export XAVIER_TARGET_DIR="$TMP_DIR"
export GUARD_TEST_MODE="1"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

mkdir -p "$TMP_DIR/src/server/mcp"
mkdir -p "$TMP_DIR/src/api"
mkdir -p "$TMP_DIR/src/context/skill_controller"

echo "Running tests for skill-controller-guards.sh..."
failed=0

run_case() {
  local name="$1"
  local expected_exit="$2"
  local expected_fail_count="$3"

  echo "--- test: $name"
  local out
  set +e
  out=$("$SCRIPT" 2>&1)
  local exit_code=$?
  set -e

  if [[ $exit_code -ne $expected_exit ]]; then
    echo "FAIL: expected exit $expected_exit, got $exit_code"
    echo "$out"
    failed=1
    return
  fi

  local fail_count
  fail_count=$(echo "$out" | grep -c 'FAIL' || true)
  if [[ $fail_count -ne $expected_fail_count ]]; then
    echo "FAIL: expected $expected_fail_count FAILs, got $fail_count"
    echo "$out"
    failed=1
    return
  fi

  echo "PASS: $name"
}

# Test 1: Clean
cat << 'CODE' > "$TMP_DIR/src/server/mcp/tools_context.rs"
name: "xavier_skill_list".to_string()
name: "xavier_dispatch_skill".to_string()
CODE
cat << 'CODE' > "$TMP_DIR/src/api/skills.rs"
pub async fn audit_skills() {}
pub async fn skill_usage() {}
CODE
cat << 'CODE' > "$TMP_DIR/src/context/skill_controller/clean.rs"
fn good() {}
#[cfg(test)]
mod tests {
    fn bad() { let x = foo.unwrap(); }
}
CODE
run_case "Clean fixtures" 0 0

# Test 2: Inject guard_01 violation
cat << 'CODE' >> "$TMP_DIR/src/server/mcp/tools_context.rs"
name: "xavier_skill_mutator".to_string()
CODE
run_case "guard_01 violation" 1 1

# Reset and Test 3: Inject guard_02 violation
cat << 'CODE' > "$TMP_DIR/src/server/mcp/tools_context.rs"
name: "xavier_skill_list".to_string()
CODE
cat << 'CODE' > "$TMP_DIR/src/api/skills.rs"
pub async fn audit_skills(ext: Extension<String>) {}
CODE
run_case "guard_02 violation" 1 1

# Reset and Test 4: Inject guard_03 violation
cat << 'CODE' > "$TMP_DIR/src/api/skills.rs"
pub async fn audit_skills() {}
CODE
cat << 'CODE' > "$TMP_DIR/src/context/skill_controller/violation.rs"
fn bad() {
    let x = y.expect("err");
}
CODE
run_case "guard_03 violation" 1 1

# Reset and Test 5: Inject guard_04 violation
rm "$TMP_DIR/src/context/skill_controller/violation.rs"
cat << 'CODE' > "$TMP_DIR/src/context/skill_controller/violation2.rs"
fn bad() {
    std::process::Command::new("ls").spawn();
}
CODE
run_case "guard_04 violation" 1 1

# Reset and Test 6: Inject guard_05 violation
rm "$TMP_DIR/src/context/skill_controller/violation2.rs"
cat << 'CODE' > "$TMP_DIR/src/context/skill_controller/violation3.rs"
async fn bad() {
}
CODE
run_case "guard_05 violation" 1 1

if [[ $failed -eq 0 ]]; then
  echo "All tests passed."
  exit 0
else
  echo "Some tests failed."
  exit 1
fi
