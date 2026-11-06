#!/bin/bash
# E2E test for triage items: plain Markdown reports in .tazk/triage/
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test configuration
TEST_NAME="triage"
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
assert_not_contains() {
    TESTS_RUN=$((TESTS_RUN + 1))
    local haystack="$1"
    local needle="$2"
    local message="${3:-Assertion failed}"
    if grep -qF -- "$needle" <<<"$haystack"; then
        fail "$message (did not expect: '$needle' in output)"
        return 1
    else
        success "$message"
    fi
}
# Print the name of the Nth (1-based) triage item.
nth_item() { "$TZ_BIN" triage list | sed -n "$1p"; }
# Add an item from text in $1 (stdin closed) and print its name.
add_item() { "$TZ_BIN" triage add "$1" </dev/null | sed -n 's/^Created triage item: //p'; }
item_count() { "$TZ_BIN" triage list | grep -c . || true; }

# Test 1: add creates a plain Markdown file named by timestamp
echo -e "\n${YELLOW}Test 1: tz triage add${NC}"
OUTPUT=$("$TZ_BIN" triage add "It breaks on Tuesdays" </dev/null 2>&1)
assert_contains "$OUTPUT" "Created triage item: " "add should confirm"
A=$(nth_item 1)
if [[ "$A" =~ ^[0-9]{8}T[0-9]{6}-[0-9a-z]{4}\.md$ ]]; then success "item name is timestamp-random.md ($A)"; else fail "unexpected item name: $A"; fi
TESTS_RUN=$((TESTS_RUN + 1))
assert_equals "It breaks on Tuesdays" "$(cat ".tazk/triage/$A")" "file should hold just the text"
assert_not_contains "$(cat ".tazk/triage/$A")" "---" "file should have no frontmatter"
assert_equals "" "$(ls .tazk/issues)" "add should not create an issue"
OUTPUT=$("$TZ_BIN" list 2>&1)
assert_not_contains "$OUTPUT" "Tuesdays" "triage items should not appear in tz list"

# Test 2: piped stdin is appended (external refs)
echo -e "\n${YELLOW}Test 2: piped stdin${NC}"
sleep 1 # distinct timestamps keep list order predictable
printf 'Ref: GH-7\nhttps://example.com/7\n' | "$TZ_BIN" triage add "Flaky auth test" >/dev/null 2>&1
B=$(nth_item 2)
assert_equals "$(printf 'Flaky auth test\n\nRef: GH-7\nhttps://example.com/7')" "$(cat ".tazk/triage/$B")" "stdin should be appended after a blank line"
sleep 1
printf 'Body with `backticks` and $(no expansion)\n' | "$TZ_BIN" triage add >/dev/null 2>&1
C=$(nth_item 3)
assert_equals 'Body with `backticks` and $(no expansion)' "$(cat ".tazk/triage/$C")" "stdin alone should be the whole body"
assert_fails "add with no text should fail" "$TZ_BIN" triage add
assert_fails "add with blank text should fail" "$TZ_BIN" triage add "   "
assert_fails "add should take a single text argument" "$TZ_BIN" triage add one two
assert_fails "add should reject issue fields" "$TZ_BIN" triage add x -p 1
assert_equals "3" "$(item_count)" "failed adds should not create items"

# Test 3: list and show
echo -e "\n${YELLOW}Test 3: list and show${NC}"
assert_equals "$(printf '%s\n%s\n%s' "$A" "$B" "$C")" "$("$TZ_BIN" triage list)" "list should print file names in creation order"
assert_equals "It breaks on Tuesdays" "$("$TZ_BIN" triage show "$A")" "show should print the text"
assert_equals "It breaks on Tuesdays" "$("$TZ_BIN" triage show "${A%.md}")" "show should accept the name without .md"
assert_equals "It breaks on Tuesdays" "$("$TZ_BIN" triage show ".tazk/triage/$A")" "show should accept a path into the triage folder"
OUTPUT=$("$TZ_BIN" triage show nope 2>&1 || true)
assert_contains "$OUTPUT" "Triage item not found: nope.md" "show of a missing item should fail clearly"
assert_contains "$("$TZ_BIN" triage list --json)" "\"name\": \"$A\"" "list --json should include names"
assert_contains "$("$TZ_BIN" --readonly triage list)" "$A" "list should work with --readonly"
assert_contains "$("$TZ_BIN" --readonly triage show "$A")" "Tuesdays" "show should work with --readonly"

# Test 4: update edits the body like tz update
echo -e "\n${YELLOW}Test 4: update${NC}"
"$TZ_BIN" triage update "$A" --search Tuesdays --replace Mondays >/dev/null 2>&1
assert_equals "It breaks on Mondays" "$(cat ".tazk/triage/$A")" "search/replace should edit the text"
"$TZ_BIN" triage update "$A" --append "Seen in CI" >/dev/null 2>&1
assert_equals "$(printf 'It breaks on Mondays\n\nSeen in CI')" "$(cat ".tazk/triage/$A")" "append should add a paragraph"
assert_fails "ambiguous search should fail" "$TZ_BIN" triage update "$A" --search " " --replace "_"
"$TZ_BIN" triage update "$A" --search " " --replace "_" --replace-all >/dev/null 2>&1
assert_equals "$(printf 'It_breaks_on_Mondays\n\nSeen_in_CI')" "$(cat ".tazk/triage/$A")" "--replace-all should replace every match"
assert_fails "update without an edit should fail" "$TZ_BIN" triage update "$A"
assert_fails "update should not take --field" "$TZ_BIN" triage update "$A" --search x --replace y --field title
assert_fails "update of a missing item should fail" "$TZ_BIN" triage update nope --append x

# Test 5: rm
echo -e "\n${YELLOW}Test 5: rm${NC}"
D=$(add_item "Throwaway")
assert_fails "rm with a missing item should fail" "$TZ_BIN" triage rm "$D" nope
assert_equals "4" "$(item_count)" "failed rm should delete nothing"
OUTPUT=$("$TZ_BIN" triage rm "$D" 2>&1)
assert_contains "$OUTPUT" "Removed triage item: $D" "rm should confirm"
assert_equals "3" "$(item_count)" "rm should delete the item"

# Test 6: promote several items into one issue
echo -e "\n${YELLOW}Test 6: promote${NC}"
"$TZ_BIN" create "Auth spec" -r human -t spec >/dev/null 2>&1
assert_fails "promote without --title should fail" "$TZ_BIN" triage promote "$A" -r agent
assert_fails "promote without --role should fail" "$TZ_BIN" triage promote "$A" --title T
assert_fails "promote of a missing item should fail" "$TZ_BIN" triage promote "$A" nope --title T -r agent
assert_equals "3" "$(item_count)" "failed promote should delete nothing"
OUTPUT=$("$TZ_BIN" triage promote "$A" "$B" --title "Fix auth" -r agent -p 1 -t bug --parent test-1 -l backend 2>&1)
assert_contains "$OUTPUT" "Created issue: test-2" "promote should create an issue"
assert_equals "open" "$(field test-2 status)" "promoted issue should be open"
assert_equals "1" "$(field test-2 priority)" "priority should be set"
assert_equals "bug" "$(field test-2 issue_type)" "type should be set"
assert_equals "agent" "$(field test-2 role)" "role should be set"
ISSUE=$(cat .tazk/issues/test-2.md)
assert_contains "$ISSUE" "It_breaks_on_Mondays" "description should hold the first item"
assert_contains "$ISSUE" "Ref: GH-7" "description should hold the second item"
assert_contains "$ISSUE" "Promoted from triage: $A, $B" "notes should record the items"
assert_contains "$ISSUE" "test-1: child_of" "--parent should be applied"
assert_equals "$C" "$("$TZ_BIN" triage list)" "promoted items should be deleted"
assert_contains "$("$TZ_BIN" ready)" "test-2:" "promoted issue should be ready"

# Test 7: promote with an explicit description
echo -e "\n${YELLOW}Test 7: promote with --description${NC}"
E=$(add_item "raw report")
"$TZ_BIN" triage promote "$E" --title "Clean" -r human -d "Rewritten" >/dev/null 2>&1
assert_contains "$(cat .tazk/issues/test-3.md)" "Rewritten" "--description should replace the item text"
assert_not_contains "$(cat .tazk/issues/test-3.md)" "raw report" "item text should not be kept"

# Test 8: attach items to an issue as comments
echo -e "\n${YELLOW}Test 8: attach${NC}"
assert_fails "attach to a missing issue should fail" "$TZ_BIN" triage attach "$C" --to test-99 --author alice
assert_fails "attach without --to should fail" "$TZ_BIN" triage attach "$C" --author alice
assert_fails "attach without --author should fail" "$TZ_BIN" triage attach "$C" --to test-2
assert_equals "$C" "$("$TZ_BIN" triage list)" "failed attach should delete nothing"
OUTPUT=$("$TZ_BIN" triage attach "$C" --to test-2 --author alice 2>&1)
assert_contains "$OUTPUT" "Attached $C to test-2 as comment" "attach should confirm"
COMMENTS=$("$TZ_BIN" comments list test-2 2>&1)
assert_contains "$COMMENTS" '[alice at' "comment should have the author"
assert_contains "$COMMENTS" 'Body with `backticks`' "comment should hold the item text"
assert_equals "0" "$(item_count)" "attached item should be deleted"

# Test 9: the triage status is gone
echo -e "\n${YELLOW}Test 9: no triage status${NC}"
assert_fails "list --status triage should fail" "$TZ_BIN" list --status triage
assert_fails "the old open command should be gone" "$TZ_BIN" open --id test-2 --title T -r agent

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
