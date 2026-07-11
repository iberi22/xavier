#!/usr/bin/env bash
# =============================================================================
# Xavier Auto-Update Script — Linux/macOS
# =============================================================================
# Checks for newer versions of Xavier, downloads or builds, and hot-swaps.
#
# Modes:
#   1. GitHub Release (fast path) — downloads pre-built binary
#   2. Git pull + cargo build (fallback) — builds from source
#
# Usage:
#   ./scripts/xavier-update.sh              # interactive update
#   ./scripts/xavier-update.sh --check      # check-only (no update)
#   ./scripts/xavier-update.sh --force      # force rebuild even if latest
#   ./scripts/xavier-update.sh --cron       # silent cron mode (logs only)
#
# Cron: */60 * * * * /path/to/xavier-update.sh --cron
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
XAVIER_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY_NAME="xavier"
BINARY_PATH="$XAVIER_ROOT/target/release/$BINARY_NAME"
LOG_FILE="$XAVIER_ROOT/data/logs/update.log"
PID_FILE="/tmp/xavier-update.pid"

mkdir -p "$(dirname "$LOG_FILE")"
touch "$LOG_FILE"

log() { echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" | tee -a "$LOG_FILE"; }
die() { log "FATAL: $*"; exit 1; }

# ─── Parse args ────────────────────────────────────────────────────────────
CHECK_ONLY=false
FORCE=false
CRON=false
for arg in "$@"; do
    case "$arg" in
        --check) CHECK_ONLY=true ;;
        --force) FORCE=true ;;
        --cron) CRON=true ;;
    esac
done

# ─── Prevent concurrent runs ───────────────────────────────────────────────
if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    $CRON && exit 0 || die "Another update is already running (PID $(cat "$PID_FILE"))"
fi
echo $$ > "$PID_FILE"
trap 'rm -f "$PID_FILE"' EXIT

# ─── Get current version ───────────────────────────────────────────────────
CURRENT_VERSION=""
if [ -f "$BINARY_PATH" ]; then
    CURRENT_VERSION=$("$BINARY_PATH" --version 2>/dev/null | grep -oP '[\d]+\.[\d]+\.[\d]+' | head -1 || echo "")
fi
log "Current version: ${CURRENT_VERSION:-unknown}"
log "Repository: $XAVIER_ROOT"

# ─── Check GitHub Releases ─────────────────────────────────────────────────
REMOTE_TAG=""
REMOTE_VERSION=""
RELEASE_URL="https://api.github.com/repos/iberi22/xavier/releases/latest"

if command -v curl &>/dev/null; then
    GH_RESPONSE=$(curl -sf "$RELEASE_URL" 2>/dev/null || true)
    if [ -n "$GH_RESPONSE" ]; then
        REMOTE_TAG=$(echo "$GH_RESPONSE" | python3 -c "import sys,json; print(json.load(sys.stdin).get('tag_name',''))" 2>/dev/null || echo "")
        REMOTE_VERSION=$(echo "$REMOTE_TAG" | sed 's/^v//')
        if [ -n "$REMOTE_VERSION" ]; then
            log "Remote release: $REMOTE_TAG (v$REMOTE_VERSION)"
        fi
    else
        log "No GitHub Release found (this is normal — no tag pushed yet)"
    fi
else
    log "curl not available, skipping GitHub release check"
fi

# ─── Determine if update is needed ──────────────────────────────────────────
NEEDS_UPDATE=false

if [ -n "$REMOTE_VERSION" ] && [ "$REMOTE_VERSION" != "$CURRENT_VERSION" ]; then
    # Simple semver comparison (major.minor.patch)
    IFS='.' read -ra CUR <<< "$CURRENT_VERSION"
    IFS='.' read -ra REM <<< "$REMOTE_VERSION"
    for i in 0 1 2; do
        if [ "${REM[$i]:-0}" -gt "${CUR[$i]:-0}" ]; then
            NEEDS_UPDATE=true
            break
        elif [ "${REM[$i]:-0}" -lt "${CUR[$i]:-0}" ]; then
            break
        fi
    done
fi

# If no remote release, check git remote for newer commits
if [ "$NEEDS_UPDATE" = false ] && [ -d "$XAVIER_ROOT/.git" ]; then
    GIT_REMOTE=$(git -C "$XAVIER_ROOT" remote -v | head -1 | awk '{print $2}' || echo "")
    if [ -n "$GIT_REMOTE" ]; then
        log "Checking git remote: $GIT_REMOTE"
        git -C "$XAVIER_ROOT" fetch --tags origin 2>/dev/null || true
        LOCAL_HASH=$(git -C "$XAVIER_ROOT" rev-parse HEAD)
        REMOTE_HASH=$(git -C "$XAVIER_ROOT" rev-parse origin/main 2>/dev/null || echo "")
        
        if [ -n "$REMOTE_HASH" ] && [ "$LOCAL_HASH" != "$REMOTE_HASH" ]; then
            COMMITS_BEHIND=$(git -C "$XAVIER_ROOT" rev-list --count "$LOCAL_HASH..origin/main" 2>/dev/null || echo "0")
            if [ "$COMMITS_BEHIND" -gt 0 ]; then
                log "Git repo is $COMMITS_BEHIND commit(s) behind origin/main"
                NEEDS_UPDATE=true
            fi
        elif [ "$FORCE" = true ]; then
            log "Force update requested"
            NEEDS_UPDATE=true
        fi
    fi
fi

# ─── Check-only mode ────────────────────────────────────────────────────
$CHECK_ONLY && log "Check complete. Update needed: $NEEDS_UPDATE" && exit 0

if [ "$NEEDS_UPDATE" = false ] && [ "$FORCE" = false ]; then
    log "Already up to date (v${CURRENT_VERSION:-?}). Nothing to do."
    exit 0
fi

log "=== Update started ==="

# ─── Strategy A: Download pre-built binary ─────────────────────────────────
DOWNLOADED=false
if [ -n "$REMOTE_TAG" ] && command -v curl &>/dev/null; then
    PLATFORM="xavier-linux"
    ARCHIVE_URL="https://github.com/iberi22/xavier/releases/download/$REMOTE_TAG/$PLATFORM.tar.gz"
    TMP_DIR=$(mktemp -d)
    
    log "Downloading $ARCHIVE_URL ..."
    if curl -sfL "$ARCHIVE_URL" -o "$TMP_DIR/xavier.tar.gz" 2>/dev/null; then
        log "Download successful, extracting..."
        tar -xzf "$TMP_DIR/xavier.tar.gz" -C "$TMP_DIR" 2>/dev/null || true
        if [ -f "$TMP_DIR/$BINARY_NAME" ]; then
            # Verify it works
            chmod +x "$TMP_DIR/$BINARY_NAME"
            if "$TMP_DIR/$BINARY_NAME" --version &>/dev/null; then
                log "Pre-built binary verified"
                DOWNLOADED=true
            else
                log "Downloaded binary failed verification"
            fi
        fi
    else
        log "No pre-built binary at $ARCHIVE_URL"
    fi
    
    if [ "$DOWNLOADED" = true ]; then
        # Hot-swap binary
        BACKUP_PATH="$XAVIER_ROOT/target/release/$BINARY_NAME.backup"
        [ -f "$BINARY_PATH" ] && cp "$BINARY_PATH" "$BACKUP_PATH"
        cp "$TMP_DIR/$BINARY_NAME" "$BINARY_PATH"
        chmod +x "$BINARY_PATH"
        rm -rf "$TMP_DIR"
        log "Binary replaced (backup at $BACKUP_PATH)"
    else
        rm -rf "$TMP_DIR"
        log "Falling back to source build..."
    fi
fi

# ─── Strategy B: Git pull + cargo build ────────────────────────────────────
if [ "$DOWNLOADED" = false ]; then
    if ! command -v cargo &>/dev/null; then
        die "Neither pre-built binary nor cargo toolchain available"
    fi
    
    log "Pulling latest source..."
    cd "$XAVIER_ROOT"
    STASHED=false
    if ! git diff --quiet 2>/dev/null; then
        git stash push -m "auto-update-stash-$(date +%s)" 2>/dev/null || true
        STASHED=true
    fi
    
    git pull origin main 2>/dev/null || log "git pull failed (will try to build from current source anyway)"
    
    log "Building Xavier (release + local-gllm)..."
    cargo build --release --bin xavier --features "local-gllm,cli-interactive" --no-default-features 2>&1 | tee -a "$LOG_FILE"
    
    if [ "$STASHED" = true ]; then
        git stash pop 2>/dev/null || true
    fi
    
    if [ ! -f "$BINARY_PATH" ]; then
        die "Build failed — binary not found at $BINARY_PATH"
    fi
    
    log "Build successful"
fi

# ─── Verify new binary ──────────────────────────────────────────────────────
NEW_VERSION=$("$BINARY_PATH" --version 2>/dev/null | grep -oP '[\d]+\.[\d]+\.[\d]+' | head -1 || echo "unknown")
log "Updated to version: v${NEW_VERSION}"

# ─── Restart Xavier ─────────────────────────────────────────────────────────
if pgrep -f "xavier http" >/dev/null 2>&1; then
    log "Restarting Xavier server..."
    pkill -f "xavier http" 2>/dev/null || true
    sleep 2
    
    # Re-launch with same env vars as current config
    nohup "$XAVIER_ROOT/target/release/xavier" http 8006 > "$XAVIER_ROOT/data/logs/server.log" 2>&1 &
    disown
    sleep 3
    
    if pgrep -f "xavier http" >/dev/null 2>&1; then
        log "Xavier restarted successfully (PID $(pgrep -f 'xavier http' | head -1))"
    else
        die "Xavier failed to restart after update"
    fi
else
    log "Xavier was not running — update complete, ready for manual start"
fi

log "=== Update completed successfully ==="
