#!/usr/bin/env bash
# Isolated e2e Xavier instance (never touches a production instance or its keyring).
# Required env: XAVIER_BIN (built xavier binary), XAVIER_TOKEN, BENCH_DOCS (dir with the benchmark PDFs),
#               PDFIUM_LIB_DIR (dir containing libpdfium.so).
# Optional env: E2E_DIR (work dir, default ./work), XAVIER_PORT (default 18006).
set -euo pipefail
: "${XAVIER_BIN:?path to the xavier binary}" "${XAVIER_TOKEN:?auth token for the instance}"
: "${BENCH_DOCS:?directory with the benchmark PDFs}" "${PDFIUM_LIB_DIR:?directory containing libpdfium}"
E2E_DIR="${E2E_DIR:-$PWD/work}"
mkdir -p "$E2E_DIR/data/state"
export XAVIER_PORT="${XAVIER_PORT:-18006}" XAVIER_HOST=127.0.0.1
export XAVIER_DATA_DIR="$E2E_DIR/data" XAVIER_STATE_DIR="$E2E_DIR/data/state"
export XAVIER_TOKEN XAVIER_AUTH_VAULT_SERVICE=xavier-auth-pageindex-e2e
export XAVIER_PAGEINDEX_DB="$E2E_DIR/data/pageindex.sqlite3"
export XAVIER_PAGEINDEX_INGEST_ROOTS="$BENCH_DOCS"
export XAVIER_PAGEINDEX_PDFIUM_LIB="$PDFIUM_LIB_DIR"
export RUST_LOG="${RUST_LOG:-info}"
exec "$XAVIER_BIN" http
