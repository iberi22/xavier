#!/bin/bash
set -euo pipefail

EXECUTE=false
ENFORCE=false
ENABLE_AUTO_MERGE=false

while [[ "$#" -gt 0 ]]; do
    case $1 in
        --execute) EXECUTE=true ;;
        --enforce) ENFORCE=true ;;
        --enable-auto-merge) ENABLE_AUTO_MERGE=true ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
    shift
done

REPO="iberi22/xavier"
RULESET_FILE=".github/rulesets/main.json"
CACHE_DIR=".cache/rulesets"

# Ensure cache dir exists
mkdir -p "$CACHE_DIR"

if [ "$EXECUTE" = false ]; then
    echo "[DRY-RUN] Planning apply for main ruleset..."
    echo "[DRY-RUN] Would read ruleset definition from $RULESET_FILE"

    if [ "$ENFORCE" = true ]; then
        echo "[DRY-RUN] Would set enforcement to 'active'"
    else
        echo "[DRY-RUN] Would keep enforcement from file (evaluate)"
    fi

    echo "[DRY-RUN] Fetching existing rulesets..."
    # Dry run fetch
    if command -v gh >/dev/null 2>&1; then
        EXISTING_RULESETS=$(gh api "repos/$REPO/rulesets" --jq '.[] | {id: .id, name: .name}' 2>/dev/null || echo "[]")
        echo "[DRY-RUN] Found rulesets: $EXISTING_RULESETS"
    else
        echo "[DRY-RUN] gh api not available, mocking output"
    fi

    echo "[DRY-RUN] Would create/update 'main-protection'"
    echo "[DRY-RUN] Would evaluate if 'r1' or 'ruleone' can be safely deleted"

    if [ "$ENABLE_AUTO_MERGE" = true ]; then
        echo "[DRY-RUN] Would set allow_auto_merge=true on $REPO"
    fi
    exit 0
fi

echo "Applying main ruleset..."

# 1. Read the target ruleset payload
if [ ! -f "$RULESET_FILE" ]; then
    echo "Error: $RULESET_FILE not found."
    exit 1
fi

PAYLOAD=$(cat "$RULESET_FILE")

# Optional enforce override
if [ "$ENFORCE" = true ]; then
    PAYLOAD=$(echo "$PAYLOAD" | jq '.enforcement = "active"')
fi

# 2. Fetch existing rulesets
EXISTING_RULESETS=$(gh api "repos/$REPO/rulesets" --jq '.')
MAIN_ID=$(echo "$EXISTING_RULESETS" | jq -r '.[] | select(.name == "main-protection") | .id')

if [ -n "$MAIN_ID" ] && [ "$MAIN_ID" != "null" ]; then
    echo "Updating existing main-protection ruleset (ID: $MAIN_ID)..."
    # Dump existing before update
    gh api "repos/$REPO/rulesets/$MAIN_ID" > "$CACHE_DIR/main-protection-previous.json" || true
    gh api -X PUT "repos/$REPO/rulesets/$MAIN_ID" --input - <<< "$PAYLOAD" >/dev/null
else
    echo "Creating new main-protection ruleset..."
    gh api -X POST "repos/$REPO/rulesets" --input - <<< "$PAYLOAD" >/dev/null
fi

# 3. Clean up duplicates (r1, ruleone)
# We only delete if they only contain deletion / non_fast_forward,
# which is a subset of main-protection.
for DUP_NAME in "r1" "ruleone"; do
    DUP_ID=$(echo "$EXISTING_RULESETS" | jq -r ".[] | select(.name == \"$DUP_NAME\") | .id")
    if [ -n "$DUP_ID" ] && [ "$DUP_ID" != "null" ]; then
        DUP_DETAILS=$(gh api "repos/$REPO/rulesets/$DUP_ID")
        # Check subset: are all rules in [deletion, non_fast_forward]?
        # We can extract the rule types
        NON_SUBSET_RULES=$(echo "$DUP_DETAILS" | jq -r '.rules[].type | select(. != "deletion" and . != "non_fast_forward")')
        if [ -z "$NON_SUBSET_RULES" ]; then
            echo "Deleting duplicate ruleset $DUP_NAME (ID: $DUP_ID)..."
            echo "$DUP_DETAILS" > "$CACHE_DIR/${DUP_NAME}-previous.json"
            gh api -X DELETE "repos/$REPO/rulesets/$DUP_ID" >/dev/null
        else
            echo "Skipping deletion of $DUP_NAME; it contains rules not present in our subset check: $NON_SUBSET_RULES"
        fi
    fi
done

# 4. Optional auto-merge
if [ "$ENABLE_AUTO_MERGE" = true ]; then
    echo "Enabling allow_auto_merge on $REPO..."
    gh api -X PATCH "repos/$REPO" -f allow_auto_merge=true >/dev/null
fi

echo "Done."
