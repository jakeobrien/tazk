#!/bin/bash
# E2E test for issue roles: required on create, required to reopen
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test configuration
TEST_NAME="role"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TZ_BIN="$WORKSPACE_ROOT/target/debug/tz"
TEST_DIR="$WORKSPACE_ROOT/scratch/e2e_${TEST_NAME}_$$"

# Counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

cleanup() {
    if [ -d "$TEST_DIR" ]; then
        rm -rf "$TEST_DIR"
    fi
}

error_handler() {
    echo -e "${RED}✗ Test failed at line $1${NC}" >&2
    echo -e "${RED}Test directory preserved: $TEST_DIR${NC}" >&2
    exit 1
}

success() {
    echo -e "${GREEN}✓ $1${NC}"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

fail() {
    echo -e "${RED}✗ $1${NC}"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

assert_equals() {
    TESTS_RUN=$((TESTS_RUN + 1))
    local expected="$1"
    local actual="$2"
    local message="${3:-Assertion failed}"
    if [ "$expected" = "$actual" ]; then
        success "$message"
    else
        fail "$message (expected: '$expected', got: '$actual')"
        return 1
    fi
}

assert_contains() {
    TESTS_RUN=$((TESTS_RUN + 1))
    local haystack="$1"
    local needle="$2"
    local message="${3:-Assertion failed}"
    if grep -qF -- "$needle" <<<"$haystack"; then
        success "$message"
    else
        fail "$message (expected to find: '$needle' in output)"
        return 1
    fi
}

# Assert a command fails (non-zero exit)
assert_fails() {
    TESTS_RUN=$((TESTS_RUN + 1))
    local message="${1:-Command should fail}"
    shift
    if "$@" >/dev/null 2>&1; then
        fail "$message (command unexpectedly succeeded)"
        return 1
    else
        success "$message"
    fi
}

trap 'error_handler $LINENO' ERR

echo "=========================================="
echo "Running E2E Test: $TEST_NAME"
echo "=========================================="
echo ""

if [ ! -f "$TZ_BIN" ]; then
    echo "Building tz binary..."
    cd "$WORKSPACE_ROOT"
    cargo build
    echo ""
fi

echo "Creating test directory: $TEST_DIR"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR"

# Setup: an empty repo
"$TZ_BIN" init --prefix test >/dev/null 2>&1

field() { grep "^$2:" ".tazk/issues/$1.md" | sed "s/^$2: *//"; }

# Test 1: role is required for create
echo -e "\n${YELLOW}Test 1: create requires --role${NC}"
assert_fails "create without --role should fail" "$TZ_BIN" create "No role"
OUTPUT=$("$TZ_BIN" create "With role" -r human 2>&1)
assert_contains "$OUTPUT" "Created issue: test-1" "create with --role should succeed"
assert_equals "open" "$(field test-1 status)" "create should produce an open issue"
assert_equals "human" "$(field test-1 role)" "create should store the role"

# Test 2: reopen requires a role
echo -e "\n${YELLOW}Test 2: reopen requires a role${NC}"
"$TZ_BIN" close test-1 >/dev/null 2>&1
# Simulate a legacy issue with no role.
sed -i.bak '/^role:/d' .tazk/issues/test-1.md && rm .tazk/issues/test-1.md.bak
OUTPUT=$("$TZ_BIN" reopen test-1 2>&1 || true)
assert_contains "$OUTPUT" "--role" "reopen without a role should ask for --role"
assert_equals "closed" "$(field test-1 status)" "Failed reopen should leave the issue closed"
OUTPUT=$("$TZ_BIN" reopen test-1 --role agent 2>&1)
assert_contains "$OUTPUT" "Reopened issue: test-1" "reopen with --role should succeed"
assert_equals "agent" "$(field test-1 role)" "Role should be set by reopen"
"$TZ_BIN" close test-1 >/dev/null 2>&1
OUTPUT=$("$TZ_BIN" reopen test-1 2>&1)
assert_contains "$OUTPUT" "Reopened issue: test-1" "reopen should keep an existing role"

# Print summary
echo ""
echo "=========================================="
echo "Test Summary"
echo "=========================================="
echo "Tests run:    $TESTS_RUN"
echo "Tests passed: $TESTS_PASSED"
echo "Tests failed: $TESTS_FAILED"
echo ""

if [ $TESTS_FAILED -eq 0 ]; then
    echo -e "${GREEN}All tests passed!${NC}"
    cleanup
    exit 0
else
    echo -e "${RED}Some tests failed!${NC}"
    echo -e "${RED}Test directory preserved: $TEST_DIR${NC}"
    exit 1
fi
