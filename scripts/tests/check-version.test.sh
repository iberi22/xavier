#!/usr/bin/env bash
set -e

# Test suite for version gate script scripts/check-version-sync.sh

SCRIPT_PATH="$(pwd)/scripts/check-version-sync.sh"
TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT

echo "Running check-version.test.sh..."

# Test 1: Manifests in sync -> exit 0
test_manifests_sync_pass() {
    local dir="$TEMP_DIR/test1"
    mkdir -p "$dir"
    cat << 'EOF' > "$dir/Cargo.toml"
[package]
name = "test-pkg"
version = "0.0.1"
EOF
    cat << 'EOF' > "$dir/package.json"
{
  "name": "test-pkg",
  "version": "0.0.1"
}
EOF
    cat << 'EOF' > "$dir/CHANGELOG.md"
# Changelog
## [Unreleased]
EOF
    if bash "$SCRIPT_PATH" "$dir" > /dev/null 2>&1; then
        echo "PASS: test_manifests_sync_pass"
    else
        echo "FAIL: test_manifests_sync_pass" >&2
        exit 1
    fi
}

# Test 2: Cargo version drift (0.0.2 vs 0.0.1) -> exit 1
test_drift_cargo_vs_package_fail() {
    local dir="$TEMP_DIR/test2"
    mkdir -p "$dir"
    cat << 'EOF' > "$dir/Cargo.toml"
[package]
name = "test-pkg"
version = "0.0.2"
EOF
    cat << 'EOF' > "$dir/package.json"
{
  "name": "test-pkg",
  "version": "0.0.1"
}
EOF
    cat << 'EOF' > "$dir/CHANGELOG.md"
# Changelog
## [Unreleased]
EOF
    if bash "$SCRIPT_PATH" "$dir" > /dev/null 2>&1; then
        echo "FAIL: test_drift_cargo_vs_package_fail (should have failed)" >&2
        exit 1
    else
        echo "PASS: test_drift_cargo_vs_package_fail"
    fi
}

# Test 3: CHANGELOG missing [Unreleased] -> exit 1
test_changelog_missing_unreleased_fail() {
    local dir="$TEMP_DIR/test3"
    mkdir -p "$dir"
    cat << 'EOF' > "$dir/Cargo.toml"
[package]
name = "test-pkg"
version = "0.0.1"
EOF
    cat << 'EOF' > "$dir/package.json"
{
  "name": "test-pkg",
  "version": "0.0.1"
}
EOF
    cat << 'EOF' > "$dir/CHANGELOG.md"
# Changelog
## [0.0.1] - 2026-08-30
EOF
    if bash "$SCRIPT_PATH" "$dir" > /dev/null 2>&1; then
        echo "FAIL: test_changelog_missing_unreleased_fail (should have failed)" >&2
        exit 1
    else
        echo "PASS: test_changelog_missing_unreleased_fail"
    fi
}

# Test 4: Preflight JSON status ready true check -> exit 0
test_preflight_json_ready_true() {
    local tmp_json="$TEMP_DIR/preflight.json"
    echo '{"ready": true, "wave": 10, "stable": 52, "total": 52}' > "$tmp_json"
    if python3 -c "import json, sys; data=json.load(open('$tmp_json')); sys.exit(0 if data.get('ready') else 1)"; then
        echo "PASS: test_preflight_json_ready_true"
    else
        echo "FAIL: test_preflight_json_ready_true" >&2
        exit 1
    fi
}

# Run all tests
test_manifests_sync_pass
test_drift_cargo_vs_package_fail
test_changelog_missing_unreleased_fail
test_preflight_json_ready_true

echo "All 4 test cases passed successfully!"
