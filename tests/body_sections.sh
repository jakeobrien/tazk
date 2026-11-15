#!/bin/bash
# E2E test for per-type body sections on create, triage promote and update (tz-51 §3, §5.1-§5.3)
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test configuration
TEST_NAME="body_sections"
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

assert_not_contains() {
    TESTS_RUN=$((TESTS_RUN + 1))
    local haystack="$1"
    local needle="$2"
    local message="${3:-Assertion failed}"
    if grep -qF -- "$needle" <<<"$haystack"; then
        fail "$message (unexpectedly found: '$needle')"
        return 1
    else
        success "$message"
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
issue_count() { { ls .tazk/issues 2>/dev/null || true; } | wc -l | tr -d ' '; }
item_count() { { ls .tazk/triage 2>/dev/null || true; } | wc -l | tr -d ' '; }
add_item() { "$TZ_BIN" triage add "$1" >/dev/null 2>&1; "$TZ_BIN" triage list | tail -1; }

# Run a command expecting failure; assert the message and that nothing was written.
assert_rejected() {
    local needle="$1" message="$2"
    shift 2
    local before_issues before_items output
    before_issues=$(issue_count)
    before_items=$(item_count)
    output=$("$@" 2>&1 || true)
    assert_contains "$output" "$needle" "$message"
    assert_equals "$before_issues" "$(issue_count)" "$message: no issue written"
    assert_equals "$before_items" "$(item_count)" "$message: no triage item consumed"
}

# Test 1: create rejects forbidden and missing sections
echo -e "\n${YELLOW}Test 1: create rejections${NC}"
for ty in epic spec; do
    assert_rejected "${ty}s have no Design" "create $ty --design" \
        "$TZ_BIN" create "T" -t "$ty" -d "Body" --design "approach"
done
assert_rejected "gates have no Design" "create gate --design" \
    "$TZ_BIN" create "T" -t gate -r agent -d "Cond" --acceptance "Check" --design "approach"
assert_rejected "specs have no Design or Acceptance Criteria" "create spec --acceptance" \
    "$TZ_BIN" create "T" -t spec -d "Body" --acceptance "criteria"
assert_rejected "A spec needs a Description" "create spec without description" \
    "$TZ_BIN" create "T" -t spec
assert_rejected "A spec needs a Description" "create spec with blank description" \
    "$TZ_BIN" create "T" -t spec -d "   "
assert_rejected "A gate needs a Description" "create gate without description" \
    "$TZ_BIN" create "T" -t gate -r agent --acceptance "Check"
assert_rejected "A gate needs Acceptance Criteria" "create gate without --acceptance" \
    "$TZ_BIN" create "T" -t gate -r agent -d "Cond"
assert_rejected "A gate needs Acceptance Criteria" "create gate with blank --acceptance" \
    "$TZ_BIN" create "T" -t gate -r agent -d "Cond" --acceptance "  "

# Test 2: create accepts valid shapes
echo -e "\n${YELLOW}Test 2: valid create${NC}"
OUTPUT=$("$TZ_BIN" create "Spec" -t spec -d "The spec" --design "" 2>&1)
assert_contains "$OUTPUT" "Created issue: tz-1" "create spec with description only"
OUTPUT=$("$TZ_BIN" create "Gate" -t gate -r human -d "Cond" --acceptance "Check" 2>&1)
assert_contains "$OUTPUT" "Created issue: tz-2" "create gate with description and acceptance"
OUTPUT=$("$TZ_BIN" create "Task" -r agent --design "d" --acceptance "a" 2>&1)
assert_contains "$OUTPUT" "Created issue: tz-3" "work types still take every section"
"$TZ_BIN" create "Bare task" -r agent >/dev/null 2>&1

# Test 3: epic default acceptance
echo -e "\n${YELLOW}Test 3: epic default acceptance${NC}"
"$TZ_BIN" create "Epic" -t epic >/dev/null 2>&1
assert_contains "$("$TZ_BIN" show tz-5)" "- All child issues closed." "epic without --acceptance gets the default"
"$TZ_BIN" create "Epic blank" -t epic --acceptance "  " >/dev/null 2>&1
assert_contains "$("$TZ_BIN" show tz-6)" "- All child issues closed." "epic with blank --acceptance gets the default"
"$TZ_BIN" create "Epic explicit" -t epic --acceptance "- Custom." >/dev/null 2>&1
assert_contains "$("$TZ_BIN" show tz-7)" "- Custom." "explicit --acceptance is kept"
assert_not_contains "$("$TZ_BIN" show tz-7)" "All child issues closed" "explicit --acceptance replaces the default"

# Test 4: triage promote
echo -e "\n${YELLOW}Test 4: triage promote${NC}"
A=$(add_item "promote me")
assert_rejected "epics have no Design" "promote epic --design" \
    "$TZ_BIN" triage promote "$A" --title T -t epic --design "approach"
assert_rejected "specs have no Design" "promote spec --design" \
    "$TZ_BIN" triage promote "$A" --title T -t spec --design "approach"
assert_rejected "gates have no Design" "promote gate --design" \
    "$TZ_BIN" triage promote "$A" --title T -t gate -r agent --acceptance "Check" --design "approach"
assert_rejected "specs have no Design or Acceptance Criteria" "promote spec --acceptance" \
    "$TZ_BIN" triage promote "$A" --title T -t spec --acceptance "criteria"
assert_rejected "A gate needs Acceptance Criteria" "promote gate without --acceptance" \
    "$TZ_BIN" triage promote "$A" --title T -t gate -r agent
assert_rejected "A gate needs Acceptance Criteria" "promote gate with blank --acceptance" \
    "$TZ_BIN" triage promote "$A" --title T -t gate -r agent --acceptance " "

"$TZ_BIN" triage promote "$A" --title "Promoted spec" -t spec >/dev/null 2>&1
assert_contains "$(cat .tazk/issues/tz-8.md)" "promote me" "promoted spec gets the item text as description"
B=$(add_item "gate text")
"$TZ_BIN" triage promote "$B" --title "Promoted gate" -t gate -r agent --acceptance "Check" >/dev/null 2>&1
assert_contains "$(cat .tazk/issues/tz-9.md)" "gate text" "promoted gate gets the item text as description"
assert_contains "$("$TZ_BIN" show tz-9)" "Check" "promoted gate keeps --acceptance"
C=$(add_item "epic text")
"$TZ_BIN" triage promote "$C" --title "Promoted epic" -t epic >/dev/null 2>&1
assert_contains "$("$TZ_BIN" show tz-10)" "- All child issues closed." "promoted epic gets the default acceptance"
D=$(add_item "epic text 2")
"$TZ_BIN" triage promote "$D" --title "Promoted epic 2" -t epic --acceptance "- Custom." >/dev/null 2>&1
assert_not_contains "$("$TZ_BIN" show tz-11)" "All child issues closed" "promoted epic with --acceptance has no default"
E=$(add_item "task text")
"$TZ_BIN" triage promote "$E" --title "Promoted task" -r agent --design "d" --acceptance "a" >/dev/null 2>&1
assert_equals "12" "$(issue_count)" "work types still promote with every section"

# Test 5: update enforces the rules on the sections it changes
echo -e "\n${YELLOW}Test 5: update${NC}"
# Run an update expecting failure; assert the message and that the file is untouched.
assert_update_rejected() {
    local id="$1" needle="$2" message="$3" before output
    shift 3
    before=$(cat ".tazk/issues/$id.md")
    output=$("$TZ_BIN" update "$id" "$@" 2>&1 || true)
    assert_contains "$output" "$needle" "$message"
    assert_equals "$before" "$(cat ".tazk/issues/$id.md")" "$message: file untouched"
}
# tz-5 is an epic, tz-1 a spec, tz-2 a gate (created above)
"$TZ_BIN" update tz-5 --design "" >/dev/null 2>&1
printf 'x\n' >"$TEST_DIR/body.txt"
assert_update_rejected tz-5 "epics have no Design" "epic --design" --design "approach"
assert_update_rejected tz-5 "epics have no Design" "epic --append design" --append "more" --field design
assert_update_rejected tz-1 "specs have no Design" "spec --design" --design "approach"
assert_update_rejected tz-1 "specs have no Design or Acceptance" "spec --acceptance" --acceptance "c"
assert_update_rejected tz-1 "specs have no Design or Acceptance" "spec --append acceptance" --append "c" --field acceptance
assert_update_rejected tz-2 "gates have no Design" "gate --design" --design "approach"
assert_update_rejected tz-2 "gates have no Design" "gate --append design" --append "x" --field design
assert_update_rejected tz-1 "A spec needs a Description" "spec --description blank" --description " "
assert_update_rejected tz-2 "A gate needs a Description" "gate --description empty" --description ""
assert_update_rejected tz-2 "A gate needs Acceptance Criteria" "gate --acceptance empty" --acceptance ""
assert_update_rejected tz-1 "A spec needs a Description" "spec search/replace to empty" \
    --search "The spec" --replace "" --field description
assert_update_rejected tz-2 "A gate needs Acceptance Criteria" "gate search/replace acceptance to empty" \
    --search "Check" --replace "" --field acceptance --replace-all
: >"$TEST_DIR/empty.txt"
output=$("$TZ_BIN" update tz-1 --description-file "$TEST_DIR/empty.txt" 2>&1 || true)
assert_contains "$output" "A spec needs a Description" "spec --description-file empty"
assert_contains "$(cat .tazk/issues/tz-1.md)" "The spec" "spec description file untouched"
output=$("$TZ_BIN" update tz-5 --design "d" tz-3 2>&1 || true)
assert_contains "$output" "epics have no Design" "multi-id update checks each issue's own type"

# Valid updates, and fields that are never checked
"$TZ_BIN" update tz-1 -d "New spec" --notes "later" --design "" >/dev/null 2>&1
"$TZ_BIN" update tz-1 --append "More" --field description >/dev/null 2>&1
"$TZ_BIN" update tz-1 --search "New" --replace "Newer" --field description >/dev/null 2>&1
assert_contains "$(cat .tazk/issues/tz-1.md)" "Newer spec" "valid spec update applies"
"$TZ_BIN" update tz-1 --append "x" --field title >/dev/null 2>&1
"$TZ_BIN" update tz-2 --acceptance "Check again" -p 1 >/dev/null 2>&1
"$TZ_BIN" update tz-2 --search "again" --replace "twice" --field acceptance >/dev/null 2>&1
assert_contains "$(cat .tazk/issues/tz-2.md)" "Check twice" "valid gate update applies"
"$TZ_BIN" update tz-3 tz-4 --design "ok" >/dev/null 2>&1
assert_contains "$(cat .tazk/issues/tz-4.md)" "ok" "work types update every section"

# Issues that already break the rules keep working
echo -e "\n${YELLOW}Test 6: existing issues that break the rules${NC}"
"$TZ_BIN" create "Old epic" -t epic >/dev/null 2>&1
"$TZ_BIN" create "Old spec" -t spec -d "Spec body" >/dev/null 2>&1
"$TZ_BIN" create "Old gate" -t gate -r agent -d "Cond" --acceptance "Check" >/dev/null 2>&1
EPIC=$("$TZ_BIN" list --json 2>/dev/null | jq -r '.[] | select(.title=="Old epic") | .id')
SPEC=$("$TZ_BIN" list --json 2>/dev/null | jq -r '.[] | select(.title=="Old spec") | .id')
GATE=$("$TZ_BIN" list --json 2>/dev/null | jq -r '.[] | select(.title=="Old gate") | .id')
printf '\n# Design\n\nOld approach\n' >>".tazk/issues/$EPIC.md"
printf '\n# Acceptance Criteria\n\nOld criteria\n' >>".tazk/issues/$SPEC.md"
sed -i.bak '/^# Acceptance Criteria$/,/^Check$/d' ".tazk/issues/$GATE.md"
rm -f .tazk/issues/*.bak
assert_contains "$("$TZ_BIN" show "$EPIC")" "Old approach" "epic with Design still loads"
assert_contains "$("$TZ_BIN" show "$SPEC")" "Old criteria" "spec with Acceptance Criteria still loads"
assert_not_contains "$(cat ".tazk/issues/$GATE.md")" "Acceptance Criteria" "gate Acceptance Criteria was removed"
for id in "$EPIC" "$SPEC" "$GATE"; do
    "$TZ_BIN" update "$id" --title "Renamed $id" -p 1 --notes "n" >/dev/null 2>&1
    "$TZ_BIN" update "$id" -d "Fresh body" >/dev/null 2>&1
    "$TZ_BIN" close "$id" >/dev/null 2>&1
done
"$TZ_BIN" reopen "$EPIC" >/dev/null 2>&1
"$TZ_BIN" reopen "$SPEC" >/dev/null 2>&1
"$TZ_BIN" reopen "$GATE" -r agent >/dev/null 2>&1
assert_contains "$("$TZ_BIN" show "$GATE")" "open" "gate with empty Acceptance Criteria reopens"
assert_update_rejected "$EPIC" "clearing it is allowed" "epic existing Design cannot be set" --design "New"
assert_update_rejected "$EPIC" "clearing it is allowed" "epic existing Design cannot be appended" --append "x" --field design
assert_update_rejected "$EPIC" "clearing it is allowed" "epic existing Design cannot be replaced" \
    --search "Old" --replace "New" --field design
assert_update_rejected "$SPEC" "clearing it is allowed" "spec existing Acceptance cannot be replaced" \
    --search "Old" --replace "New" --field acceptance --replace-all
assert_update_rejected "$GATE" "A gate needs Acceptance Criteria" "gate empty Acceptance cannot be set empty" --acceptance ""
"$TZ_BIN" update "$EPIC" --design "" >/dev/null 2>&1
"$TZ_BIN" update "$SPEC" --acceptance "" >/dev/null 2>&1
assert_not_contains "$(cat ".tazk/issues/$EPIC.md")" "Old approach" "epic Design can be cleared"
assert_not_contains "$(cat ".tazk/issues/$SPEC.md")" "Old criteria" "spec Acceptance Criteria can be cleared"

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
