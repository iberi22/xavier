#!/usr/bin/env bash
set -euo pipefail

# Scripts directory and repository root resolution
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DATA_DIR="${XAVIER_DATA_DIR:-${REPO_ROOT}/data}"

echo "==> Running SQLite WAL checkpoint and VACUUM..."

DBS=(
    "${DATA_DIR}/code_graph.db"
    "${DATA_DIR}/vec-store.sqlite3"
)

for db in "${DBS[@]}"; do
    if [ -f "$db" ]; then
        echo "Processing database: ${db}"
        sqlite3 "$db" "PRAGMA wal_checkpoint(TRUNCATE);"
        sqlite3 "$db" "VACUUM;"
        echo "Completed checkpoint & vacuum for ${db}"
    else
        echo "Database not found (skipping): ${db}"
    fi
done

echo "==> WAL checkpoint and VACUUM finished."
