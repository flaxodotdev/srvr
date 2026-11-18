#!/bin/sh
# srvr installer — https://github.com/flaxodotdev/srvr
#   curl -fsSL https://raw.githubusercontent.com/flaxodotdev/srvr/main/install.sh | sh
# Env: SRVR_VERSION (default: latest), SRVR_INSTALL_DIR (default: ~/.local/bin)
set -eu

REPO="flaxodotdev/srvr"
VERSION="${SRVR_VERSION:-latest}"
INSTALL_DIR="${SRVR_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '\033[1;38;5;209m▲ srvr\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "'$1' is required"; }

need uname
need tar
if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO "$2" "$1"; }
else
  die "curl or wget is required"
fi

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Linux)   os_part="unknown-linux-musl" ;;
  Darwin)  os_part="apple-darwin" ;;
  FreeBSD) die "no prebuilt FreeBSD binary yet — install with: cargo install --git https://github.com/$REPO" ;;
  MINGW*|MSYS*|CYGWIN*) die "on Windows use: irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex" ;;
  *) die "unsupported OS: $os (try: cargo install --git https://github.com/$REPO)" ;;
esac
case "$arch" in
  x86_64|amd64)   arch_part="x86_64" ;;
  arm64|aarch64)  arch_part="aarch64" ;;
  armv7*|armv6*)  arch_part="armv7"; os_part="unknown-linux-musleabihf" ;;
  i386|i686)      arch_part="i686" ;;
  *) die "unsupported architecture: $arch (try: cargo install --git https://github.com/$REPO)" ;;
esac
# Apple Silicon running under Rosetta reports x86_64; prefer the native build.
if [ "$os" = Darwin ] && [ "$arch_part" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
  arch_part="aarch64"
fi
target="${arch_part}-${os_part}"
asset="srvr-${target}.tar.gz"

if [ "$VERSION" = latest ]; then
  base="https://github.com/$REPO/releases/latest/download"
else
  base="https://github.com/$REPO/releases/download/$VERSION"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

say "downloading $asset ($VERSION)"
fetch "$base/$asset" "$tmp/$asset" || die "download failed: $base/$asset"

if fetch "$base/$asset.sha256" "$tmp/$asset.sha256" 2>/dev/null; then
  expected=$(cut -d' ' -f1 < "$tmp/$asset.sha256")
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$tmp/$asset" | cut -d' ' -f1)
  elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)
  else
    actual="$expected"
  fi
  [ "$expected" = "$actual" ] || die "checksum mismatch for $asset"
  say "checksum ok"
fi

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$INSTALL_DIR"
mv "$tmp/srvr" "$INSTALL_DIR/srvr"
chmod +x "$INSTALL_DIR/srvr"
# Clear the quarantine flag so Gatekeeper doesn't block the unsigned binary.
[ "$os" = Darwin ] && xattr -d com.apple.quarantine "$INSTALL_DIR/srvr" 2>/dev/null || true

say "installed $("$INSTALL_DIR/srvr" --version) to $INSTALL_DIR/srvr"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) say "run: srvr" ;;
  *)
    shell_rc="~/.profile"
    case "${SHELL:-}" in
      */zsh)  shell_rc="~/.zshrc" ;;
      */bash) shell_rc="~/.bashrc" ;;
      */fish) shell_rc="~/.config/fish/config.fish" ;;
    esac
    say "$INSTALL_DIR is not on your PATH. Add it with:"
    if [ "$shell_rc" = "~/.config/fish/config.fish" ]; then
      echo "    fish_add_path $INSTALL_DIR"
    else
      echo "    echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> $shell_rc && . $shell_rc"
    fi
    ;;
esac
