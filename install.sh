#!/bin/sh
# tazk installer: downloads a prebuilt `tz` binary from GitHub Releases.
#
#   Latest release:
#     curl -fsSL https://github.com/jakeobrien/tazk/releases/latest/download/install.sh | sh
#
#   Pin a version (the env var must be set on the `sh` side of the pipe):
#     curl -fsSL https://github.com/jakeobrien/tazk/releases/latest/download/install.sh | sh -s -- --version v0.9.0
#     curl -fsSL https://github.com/jakeobrien/tazk/releases/latest/download/install.sh | TAZK_VERSION=v0.9.0 sh
#
# Supported platforms: macOS on Apple Silicon (arm64) and Linux on ARM64 (aarch64).
#
# Environment (flags take precedence):
#   TAZK_VERSION       Version to install, e.g. v0.9.0 or 0.9.0 (default: latest release)
#   TAZK_INSTALL_DIR   Directory to install `tz` into (default: ~/.local/bin)
#   TAZK_BASE_URL      Override the releases base URL (used by the test suite)

set -eu

REPO="jakeobrien/tazk"
BASE_URL="${TAZK_BASE_URL:-https://github.com/$REPO/releases}"

tmp_dir=""

say() {
    printf 'tazk-install: %s\n' "$*"
}

err() {
    printf 'tazk-install: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Install the tazk (`tz`) binary.

Usage: install.sh [--version <version>] [--install-dir <dir>]

Options:
  --version <version>    Install a specific release, e.g. v0.9.0 (default: latest)
  --install-dir <dir>    Install directory (default: ~/.local/bin)
  -h, --help             Show this help

Environment: TAZK_VERSION, TAZK_INSTALL_DIR
EOF
}

have() {
    command -v "$1" >/dev/null 2>&1
}

# download <url> <dest>
download() {
    if have curl; then
        if [ -n "${TAZK_BASE_URL:-}" ]; then
            curl -fsSL --retry 3 -o "$2" "$1"
        else
            curl -fsSL --proto '=https' --tlsv1.2 --retry 3 -o "$2" "$1"
        fi
    elif have wget; then
        wget -q -O "$2" "$1"
    else
        err "need curl or wget to download files"
    fi
}

sha256_of() {
    if have sha256sum; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif have shasum; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        err "need sha256sum or shasum to verify the download"
    fi
}

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)

    case "$arch" in
        arm64 | aarch64) arch_part="aarch64" ;;
        *) err "unsupported architecture '$arch' (supported: arm64/aarch64)" ;;
    esac

    case "$os" in
        Darwin) os_part="apple-darwin" ;;
        Linux) os_part="unknown-linux-musl" ;;
        *) err "unsupported OS '$os' (supported: macOS, Linux)" ;;
    esac

    target="$arch_part-$os_part"
}

# Accepts 0.9.0 or v0.9.0 (optionally with a -prerelease suffix); prints v-prefixed.
normalize_version() {
    case "$1" in
        *[!A-Za-z0-9.+-]*) err "invalid version '$1'" ;;
    esac
    case "$1" in
        v[0-9]*.[0-9]*.[0-9]*) printf '%s' "$1" ;;
        [0-9]*.[0-9]*.[0-9]*) printf 'v%s' "$1" ;;
        *) err "invalid version '$1' (expected something like v0.9.0)" ;;
    esac
}

main() {
    version="${TAZK_VERSION:-}"
    install_dir="${TAZK_INSTALL_DIR:-}"

    while [ $# -gt 0 ]; do
        case "$1" in
            --version)
                [ $# -ge 2 ] || err "--version requires a value"
                version="$2"
                shift 2
                ;;
            --version=*)
                version="${1#--version=}"
                shift
                ;;
            --install-dir)
                [ $# -ge 2 ] || err "--install-dir requires a value"
                install_dir="$2"
                shift 2
                ;;
            --install-dir=*)
                install_dir="${1#--install-dir=}"
                shift
                ;;
            -h | --help)
                usage
                exit 0
                ;;
            *)
                err "unknown option '$1' (try --help)"
                ;;
        esac
    done

    if [ -z "$install_dir" ]; then
        [ -n "${HOME:-}" ] || err "HOME is not set; pass --install-dir"
        install_dir="$HOME/.local/bin"
    fi

    have tar || err "need tar to unpack the download"

    detect_target

    if [ -z "$version" ] || [ "$version" = "latest" ]; then
        version=""
        url_base="$BASE_URL/latest/download"
        label="latest release"
    else
        version=$(normalize_version "$version")
        url_base="$BASE_URL/download/$version"
        label="$version"
    fi

    archive="tazk-$target.tar.gz"

    tmp_dir=$(mktemp -d)
    trap 'rm -rf "$tmp_dir"' EXIT
    trap 'exit 1' HUP INT TERM

    say "downloading tazk ($label) for $target"
    download "$url_base/$archive" "$tmp_dir/$archive" ||
        err "could not download $url_base/$archive (does that release exist?)"
    download "$url_base/SHA256SUMS" "$tmp_dir/SHA256SUMS" ||
        err "could not download $url_base/SHA256SUMS"

    expected=$(awk -v f="$archive" '$2 == f || $2 == "*" f { print $1; exit }' "$tmp_dir/SHA256SUMS")
    [ -n "$expected" ] || err "no checksum listed for $archive in SHA256SUMS"
    actual=$(sha256_of "$tmp_dir/$archive")
    [ "$expected" = "$actual" ] ||
        err "checksum mismatch for $archive (expected $expected, got $actual)"
    say "checksum verified"

    tar -xzf "$tmp_dir/$archive" -C "$tmp_dir"
    bin_src="$tmp_dir/tazk-$target/tz"
    [ -f "$bin_src" ] || err "archive did not contain tazk-$target/tz"

    mkdir -p "$install_dir" || err "cannot create $install_dir"
    # Copy then rename so an existing tz is replaced atomically.
    staged="$install_dir/.tz.install.$$"
    cp "$bin_src" "$staged" || err "cannot write to $install_dir"
    chmod 755 "$staged"
    mv -f "$staged" "$install_dir/tz"

    installed=$("$install_dir/tz" --version 2>/dev/null | head -n 1) || installed=""
    say "installed ${installed:-tz} to $install_dir/tz"

    case ":${PATH:-}:" in
        *":$install_dir:"*) ;;
        *)
            say "note: $install_dir is not on your PATH; add it, e.g.:"
            say "  export PATH=\"$install_dir:\$PATH\""
            ;;
    esac
}

# Everything is wrapped in main so a truncated download can never run a partial script.
main "$@"
