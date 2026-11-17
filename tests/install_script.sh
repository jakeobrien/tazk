#!/bin/bash
# E2E test for install.sh, run against a fake release tree served over file://.
# `uname` is stubbed so the test is host-independent.
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
INSTALL_SH="$WORKSPACE_ROOT/install.sh"

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

echo "=========================================="
echo "Running E2E Test: install_script"
echo "=========================================="

success() {
    echo -e "${GREEN}✓ $1${NC}"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

fail() {
    echo -e "${RED}✗ $1${NC}"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

check() {
    # check <description> <command...>: passes when the command succeeds
    TESTS_RUN=$((TESTS_RUN + 1))
    local description="$1"
    shift
    if "$@"; then success "$description"; else fail "$description"; fi
}

contains() { grep -qF -- "$2" <<<"$1"; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

# make_release <releases-dir> <subdir> <version> <target>...
# Creates <releases-dir>/<subdir>/{tazk-<target>.tar.gz,SHA256SUMS} with a fake tz.
make_release() {
    local root="$1" subdir="$2" version="$3"
    shift 3
    local dir="$root/$subdir"
    mkdir -p "$dir"
    : >"$dir/SHA256SUMS"
    local target
    for target in "$@"; do
        local stage="$TMP/stage/$version/tazk-$target"
        mkdir -p "$stage"
        printf '#!/bin/sh\necho "tz %s"\n' "${version#v}" >"$stage/tz"
        chmod 755 "$stage/tz"
        tar -czf "$dir/tazk-$target.tar.gz" -C "$TMP/stage/$version" "tazk-$target"
        echo "$(sha256 "$dir/tazk-$target.tar.gz")  tazk-$target.tar.gz" >>"$dir/SHA256SUMS"
    done
}

RELEASES="$TMP/releases"
TARGETS=(aarch64-apple-darwin aarch64-unknown-linux-musl)
make_release "$RELEASES" "latest/download" v0.9.1 "${TARGETS[@]}"
make_release "$RELEASES" "download/v0.9.1" v0.9.1 "${TARGETS[@]}"
make_release "$RELEASES" "download/v0.9.0" v0.9.0 "${TARGETS[@]}"

# Stub uname: reads FAKE_OS / FAKE_ARCH.
STUBS="$TMP/stubs"
mkdir -p "$STUBS"
cat >"$STUBS/uname" <<'EOF'
#!/bin/sh
case "$1" in
    -s) echo "$FAKE_OS" ;;
    -m) echo "$FAKE_ARCH" ;;
    *) echo "$FAKE_OS" ;;
esac
EOF
chmod 755 "$STUBS/uname"

# run_install <os> <arch> [args...]: runs install.sh; sets OUT and STATUS.
run_install() {
    local os="$1" arch="$2"
    shift 2
    set +e
    OUT=$(PATH="$STUBS:$PATH" FAKE_OS="$os" FAKE_ARCH="$arch" \
        TAZK_BASE_URL="file://$RELEASES" sh "$INSTALL_SH" "$@" 2>&1)
    STATUS=$?
    set -e
}

echo ""
echo "Test 1: latest install on Linux aarch64"
DEST="$TMP/t1/bin"
run_install Linux aarch64 --install-dir "$DEST"
check "exits 0" test "$STATUS" -eq 0
check "installs executable tz" test -x "$DEST/tz"
check "installs latest (0.9.1)" test "$("$DEST/tz")" = "tz 0.9.1"
check "reports checksum verified" contains "$OUT" "checksum verified"
check "warns when dir not on PATH" contains "$OUT" "not on your PATH"

echo ""
echo "Test 2: latest install on macOS arm64"
DEST="$TMP/t2/bin"
run_install Darwin arm64 --install-dir "$DEST"
check "exits 0" test "$STATUS" -eq 0
check "installs tz" test "$("$DEST/tz")" = "tz 0.9.1"

echo ""
echo "Test 3: version pinning via --version (with and without v prefix)"
DEST="$TMP/t3/bin"
run_install Linux aarch64 --install-dir "$DEST" --version v0.9.0
check "pinned install exits 0" test "$STATUS" -eq 0
check "installs pinned 0.9.0" test "$("$DEST/tz")" = "tz 0.9.0"
run_install Linux aarch64 --install-dir="$DEST" --version=0.9.1
check "--opt=value form, no v prefix" test "$("$DEST/tz")" = "tz 0.9.1"

echo ""
echo "Test 4: configuration via environment variables"
DEST="$TMP/t4/bin"
set +e
OUT=$(PATH="$STUBS:$PATH:$DEST" FAKE_OS=Linux FAKE_ARCH=aarch64 TAZK_BASE_URL="file://$RELEASES" \
    TAZK_VERSION=v0.9.0 TAZK_INSTALL_DIR="$DEST" sh "$INSTALL_SH" 2>&1)
STATUS=$?
set -e
check "env install exits 0" test "$STATUS" -eq 0
check "TAZK_VERSION honored" test "$("$DEST/tz")" = "tz 0.9.0"
check "no PATH warning when dir is on PATH" bash -c '! grep -qF "not on your PATH" <<<"$1"' _ "$OUT"

echo ""
echo "Test 5: unsupported platforms"
run_install Linux x86_64 --install-dir "$TMP/t5/bin"
check "x86_64 rejected" test "$STATUS" -ne 0
check "x86_64 error is explicit" contains "$OUT" "unsupported architecture"
run_install FreeBSD aarch64 --install-dir "$TMP/t5/bin"
check "unsupported OS rejected" test "$STATUS" -ne 0
check "nothing installed on rejection" test ! -e "$TMP/t5/bin/tz"

echo ""
echo "Test 6: bad input"
run_install Linux aarch64 --install-dir "$TMP/t6/bin" --version v9.9.9
check "missing release fails" test "$STATUS" -ne 0
check "missing release error is explicit" contains "$OUT" "does that release exist"
run_install Linux aarch64 --install-dir "$TMP/t6/bin" --version 'v1.0.0/../../x'
check "malformed version rejected" test "$STATUS" -ne 0
run_install Linux aarch64 --install-dir "$TMP/t6/bin" --bogus
check "unknown option rejected" test "$STATUS" -ne 0
run_install Linux aarch64 --help
check "--help exits 0" test "$STATUS" -eq 0
check "--help shows usage" contains "$OUT" "Usage: install.sh"

echo ""
echo "Test 7: checksum mismatch is fatal"
BAD="$TMP/bad-releases"
mkdir -p "$BAD"
cp -R "$RELEASES"/. "$BAD"/
printf 'tampered' >>"$BAD/download/v0.9.0/tazk-aarch64-unknown-linux-musl.tar.gz"
DEST="$TMP/t7/bin"
set +e
OUT=$(PATH="$STUBS:$PATH" FAKE_OS=Linux FAKE_ARCH=aarch64 TAZK_BASE_URL="file://$BAD" \
    sh "$INSTALL_SH" --install-dir "$DEST" --version v0.9.0 2>&1)
STATUS=$?
set -e
check "tampered archive rejected" test "$STATUS" -ne 0
check "mismatch error is explicit" contains "$OUT" "checksum mismatch"
check "nothing installed after mismatch" test ! -e "$DEST/tz"

echo ""
echo "Test 8: reinstall replaces an existing binary"
DEST="$TMP/t8/bin"
run_install Linux aarch64 --install-dir "$DEST" --version v0.9.0
run_install Linux aarch64 --install-dir "$DEST"
check "upgrade replaced tz" test "$("$DEST/tz")" = "tz 0.9.1"
check "no staging files left behind" test "$(find "$DEST" -mindepth 1 | wc -l | tr -d ' ')" = "1"

echo ""
echo "=========================================="
echo "Test Summary"
echo "=========================================="
echo "Tests run:    $TESTS_RUN"
echo "Tests passed: $TESTS_PASSED"
echo "Tests failed: $TESTS_FAILED"
echo ""

if [ "$TESTS_FAILED" -eq 0 ]; then
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}Some tests failed!${NC}"
    exit 1
fi
