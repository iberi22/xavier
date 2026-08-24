#!/usr/bin/env bash
# scripts/package-desktop-release.sh
# Automated Linux & Windows AppImage / NSIS / Portable Desktop Release Packaging Script
#
# Builds Panel UI frontend, compiles release Rust binaries, configures Tauri sidecars,
# packages production desktop bundles (AppImage / deb / NSIS / msi / portable),
# and generates SHA-256 checksums in dist/SHA256SUMS.

set -euo pipefail

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

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

BUILD_FRONTEND=true
BUILD_BINARIES=true
BUILD_TAURI=true
DRY_RUN=false
TARGET_TRIPLE=""

usage() {
    echo "Usage: $0 [options]"
    echo "Options:"
    echo "  -h, --help           Show this help message"
    echo "  --skip-frontend      Skip building Panel UI React/Vite assets"
    echo "  --skip-binaries      Skip compiling Rust release binaries"
    echo "  --skip-tauri         Skip running Tauri bundle build"
    echo "  --dry-run            Simulate build steps and create mockup structure"
    echo "  --target TRIPLE      Specify host or cross-target triple"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help)
            usage
            ;;
        --skip-frontend)
            BUILD_FRONTEND=false
            shift
            ;;
        --skip-binaries)
            BUILD_BINARIES=false
            shift
            ;;
        --skip-tauri)
            BUILD_TAURI=false
            shift
            ;;
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        --target)
            TARGET_TRIPLE="$2"
            shift 2
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

HOST_TRIPLE=$(rustc -Vv 2>/dev/null | grep host: | cut -d ' ' -f 2 || echo "x86_64-unknown-linux-gnu")
if [ -z "${TARGET_TRIPLE}" ]; then
    TARGET_TRIPLE="${HOST_TRIPLE}"
fi

log_info "Host Target: ${HOST_TRIPLE}"
log_info "Build Target: ${TARGET_TRIPLE}"

build_release_dist() {
    log_info "Starting release distribution packaging into dist/ ..."
    local dist_dir="${ROOT_DIR}/dist"
    mkdir -p "${dist_dir}"
    mkdir -p "${dist_dir}/panel-ui/build"

    # 1. Frontend Build
    if [ "$BUILD_FRONTEND" = true ]; then
        if [ "$DRY_RUN" = true ]; then
            log_info "[DRY-RUN] Creating mock panel-ui build directory..."
            mkdir -p panel-ui/build
            echo "<html><body>Mock Xavier Panel</body></html>" > panel-ui/build/index.html
        else
            log_info "Step 1: Building Panel UI frontend..."
            if command -v pnpm &>/dev/null; then
                (cd panel-ui && pnpm install && pnpm run build)
            else
                log_error "pnpm is required to build Panel UI."
                exit 1
            fi
        fi
    fi

    # Copy frontend build output to dist/
    if [ -d "panel-ui/build" ]; then
        cp -r panel-ui/build/* "${dist_dir}/panel-ui/build/" || true
        log_success "Copied panel-ui/build to dist/panel-ui/build"
    else
        log_warning "panel-ui/build missing. Skipping frontend dist copy."
    fi

    # 2. Rust Binaries Build
    local bin_ext=""
    if [[ "${TARGET_TRIPLE}" == *"windows"* ]]; then
        bin_ext=".exe"
    fi

    if [ "$BUILD_BINARIES" = true ]; then
        if [ "$DRY_RUN" = true ]; then
            log_info "[DRY-RUN] Creating mock release binaries..."
            mkdir -p target/release
            echo "#!/bin/sh" > "target/release/xavier${bin_ext}"
            echo "echo mock xavier" >> "target/release/xavier${bin_ext}"
            chmod +x "target/release/xavier${bin_ext}"
            echo "#!/bin/sh" > "target/release/xavier-tui${bin_ext}"
            echo "echo mock xavier-tui" >> "target/release/xavier-tui${bin_ext}"
            chmod +x "target/release/xavier-tui${bin_ext}"
        else
            log_info "Step 2: Compiling Rust core binaries in release mode..."
            CARGO_FLAGS=("--release" "--bin" "xavier" "--features" "cli-interactive")
            if [ "${TARGET_TRIPLE}" != "${HOST_TRIPLE}" ]; then
                CARGO_FLAGS+=("--target" "${TARGET_TRIPLE}")
            fi
            cargo build "${CARGO_FLAGS[@]}"

            # Optionally build tui if configured
            cargo build --release --bin xavier-tui --features "cli-interactive" 2>/dev/null || log_warning "xavier-tui binary build skipped or failed."
        fi
    fi

    # Locate binary target paths
    local bin_src_dir="target/release"
    if [ "${TARGET_TRIPLE}" != "${HOST_TRIPLE}" ]; then
        bin_src_dir="target/${TARGET_TRIPLE}/release"
    fi

    if [ -f "${bin_src_dir}/xavier${bin_ext}" ]; then
        cp "${bin_src_dir}/xavier${bin_ext}" "${dist_dir}/xavier${bin_ext}"
        log_success "Copied xavier${bin_ext} to dist/"
    fi

    if [ -f "${bin_src_dir}/xavier-tui${bin_ext}" ]; then
        cp "${bin_src_dir}/xavier-tui${bin_ext}" "${dist_dir}/xavier-tui${bin_ext}"
        log_success "Copied xavier-tui${bin_ext} to dist/"
    fi

    # 3. Setup Tauri Sidecar & Package Bundles
    local tauri_bin_dir="panel-ui/src-tauri/binaries"
    mkdir -p "${tauri_bin_dir}"

    if [ -f "${bin_src_dir}/xavier${bin_ext}" ]; then
        cp "${bin_src_dir}/xavier${bin_ext}" "${tauri_bin_dir}/xavier-${TARGET_TRIPLE}${bin_ext}"
        log_info "Copied Tauri sidecar binary to ${tauri_bin_dir}/xavier-${TARGET_TRIPLE}${bin_ext}"
    fi

    if [ "$BUILD_TAURI" = true ]; then
        if [ "$DRY_RUN" = true ]; then
            log_info "[DRY-RUN] Mocking Tauri bundle outputs..."
            local mock_bundle_dir="panel-ui/src-tauri/target/release/bundle"
            if [[ "${TARGET_TRIPLE}" == *"windows"* ]]; then
                mkdir -p "${mock_bundle_dir}/nsis"
                echo "mock nsis setup" > "${mock_bundle_dir}/nsis/xavier_0.1.0_x64-setup.exe"
            else
                mkdir -p "${mock_bundle_dir}/appimage"
                echo "mock appimage" > "${mock_bundle_dir}/appimage/xavier_0.1.0_amd64.AppImage"
                mkdir -p "${mock_bundle_dir}/deb"
                echo "mock deb" > "${mock_bundle_dir}/deb/xavier_0.1.0_amd64.deb"
            fi
        else
            log_info "Step 3: Building Tauri desktop application package..."
            if command -v pnpm &>/dev/null && [ -f "panel-ui/src-tauri/tauri.conf.json" ]; then
                (cd panel-ui && pnpm tauri build) || log_warning "Tauri build exited with non-zero status; checking for partial artifacts."
            else
                log_warning "Tauri configuration or pnpm missing. Skipping tauri build step."
            fi
        fi

        # Collect Tauri bundle artifacts into dist/
        local bundle_search_dir="panel-ui/src-tauri/target/release/bundle"
        if [ -d "${bundle_search_dir}" ]; then
            log_info "Collecting bundle artifacts from ${bundle_search_dir}..."
            find "${bundle_search_dir}" -type f \( -name "*.AppImage" -o -name "*.deb" -o -name "*.exe" -o -name "*.msi" -o -name "*.dmg" \) | while read -r artifact; do
                local filename
                filename="$(basename "${artifact}")"
                cp "${artifact}" "${dist_dir}/${filename}"
                log_success "Exported release package: dist/${filename}"
            done
        fi
    fi

    # 4. Generate SHA-256 Checksums
    log_info "Step 4: Generating SHA-256 artifact manifest (dist/SHA256SUMS)..."
    (
        cd "${dist_dir}"
        rm -f SHA256SUMS
        if command -v sha256sum &>/dev/null; then
            find . -maxdepth 2 -type f ! -name "SHA256SUMS" -exec sha256sum {} + > SHA256SUMS
        elif command -v shasum &>/dev/null; then
            find . -maxdepth 2 -type f ! -name "SHA256SUMS" -exec shasum -a 256 {} + > SHA256SUMS
        else
            log_warning "Neither sha256sum nor shasum is available. SHA256SUMS not generated."
        fi
    )

    if [ -f "${dist_dir}/SHA256SUMS" ]; then
        log_success "SHA-256 checksum manifest created at dist/SHA256SUMS"
    fi

    log_success "Desktop release packaging complete! Output directory: ${dist_dir}"
}

# Execute packaging function
build_release_dist
