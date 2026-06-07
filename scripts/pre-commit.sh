#!/bin/bash
# scripts/pre-commit.sh
# Pre-commit hook for Xavier: cargo fmt + clippy + test check.

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}Running pre-commit checks...${NC}"

# Identify staged Rust files
STAGED_RS_FILES=$(git diff --cached --name-only --diff-filter=ACMR | grep '\.rs$' || true)

if [ -z "$STAGED_RS_FILES" ]; then
    echo -e "${GREEN}No Rust files staged. Skipping Rust-specific checks.${NC}"
    exit 0
fi

echo -e "${CYAN}Detected staged Rust files. Running checks...${NC}"

# 1. cargo fmt --check
echo -e "${YELLOW}Step 1: Running cargo fmt --check...${NC}"
if ! cargo fmt --check; then
    echo -e "${RED}Error: cargo fmt --check failed.${NC}"
    echo -e "${YELLOW}Run 'cargo fmt' to fix formatting issues.${NC}"
    exit 1
fi

# 2. cargo clippy
echo -e "${YELLOW}Step 2: Running cargo clippy...${NC}"
if ! cargo clippy --lib --features ci-safe -- -D warnings; then
    echo -e "${RED}Error: cargo clippy failed.${NC}"
    exit 1
fi

# 3. cargo test
echo -e "${YELLOW}Step 3: Running cargo test...${NC}"
if ! cargo test --lib --features ci-safe -q; then
    echo -e "${RED}Error: cargo test failed.${NC}"
    exit 1
fi

echo -e "${GREEN}All pre-commit checks passed!${NC}"
exit 0
