#!/usr/bin/env bash
# scripts/test-package-artifacts.sh
# Desktop Release Packaging Artifact & Integrity Validator Script
#
# Inspects dist/ directory for expected release assets:
# 1. Executable binaries (xavier or xavier.exe)
# 2. Panel UI static frontend build files (panel-ui/build/index.html)
# 3. Checksum manifest file (SHA256SUMS)
# 4. Verifies SHA-256 hash checksum integrity

set -euo pipefail

log_info() {
    echo -e "\033[1;34m[INFO]\033[0m $1"
}

log_success() {
    echo -e "\033[1;32m[SUCCESS]\033[0m $1"
}

log_error() {
    echo -e "\033[1;31m[ERROR]\033[0m $1" >&2
}

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${ROOT_DIR}/dist"

log_info "Validating desktop release packaging artifacts in ${DIST_DIR}..."

if [ ! -d "${DIST_DIR}" ]; then
    log_error "Distribution directory ${DIST_DIR} does not exist!"
    exit 1
fi

# 1. Check core binary presence
if [ ! -f "${DIST_DIR}/xavier" ] && [ ! -f "${DIST_DIR}/xavier.exe" ]; then
    log_error "Missing xavier binary in ${DIST_DIR} (expected xavier or xavier.exe)"
    exit 1
fi
log_success "Core xavier binary found in ${DIST_DIR}."

# 2. Check panel-ui build presence
if [ ! -f "${DIST_DIR}/panel-ui/build/index.html" ]; then
    log_error "Missing frontend panel-ui build assets in ${DIST_DIR}/panel-ui/build/index.html!"
    exit 1
fi
log_success "Panel UI frontend assets found in ${DIST_DIR}/panel-ui/build/."

# 3. Check SHA256SUMS presence
if [ ! -f "${DIST_DIR}/SHA256SUMS" ]; then
    log_error "Missing SHA256SUMS checksum file in ${DIST_DIR}!"
    exit 1
fi
log_success "Checksum manifest SHA256SUMS found."

# 4. Verify SHA-256 integrity
(
    cd "${DIST_DIR}"
    if command -v sha256sum &>/dev/null; then
        log_info "Verifying SHA-256 checksums with sha256sum..."
        sha256sum -c SHA256SUMS
    elif command -v shasum &>/dev/null; then
        log_info "Verifying SHA-256 checksums with shasum..."
        shasum -a 256 -c SHA256SUMS
    else
        log_error "Neither sha256sum nor shasum tool is available for checksum verification!"
        exit 1
    fi
)

log_success "All desktop packaging release artifacts validated successfully!"
