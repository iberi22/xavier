#!/usr/bin/env bash
# Fails if a representative subset of the test suite touches the real user home.
#
# The tests run with HOME/XDG_* pointing at an empty tempdir. The library's
# test guard (src/test_support.rs) panics on any attempt to open a DB / key
# under the REAL home (XAVIER_GUARD_REAL_HOME), and this script additionally
# checks that no new file appeared under the real ~/.xavier or XDG data dir.
# Usage: scripts/check-test-writes.sh [extra cargo test args]
set -euo pipefail

REAL_HOME="${REAL_HOME:-$HOME}"
SBX="$(mktemp -d)"
trap 'rm -rf "$SBX"' EXIT

snapshot() {
  for d in "$REAL_HOME/.xavier" "$REAL_HOME/.local/share/xavier"; do
    [ -d "$d" ] && find "$d" -mindepth 1 -not -path '*/.quarantine*' 2>/dev/null
  done | sort
}
snapshot >"$SBX/before.txt"

# Every integration-test / bench crate root must install the sandbox hook.
missing=$(grep -L 'isolate_test_process!' tests/*.rs benches/*.rs || true)
if [ -n "$missing" ]; then
  echo "FAIL: missing xavier::isolate_test_process!(); in:" >&2
  echo "$missing" >&2
  exit 1
fi

export RUSTUP_HOME="${RUSTUP_HOME:-$REAL_HOME/.rustup}"
export CARGO_HOME="${CARGO_HOME:-$REAL_HOME/.cargo}"
export XAVIER_GUARD_REAL_HOME="$REAL_HOME"
# Set every var `isolate_user_dirs()` rewrites, so this script exercises the
# SAME sandbox the library installs instead of a half-overridden one.
export HOME="$SBX/home" XDG_DATA_HOME="$SBX/data" XDG_CONFIG_HOME="$SBX/config" \
       XDG_STATE_HOME="$SBX/state" XDG_CACHE_HOME="$SBX/cache" \
       XAVIER_DATA_DIR="$SBX/xavier-data" XAVIER_HOME="$SBX/xavier-home" \
       XAVIER_STATE_DIR="$SBX/xavier-state" XAVIER_CONFIG_DIR="$SBX/xavier-config" \
       XAVIER_WORKSPACE_DIR="$SBX/xavier-workspace"
mkdir -p "$HOME"

# Representative subset: unit tests that open workspaces, plus the integration
# tests that used to create conversations DBs (mcp-edge-*, rbac-mcp-*, ws-*).
cargo test -p xavier --features ci-safe --lib "$@" -- --test-threads=1 workspace::
for t in mcp_session_edge_test memory_prune_test mesh_full_simulation_test; do
  cargo test -p xavier --features ci-safe --test "$t" "$@" -- --test-threads=1
done

export HOME="$REAL_HOME"
snapshot >"$SBX/after.txt"
if ! diff -u "$SBX/before.txt" "$SBX/after.txt"; then
  echo "FAIL: tests wrote under the real home (see diff above)" >&2
  exit 1
fi
echo "OK: no writes outside the sandbox home"
