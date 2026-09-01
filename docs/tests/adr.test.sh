#!/usr/bin/env bash
set -euo pipefail

echo "=== Running ADR & SRS Verification Suite ==="

ERRORS=0

# Case 1: ADR-030 validation
ADR_030="docs/adr/ADR-030-panel-browser-compat.md"
if [ ! -f "$ADR_030" ]; then
    echo "FAIL: $ADR_030 does not exist"
    ERRORS=$((ERRORS + 1))
else
    LINES=$(wc -l < "$ADR_030")
    if [ "$LINES" -lt 80 ]; then
        echo "FAIL: $ADR_030 has $LINES lines (expected >= 80)"
        ERRORS=$((ERRORS + 1))
    fi

    for SECTION in "## Context" "## Decision" "## Consequences" "## Alternatives"; do
        if ! grep -q "$SECTION" "$ADR_030"; then
            echo "FAIL: $ADR_030 missing section '$SECTION'"
            ERRORS=$((ERRORS + 1))
        fi
    done

    if ! grep -q "__TAURI_INTERNALS__" "$ADR_030"; then
        echo "FAIL: $ADR_030 does not mention __TAURI_INTERNALS__"
        ERRORS=$((ERRORS + 1))
    fi
fi

# Case 2: ADR-031 validation
ADR_031="docs/adr/ADR-031-swal-versioning-gate.md"
if [ ! -f "$ADR_031" ]; then
    echo "FAIL: $ADR_031 does not exist"
    ERRORS=$((ERRORS + 1))
else
    LINES=$(wc -l < "$ADR_031")
    if [ "$LINES" -lt 60 ]; then
        echo "FAIL: $ADR_031 has $LINES lines (expected >= 60)"
        ERRORS=$((ERRORS + 1))
    fi

    for SECTION in "## Context" "## Decision" "## Consequences" "## Alternatives"; do
        if ! grep -q "$SECTION" "$ADR_031"; then
            echo "FAIL: $ADR_031 missing section '$SECTION'"
            ERRORS=$((ERRORS + 1))
        fi
    done

    if ! grep -q "VERSIONING.md" "$ADR_031"; then
        echo "FAIL: $ADR_031 does not mention VERSIONING.md"
        ERRORS=$((ERRORS + 1))
    fi
fi

# Case 3: REQ-044 in SRS requirements
SRS_FILE="docs/SRS/REQUIREMENTS.md"
if [ ! -f "$SRS_FILE" ]; then
    echo "FAIL: $SRS_FILE does not exist"
    ERRORS=$((ERRORS + 1))
else
    if ! grep -q "REQ-044" "$SRS_FILE"; then
        echo "FAIL: $SRS_FILE missing REQ-044"
        ERRORS=$((ERRORS + 1))
    fi

    if ! grep -A 5 "REQ-044" "$SRS_FILE" | grep -Eq "browser.*compat|Panel"; then
        echo "FAIL: $SRS_FILE REQ-044 missing browser compat context"
        ERRORS=$((ERRORS + 1))
    fi
fi

if [ "$ERRORS" -gt 0 ]; then
    echo "=== FAIL: $ERRORS test(s) failed ==="
    exit 1
else
    echo "=== PASS: All ADR and SRS verification tests passed! ==="
    exit 0
fi
