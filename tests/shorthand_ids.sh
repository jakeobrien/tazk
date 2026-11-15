#!/bin/bash
# E2E test for basic tz operations
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test configuration
TEST_NAME="shorthand_ids"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TZ_BIN="$WORKSPACE_ROOT/target/debug/tz"
TEST_DIR="$WORKSPACE_ROOT/scratch/e2e_${TEST_NAME}_$$"

# Counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

# Cleanup function
cleanup() {
    if [ -d "$TEST_DIR" ]; then
        rm -rf "$TEST_DIR"
    fi
}

# Error handler
error_handler() {
    echo -e "${RED}✗ Test failed at line $1${NC}" >&2
    echo -e "${RED}Test directory preserved: $TEST_DIR${NC}" >&2
    exit 1
}

# Success message
success() {
    echo -e "${GREEN}✓ $1${NC}"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

# Fail message
fail() {
    echo -e "${RED}✗ $1${NC}"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

# Assert equals
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

# Assert contains
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

# Set up error handling
trap 'error_handler $LINENO' ERR

echo "=========================================="
echo "Running E2E Test: $TEST_NAME"
echo "=========================================="
echo ""

# Ensure tz binary exists
if [ ! -f "$TZ_BIN" ]; then
    echo "Building tz binary..."
    cd "$WORKSPACE_ROOT"
    cargo build
    echo ""
fi

# Create test directory
echo "Creating test directory: $TEST_DIR"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR"

"$TZ_BIN" init --tz-hash-ids=false >/dev/null 2>&1
# Create issues: tz-1 .. tz-6
for i in 1 2 3 4 5 6; do
    "$TZ_BIN" create "Issue $i" -r agent >/dev/null 2>&1
done

echo -e "\n${YELLOW}Test 1: read commands${NC}"
assert_contains "$("$TZ_BIN" show 1)" "ID: tz-1" "show 1"
assert_contains "$("$TZ_BIN" show TZ-2)" "ID: tz-2" "show TZ-2 (case-insensitive)"
OUTPUT=$("$TZ_BIN" show 1 2 2>&1)
assert_contains "$OUTPUT" "ID: tz-2" "show 1 2"
OUTPUT=$("$TZ_BIN" list --id 1,tz-2,3 2>&1)
assert_contains "$OUTPUT" "tz-1:" "list --id includes tz-1"
assert_contains "$OUTPUT" "tz-3:" "list --id includes tz-3"
assert_equals "3" "$(grep -c '^tz-' <<<"$OUTPUT")" "list --id N,M filters to the named issues"
OUTPUT=$("$TZ_BIN" ready --id 4,5 2>&1)
assert_equals "2" "$(grep -c 'tz-[45]' <<<"$OUTPUT")" "ready --id N,M filters"

echo -e "\n${YELLOW}Test 2: invalid IDs${NC}"
for bad in 0 01 tz- x-1 a-b; do
    OUTPUT=$("$TZ_BIN" show "$bad" 2>&1 || true)
    assert_contains "$OUTPUT" "expected tz-<n> or tz-<hash> (or just <n> / <hash>)" "show $bad is rejected"
done
OUTPUT=$("$TZ_BIN" show "" 2>&1 || true)
assert_contains "$OUTPUT" "Invalid issue ID" "show '' is rejected"
assert_fails "list --id with a bad entry" "$TZ_BIN" list --id 1,x-1

echo -e "\n${YELLOW}Test 3: write commands${NC}"
assert_contains "$("$TZ_BIN" update 1 --notes n 2>&1)" "Updated issue: tz-1" "update 1"
assert_contains "$("$TZ_BIN" claim 1 --as bob 2>&1)" "tz-1" "claim 1"
assert_contains "$("$TZ_BIN" claim 1 --as bob --release 2>&1)" "tz-1" "claim --release 1"
assert_contains "$("$TZ_BIN" block 2 2>&1)" "tz-2" "block 2"
assert_contains "$("$TZ_BIN" block 2 --release 2>&1)" "tz-2" "block --release 2"
assert_contains "$("$TZ_BIN" label add 1 2 urgent 2>&1)" "tz-2" "label add 1 2 urgent"
assert_contains "$("$TZ_BIN" label list 2 2>&1)" "urgent" "label list 2"
assert_contains "$("$TZ_BIN" comments add 1 --body hi --author me 2>&1)" "Added comment" "comments add 1"
assert_contains "$("$TZ_BIN" comments list 1 2>&1)" "hi" "comments list 1"
assert_contains "$("$TZ_BIN" close 6 2>&1)" "Closed issue: tz-6" "close 6"
assert_contains "$("$TZ_BIN" reopen 6 --role agent 2>&1)" "tz-6" "reopen 6"

echo -e "\n${YELLOW}Test 4: dependencies and parents${NC}"
assert_contains "$("$TZ_BIN" dep add 2 --target 1 2>&1)" "Added dependency: tz-2 depends on tz-1" "dep add (both IDs)"
assert_contains "$("$TZ_BIN" dep tree 2 2>&1)" "tz-1" "dep tree 2"
assert_contains "$("$TZ_BIN" dep list 2 2>&1)" "tz-1" "dep list 2"
assert_contains "$("$TZ_BIN" dep remove 2 --target 1 2>&1)" "Removed dependency" "dep remove 2 --target 1"
"$TZ_BIN" create "Child" -r agent --parent 3 --deps blocked_by:4,5 >/dev/null 2>&1
CHILD=.tazk/issues/tz-7.md
assert_equals "1" "$(grep -c 'tz-3: child_of' $CHILD)" "create --parent 3"
assert_equals "1" "$(grep -c 'tz-4: blocked_by' $CHILD)" "create --deps blocked_by:4,5 (tz-4)"
assert_equals "1" "$(grep -c 'tz-5: blocked_by' $CHILD)" "create --deps blocked_by:4,5 (tz-5)"
"$TZ_BIN" create "Plain dep" -r agent --deps 4,tz-5 >/dev/null 2>&1
assert_equals "1" "$(grep -c 'tz-4: blocked_by' .tazk/issues/tz-8.md)" "create --deps 4,tz-5"
assert_contains "$("$TZ_BIN" children 3 2>&1)" "tz-7" "children 3"
assert_contains "$("$TZ_BIN" list --parent 3 2>&1)" "tz-7" "list --parent 3"
assert_fails "create --parent with a bad ID" "$TZ_BIN" create X -r agent --parent x-1

echo -e "\n${YELLOW}Test 5: triage${NC}"
NAME=$("$TZ_BIN" triage add "Found while on tz-3" 2>&1 | sed -n 's/^Added triage item: *//p' | head -1)
NAME=${NAME:-$("$TZ_BIN" triage list | head -1)}
assert_contains "$("$TZ_BIN" triage show "$NAME" 2>&1)" "Found while" "triage names are unaffected"
OUTPUT=$("$TZ_BIN" triage promote "$NAME" --title Promoted -r agent --parent 3 --deps related_to:4 2>&1)
assert_contains "$OUTPUT" "Created issue: tz-9" "triage promote --parent 3 --deps related_to:4"
assert_equals "1" "$(grep -c 'tz-3: child_of' .tazk/issues/tz-9.md)" "promote --parent normalized"
assert_equals "1" "$(grep -c 'tz-4: related_to' .tazk/issues/tz-9.md)" "promote --deps normalized"
assert_contains "$("$TZ_BIN" ready --parent 3 2>&1)" "tz-9" "ready --parent 3"
"$TZ_BIN" triage add "Second item" >/dev/null 2>&1
NAME=$("$TZ_BIN" triage list | head -1)
OUTPUT=$("$TZ_BIN" triage attach "$NAME" --to 3 --author me 2>&1)
assert_contains "$OUTPUT" "tz-3" "triage attach --to 3"

echo -e "\n${YELLOW}Test 6: hash IDs${NC}"
cd "$TEST_DIR"
mkdir hash && cd hash
"$TZ_BIN" init --tz-hash-ids >/dev/null 2>&1
ID=$("$TZ_BIN" create "Hashed" -r agent 2>&1 | sed -n 's/^Created issue: //p')
SUFFIX=${ID#tz-}
assert_contains "$("$TZ_BIN" show "$SUFFIX" 2>&1)" "ID: $ID" "show with a bare hash"
UPPER=$(tr 'a-z' 'A-Z' <<<"$SUFFIX")
assert_contains "$("$TZ_BIN" show "$UPPER" 2>&1)" "ID: $ID" "show with an uppercase bare hash"
assert_contains "$("$TZ_BIN" close "$SUFFIX" 2>&1)" "Closed issue: $ID" "close with a bare hash"

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
