#!/usr/bin/env bash
# Cleanup ephemeral test databases and data litter without touching production/persistent DBs.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

DATA_DIR="${REPO_ROOT}/data"

echo "=== Cleaning data litter in ${DATA_DIR} ==="

if [ ! -d "${DATA_DIR}" ]; then
    echo "No data directory found at ${DATA_DIR}."
    exit 0
fi

REMOVED_COUNT=0

for pattern in "headless-test-*.db*" "health-test-*.db*" "memory_vec.db*"; do
    shopt -s nullglob
    files=("${DATA_DIR}"/${pattern})
    shopt -u nullglob
    for f in "${files[@]}"; do
        if [ -f "$f" ]; then
            echo "Removing ephemeral test DB: $f"
            rm -f "$f"
            REMOVED_COUNT=$((REMOVED_COUNT + 1))
        fi
    done
done

echo "=== Cleanup complete. Total files removed: ${REMOVED_COUNT} ==="
