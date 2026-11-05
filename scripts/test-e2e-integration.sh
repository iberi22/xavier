#!/usr/bin/env bash
# Test End-to-End de integracion Xavier + Gestalt + AGY (Cross-Platform POSIX)
set -eo pipefail

TESTS_PASSED=0
TESTS_FAILED=0

test_result() {
    local name="$1"
    local status="$2"
    local msg="${3:-}"
    if [ "$status" -eq 0 ]; then
        echo "[PASS] $name"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "[FAIL] $name"
        [ -n "$msg" ] && echo "       $msg"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

echo "================================================================================"
echo "  TEST 1: Xavier Standalone"
echo "================================================================================"

XAVIER_BIN="${XAVIER_BIN:-$(which xavier 2>/dev/null || echo "./target/release/xavier")}"
if [ -x "$XAVIER_BIN" ] || [ -f "$XAVIER_BIN" ]; then
    test_result "Xavier binary exists" 0
    
    # Version check
    VERSION_OUT=$("$XAVIER_BIN" --version 2>&1 || true)
    if echo "$VERSION_OUT" | grep -q "xavier"; then
        test_result "Xavier version command" 0
    else
        test_result "Xavier version command" 1 "$VERSION_OUT"
    fi

    # LLM status
    LLM_OUT=$("$XAVIER_BIN" llm status 2>&1 || true)
    if echo "$LLM_OUT" | grep -qiE "provider|model|status|ok"; then
        test_result "Xavier LLM status" 0
    else
        test_result "Xavier LLM status" 1 "$LLM_OUT"
    fi

    # Help command
    HELP_OUT=$("$XAVIER_BIN" --help 2>&1 || true)
    if echo "$HELP_OUT" | grep -qiE "memory|llm|chat|code"; then
        test_result "Xavier help command" 0
    else
        test_result "Xavier help command" 1 "$HELP_OUT"
    fi
else
    test_result "Xavier binary exists" 1 "Xavier binary not found at $XAVIER_BIN"
fi

echo "================================================================================"
echo "  TEST 2: AGY CLI Standalone"
echo "================================================================================"

AGY_BIN="${AGY_BIN:-$(which agy 2>/dev/null || echo "$HOME/.local/bin/agy")}"
if command -v agy >/dev/null 2>&1 || [ -x "$AGY_BIN" ]; then
    test_result "AGY CLI installed" 0
    AGY_CMD="${AGY_CMD:-$(command -v agy 2>/dev/null || echo "$AGY_BIN")}"
    AGY_HELP=$("$AGY_CMD" --help 2>&1 || true)
    if echo "$AGY_HELP" | grep -qiE "ask|search|help|print"; then
        test_result "AGY help command" 0
    else
        test_result "AGY help command" 1 "$AGY_HELP"
    fi
else
    test_result "AGY CLI installed" 1 "AGY not found in PATH or at $AGY_BIN"
fi

echo "================================================================================"
echo "  TEST 3: Gestalt Configuration"
echo "================================================================================"

GESTALT_DIR="${GESTALT_DIR:-../gestalt}"
if [ -d "$GESTALT_DIR" ]; then
    test_result "Gestalt directory exists" 0
    REGISTRY_FILE="$GESTALT_DIR/agent-registry.toml"
    if [ -f "$REGISTRY_FILE" ]; then
        test_result "agent-registry.toml exists" 0
        grep -q 'name = "xavier-chat"' "$REGISTRY_FILE" && test_result "Xavier-chat configured" 0 || test_result "Xavier-chat configured" 1
        grep -q 'name = "xavier-memory"' "$REGISTRY_FILE" && test_result "Xavier-memory configured" 0 || test_result "Xavier-memory configured" 1
        grep -q 'name = "agy"' "$REGISTRY_FILE" && test_result "AGY configured" 0 || test_result "AGY configured" 1
    else
        test_result "agent-registry.toml exists" 1
    fi
else
    test_result "Gestalt directory exists" 1 "Not found at $GESTALT_DIR"
fi

echo "================================================================================"
echo "  TEST SUMMARY"
echo "================================================================================"
echo "  Passed: $TESTS_PASSED"
echo "  Failed: $TESTS_FAILED"
echo "================================================================================"

if [ "$TESTS_FAILED" -eq 0 ]; then
    echo "ALL TESTS PASSED! Integration verified."
    exit 0
else
    echo "Some tests failed."
    exit 1
fi
