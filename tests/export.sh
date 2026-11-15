#!/bin/bash
# E2E test for tz export
set -euo pipefail

# Parse command line arguments
KEEP_TEMP=false
while [[ $# -gt 0 ]]; do
    case $1 in
        --keep)
            KEEP_TEMP=true
            shift
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--keep]"
            echo "  --keep    Keep temporary test directories instead of cleaning up"
            exit 1
            ;;
    esac
done

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Test configuration
TEST_NAME="export"
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

# Assert file exists
assert_file_exists() {
    TESTS_RUN=$((TESTS_RUN + 1))
    local file="$1"
    local message="${2:-File should exist}"

    if [ -f "$file" ]; then
        success "$message"
    else
        fail "$message (file not found: $file)"
        return 1
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

# Test 1: Initialize tazk database
echo -e "\n${YELLOW}Test 1: Initialize tazk database${NC}"
OUTPUT=$("$TZ_BIN" init --tz-hash-ids=false 2>&1)
assert_contains "$OUTPUT" "Initialized tazk database" "Initialize should succeed"

# Test 2: Create test issues with various types and states
echo -e "\n${YELLOW}Test 2: Create test issues${NC}"
"$TZ_BIN" create "Bug Fix A" -r agent -p 0 -t bug -d "Fix critical authentication bug" >/dev/null 2>&1
"$TZ_BIN" create "Feature B" -r agent -p 1 -t feature -d "Add export functionality" >/dev/null 2>&1
"$TZ_BIN" create "Task C" -r agent -p 2 -t task --deps tz-1 >/dev/null 2>&1
"$TZ_BIN" claim tz-2 --as tester >/dev/null 2>&1
"$TZ_BIN" close tz-1 --reason "Fixed" >/dev/null 2>&1

# Verify issues were created
assert_file_exists ".tazk/issues/tz-1.md" "tz-1 file should exist"
assert_file_exists ".tazk/issues/tz-2.md" "tz-2 file should exist"
assert_file_exists ".tazk/issues/tz-3.md" "tz-3 file should exist"

# Test 3: Export to stdout
echo -e "\n${YELLOW}Test 3: Export to stdout (default)${NC}"
STDOUT_EXPORT=$("$TZ_BIN" export 2>/dev/null)
LINE_COUNT=$(echo "$STDOUT_EXPORT" | wc -l | tr -d ' ')
assert_equals "3" "$LINE_COUNT" "Should export 3 issues to stdout"

# Verify it's valid JSON
assert_contains "$STDOUT_EXPORT" '"id":"tz-1"' "Should contain tz-1 in JSON"
assert_contains "$STDOUT_EXPORT" '"id":"tz-2"' "Should contain tz-2 in JSON"
assert_contains "$STDOUT_EXPORT" '"id":"tz-3"' "Should contain tz-3 in JSON"

# Test 4: Export to file with -o flag
echo -e "\n${YELLOW}Test 4: Export to file with -o flag${NC}"
OUTPUT=$("$TZ_BIN" export -o custom_export.jsonl 2>&1)
assert_contains "$OUTPUT" "Exported 3 issues to custom_export.jsonl" "Should report export to custom file"
assert_file_exists "custom_export.jsonl" "Custom export file should exist"

# Verify file contents
FILE_LINES=$(wc -l < custom_export.jsonl | tr -d ' ')
assert_equals "3" "$FILE_LINES" "Custom export should have 3 lines"

# Test 5: Export with --tz-output-default flag
echo -e "\n${YELLOW}Test 5: Export with --tz-output-default flag${NC}"
OUTPUT=$("$TZ_BIN" export --tz-output-default 2>&1)
assert_contains "$OUTPUT" "Exported 3 issues to" "Should report export"
assert_file_exists ".tazk/issues.jsonl" "Default export file should exist"

JSONL_FILE="$TEST_DIR/.tazk/issues.jsonl"
JSONL_LINES=$(wc -l < "$JSONL_FILE" | tr -d ' ')
assert_equals "3" "$JSONL_LINES" "Export should have 3 lines"

# Test 6: Verify JSONL format is valid
echo -e "\n${YELLOW}Test 6: Verify JSONL format${NC}"
# Each line should be valid JSON
TESTS_RUN=$((TESTS_RUN + 1))
if jq -e '.' "$JSONL_FILE" >/dev/null 2>&1; then
    success "All lines are valid JSON"
else
    fail "JSONL contains invalid JSON"
    return 1
fi

# Verify required fields exist
FIRST_ISSUE=$(head -n 1 "$JSONL_FILE")
assert_contains "$FIRST_ISSUE" '"id":' "Issue should have id field"
assert_contains "$FIRST_ISSUE" '"title":' "Issue should have title field"
assert_contains "$FIRST_ISSUE" '"status":' "Issue should have status field"
assert_contains "$FIRST_ISSUE" '"priority":' "Issue should have priority field"
assert_contains "$FIRST_ISSUE" '"issue_type":' "Issue should have issue_type field"
assert_contains "$FIRST_ISSUE" '"created_at":' "Issue should have created_at field"
assert_contains "$FIRST_ISSUE" '"updated_at":' "Issue should have updated_at field"

# Test 7: Verify dependencies/dependents format (MCP compatible)
echo -e "\n${YELLOW}Test 7: Verify dependencies/dependents format${NC}"
# tz-3 depends on tz-1
EXP3_JSON=$(grep '"id":"tz-3"' "$JSONL_FILE")
assert_contains "$EXP3_JSON" '"dependencies":[' "Should have dependencies array"
assert_contains "$EXP3_JSON" '"id":"tz-1"' "Should have tz-1 dependency"

# tz-1 should have tz-3 as dependent
EXP1_JSON=$(grep '"id":"tz-1"' "$JSONL_FILE")
assert_contains "$EXP1_JSON" '"dependents":[' "Should have dependents array"
assert_contains "$EXP1_JSON" '"id":"tz-3"' "Should have tz-3 as dependent"

# Test 8: Export with filters
echo -e "\n${YELLOW}Test 8: Export with status filter${NC}"
OPEN_EXPORT=$("$TZ_BIN" export --status open 2>/dev/null)
OPEN_COUNT=$(echo "$OPEN_EXPORT" | wc -l | tr -d ' ')
assert_equals "1" "$OPEN_COUNT" "Should export 1 open issue (tz-3)"

# Also test closed filter
CLOSED_EXPORT=$("$TZ_BIN" export --status closed 2>/dev/null)
CLOSED_COUNT=$(echo "$CLOSED_EXPORT" | wc -l | tr -d ' ')
assert_equals "1" "$CLOSED_COUNT" "Should export 1 closed issue (tz-1)"

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
    if [ "$KEEP_TEMP" = true ]; then
        echo -e "${BLUE}Test directory preserved (--keep): $TEST_DIR${NC}"
    else
        cleanup
    fi
    exit 0
else
    echo -e "${RED}Some tests failed!${NC}"
    echo -e "${RED}Test directory preserved: $TEST_DIR${NC}"
    exit 1
fi
