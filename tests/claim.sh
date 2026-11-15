#!/bin/bash
# E2E test for the claim/release workflow (cross-machine coordination)
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test configuration
TEST_NAME="claim"
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

# Setup: a repo with two open issues
"$TZ_BIN" init --tz-hash-ids=false >/dev/null 2>&1
"$TZ_BIN" create "First task" -r agent -p 1 -t task >/dev/null 2>&1
"$TZ_BIN" create "Second task" -r agent -p 2 -t task >/dev/null 2>&1

# Test 1: claim sets assignee and status (no claim window)
echo -e "\n${YELLOW}Test 1: Claim an issue${NC}"
OUTPUT=$("$TZ_BIN" claim tz-1 --as boxA 2>&1)
assert_contains "$OUTPUT" "Claimed tz-1 as 'boxA'" "Claim should report holder"
assert_equals "in_progress" "$(grep '^status:' .tazk/issues/tz-1.md | awk '{print $2}')" "Status should be in_progress"
assert_equals "boxA" "$(grep '^assignee:' .tazk/issues/tz-1.md | awk '{print $2}')" "Assignee should be boxA"
assert_equals "false" "$(grep -q '^claimed_' .tazk/issues/tz-1.md && echo true || echo false)" "No claim window fields should be written"

# Test 2: --as is required, and update --claim / --for are gone
echo -e "\n${YELLOW}Test 2: Flag validation${NC}"
assert_fails "claim without --as should fail" "$TZ_BIN" claim tz-2
assert_fails "update --claim should no longer exist" "$TZ_BIN" update tz-2 --claim --as boxA
assert_fails "claim --for should no longer exist" "$TZ_BIN" claim tz-2 --as boxA --for 4h

# Test 3: an in_progress issue cannot be claimed, even by the same identity
echo -e "\n${YELLOW}Test 3: In-progress issue is protected${NC}"
assert_fails "Claim by another worker should fail" "$TZ_BIN" claim tz-1 --as boxB
assert_fails "Re-claim by the same worker should fail" "$TZ_BIN" claim tz-1 --as boxA
assert_equals "boxA" "$(grep '^assignee:' .tazk/issues/tz-1.md | awk '{print $2}')" "Holder should be unchanged after failed claim"

# Test 4: claimed issue drops out of ready
echo -e "\n${YELLOW}Test 4: Claimed work leaves the ready queue${NC}"
OUTPUT=$("$TZ_BIN" ready 2>&1)
if echo "$OUTPUT" | grep -qF "tz-1:"; then
    fail "tz-1 should not appear in ready while claimed"
else
    success "tz-1 excluded from ready while claimed"
fi
TESTS_RUN=$((TESTS_RUN + 1))

# Test 5: --force overrides the lock
echo -e "\n${YELLOW}Test 5: Force-claim an in-progress issue${NC}"
OUTPUT=$("$TZ_BIN" claim tz-1 --as boxB --force 2>&1)
assert_contains "$OUTPUT" "Claimed tz-1 as 'boxB'" "Forced claim should succeed"
assert_equals "boxB" "$(grep '^assignee:' .tazk/issues/tz-1.md | awk '{print $2}')" "Assignee should be boxB after forced claim"

# Test 6: host/team identity
echo -e "\n${YELLOW}Test 6: host/team identity${NC}"
OUTPUT=$("$TZ_BIN" claim tz-2 --as boxA --team backend 2>&1)
assert_contains "$OUTPUT" "Claimed tz-2 as 'boxA/backend'" "claim should support --team"
assert_equals "boxA/backend" "$(grep '^assignee:' .tazk/issues/tz-2.md | awk '{print $2}')" "Assignee should be boxA/backend"

# Test 7: release returns the issue to the backlog
echo -e "\n${YELLOW}Test 7: Release a claim${NC}"
OUTPUT=$("$TZ_BIN" claim tz-1 --release --as boxB 2>&1)
assert_contains "$OUTPUT" "Released tz-1" "Release should be confirmed"
assert_equals "open" "$(grep '^status:' .tazk/issues/tz-1.md | awk '{print $2}')" "Status should be open after release"
assert_equals "false" "$(grep -q '^assignee:' .tazk/issues/tz-1.md && echo true || echo false)" "Assignee should be cleared after release"
OUTPUT=$("$TZ_BIN" ready 2>&1)
assert_contains "$OUTPUT" "tz-1:" "tz-1 should return to ready after release"

# Test 8: releasing someone else's claim requires --force
echo -e "\n${YELLOW}Test 8: Force-release another worker's claim${NC}"
"$TZ_BIN" claim tz-1 --as boxA >/dev/null 2>&1
assert_fails "Releasing another worker's claim should fail without --force" "$TZ_BIN" claim tz-1 --release --as boxB
OUTPUT=$("$TZ_BIN" claim tz-1 --release --as boxB --force 2>&1)
assert_contains "$OUTPUT" "Released tz-1" "Force release should succeed"

# Test 9: claim survives JSONL round-trip
echo -e "\n${YELLOW}Test 9: Claim survives JSONL sync round-trip${NC}"
"$TZ_BIN" claim tz-1 --as boxA >/dev/null 2>&1
BEFORE=$(cat .tazk/issues/tz-1.md)
"$TZ_BIN" export --tz-output-default >/dev/null 2>&1
rm .tazk/issues/tz-1.md
"$TZ_BIN" sync >/dev/null 2>&1
AFTER=$(cat .tazk/issues/tz-1.md)
assert_equals "$BEFORE" "$AFTER" "Claimed issue should round-trip identically through JSONL"

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
