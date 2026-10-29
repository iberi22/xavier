#!/bin/bash
set -euo pipefail

source scripts/xavier-test-target.sh

FAILED=0

echo "Testing valid suite name: suite_01"
out=$(xavier_test_suite suite_01)
if [ "$out" != "suite_01" ]; then
    echo "Expected suite_01, got $out"
    FAILED=1
fi

echo "Testing valid suite name filter: suite_01"
out_filter=$(xavier_test_filter suite_01)
if [ -n "$out_filter" ]; then
    echo "Expected empty filter for suite_01, got '$out_filter'"
    FAILED=1
fi



echo "Testing unknown suite name: suite_99"
if xavier_test_suite suite_99 >/dev/null 2>&1; then
    echo "Expected failure for unknown suite, but it succeeded"
    FAILED=1
else
    echo "Unknown suite rejected correctly"
fi

if [ $FAILED -ne 0 ]; then
    echo "Tests failed"
    exit 1
fi
echo "Success"
