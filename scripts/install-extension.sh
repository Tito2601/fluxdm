#!/usr/bin/env bash
# FluxDM — Browser Extension Native Host Installer (macOS / Linux)
# Usage: ./scripts/install-extension.sh YOUR_CHROME_EXTENSION_ID /path/to/fluxdm-binary

set -euo pipefail

EXTENSION_ID="${1:?Usage: $0 <extension-id> <fluxdm-binary-path>}"
BINARY_PATH="${2:?Usage: $0 <extension-id> <fluxdm-binary-path>}"

OS="$(uname -s)"

case "$OS" in
  Darwin)
    CHROME_DIR="$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts"
    EDGE_DIR="$HOME/Library/Application Support/Microsoft Edge/NativeMessagingHosts"
    FIREFOX_DIR="$HOME/Library/Application Support/Mozilla/NativeMessagingHosts"
    ;;
  Linux)
    CHROME_DIR="$HOME/.config/google-chrome/NativeMessagingHosts"
    EDGE_DIR="$HOME/.config/microsoft-edge/NativeMessagingHosts"
    FIREFOX_DIR="$HOME/.mozilla/native-messaging-hosts"
    ;;
  *)
    echo "Unsupported OS: $OS"
    exit 1
    ;;
esac

MANIFEST='{
  "name": "com.fluxdm.host",
  "description": "FluxDM native messaging host",
  "path": "'"$BINARY_PATH"'",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://'"$EXTENSION_ID"'/"]
}'

FIREFOX_MANIFEST='{
  "name": "com.fluxdm.host",
  "description": "FluxDM native messaging host",
  "path": "'"$BINARY_PATH"'",
  "type": "stdio",
  "allowed_extensions": ["fluxdm@fluxdev.app"]
}'

install_manifest() {
  local dir="$1"
  local content="$2"
  mkdir -p "$dir"
  echo "$content" > "$dir/com.fluxdm.host.json"
  echo "  ✓ Installed to $dir"
}

echo "Installing FluxDM native messaging host..."
install_manifest "$CHROME_DIR"  "$MANIFEST"
install_manifest "$EDGE_DIR"    "$MANIFEST"
install_manifest "$FIREFOX_DIR" "$FIREFOX_MANIFEST"

echo ""
echo "Done! Restart your browser and reload the FluxDM extension."
