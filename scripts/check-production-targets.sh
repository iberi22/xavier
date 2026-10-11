#!/usr/bin/env bash
# =============================================================================
# WAVE-1012 production-target-preflight
# Validates the guardian inventory and required credentials.
# =============================================================================
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <inventory.json>"
    exit 2
fi

INVENTORY="$1"

# 1. Run the existing target validator
python3 scripts/guardian/targets.py "$INVENTORY" >/dev/null

# 2. Collect unique target kinds
KINDS=$(jq -r '.targets[].kind' "$INVENTORY" | sort -u)

FAILED=0

# Helper to check a variable and print OK or MISSING.
check_var() {
    local VAR_NAME="$1"
    local EXPECTED_VAL="${2:-}" # if provided, enforce exact value match

    if [ -n "$EXPECTED_VAL" ]; then
        if [ "${!VAR_NAME:-}" = "$EXPECTED_VAL" ]; then
            echo "OK $VAR_NAME"
        else
            echo "MISSING $VAR_NAME"
            FAILED=1
        fi
    else
        if [ -n "${!VAR_NAME:-}" ]; then
            echo "OK $VAR_NAME"
        else
            echo "MISSING $VAR_NAME"
            FAILED=1
        fi
    fi
}

# 3. Check XAVIER_GUARDIAN_ACCEPTED=1 globally
check_var "XAVIER_GUARDIAN_ACCEPTED" "1"

# 4. Check variables per kind
declare -A CHECKED
for KIND in $KINDS; do
    case "$KIND" in
        cf_pages|cf_worker)
            if [ -z "${CHECKED["CLOUDFLARE_API_TOKEN"]:-}" ]; then
                check_var "CLOUDFLARE_API_TOKEN"
                CHECKED["CLOUDFLARE_API_TOKEN"]=1
            fi
            if [ -z "${CHECKED["CLOUDFLARE_ACCOUNT_ID"]:-}" ]; then
                check_var "CLOUDFLARE_ACCOUNT_ID"
                CHECKED["CLOUDFLARE_ACCOUNT_ID"]=1
            fi
            ;;
        gh_pages|release_tag)
            if [ -z "${CHECKED["GH_TOKEN"]:-}" ]; then
                check_var "GH_TOKEN"
                CHECKED["GH_TOKEN"]=1
            fi
            ;;
        local_binary)
            # local_binary requires no specific variables
            ;;
        *)
            echo "Unknown kind: $KIND" >&2
            FAILED=1
            ;;
    esac
done

if [ "$FAILED" -ne 0 ]; then
    exit 1
fi

exit 0
