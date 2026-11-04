#!/bin/bash
# E2E test for `tz merge-driver`: real `git merge`s of diverged issue and comment files
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test configuration
TEST_NAME="merge_driver"
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

export GIT_AUTHOR_NAME=tester GIT_AUTHOR_EMAIL=tester@example.com
export GIT_COMMITTER_NAME=tester GIT_COMMITTER_EMAIL=tester@example.com
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
TZ() { "$TZ_BIN" --tz-no-cmd-logging "$@"; }
ISSUE=.tazk/issues/test-1.md
COMMENTS=.tazk/comments/test-1.json

git init -q -b main
TZ init --prefix test >/dev/null
TZ create "Shared title" -r agent -p 2 -t task -d "First paragraph.

Second paragraph.

Third paragraph." >/dev/null
TZ comments add test-1 -b "Base comment" --author tester >/dev/null
git add -A
git commit -qm base

# Test 1: install registers the driver and routes tazk files to it
echo -e "\n${YELLOW}Test 1: tz merge-driver install${NC}"
OUTPUT=$(TZ merge-driver install --command "$TZ_BIN" 2>&1)
assert_contains "$OUTPUT" "merge.tz.driver" "Install should report the git config it wrote"
assert_contains "$(git config merge.tz.driver)" "merge-driver run %O %A %B --marker-size %L" "merge.tz.driver should pass git's placeholders"
assert_equals "tz" "$(git check-attr merge -- "$ISSUE" | awk '{print $3}')" "Issue files should use the tz driver"
assert_equals "tz" "$(git check-attr merge -- "$COMMENTS" | awk '{print $3}')" "Comment files should use the tz driver"
TZ merge-driver install --command "$TZ_BIN" >/dev/null 2>&1
assert_equals "1" "$(grep -c '^\*\*/.tazk/issues/\*\*/\*.md merge=tz$' .gitattributes)" "Re-running install should not duplicate .gitattributes lines"
assert_equals "tz" "$(git check-attr merge -- sub/project/.tazk/issues/p-1.md | awk '{print $3}')" "Nested databases should use the tz driver too"
git add .gitattributes
git commit -qm "Use the tz merge driver"

# Test 2: disjoint edits plus comments on both branches merge cleanly
echo -e "\n${YELLOW}Test 2: Clean merge of diverged edits and comments${NC}"
git checkout -qb feature
TZ update test-1 --title "Feature title" >/dev/null
TZ update test-1 --search "Third paragraph." --replace "Third paragraph, edited on feature." >/dev/null
TZ label add test-1 feature-label >/dev/null
TZ comments add test-1 -b "Comment from feature" --author tester >/dev/null
git commit -qam "feature edits"
git checkout -q main
sleep 1
TZ update test-1 -p 0 >/dev/null
TZ update test-1 --search "First paragraph." --replace "First paragraph, edited on main." >/dev/null
TZ label add test-1 main-label >/dev/null
TZ comments add test-1 -b "Comment from main" --author tester >/dev/null
git commit -qam "main edits"
MERGE_OUTPUT=$(git merge --no-edit feature 2>&1)
assert_equals "" "$(git diff --name-only --diff-filter=U)" "No files should be left conflicted"
assert_equals "title: Feature title" "$(grep '^title:' "$ISSUE")" "Feature's title should be merged in"
assert_equals "priority: 0" "$(grep '^priority:' "$ISSUE")" "Main's priority should be kept"
assert_contains "$(cat "$ISSUE")" "First paragraph, edited on main." "Main's paragraph edit should be kept"
assert_contains "$(cat "$ISSUE")" "Third paragraph, edited on feature." "Feature's paragraph edit should be merged in"
assert_contains "$(TZ label list test-1)" "feature-label" "Feature's label should be merged in"
assert_contains "$(TZ label list test-1)" "main-label" "Main's label should be kept"
COMMENT_LIST=$(TZ comments list test-1)
for body in "Base comment" "Comment from feature" "Comment from main"; do
    assert_equals "1" "$(echo "$COMMENT_LIST" | grep -cF "$body")" "Comment '$body' should appear exactly once"
done
assert_equals "" "$(grep -E '^(<<<<<<<|=======|>>>>>>>)' "$ISSUE" "$COMMENTS" || true)" "Clean merge should leave no markers"
TZ show test-1 >/dev/null
success "Merged issue should still parse"
TESTS_RUN=$((TESTS_RUN + 1))

# Test 3: comment deletions propagate like line deletions in git
echo -e "\n${YELLOW}Test 3: Comment deletions merge as a three-way set${NC}"
comment_id() {
    TZ --json comments list test-1 | grep -B3 -F "\"body\": \"$1\"" | sed -n 's/.*"id": *"\([^"]*\)".*/\1/p' | head -1
}
# 3a: one branch deletes a comment while the other adds one
git checkout -qb deleter
TZ comments delete test-1 "$(comment_id "Base comment")" >/dev/null
git commit -qam "delete a comment"
git checkout -q main
TZ comments add test-1 -b "Comment after the split" --author tester >/dev/null
git commit -qam "another comment on main"
MERGE_OUTPUT=$(git merge --no-edit deleter 2>&1)
assert_equals "" "$(echo "$MERGE_OUTPUT" | grep -F "tz merge-driver:" || true)" "A clean set merge should print no conflicts or notices"
assert_equals "0" "$(TZ comments list test-1 | grep -cF "Base comment")" "A comment deleted on one side and unchanged on the other should be deleted"
assert_equals "1" "$(TZ comments list test-1 | grep -cF "Comment after the split")" "New comment should be kept"
# 3b: one branch deletes a comment and the other leaves the comment file alone
git checkout -qb deleter2
TZ comments delete test-1 "$(comment_id "Comment from feature")" >/dev/null
git commit -qam "delete another comment"
git checkout -q main
TZ update test-1 --priority 1 >/dev/null
git commit -qam "touch only the issue on main"
git merge --no-edit deleter2 >/dev/null 2>&1
assert_equals "0" "$(TZ comments list test-1 | grep -cF "Comment from feature")" "A deletion against an untouched comment file should be deleted"
# 3c: one branch deletes a comment that the other edits: the edit is kept and reported
git checkout -qb deleter3
TZ comments delete test-1 "$(comment_id "Comment from main")" >/dev/null
git commit -qam "delete a third comment"
git checkout -q main
# Portable in-place edit: BSD sed (macOS) reads `-i`'s next argument as a suffix.
sed 's/"body": "Comment from main"/"body": "Comment from main, edited"/' "$COMMENTS" >"$COMMENTS.edited"
mv "$COMMENTS.edited" "$COMMENTS"
assert_contains "$(TZ comments list test-1)" "Comment from main, edited" "The hand-edited comment file should still parse"
git commit -qam "edit that comment on main"
EDITED_ID=$(comment_id "Comment from main, edited")
MERGE_OUTPUT=$(git merge --no-edit deleter3 2>&1)
assert_contains "$MERGE_OUTPUT" "NOTICE $COMMENTS: kept comment $EDITED_ID: theirs deleted it but the other side edited it" "Driver should report the kept edit"
assert_equals "1" "$(TZ comments list test-1 | grep -cF "Comment from main, edited")" "The edited comment should be kept"

# Test 4: competing title edits conflict on exactly the title line
echo -e "\n${YELLOW}Test 4: Precise conflict hunk for competing title edits${NC}"
git checkout -qb rival
TZ update test-1 --title "Rival title" >/dev/null
TZ update test-1 --search "Second paragraph." --replace "Second paragraph, rival." >/dev/null
git commit -qam "rival title"
git checkout -q main
TZ update test-1 --title "Main title" >/dev/null
git commit -qam "main title"
MERGE_STATUS=0
MERGE_OUTPUT=$(git merge --no-edit rival 2>&1) || MERGE_STATUS=$?
assert_equals "1" "$MERGE_STATUS" "git merge should stop on the conflict"
assert_contains "$MERGE_OUTPUT" "CONFLICT $ISSUE: field title: both sides changed it" "Driver should name the conflicting field"
assert_equals "$ISSUE" "$(git diff --name-only --diff-filter=U)" "Only the issue file should be conflicted"
EXPECTED_HUNK=$(printf '%s\n' "<<<<<<< ours" "title: Main title" "||||||| base" "title: Feature title" "=======" "title: Rival title" ">>>>>>> theirs")
assert_equals "$EXPECTED_HUNK" "$(sed -n '/^<<<<<<< ours$/,/^>>>>>>> theirs$/p' "$ISSUE")" "The only hunk should hold the three titles"
assert_equals "1" "$(grep -c '^<<<<<<< ours$' "$ISSUE")" "There should be exactly one hunk"
assert_contains "$(cat "$ISSUE")" "Second paragraph, rival." "Rival's non-conflicting edit should already be merged"
SHOW_OUTPUT=$(TZ show test-1 2>&1 || true)
assert_contains "$SHOW_OUTPUT" "unresolved merge conflict" "tz should refuse to read a file with an unresolved hunk"
# Resolve mechanically by taking theirs, as a tool would
awk 'BEGIN{state=0} /^<<<<<<< ours$/{state=1; next} /^\|\|\|\|\|\|\| base$/{state=2; next} /^=======$/{state=3; next} /^>>>>>>> theirs$/{state=0; next} state==0 || state==3' "$ISSUE" > "$ISSUE.resolved"
mv "$ISSUE.resolved" "$ISSUE"
assert_equals "Rival title" "$(TZ --json show test-1 | sed -n 's/.*"title": *"\([^"]*\)".*/\1/p' | head -1)" "Resolved file should parse with theirs' title"
git add "$ISSUE"
git commit -qm "resolve"

# Test 5: run --stdout previews a merge without touching files
echo -e "\n${YELLOW}Test 5: merge-driver run --stdout${NC}"
printf 'a\nb\nc\n' > base.txt
printf 'A\nb\nc\n' > ours.txt
printf 'a\nb\nC\n' > theirs.txt
assert_equals "$(printf 'A\nb\nC')" "$(TZ merge-driver run base.txt ours.txt theirs.txt --stdout)" "Plain text should merge line by line"
assert_equals "$(printf 'A\nb\nc')" "$(cat ours.txt)" "--stdout should leave ours untouched"
rm base.txt ours.txt theirs.txt

echo ""
echo "=========================================="
echo "Test Summary: $TEST_NAME"
echo "=========================================="
echo "Tests run:    $TESTS_RUN"
echo -e "Tests passed: ${GREEN}$TESTS_PASSED${NC}"
if [ $TESTS_FAILED -gt 0 ]; then
    echo -e "Tests failed: ${RED}$TESTS_FAILED${NC}"
    exit 1
fi
echo -e "${GREEN}All tests passed!${NC}"
cleanup
