#!/bin/bash
set -e

# Xavier Build Script for Tauri Sidecar
echo "Building Xavier backend..."
cargo build --release --bin xavier

# Get the target triple
TARGET_TRIPLE=$(rustc -Vv | grep host: | cut -d ' ' -f 2)
echo "Detected target triple: $TARGET_TRIPLE"

# Create binaries directory if it doesn't exist
mkdir -p panel-ui/src-tauri/binaries

# Copy and rename binary
BINARY_EXT=""
if [[ "$TARGET_TRIPLE" == *"windows"* ]]; then
    BINARY_EXT=".exe"
fi

cp "target/release/xavier$BINARY_EXT" "panel-ui/src-tauri/binaries/xavier-$TARGET_TRIPLE$BINARY_EXT"
echo "Binary copied to panel-ui/src-tauri/binaries/xavier-$TARGET_TRIPLE$BINARY_EXT"

# Build Tauri App
echo "Building Tauri App..."
cd panel-ui
pnpm install
pnpm tauri build
