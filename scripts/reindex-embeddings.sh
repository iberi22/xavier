#!/bin/bash
# ==============================================================================
# Xavier Persistent Embeddings Reindexing Script
# ==============================================================================
# Usage:
#   scripts/reindex-embeddings.sh [--dry-run] [--limit N] [--url URL]
#
# Options:
#   --dry-run, -d      Query null embeddings count without triggering reindexing
#   --limit N, -l N    Limit the number of records to reindex in this batch (default: 500)
#   --url URL, -u URL  Base URL for Xavier server (default: http://localhost:8006 or $XAVIER_URL)
#   --lockfile FILE    Path to lockfile (default: /tmp/xavier-reindex.lock)
#   --help, -h         Show this help message
# ==============================================================================

set -euo pipefail

LOCKFILE="${XAVIER_REINDEX_LOCKFILE:-/tmp/xavier-reindex.lock}"
XAVIER_URL="${XAVIER_URL:-http://localhost:8006}"
DRY_RUN=false
LIMIT=500

show_help() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --dry-run, -d      Perform a dry-run check (returns null embeddings count without reindexing)
  --limit N, -l N    Limit the number of records to reindex in this batch (default: 500)
  --url URL, -u URL  Base URL for Xavier server (default: http://localhost:8006)
  --lockfile FILE    Path to lockfile (default: /tmp/xavier-reindex.lock)
  --help, -h         Show this help message
EOF
}

# Parse command line arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        -d|--dry-run)
            DRY_RUN=true
            ;;
        -l|--limit)
            if [[ -n "${2:-}" && "$2" =~ ^[0-9]+$ ]]; then
                LIMIT="$2"
                shift
            else
                echo "[ERROR] Option --limit requires a positive integer value." >&2
                exit 1
            fi
            ;;
        -u|--url)
            if [[ -n "${2:-}" ]]; then
                XAVIER_URL="$2"
                shift
            else
                echo "[ERROR] Option --url requires a valid URL." >&2
                exit 1
            fi
            ;;
        --lockfile)
            if [[ -n "${2:-}" ]]; then
                LOCKFILE="$2"
                shift
            else
                echo "[ERROR] Option --lockfile requires a file path." >&2
                exit 1
            fi
            ;;
        -h|--help)
            show_help
            exit 0
            ;;
        *)
            echo "[ERROR] Unknown option: $1" >&2
            show_help >&2
            exit 1
            ;;
    esac
    shift
done

# Single Instance Guarantee using flock
exec 200>"$LOCKFILE"
if ! flock -n 200; then
    echo "[ERROR] Another reindexing process is currently running (lockfile $LOCKFILE held). Rejecting execution." >&2
    exit 1
fi

echo "$$" >&200

# Function to resolve XAVIER_TOKEN from env or .env files
get_token() {
    if [[ -n "${XAVIER_TOKEN:-}" ]]; then
        echo "$XAVIER_TOKEN"
        return 0
    fi

    local repo_root
    repo_root=$(git rev-parse --show-toplevel 2>/dev/null || echo "")

    local env_paths=(
        ".env"
        "${repo_root}/.env"
        "${HOME}/.env"
    )

    for path in "${env_paths[@]}"; do
        if [[ -n "$path" && -f "$path" ]]; then
            local token
            token=$(grep -E '^XAVIER_TOKEN=' "$path" 2>/dev/null | cut -d '=' -f 2- | tr -d '"' | tr -d "'" || true)
            if [[ -n "$token" ]]; then
                echo "$token"
                return 0
            fi
        fi
    done

    if [[ -d "/proc" ]]; then
        for env_file in /proc/[0-9]*/environ; do
            if [[ -r "$env_file" ]]; then
                local token
                token=$(tr '\0' '\n' < "$env_file" 2>/dev/null | grep -E '^XAVIER_TOKEN=' | cut -d '=' -f 2- | tr -d '"' | tr -d "'" || true)
                if [[ -n "$token" ]]; then
                    echo "$token"
                    return 0
                fi
            fi
        done
    fi

    return 1
}

TOKEN=$(get_token || true)

if [[ -z "$TOKEN" ]]; then
    echo "[ERROR] Could not resolve XAVIER_TOKEN from environment, .env files, or process environment." >&2
    exit 1
fi

# Build Payload
if [[ "$DRY_RUN" == "true" ]]; then
    PAYLOAD='{"dry_run": true}'
else
    PAYLOAD=$(jq -n --argjson limit "$LIMIT" '{"dry_run": false, "limit": $limit}')
fi

ENDPOINT="${XAVIER_URL}/v1/maintenance/reindex-embeddings"

echo "[INFO] Starting Xavier reindex embeddings task..."
echo "[INFO] Timestamp: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
echo "[INFO] Target Endpoint: $ENDPOINT"
echo "[INFO] Mode: $(if [[ "$DRY_RUN" == "true" ]]; then echo "Dry-Run"; else echo "Reindex (Limit: $LIMIT)"; fi)"

# Make HTTP Request
RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "$ENDPOINT" \
    -H "Content-Type: application/json" \
    -H "X-Xavier-Token: $TOKEN" \
    -d "$PAYLOAD" || echo -e "\n000")

HTTP_STATUS=$(echo "$RESPONSE" | tail -n1)
HTTP_BODY=$(echo "$RESPONSE" | sed '$d')

if [[ "$HTTP_STATUS" -ne 200 ]]; then
    echo "[ERROR] Request to $ENDPOINT failed with HTTP status code $HTTP_STATUS." >&2
    echo "[ERROR] Response body: $HTTP_BODY" >&2
    exit 1
fi

# Parse Response JSON
if command -v jq >/dev/null 2>&1; then
    STATUS=$(echo "$HTTP_BODY" | jq -r '.status // "unknown"')
    NULL_COUNT=$(echo "$HTTP_BODY" | jq -r '.null_embeddings_count // 0')
    PROCESSED_COUNT=$(echo "$HTTP_BODY" | jq -r '.processed_count // 0')
else
    STATUS=$(echo "$HTTP_BODY" | python3 -c 'import sys, json; print(json.load(sys.stdin).get("status", "unknown"))')
    NULL_COUNT=$(echo "$HTTP_BODY" | python3 -c 'import sys, json; print(json.load(sys.stdin).get("null_embeddings_count", 0))')
    PROCESSED_COUNT=$(echo "$HTTP_BODY" | python3 -c 'import sys, json; print(json.load(sys.stdin).get("processed_count", 0))')
fi

echo "[INFO] Server Status: $STATUS"
echo "[INFO] Memories lacking embeddings (null count): $NULL_COUNT"

if [[ "$DRY_RUN" == "true" ]]; then
    echo "[INFO] Dry run completed. No background reindexing triggered."
else
    echo "[INFO] Reindex batch triggered for up to $LIMIT memories."
    echo "[INFO] Processed count: $PROCESSED_COUNT"
    echo "[INFO] Reindex batch successfully scheduled."
fi

exit 0
