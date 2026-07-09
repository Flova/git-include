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
    Linux)  os_part="unknown-linux-gnu" ;;
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

# --- download and install --------------------------------------------------
url="https://github.com/${REPO}/releases/download/${version}/git-include-${target}"
say "Downloading git-include ${version} for ${target} ..."
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
curl -fsSL --retry 3 "$url" -o "$tmp" \
    || fail "download failed: $url"
chmod 755 "$tmp"
mv "$tmp" "${bin_dir}/git-include"
trap - EXIT

say "Installed ${bin_dir}/git-include ($("${bin_dir}/git-include" --version))"

case ":$PATH:" in
    *":${bin_dir}:"*) ;;
    *) say "note: ${bin_dir} is not on your PATH; add it to use 'git include'" ;;
esac
say "Update later with: git include self-update"
say "Tab completion:    git include completions bash  (see README)"
