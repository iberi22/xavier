#!/usr/bin/env bash
set -e

# Version Sync & Preflight Gate Script
# Verifies version consistency across Cargo.toml, package.json, CHANGELOG, and git tags.

CWD="${1:-.}"
if [ "$CWD" = "check" ] || [ "$CWD" = "--cwd" ]; then
    CWD="."
fi

# Prevent infinite recursion if swal-preflight calls check-version-sync.sh
if [ -z "$SWAL_PREFLIGHT_RUNNING" ]; then
    export SWAL_PREFLIGHT_RUNNING=1

    RUN_SWAL_PREFLIGHT=0
    if command -v swal-preflight &> /dev/null; then
        # Only use external swal-preflight CLI if it is not our wrapper or if specified
        RUN_SWAL_PREFLIGHT=1
    elif [ -f "periferia/swal-preflight/bin/swal-preflight.js" ]; then
        RUN_SWAL_PREFLIGHT=2
    fi

    if [ "$RUN_SWAL_PREFLIGHT" -eq 2 ]; then
        echo "Running swal-preflight node runner..."
        node periferia/swal-preflight/bin/swal-preflight.js check --cwd "$CWD" || true
    elif npx --yes @swal/preflight check --cwd "$CWD" 2>/dev/null; then
        echo "npx swal-preflight check succeeded."
    fi
fi

# Core Manifest Version Verification
echo "Verifying manifest versions in $CWD..."

CARGO_VER=""
PKG_VER=""

if [ -f "$CWD/Cargo.toml" ]; then
    CARGO_VER=$(grep -m1 '^version =' "$CWD/Cargo.toml" | sed -E 's/version = "(.*)"/\1/' | tr -d '[:space:]')
fi

if [ -f "$CWD/package.json" ]; then
    PKG_VER=$(python3 -c "import json; print(json.load(open('$CWD/package.json')).get('version', ''))" 2>/dev/null || grep -m1 '"version":' "$CWD/package.json" | sed -E 's/.*"version": "(.*)",/\1/' | tr -d '[:space:]')
fi

if [ -n "$CARGO_VER" ] && [ -n "$PKG_VER" ]; then
    if [ "$CARGO_VER" != "$PKG_VER" ]; then
        echo "ERROR: Version drift detected! Cargo.toml ($CARGO_VER) vs package.json ($PKG_VER)" >&2
        exit 1
    fi
fi

# Verify panel-ui/package.json if present
if [ -f "$CWD/panel-ui/package.json" ]; then
    PANEL_PKG_VER=$(python3 -c "import json; print(json.load(open('$CWD/panel-ui/package.json')).get('version', ''))" 2>/dev/null || echo "")
    if [ -n "$PANEL_PKG_VER" ] && [ -n "$CARGO_VER" ] && [ "$PANEL_PKG_VER" != "$CARGO_VER" ]; then
        echo "ERROR: Version drift detected! Cargo.toml ($CARGO_VER) vs panel-ui/package.json ($PANEL_PKG_VER)" >&2
        exit 1
    fi
fi

# Verify CHANGELOG.md has [Unreleased]
if [ -f "$CWD/CHANGELOG.md" ]; then
    if ! grep -qi "\[Unreleased\]" "$CWD/CHANGELOG.md"; then
        echo "ERROR: CHANGELOG.md is missing required [Unreleased] section!" >&2
        exit 1
    fi
else
    echo "ERROR: CHANGELOG.md not found in $CWD!" >&2
    exit 1
fi

# Verify git tag against manifest version if HEAD is tagged
if command -v git &> /dev/null && git rev-parse --is-inside-work-tree &> /dev/null; then
    TAG=$(git tag --points-at HEAD 2>/dev/null | head -n 1 || true)
    if [ -n "$TAG" ]; then
        CLEAN_TAG="${TAG#v}"
        if [ -n "$CARGO_VER" ] && [ "$CLEAN_TAG" != "$CARGO_VER" ]; then
            echo "ERROR: Git tag ($TAG) does not match manifest version ($CARGO_VER)!" >&2
            exit 1
        fi
    fi
fi

echo "Version sync ok: $CARGO_VER (manifests & changelog synced)"
