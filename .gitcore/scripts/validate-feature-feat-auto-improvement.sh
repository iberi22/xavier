#!/bin/bash
# Validate: Auto-Improvement Loop
# Feature: feat-auto-improvement
# Status: draft
# Progress: 85%
# Steps: 6 validation steps
echo "=== Validate feat-auto-improvement: Auto-Improvement Loop ==="

# 1. Check if binary exists
if ! command -v xavier &>/dev/null; then
    echo "[FAIL] xavier binary not found in PATH"
    exit 1
fi

# 2. Health check (if running)
HEALTH_URL="http://localhost:8006/health"
if curl -sf "$HEALTH_URL" >/dev/null 2>&1; then
    echo "[OK] Xavier reachable at $HEALTH_URL"
else
    echo "[WARN] Xavier not running - skipping runtime checks"
fi

# 3. Check token
if [ -n "$XAVIER_TOKEN" ] || [ -f /home/belal/.xavier/.env ]; then
    echo "[OK] XAVIER_TOKEN configured"
else
    echo "[FAIL] XAVIER_TOKEN not configured"
fi

# 4. Score: 0/100
echo "[INFO] Feature score: 0/100"
echo "[INFO] Progress: 85%"
echo "[INFO] Steps passed: 0/6"
