#!/bin/sh
# Kerf installer for Linux / macOS:  curl -fsSL https://raw.githubusercontent.com/hotschmoe/kerf/main/install.sh | sh
# Installs `kerf` to ~/.local/bin (override: KERF_INSTALL_DIR). Pin a version: KERF_VERSION=v0.1.0-alpha.1
set -eu
os=$(uname -s); arch=$(uname -m)
case "$os" in Linux) os=linux ;; Darwin) os=macos ;; *) echo "kerf: unsupported OS $os" >&2; exit 1 ;; esac
case "$arch" in x86_64|amd64) arch=x86_64 ;; aarch64|arm64) arch=aarch64 ;; *) echo "kerf: unsupported arch $arch" >&2; exit 1 ;; esac
asset="kerf-$arch-$os"
if [ -n "${KERF_VERSION:-}" ]; then base="https://github.com/hotschmoe/kerf/releases/download/$KERF_VERSION"; else base="https://github.com/hotschmoe/kerf/releases/latest/download"; fi
dir="${KERF_INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$dir"
echo "downloading $asset ..."
if command -v curl >/dev/null 2>&1; then curl -fsSL "$base/$asset" -o "$dir/kerf.tmp"; else wget -qO "$dir/kerf.tmp" "$base/$asset"; fi
chmod +x "$dir/kerf.tmp" && mv "$dir/kerf.tmp" "$dir/kerf"
"$dir/kerf" version; echo
echo "kerf installed: $dir/kerf"
case ":$PATH:" in *":$dir:"*) ;; *) echo "note: add $dir to your PATH (e.g. echo 'export PATH=\"$dir:\$PATH\"' >> ~/.bashrc)";; esac
echo "next:  mkdir details && cd details && kerf init   then open Claude Code / Grok there and ask for a detail."
