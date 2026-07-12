#!/usr/bin/env bash
set -euo pipefail

GH="0xClumzzy/rsh-stab"
BIN="${BIN:-rsh-stab}"

# --- detect target ---
ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
case "$ARCH" in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) echo "unsupported arch: $ARCH"; exit 1 ;;
esac

# --- dest dir ---
if [ -w /usr/local/bin ]; then
  DEST="/usr/local/bin"
elif [ -w "$HOME/.local/bin" ]; then
  DEST="$HOME/.local/bin"
else
  DEST="$HOME/.local/bin"
  mkdir -p "$DEST"
fi

# --- try prebuilt ---
URL="https://github.com/$GH/releases/latest/download/rsh-stab-${OS}-${ARCH}"
echo "[*] trying prebuilt: $URL"
if curl -sLf "$URL" -o /tmp/rsh-stab 2>/dev/null; then
  chmod +x /tmp/rsh-stab
  mv /tmp/rsh-stab "$DEST/$BIN"
  echo "[+] installed $BIN to $DEST/$BIN (prebuilt)"
  exit 0
fi
echo "[!] no prebuilt binary — falling back to cargo"

# --- build from source ---
if ! command -v cargo &>/dev/null; then
  echo "[-] cargo not found; install Rust first: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  exit 1
fi

TMP=$(mktemp -d)
git clone --depth 1 "https://github.com/$GH.git" "$TMP"
cargo build --release --manifest-path "$TMP/Cargo.toml"
cp "$TMP/target/release/$BIN" "$DEST/$BIN"
rm -rf "$TMP"
echo "[+] installed $BIN to $DEST/$BIN (built from source)"
