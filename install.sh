#!/bin/sh
# Install (or uninstall) the `kreta` terminal client for e-KRÉTA.
#   curl -fsSL https://raw.githubusercontent.com/DarkAaronfox/kreta-tui/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/DarkAaronfox/kreta-tui/main/install.sh | sh -s -- --uninstall
set -eu

REPO="DarkAaronfox/kreta-tui"
BIN_DIR="${KRETA_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$BIN_DIR/kreta"
    rm -rf "${XDG_DATA_HOME:-$HOME/.local/share}/kreta-tui" "${XDG_CACHE_HOME:-$HOME/.cache}/kreta-tui"
    say "Removed kreta, its saved session and cache."
    exit 0
fi

[ "$(uname -s)" = "Linux" ] || die "prebuilt binaries are Linux-only; build from source: cargo install --git https://github.com/$REPO"
case "$(uname -m)" in
    x86_64 | amd64) target="x86_64-unknown-linux-gnu" ;;
    aarch64 | arm64) target="aarch64-unknown-linux-gnu" ;;
    *) die "unsupported architecture $(uname -m); build from source: cargo install --git https://github.com/$REPO" ;;
esac
command -v curl >/dev/null || die "curl is required"
command -v tar >/dev/null || die "tar is required"

url="https://github.com/$REPO/releases/latest/download/kreta-$target.tar.gz"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "Downloading $url"
curl -fsSL "$url" | tar -xz -C "$tmp" || die "download failed"
mkdir -p "$BIN_DIR"
install -m 755 "$tmp/kreta" "$BIN_DIR/kreta"
say "Installed kreta to $BIN_DIR/kreta"

case ":$PATH:" in
    *":$BIN_DIR:"*) say "Run: kreta" ;;
    *) say "$BIN_DIR is not on your PATH. Add it, e.g.: export PATH=\"$BIN_DIR:\$PATH\"" ;;
esac
