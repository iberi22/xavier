#!/bin/bash
# scripts/ci-local.sh
# Automated Local CI pipeline for Xavier: cargo check + test + clippy + fmt.

set -euo pipefail

# Colors and styles
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# Default modes
QUICK_MODE=false
FIX_MODE=false

# Print beautiful header
echo -e "${BLUE}${BOLD}======================================================================${NC}"
echo -e "${PURPLE}${BOLD}                   XAVIER LOCAL CI PIPELINE                           ${NC}"
echo -e "${BLUE}${BOLD}======================================================================${NC}"

# Parse command-line arguments
for arg in "$@"; do
  case $arg in
    --quick)
      QUICK_MODE=true
      shift
      ;;
    --fix)
      FIX_MODE=true
      shift
      ;;
    -h|--help)
      echo "Usage: ./scripts/ci-local.sh [options]"
      echo ""
      echo "Options:"
      echo "  --quick    Run only 'cargo check' (fast feedback loop)"
      echo "  --fix      Auto-format the codebase and auto-fix clippy issues before running checks"
      echo "  -h, --help Show this help message"
      exit 0
      ;;
    *)
      echo -e "${RED}Unknown argument: $arg${NC}"
      echo "Use -h or --help for usage instructions."
      exit 1
      ;;
  esac
done

# Check if tools are installed
echo -e "${CYAN}Verifying required toolchain...${NC}"
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: 'cargo' is not installed or not in PATH.${NC}"
    exit 1
fi
if ! cargo clippy --version &> /dev/null; then
    echo -e "${YELLOW}Warning: 'clippy' is not available. Clippy checks will be skipped.${NC}"
    HAS_CLIPPY=false
else
    HAS_CLIPPY=true
fi
if ! rustfmt --version &> /dev/null; then
    echo -e "${YELLOW}Warning: 'rustfmt' is not available. Formatting checks will be skipped.${NC}"
    HAS_FMT=false
else
    HAS_FMT=true
fi

# Initialize results tracking
CHECK_STATUS="SKIPPED"
TEST_STATUS="SKIPPED"
CLIPPY_STATUS="SKIPPED"
FMT_STATUS="SKIPPED"

# Track overall exit code
EXIT_CODE=0

# Step 1: Fix / Format Mode (if enabled)
if [ "$FIX_MODE" = true ]; then
    echo -e "\n${YELLOW}${BOLD}[Fix Mode Active] Auto-fixing formatting and clippy issues...${NC}"
    if [ "$HAS_FMT" = true ]; then
        echo -e "${CYAN}Running cargo fmt...${NC}"
        if cargo fmt; then
            echo -e "${GREEN}✓ Formatting applied successfully.${NC}"
        else
            echo -e "${RED}✗ Failed to run cargo fmt.${NC}"
        fi
    fi
    if [ "$HAS_CLIPPY" = true ]; then
        echo -e "${CYAN}Running cargo clippy --fix...${NC}"
        if cargo clippy --fix --allow-dirty --allow-staged -p xavier --lib --bins --no-deps; then
            echo -e "${GREEN}✓ Clippy auto-fixes applied successfully.${NC}"
        else
            echo -e "${RED}✗ Failed to run cargo clippy --fix.${NC}"
        fi
    fi
fi

# Step 2: Run Checks
# Check Step
echo -e "\n${CYAN}${BOLD}Step 1: Running cargo check...${NC}"
if cargo check -p xavier --lib --bins; then
    echo -e "${GREEN}✓ cargo check passed!${NC}"
    CHECK_STATUS="PASSED"
else
    echo -e "${RED}✗ cargo check failed!${NC}"
    CHECK_STATUS="FAILED"
    EXIT_CODE=1
fi

if [ "$QUICK_MODE" = false ] && [ "$EXIT_CODE" -eq 0 ]; then
    # Clippy Step
    if [ "$HAS_CLIPPY" = true ]; then
        echo -e "\n${CYAN}${BOLD}Step 2: Running cargo clippy...${NC}"
        if cargo clippy -p xavier --lib --bins --no-deps; then
            echo -e "${GREEN}✓ cargo clippy passed!${NC}"
            CLIPPY_STATUS="PASSED"
        else
            echo -e "${RED}✗ cargo clippy failed!${NC}"
            CLIPPY_STATUS="FAILED"
            EXIT_CODE=1
        fi
    fi

    # Format Check Step
    if [ "$EXIT_CODE" -eq 0 ] && [ "$HAS_FMT" = true ]; then
        echo -e "\n${CYAN}${BOLD}Step 3: Running cargo fmt --check...${NC}"
        if cargo fmt --check; then
            echo -e "${GREEN}✓ Code formatting check passed!${NC}"
            FMT_STATUS="PASSED"
        else
            echo -e "${RED}✗ Code formatting check failed!${NC}"
            echo -e "${YELLOW}Tip: Run './scripts/ci-local.sh --fix' to auto-format the codebase.${NC}"
            FMT_STATUS="FAILED"
            EXIT_CODE=1
        fi
    fi

    # Test Step
    if [ "$EXIT_CODE" -eq 0 ]; then
        echo -e "\n${CYAN}${BOLD}Step 4: Running cargo test...${NC}"
        if cargo test -p xavier --lib --bins; then
            echo -e "${GREEN}✓ All tests passed!${NC}"
            TEST_STATUS="PASSED"
        else
            echo -e "${RED}✗ Some tests failed!${NC}"
            TEST_STATUS="FAILED"
            EXIT_CODE=1
        fi
    fi
fi

# Beautiful Summary Table
echo -e "\n${BLUE}${BOLD}======================================================================${NC}"
echo -e "${BOLD}                           CI Run Summary                             ${NC}"
echo -e "${BLUE}${BOLD}======================================================================${NC}"

format_status() {
    local status=$1
    case $status in
        PASSED)  echo -e "${GREEN}${BOLD}PASSED${NC}" ;;
        FAILED)  echo -e "${RED}${BOLD}FAILED${NC}" ;;
        SKIPPED) echo -e "${YELLOW}SKIPPED${NC}" ;;
    esac
}

echo -e "  1. Cargo Check:       $(format_status "$CHECK_STATUS")"
if [ "$QUICK_MODE" = false ]; then
    echo -e "  2. Cargo Clippy:      $(format_status "$CLIPPY_STATUS")"
    echo -e "  3. Code Formatting:   $(format_status "$FMT_STATUS")"
    echo -e "  4. Cargo Test:        $(format_status "$TEST_STATUS")"
fi
echo -e "${BLUE}${BOLD}======================================================================${NC}"

if [ "$EXIT_CODE" -eq 0 ]; then
    echo -e "${GREEN}${BOLD}🎉 SUCCESS: Local CI pipeline completed successfully!${NC}\n"
else
    echo -e "${RED}${BOLD}❌ FAILURE: Local CI pipeline failed. Please resolve the issues above.${NC}\n"
fi

exit $EXIT_CODE
