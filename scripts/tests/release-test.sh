#!/usr/bin/env bash
set -euo pipefail

# Test suite for release script scripts/release.sh

SCRIPT_PATH="$(pwd)/scripts/release.sh"
TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT

echo "Running release-test.sh..."

setup_repo() {
    local dir="$1"
    mkdir -p "$dir"
    cd "$dir"
    git init -q
    git branch -m main

    cat << 'EOF' > Cargo.toml
[package]
name = "test-pkg"
version = "0.0.1"
EOF
    cat << 'EOF' > package.json
{
  "name": "test-pkg",
  "version": "0.0.1"
}
EOF
    cat << 'EOF' > CHANGELOG.md
# Changelog
## [Unreleased]
- test item
EOF
    mkdir -p scripts
    cat << 'EOF' > scripts/check-version-sync.sh
#!/usr/bin/env bash
exit 0
EOF
    chmod +x scripts/check-version-sync.sh

    git add .
    git commit -q -m "init"
}

# Test 1: Dry run changes nothing
test_dry_run() {
    local dir="$TEMP_DIR/test1"
    setup_repo "$dir"
    cd "$dir"

    if bash "$SCRIPT_PATH" 0.0.2 > ../output.log 2>&1; then
        echo "PASS: test_dry_run (exit 0)"
    else
        echo "FAIL: test_dry_run exited non-zero" >&2
        cat ../output.log
        echo "git status:" >&2
        git status --porcelain >&2
        exit 1
    fi

    # Check no tag
    if git rev-parse v0.0.2 >/dev/null 2>&1; then
        echo "FAIL: test_dry_run created a tag!" >&2
        exit 1
    fi

    # Check clean status
    if [[ -n "$(git status --porcelain)" ]]; then
        echo "FAIL: test_dry_run dirtied tree!" >&2
        exit 1
    fi
}

# Test 2: Execute works properly
test_execute() {
    local dir="$TEMP_DIR/test2"
    setup_repo "$dir"
    cd "$dir"

    if bash "$SCRIPT_PATH" 0.0.2 --execute > ../output.log 2>&1; then
        echo "PASS: test_execute (exit 0)"
    else
        echo "FAIL: test_execute exited non-zero" >&2
        cat ../output.log
        exit 1
    fi

    # Check tag
    if ! git rev-parse v0.0.2 >/dev/null 2>&1; then
        echo "FAIL: test_execute failed to create tag!" >&2
        exit 1
    fi

    # Check updated version
    if ! grep -q 'version = "0.0.2"' Cargo.toml; then
        echo "FAIL: Cargo.toml not updated" >&2
        exit 1
    fi

    # Check changed branch is clean
    if [[ -n "$(git status --porcelain)" ]]; then
        echo "FAIL: test_execute left tree dirty!" >&2
        exit 1
    fi
}

# Test 3: Dirty tree refusal
test_dirty_tree() {
    local dir="$TEMP_DIR/test3"
    setup_repo "$dir"
    cd "$dir"
    echo "dirty" >> Cargo.toml

    if bash "$SCRIPT_PATH" 0.0.2 > ../output.log 2>&1; then
        echo "FAIL: test_dirty_tree should have failed" >&2
        exit 1
    else
        echo "PASS: test_dirty_tree"
    fi
}

# Test 4: Not on main refusal
test_not_main() {
    local dir="$TEMP_DIR/test4"
    setup_repo "$dir"
    cd "$dir"
    git checkout -q -b feature

    if bash "$SCRIPT_PATH" 0.0.2 > ../output.log 2>&1; then
        echo "FAIL: test_not_main should have failed" >&2
        exit 1
    else
        echo "PASS: test_not_main"
    fi
}

# Test 5: Version not greater refusal
test_version_not_greater() {
    local dir="$TEMP_DIR/test5"
    setup_repo "$dir"
    cd "$dir"

    if bash "$SCRIPT_PATH" 0.0.1 > ../output.log 2>&1; then
        echo "FAIL: test_version_not_greater should have failed" >&2
        exit 1
    else
        echo "PASS: test_version_not_greater"
    fi
}

# Test 6: Tag already exists refusal
test_tag_exists() {
    local dir="$TEMP_DIR/test6"
    setup_repo "$dir"
    cd "$dir"
    git tag v0.0.2

    if bash "$SCRIPT_PATH" 0.0.2 > ../output.log 2>&1; then
        echo "FAIL: test_tag_exists should have failed" >&2
        exit 1
    else
        echo "PASS: test_tag_exists"
    fi
}

# Test 7: Empty changelog refusal
test_empty_changelog() {
    local dir="$TEMP_DIR/test7"
    setup_repo "$dir"
    cd "$dir"

    cat << 'EOF' > CHANGELOG.md
# Changelog
## [Unreleased]
EOF
    git commit -q -am "empty changelog"

    if bash "$SCRIPT_PATH" 0.0.2 > ../output.log 2>&1; then
        echo "FAIL: test_empty_changelog should have failed" >&2
        exit 1
    else
        echo "PASS: test_empty_changelog"
    fi
}

test_dry_run
test_execute
test_dirty_tree
test_not_main
test_version_not_greater
test_tag_exists
test_empty_changelog

echo "All test cases passed successfully!"