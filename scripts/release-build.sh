#!/usr/bin/env bash
# scripts/release-build.sh
# Xavier Release Build Orchestrator
# This script prepares the release assets by compiling core binaries,
# building the Panel UI frontend, and setting up the Tauri sidecar structure.

set -euo pipefail

# Print banner
echo "========================================="
echo "   Xavier Release Build Orchestrator"
echo "========================================="

# Helper functions for logging
log_info() {
    echo -e "\033[1;34m[INFO]\033[0m $1"
}

log_success() {
    echo -e "\033[1;32m[SUCCESS]\033[0m $1"
}

log_warning() {
    echo -e "\033[1;33m[WARNING]\033[0m $1"
}

log_error() {
    echo -e "\033[1;31m[ERROR]\033[0m $1" >&2
}

# Root directory detection
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${ROOT_DIR}"

# Command line options
BUILD_BINARIES=true
BUILD_FRONTEND=true
BUILD_TAURI=false
TARGET_TRIPLE=""

usage() {
    echo "Usage: $0 [options]"
    echo "Options:"
    echo "  -h, --help          Show this help message"
    echo "  --only-binaries     Only build the Rust backend binaries"
    echo "  --only-frontend     Only build the Panel UI React/Vite assets"
    echo "  --tauri             Trigger a production Tauri desktop application bundle build"
    echo "  --target TRIPLE     Specify a custom target triple (e.g. x86_64-pc-windows-gnu)"
    exit 1
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help)
            usage
            ;;
        --only-binaries)
            BUILD_BINARIES=true
            BUILD_FRONTEND=false
            BUILD_TAURI=false
            shift
            ;;
        --only-frontend)
            BUILD_BINARIES=false
            BUILD_FRONTEND=true
            BUILD_TAURI=false
            shift
            ;;
        --tauri)
            BUILD_TAURI=true
            shift
            ;;
        --target)
            TARGET_TRIPLE="$2"
            shift 2
            ;;
        *)
            log_error "Unknown option: $1"
            usage
            ;;
    esac
done

# Ensure prerequisites
check_cmd() {
    if ! command -v "$1" &>/dev/null; then
        log_error "Required command '$1' is missing. Please install it to continue."
        exit 1
    fi
}

check_cmd "cargo"
if [ "$BUILD_FRONTEND" = true ] || [ "$BUILD_TAURI" = true ]; then
    check_cmd "node"
    check_cmd "pnpm"
fi

# Determine target triple
HOST_TRIPLE=$(rustc -Vv | grep host: | cut -d ' ' -f 2)
if [ -z "${TARGET_TRIPLE}" ]; then
    TARGET_TRIPLE="${HOST_TRIPLE}"
fi
log_info "Host Target: ${HOST_TRIPLE}"
log_info "Build Target: ${TARGET_TRIPLE}"

# 1. Build Rust binaries
if [ "$BUILD_BINARIES" = true ]; then
    log_info "Step 1: Compiling core Xavier binaries in Release mode..."

    CARGO_FLAGS=("--release" "--features" "cli-interactive")
    if [ "${TARGET_TRIPLE}" != "${HOST_TRIPLE}" ]; then
        log_info "Cross-compiling target to ${TARGET_TRIPLE}"
        CARGO_FLAGS+=("--target" "${TARGET_TRIPLE}")
    fi

    cargo build "${CARGO_FLAGS[@]}"
    log_success "Core binaries compiled successfully!"
fi

# 2. Build Panel UI frontend
if [ "$BUILD_FRONTEND" = true ]; then
    log_info "Step 2: Building Panel UI production assets..."

    cd panel-ui
    pnpm install
    pnpm build
    cd ..

    if [ -d "panel-ui/build" ]; then
        log_success "Panel UI React/Vite assets generated under panel-ui/build/ and mirrored to panel-ui/dist/"
    else
        log_error "Panel UI build failed. Output directory 'panel-ui/build' is missing."
        exit 1
    fi
fi

# 3. Handle Tauri sidecar & Bundle Build
if [ "$BUILD_TAURI" = true ]; then
    log_info "Step 3: Preparing Tauri app with sidecar..."

    # Locate built sidecar binary
    BINARY_EXT=""
    if [[ "$TARGET_TRIPLE" == *"windows"* ]]; then
        BINARY_EXT=".exe"
    fi

    SRC_BIN_PATH="target/release/xavier${BINARY_EXT}"
    if [ "${TARGET_TRIPLE}" != "${HOST_TRIPLE}" ]; then
        SRC_BIN_PATH="target/${TARGET_TRIPLE}/release/xavier${BINARY_EXT}"
    fi

    if [ ! -f "${SRC_BIN_PATH}" ]; then
        log_warning "Xavier sidecar binary not found at ${SRC_BIN_PATH}. Re-building sidecar binary now..."
        cargo build --release --bin xavier
        SRC_BIN_PATH="target/release/xavier${BINARY_EXT}"
    fi

    # Place sidecar in Tauri binaries directory with the required naming scheme
    TAURI_BIN_DIR="panel-ui/src-tauri/binaries"
    mkdir -p "${TAURI_BIN_DIR}"

    TARGET_BIN_PATH="${TAURI_BIN_DIR}/xavier-${TARGET_TRIPLE}${BINARY_EXT}"
    cp "${SRC_BIN_PATH}" "${TARGET_BIN_PATH}"
    log_info "Copied sidecar binary to: ${TARGET_BIN_PATH}"

    log_info "Executing Tauri production bundle build..."
    cd panel-ui
    pnpm tauri build
    cd ..

    log_success "Tauri Desktop app bundle compiled successfully!"
fi

# Post-Build Status Check for Installer Scripts
if [[ "${TARGET_TRIPLE}" == *"windows"* ]]; then
    log_info "Windows build targets prepared."
    log_info "To generate the Windows installer, please run the following from a Windows host:"
    echo "  cd installer"
    echo "  powershell -ExecutionPolicy Bypass -File .\\build-installer.ps1"
else
    log_warning "Current host is non-Windows. Windows WiX/Inno Setup installers must be compiled on a Windows host or in a cross-environment."
fi

echo "========================================="
log_success "Xavier release build orchestration complete."
echo "========================================="
