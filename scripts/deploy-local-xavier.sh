#!/usr/bin/env bash
# deploy-local-xavier.sh — WAVE-20.01: incremental rebuild + backup + install + restart + health gate.
# Canonical build (AGENTS.md): cargo build --release --features local-gllm
# Usage: bash scripts/deploy-local-xavier.sh [--skip-build]
set -euo pipefail

REPO="$HOME/proyectosSWAL/apps/xavier"
BIN_SRC="$REPO/target/release/xavier"
BIN_DST="$HOME/.local/bin/xavier-real"
DROPIN="$HOME/.config/systemd/user/xavier.service.d/zzz-embeddings-cloud.conf"
HEALTH_URL="http://127.0.0.1:8006/health"
STAMP="$(date +%Y%m%d-%H%M%S)"
LOG="/tmp/xavier-deploy-$STAMP.log"

log() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }
fail() { log "FATAL: $*"; exit 1; }

SKIP_BUILD=0
[ "${1:-}" = "--skip-build" ] && SKIP_BUILD=1

log "repo=$REPO stamp=$STAMP skip_build=$SKIP_BUILD"

# 0. Preconditions: never wipe target/ (54G warm cache, metered connection)
[ -d "$REPO/target" ] || fail "target/ missing — refusing to cold-build on metered connection"
command -v cargo >/dev/null || fail "cargo not in PATH"
command -v systemctl >/dev/null || fail "systemctl not in PATH"

# 1. Backups (binary + drop-in) before touching anything live
cp "$BIN_DST" "$BIN_DST.bak-$STAMP" && log "binary backup: $BIN_DST.bak-$STAMP"
cp "$DROPIN" "$DROPIN.bak-$STAMP" && log "drop-in backup: $DROPIN.bak-$STAMP"

rollback() {
    log "ROLLBACK: restoring backups and restarting previous binary"
    cp "$BIN_DST.bak-$STAMP" "$BIN_DST"
    cp "$DROPIN.bak-$STAMP" "$DROPIN"
    systemctl --user restart xavier.service || true
    log "rollback done — previous binary restored"
}

# 2. Incremental release build with the local embedding backend compiled in
if [ "$SKIP_BUILD" -eq 0 ]; then
    log "building (incremental, warm target/)..."
    if ! (cd "$REPO" && CARGO_TARGET_DIR=target cargo build --release --features local-gllm >>"$LOG" 2>&1); then
        log "build failed — service untouched, see $LOG"
        exit 2
    fi
    log "build ok"
else
    log "build skipped by flag"
fi
[ -x "$BIN_SRC" ] || fail "built binary missing: $BIN_SRC"

# 3. Install + restart (stop FIRST: cp over a running binary fails with ETXTBSY)
systemctl --user stop xavier.service && log "service stopped for install"
sleep 2
cp "$BIN_SRC" "$BIN_DST" && log "installed $BIN_DST"
systemctl --user start xavier.service && log "service started"

# 4. Health gate: 200, up to 300s of retries (fresh start warms a 1GB+ codegraph).
# Rollback on failure.
log "waiting for health gate..."
for i in $(seq 1 100); do
    CODE="$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$HEALTH_URL" || echo 000)"
    if [ "$CODE" = "200" ]; then
        TIME="$(curl -s -o /dev/null -w '%{time_total}' --max-time 5 "$HEALTH_URL")"
        log "health gate PASS: 200 in ${TIME}s (attempt $i)"
        echo "DEPLOY_OK binary=$BIN_DST stamp=$STAMP log=$LOG"
        exit 0
    fi
    sleep 3
done
log "health gate FAILED after ~400s (last code=$CODE)"
rollback
fail "deploy aborted, rollback applied"
