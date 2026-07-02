#!/usr/bin/env bash
# Xavier HTTP server launcher for Windows (Git Bash)
# Loads .env and starts the Xavier HTTP + MCP HTTP+SSE server in the background.
# Designed to be invoked by the Windows Task Scheduler at logon ("XavierServer").
set -euo pipefail

XAVIER_DIR="E:/scripts-python/xavier"
XAVIER_BIN="C:/Users/belal/bin/xavier.exe"
LOG_DIR="$XAVIER_DIR/data/logs"
mkdir -p "$LOG_DIR"

cd "$XAVIER_DIR"

# Load environment (XAVIER_TOKEN, embedding key, data dir, etc.)
set -a
# shellcheck disable=SC1091
source "$XAVIER_DIR/.env"
set +a

# If a previous instance is still listening on 8006, leave it alone.
if netstat -ano 2>/dev/null | grep -q ":8006.*LISTENING"; then
  echo "[$(date -Iseconds)] Xavier already listening on 8006 — nothing to do." >> "$LOG_DIR/autostart.log"
  exit 0
fi

echo "[$(date -Iseconds)] Starting Xavier HTTP server..." >> "$LOG_DIR/autostart.log"
nohup "$XAVIER_BIN" http >> "$LOG_DIR/server.log" 2>&1 &
echo "[$(date -Iseconds)] Launched PID $!" >> "$LOG_DIR/autostart.log"
