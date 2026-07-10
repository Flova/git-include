#!/usr/bin/env bash
# git-include installer:
#   curl -fsSL https://raw.githubusercontent.com/flova/git-include/main/install.sh | bash
#
# Environment overrides:
#   GIT_INCLUDE_VERSION   install a specific release tag (default: latest)
#   GIT_INCLUDE_BIN_DIR   install directory (default: ~/.local/bin, or
#                         /usr/local/bin when run as root)
set -euo pipefail

REPO="flova/git-include"

say()  { printf '%s\n' "$*" >&2; }
fail() { say "error: $*"; exit 1; }

command -v curl >/dev/null 2>&1 || fail "curl is required"

# --- detect platform -------------------------------------------------------
os=$(uname -s)
arch=$(uname -m)
case "$os" in
    Linux)  os_part="unknown-linux-musl" ;;  # fully static, runs on any distro
    Darwin) os_part="apple-darwin" ;;
    *) fail "unsupported OS: $os (on Windows, use the MSI installer instead)" ;;
esac
case "$arch" in
    x86_64|amd64)  arch_part="x86_64" ;;
    aarch64|arm64) arch_part="aarch64" ;;
    *) fail "unsupported architecture: $arch" ;;
esac
target="${arch_part}-${os_part}"

# --- resolve version -------------------------------------------------------
version="${GIT_INCLUDE_VERSION:-}"
if [ -z "$version" ]; then
    version=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
        | grep -o '"tag_name"[^,]*' | head -1 | sed 's/.*"\(v[^"]*\)".*/\1/')
    [ -n "$version" ] || fail "could not determine the latest release"
fi

# --- pick install dir ------------------------------------------------------
bin_dir="${GIT_INCLUDE_BIN_DIR:-}"
if [ -z "$bin_dir" ]; then
    if [ "$(id -u)" -eq 0 ]; then
        bin_dir="/usr/local/bin"
    else
        bin_dir="${HOME}/.local/bin"
    fi
fi
mkdir -p "$bin_dir"

# --- download, verify, install ----------------------------------------------
base="https://github.com/${REPO}/releases/download/${version}"
asset="git-include-${target}"
say "Downloading git-include ${version} for ${target} ..."
tmp=$(mktemp) sums=$(mktemp)
trap 'rm -f "$tmp" "$sums"' EXIT
curl -fsSL --retry 3 "${base}/${asset}" -o "$tmp" \
    || fail "download failed: ${base}/${asset}"
curl -fsSL --retry 3 "${base}/SHA256SUMS" -o "$sums" \
    || fail "could not download SHA256SUMS for verification"
expected=$(awk -v a="$asset" '{ n=$2; sub(/^\*/, "", n) } n == a { print $1 }' "$sums")
[ -n "$expected" ] || fail "SHA256SUMS has no entry for ${asset}"
if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$tmp" | awk '{print $1}')
else
    actual=$(shasum -a 256 "$tmp" | awk '{print $1}')
fi
[ "$expected" = "$actual" ] || fail "checksum mismatch for ${asset} (expected $expected, got $actual)"
chmod 755 "$tmp"
mv "$tmp" "${bin_dir}/git-include"
rm -f "$sums"
trap - EXIT

say "Installed ${bin_dir}/git-include ($("${bin_dir}/git-include" --version))"

case ":$PATH:" in
    *":${bin_dir}:"*) ;;
    *) say "note: ${bin_dir} is not on your PATH; add it to use 'git include'" ;;
esac
say "Update later with: git include self-update"
say "Tab completion:    git include completions bash  (see README)"
