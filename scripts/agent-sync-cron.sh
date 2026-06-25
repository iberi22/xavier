#!/bin/bash
# Xavier Agent Memory Sync - Reference Cron Script
# Usage: ./agent-sync-cron.sh [--agent name] [--pull] [--json] [--quiet]

AGENT_NAME=""
MODE="push"
JSON=""
QUIET=""

while [[ "$#" -gt 0 ]]; do
    case $1 in
        -a|--agent) AGENT_NAME="$2"; shift ;;
        -p|--pull) MODE="pull" ;;
        -j|--json) JSON="--json" ;;
        -q|--quiet) QUIET="1" ;;
    esac
    shift
done

# Binary path (configurable via env)
XAVIER_BIN=${XAVIER_BIN_PATH:-"/usr/local/bin/xavier"}

if [ ! -f "$XAVIER_BIN" ]; then
    [ -z "$QUIET" ] && echo "Error: Xavier binary not found at $XAVIER_BIN"
    exit 1
fi

[ -z "$QUIET" ] && echo "=== Agent Memory Sync (Cron) ==="
[ -z "$QUIET" ] && echo "Date: $(date)"

# Arguments
ARGS=""
[ ! -z "$AGENT_NAME" ] && ARGS="$ARGS --agent $AGENT_NAME"
[ ! -z "$JSON" ] && ARGS="$ARGS $JSON"

# 1. Scan
[ -z "$QUIET" ] && echo ">> Phase 1/3: Scanning..."
$XAVIER_BIN agent scan $ARGS
[ $? -ne 0 ] && exit 1

# 2. Index
[ -z "$QUIET" ] && echo ">> Phase 2/3: Indexing..."
$XAVIER_BIN agent index $ARGS
[ $? -ne 0 ] && exit 1

# 3. Sync
[ -z "$QUIET" ] && echo ">> Phase 3/3: Syncing ($MODE)..."
$XAVIER_BIN agent $MODE $ARGS
[ $? -ne 0 ] && exit 1

[ -z "$QUIET" ] && echo "=== Sync Complete ==="
