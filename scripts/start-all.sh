#!/usr/bin/env bash
# Start-All: Unified multi-daemon launcher for Xavier + Gestalt + Web Panel UI (POSIX: Linux/macOS/Android)
set -eo pipefail

XAVIER_PORT="${XAVIER_PORT:-8006}"
GESTALT_PORT="${GESTALT_PORT:-8081}"
UI_PORT="${UI_PORT:-4174}"
NO_UI="${NO_UI:-0}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "================================================================================"
echo "  STARTING SWAL ECOSYSTEM: Xavier + Gestalt + Panel UI"
echo "================================================================================"

# 1. Resolve Xavier Binary
XAVIER_BIN="${XAVIER_BIN:-$(which xavier 2>/dev/null || echo "$REPO_ROOT/target/release/xavier")}"
GESTALT_BIN="${GESTALT_BIN:-$(which gestalt_cli 2>/dev/null || echo "$REPO_ROOT/../gestalt/target/release/gestalt_cli")}"

# 2. Check Xavier Core
echo ""
echo "[1/3] Checking Xavier Core Memory Server (Port $XAVIER_PORT)..."
if curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$XAVIER_PORT/health" | grep -q "200"; then
    echo "[OK] Xavier Core already running at http://127.0.0.1:$XAVIER_PORT"
else
    echo "Starting Xavier Core daemon in background..."
    nohup "$XAVIER_BIN" http "$XAVIER_PORT" > /dev/null 2>&1 &
    for i in {1..15}; do
        sleep 1
        if curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$XAVIER_PORT/health" | grep -q "200"; then
            echo "[PASS] Xavier Core ready at http://127.0.0.1:$XAVIER_PORT"
            break
        fi
    done
fi

# 3. Check Gestalt Event Bus
echo ""
echo "[2/3] Checking Gestalt Event Bus (Port $GESTALT_PORT)..."
if curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$GESTALT_PORT/healthz" | grep -q "200"; then
    echo "[OK] Gestalt Event Bus already running at http://127.0.0.1:$GESTALT_PORT"
else
    echo "Starting Gestalt Event Bus daemon in background..."
    nohup "$GESTALT_BIN" bus serve --port "$GESTALT_PORT" > /dev/null 2>&1 &
    for i in {1..15}; do
        sleep 1
        if curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$GESTALT_PORT/healthz" | grep -q "200"; then
            echo "[PASS] Gestalt Event Bus ready at http://127.0.0.1:$GESTALT_PORT"
            break
        fi
    done
fi

# 4. Check Web Panel UI
if [ "$NO_UI" != "1" ]; then
    echo ""
    echo "[3/3] Checking Web Panel UI (Port $UI_PORT)..."
    if curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$UI_PORT/" | grep -q "200"; then
        echo "[OK] Web Panel UI already running at http://127.0.0.1:$UI_PORT"
    else
        echo "Starting Web Panel UI via pnpm dev..."
        cd "$REPO_ROOT/panel-ui"
        nohup pnpm run dev > /dev/null 2>&1 &
        sleep 2
        echo "[PASS] Web Panel UI active at http://127.0.0.1:$UI_PORT"
    fi
fi

echo "================================================================================"
echo "  ECOSYSTEM OPERATIONAL STATUS"
echo "================================================================================"
echo "  * Xavier Core:        http://127.0.0.1:$XAVIER_PORT"
echo "  * Gestalt Event Bus:  http://127.0.0.1:$GESTALT_PORT"
[ "$NO_UI" != "1" ] && echo "  * Web Panel UI:       http://127.0.0.1:$UI_PORT"
echo "================================================================================"
