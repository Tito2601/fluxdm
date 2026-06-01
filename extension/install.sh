#!/usr/bin/env bash
# ==============================================================================
# FluxDM Native Messaging Host — macOS / Linux Installer
# ==============================================================================
# Registers the native messaging host so Chrome and Firefox can communicate
# with the FluxDM desktop app.
#
# Usage:
#   ./extension/install.sh                         # auto-detect binary
#   ./extension/install.sh /path/to/fluxdm-host   # explicit binary path
#   ./extension/install.sh --uninstall             # remove registration
#   ./extension/install.sh --firefox               # also register for Firefox
# ==============================================================================

set -euo pipefail

HOST_NAME="com.fluxdm.host"
BINARY_PATH=""
UNINSTALL=false
FIREFOX=false

# ── Argument parsing ───────────────────────────────────────────────────────────

for arg in "$@"; do
    case "$arg" in
        --uninstall) UNINSTALL=true ;;
        --firefox)   FIREFOX=true ;;
        --*)         echo "Unknown option: $arg"; exit 1 ;;
        *)           BINARY_PATH="$arg" ;;
    esac
done

# ── Platform detection ─────────────────────────────────────────────────────────

OS="$(uname -s)"

if [[ "$OS" == "Darwin" ]]; then
    CHROME_HOST_DIR="$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts"
    CHROMIUM_HOST_DIR="$HOME/Library/Application Support/Chromium/NativeMessagingHosts"
    FIREFOX_HOST_DIR="$HOME/Library/Application Support/Mozilla/NativeMessagingHosts"
    BINARY_SEARCH_DIRS=(
        "$(dirname "$0")/../"
        "$(dirname "$0")/../target/release"
        "/Applications/FluxDM.app/Contents/MacOS"
        "$HOME/Applications/FluxDM.app/Contents/MacOS"
    )
else
    CHROME_HOST_DIR="$HOME/.config/google-chrome/NativeMessagingHosts"
    CHROMIUM_HOST_DIR="$HOME/.config/chromium/NativeMessagingHosts"
    FIREFOX_HOST_DIR="$HOME/.mozilla/native-messaging-hosts"
    BINARY_SEARCH_DIRS=(
        "$(dirname "$0")/../"
        "$(dirname "$0")/../target/release"
        "$HOME/.local/bin"
        "/usr/local/bin"
        "/opt/FluxDM"
    )
fi

# ── Uninstall ──────────────────────────────────────────────────────────────────

if $UNINSTALL; then
    echo "Removing FluxDM native messaging host registration..."
    for dir in "$CHROME_HOST_DIR" "$CHROMIUM_HOST_DIR" "$FIREFOX_HOST_DIR"; do
        manifest="$dir/$HOST_NAME.json"
        if [[ -f "$manifest" ]]; then
            rm -f "$manifest"
            echo "  Removed: $manifest"
        fi
    done
    echo "Done."
    exit 0
fi

# ── Locate binary ──────────────────────────────────────────────────────────────

if [[ -z "$BINARY_PATH" ]]; then
    for dir in "${BINARY_SEARCH_DIRS[@]}"; do
        candidate="$dir/fluxdm-host"
        if [[ -x "$candidate" ]]; then
            BINARY_PATH="$(cd "$(dirname "$candidate")" && pwd)/fluxdm-host"
            break
        fi
    done
fi

if [[ -z "$BINARY_PATH" || ! -x "$BINARY_PATH" ]]; then
    echo "Error: could not find fluxdm-host binary."
    echo "Please pass the path explicitly:"
    echo "  ./install.sh /path/to/fluxdm-host"
    exit 1
fi

BINARY_PATH="$(cd "$(dirname "$BINARY_PATH")" && pwd)/$(basename "$BINARY_PATH")"
echo "Installing FluxDM native messaging host..."
echo "  Binary: $BINARY_PATH"

# ── Write Chrome / Chromium manifests ─────────────────────────────────────────

EXTENSION_ID="${FLUXDM_EXTENSION_ID:-YOUR_CHROME_EXTENSION_ID}"

CHROME_MANIFEST=$(cat <<JSON
{
  "name": "$HOST_NAME",
  "description": "FluxDM native messaging host",
  "path": "$BINARY_PATH",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://$EXTENSION_ID/"
  ]
}
JSON
)

for dir in "$CHROME_HOST_DIR" "$CHROMIUM_HOST_DIR"; do
    mkdir -p "$dir"
    echo "$CHROME_MANIFEST" > "$dir/$HOST_NAME.json"
    echo "  Manifest: $dir/$HOST_NAME.json"
done

# ── Write Firefox manifest ─────────────────────────────────────────────────────

if $FIREFOX; then
    FIREFOX_MANIFEST=$(cat <<JSON
{
  "name": "$HOST_NAME",
  "description": "FluxDM native messaging host (Firefox)",
  "path": "$BINARY_PATH",
  "type": "stdio",
  "allowed_extensions": [
    "fluxdm@fluxdev.app"
  ]
}
JSON
)
    mkdir -p "$FIREFOX_HOST_DIR"
    echo "$FIREFOX_MANIFEST" > "$FIREFOX_HOST_DIR/$HOST_NAME.json"
    echo "  Firefox manifest: $FIREFOX_HOST_DIR/$HOST_NAME.json"
fi

echo ""
echo "FluxDM native messaging host installed successfully!"
echo ""
echo "Next steps:"
echo "  1. Install the FluxDM Chrome extension"
echo "     (load unpacked from: $(cd "$(dirname "$0")" && pwd))"
echo "  2. Open FluxDM and start downloading!"
echo ""
echo "To set your Chrome extension ID:"
echo "  export FLUXDM_EXTENSION_ID=your-id-here && ./install.sh"
