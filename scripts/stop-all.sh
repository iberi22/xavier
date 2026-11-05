#!/usr/bin/env bash
# Stop-All: Graceful shutdown for Xavier + Gestalt + Web Panel UI (POSIX: Linux/macOS/Android)
set -eo pipefail

echo "Shutting down SWAL Ecosystem services..."

pkill -f "xavier http" 2>/dev/null || true
pkill -f "gestalt_cli bus serve" 2>/dev/null || true
pkill -f "vite" 2>/dev/null || true

echo "Shutdown completed."
