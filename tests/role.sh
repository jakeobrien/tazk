#!/bin/bash
# E2E test for issue roles: required on work types and gates, forbidden on epics and specs
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
"$TZ_BIN" init --tz-hash-ids=false >/dev/null 2>&1

field() { grep "^$2:" ".tazk/issues/$1.md" | sed "s/^$2: *//"; }

# Test 1: role is required for create
echo -e "\n${YELLOW}Test 1: create requires --role${NC}"
assert_fails "create without --role should fail" "$TZ_BIN" create "No role"
OUTPUT=$("$TZ_BIN" create "With role" -r human 2>&1)
assert_contains "$OUTPUT" "Created issue: tz-1" "create with --role should succeed"
assert_equals "open" "$(field tz-1 status)" "create should produce an open issue"
assert_equals "human" "$(field tz-1 role)" "create should store the role"

# Test 2: reopen requires a role
echo -e "\n${YELLOW}Test 2: reopen requires a role${NC}"
"$TZ_BIN" close tz-1 >/dev/null 2>&1
# Simulate a legacy issue with no role.
sed -i.bak '/^role:/d' .tazk/issues/tz-1.md && rm .tazk/issues/tz-1.md.bak
OUTPUT=$("$TZ_BIN" reopen tz-1 2>&1 || true)
assert_contains "$OUTPUT" "--role" "reopen without a role should ask for --role"
assert_equals "closed" "$(field tz-1 status)" "Failed reopen should leave the issue closed"
OUTPUT=$("$TZ_BIN" reopen tz-1 --role agent 2>&1)
assert_contains "$OUTPUT" "Reopened issue: tz-1" "reopen with --role should succeed"
assert_equals "agent" "$(field tz-1 role)" "Role should be set by reopen"
"$TZ_BIN" close tz-1 >/dev/null 2>&1
OUTPUT=$("$TZ_BIN" reopen tz-1 2>&1)
assert_contains "$OUTPUT" "Reopened issue: tz-1" "reopen should keep an existing role"

# Test 3: epics and specs take no role (create)
echo -e "\n${YELLOW}Test 3: epic/spec create${NC}"
for ty in epic spec; do
    OUTPUT=$("$TZ_BIN" create "A $ty" -t "$ty" -r agent 2>&1 || true)
    assert_contains "$OUTPUT" "take no role" "create $ty with --role should fail"
done
assert_fails "create epic with --role should leave no issue" test -e .tazk/issues/tz-2.md
"$TZ_BIN" create "An epic" -t epic >/dev/null 2>&1
"$TZ_BIN" create "A spec" -t spec -d "The spec" >/dev/null 2>&1
assert_equals "" "$(field tz-2 role)" "epic should have no role"
assert_equals "" "$(field tz-3 role)" "spec should have no role"
for ty in task bug feature chore gate; do
    assert_fails "create $ty without --role should fail" "$TZ_BIN" create "No role" -t "$ty"
done
OUTPUT=$("$TZ_BIN" create "A gate" -t gate -r human -d "The condition" --acceptance "Check it" 2>&1)
assert_contains "$OUTPUT" "Created issue" "create gate with --role should succeed"
assert_equals "human" "$(field tz-4 role)" "gate should store the role"

# Test 4: update -r / --clear-role
echo -e "\n${YELLOW}Test 4: update role${NC}"
OUTPUT=$("$TZ_BIN" update tz-2 -r human 2>&1 || true)
assert_contains "$OUTPUT" "take no role" "update -r on an epic should fail"
assert_contains "$OUTPUT" "--clear-role" "the error should point at --clear-role"
assert_fails "update -r on a spec should fail" "$TZ_BIN" update tz-3 -r agent
"$TZ_BIN" update tz-2 --clear-role >/dev/null 2>&1
assert_fails "update --clear-role on a task should fail" "$TZ_BIN" update tz-1 --clear-role
assert_fails "update --clear-role on a gate should fail" "$TZ_BIN" update tz-4 --clear-role
assert_equals "agent" "$(field tz-1 role)" "failed --clear-role should keep the role"
"$TZ_BIN" update tz-1 -r human >/dev/null 2>&1
assert_equals "human" "$(field tz-1 role)" "update -r on a task should change the role"

# Test 5: reopen epics and specs
echo -e "\n${YELLOW}Test 5: reopen epic/spec${NC}"
"$TZ_BIN" close tz-2 >/dev/null 2>&1
OUTPUT=$("$TZ_BIN" reopen tz-2 --role agent 2>&1 || true)
assert_contains "$OUTPUT" "take no role" "reopen of an epic rejects --role"
assert_equals "closed" "$(field tz-2 status)" "failed reopen should leave the epic closed"
OUTPUT=$("$TZ_BIN" reopen tz-2 2>&1)
assert_contains "$OUTPUT" "Reopened issue: tz-2" "reopen of an epic needs no role"
assert_equals "" "$(field tz-2 role)" "reopen should not add a role"

# Test 6: -r none filter
echo -e "\n${YELLOW}Test 6: -r none${NC}"
OUTPUT=$("$TZ_BIN" list -r none 2>&1)
assert_contains "$OUTPUT" "tz-2" "list -r none should include the epic"
assert_contains "$OUTPUT" "tz-3" "list -r none should include the spec"
assert_equals "0" "$(grep -c 'tz-1\|tz-4' <<<"$OUTPUT" || true)" "list -r none should exclude issues with roles"
OUTPUT=$("$TZ_BIN" list -r agent 2>&1)
assert_equals "0" "$(grep -c 'tz-2\|tz-3' <<<"$OUTPUT" || true)" "list -r agent should exclude epics and specs"
OUTPUT=$("$TZ_BIN" ready -r none --type epic 2>&1)
assert_contains "$OUTPUT" "tz-2" "ready -r none --type epic should include the epic"
OUTPUT=$("$TZ_BIN" ready -r none 2>&1)
assert_equals "0" "$(grep -c 'tz-' <<<"$OUTPUT" || true)" "ready -r none should match nothing when every ready issue has a role"
OUTPUT=$("$TZ_BIN" ready -r agent --type epic 2>&1)
assert_equals "0" "$(grep -c 'tz-' <<<"$OUTPUT" || true)" "ready -r agent --type epic should not match a role-less epic"
assert_fails "list -r bogus should fail" "$TZ_BIN" list -r bogus

# Test 7: legacy epic/spec with a stored role
echo -e "\n${YELLOW}Test 7: legacy role on epic/spec${NC}"
sed -i.bak 's/^issue_type: epic$/issue_type: epic\nrole: agent/' .tazk/issues/tz-2.md && rm .tazk/issues/tz-2.md.bak
assert_equals "agent" "$(field tz-2 role)" "legacy epic should have a stored role"
"$TZ_BIN" show tz-2 >/dev/null 2>&1
OUTPUT=$("$TZ_BIN" update tz-2 --title "Renamed epic" -p 1 2>&1)
assert_contains "$OUTPUT" "Updated issue: tz-2" "other updates on a legacy epic should succeed"
assert_equals "agent" "$(field tz-2 role)" "other updates should keep the legacy role"
assert_fails "update -r on a legacy epic should fail" "$TZ_BIN" update tz-2 -r human
OUTPUT=$("$TZ_BIN" close tz-2 2>&1)
assert_contains "$OUTPUT" "Closed issue: tz-2" "close of a legacy epic should succeed"
"$TZ_BIN" reopen tz-2 >/dev/null 2>&1
"$TZ_BIN" update tz-2 --clear-role >/dev/null 2>&1
assert_equals "" "$(field tz-2 role)" "--clear-role should remove the legacy role"

# Test 8: triage promote
echo -e "\n${YELLOW}Test 8: triage promote${NC}"
"$TZ_BIN" triage add "found one" >/dev/null 2>&1
"$TZ_BIN" triage add "found two" >/dev/null 2>&1
"$TZ_BIN" triage add "found three" >/dev/null 2>&1
NAMES=($("$TZ_BIN" triage list))
OUTPUT=$("$TZ_BIN" triage promote "${NAMES[0]}" --title "Epic" -t epic -r agent 2>&1 || true)
assert_contains "$OUTPUT" "take no role" "promote an epic with --role should fail"
assert_equals "3" "$(ls .tazk/triage | wc -l | tr -d ' ')" "failed promote should delete nothing"
assert_fails "promote a task without --role should fail" "$TZ_BIN" triage promote "${NAMES[0]}" --title "T"
"$TZ_BIN" triage promote "${NAMES[0]}" --title "Epic" -t epic >/dev/null 2>&1
"$TZ_BIN" triage promote "${NAMES[1]}" --title "Spec" -t spec >/dev/null 2>&1
"$TZ_BIN" triage promote "${NAMES[2]}" --title "Task" -r agent >/dev/null 2>&1
assert_equals "" "$(field tz-5 role)" "promoted epic should have no role"
assert_equals "" "$(field tz-6 role)" "promoted spec should have no role"
assert_equals "agent" "$(field tz-7 role)" "promoted task should keep its role"

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
