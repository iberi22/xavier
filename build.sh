#!/usr/bin/env bash
# Build Xavier in RAM tmpfs with nix-shell
# Usage: ./build.sh [check|test|build|clippy|fmt|clean]
set -eo pipefail

cd "$(dirname "$0")"

CMD="${1:-check}"
EXTRA="${@:2}"

case "$CMD" in
  check)
    nix-shell --command "CARGO_TARGET_DIR=/build/rust-target cargo check --workspace $EXTRA"
    ;;
  test)
    nix-shell --command "CARGO_TARGET_DIR=/build/rust-target cargo test --workspace $EXTRA"
    ;;
  build)
    nix-shell --command "CARGO_TARGET_DIR=/build/rust-target cargo build --release $EXTRA"
    ;;
  clippy)
    nix-shell --command "CARGO_TARGET_DIR=/build/rust-target cargo clippy --workspace -- -D warnings $EXTRA"
    ;;
  fmt)
    nix-shell --command "cargo fmt --check $EXTRA"
    ;;
  clean)
    echo "Cleaning /build/rust-target..."
    rm -rf /build/rust-target 2>/dev/null && echo "✅ Done" || echo "⚠️ Could not clean"
    ;;
  *)
    echo "Usage: $0 [check|test|build|clippy|fmt|clean]"
    echo "Default: check (compiles in RAM tmpfs /build, auto-cleans on exit)"
    exit 1
    ;;
esac
