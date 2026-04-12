#!/bin/sh
# Rove Installer
# Primary:  Cloudflare R2 (registry.roveai.co)
# Fallback: GitHub Releases (github.com/orvislab/rove)
#
# Usage: curl -fsSL https://roveai.co/install.sh | sh
set -e

REPO="orvislab/rove"
BINARY="rove"
INSTALL_DIR="/usr/local/bin"
R2_BASE="https://registry.roveai.co"
GH_BASE="https://github.com/${REPO}/releases/download"

# ── Detect platform ──────────────────────────────

detect_platform() {
    OS="$(uname -s)"
    case "$OS" in
        Linux)  OS_TARGET="linux" ;;
        Darwin) OS_TARGET="darwin" ;;
        MINGW*|MSYS*|CYGWIN*)
            echo "Error: Use install.ps1 for Windows"
            exit 1
            ;;
        *)
            echo "Error: Unsupported OS: $OS"
            exit 1
            ;;
    esac

    ARCH="$(uname -m)"
    case "$ARCH" in
        x86_64|amd64)  ARCH_TARGET="x86_64" ;;
        aarch64|arm64) ARCH_TARGET="aarch64" ;;
        *)
            echo "Error: Unsupported architecture: $ARCH"
            exit 1
            ;;
    esac

    TARGET="${OS_TARGET}-${ARCH_TARGET}"
}

# ── Fetch latest version from manifest ───────────

fetch_version() {
    echo "Fetching latest version manifest..."

    # Fetch OTA Manifest
    MANIFEST=$(curl -fsSL "https://raw.githubusercontent.com/orvislab/rove-registry/main/manifest.json" 2>/dev/null || true)

    if [ -n "$MANIFEST" ]; then
        VERSION=$(echo "$MANIFEST" | grep -o '"version"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed 's/.*"\([^"]*\)"/\1/')
        ENGINES_BLOCK=$(echo "$MANIFEST" | sed -n '/"engines":/,$p')
        BLOCK=$(echo "$ENGINES_BLOCK" | awk "/\"${TARGET}\"/,/\}/")
        EXPECTED_HASH=$(echo "$BLOCK" | grep '"sha256"' | head -1 | sed 's/.*"sha256"[[:space:]]*:[[:space:]]*"\([a-f0-9]\{64\}\)".*/\1/' || true)
        R2_URL=$(echo "$BLOCK" | grep '"url"' | head -1 | sed 's/.*"url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')
        GH_URL=$(echo "$BLOCK" | grep '"fallback_url"' | head -1 | sed 's/.*"fallback_url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')
        SOURCE="ota-registry"
    fi

    if [ -z "$VERSION" ] || [ -z "$R2_URL" ]; then
        echo "Error: Could not determine latest version dynamically from manifest.json"
        exit 1
    fi

    echo "  Version: $VERSION (source: $SOURCE)"
}

# ── Download binary ──────────────────────────────

download_binary() {
    TEMP_FILE="$(mktemp)"

    echo "Downloading from R2: ${R2_URL}"
    if ! curl -fSL --progress-bar "$R2_URL" -o "$TEMP_FILE"; then
        echo "Primary download failed. Trying GitHub Fallback..."
        if [ -n "$GH_URL" ]; then
            echo "Downloading from GitHub: ${GH_URL}"
            if ! curl -fSL --progress-bar "$GH_URL" -o "$TEMP_FILE"; then
                rm -f "$TEMP_FILE"
                echo ""
                echo "Error: Download failed from both R2 and GitHub Fallback."
                exit 1
            fi
        else
            echo "Error: No fallback URL provided by manifest."
            exit 1
        fi
    fi
}

# ── Verify SHA-256 ───────────────────────────────

verify_hash() {
    if [ -n "$EXPECTED_HASH" ]; then
        echo "Verifying SHA-256..."
        if command -v sha256sum >/dev/null 2>&1; then
            ACTUAL_HASH=$(sha256sum "$TEMP_FILE" | awk '{print $1}')
        elif command -v shasum >/dev/null 2>&1; then
            ACTUAL_HASH=$(shasum -a 256 "$TEMP_FILE" | awk '{print $1}')
        else
            echo "Warning: No SHA-256 tool found, skipping verification"
            return
        fi

        if [ "$ACTUAL_HASH" != "$EXPECTED_HASH" ]; then
            rm -f "$TEMP_FILE"
            echo "Error: SHA-256 mismatch!"
            echo "  Expected: $EXPECTED_HASH"
            echo "  Got:      $ACTUAL_HASH"
            echo ""
            echo "The binary may have been tampered with. Aborting."
            exit 1
        fi
        echo "  SHA-256 verified ✓"
    else
        echo "Warning: No expected hash available, skipping verification"
    fi
}

# ── Install binary ───────────────────────────────

install_binary() {
    chmod +x "$TEMP_FILE"

    if [ -w "$INSTALL_DIR" ]; then
        mv "$TEMP_FILE" "${INSTALL_DIR}/${BINARY}"
        echo "Installed to ${INSTALL_DIR}/${BINARY}"
    elif command -v sudo >/dev/null 2>&1; then
        echo "Installing to ${INSTALL_DIR}/${BINARY} (requires sudo)..."
        sudo mv "$TEMP_FILE" "${INSTALL_DIR}/${BINARY}"
        echo "Installed to ${INSTALL_DIR}/${BINARY}"
    else
        INSTALL_DIR="${HOME}/.local/bin"
        mkdir -p "$INSTALL_DIR"
        mv "$TEMP_FILE" "${INSTALL_DIR}/${BINARY}"
        echo "Installed to ${INSTALL_DIR}/${BINARY}"
        echo "Make sure ${INSTALL_DIR} is in your PATH"
    fi
}

# ── Main ─────────────────────────────────────────

main() {
    echo ""
    echo "  ╭──────────────────────────╮"
    echo "  │     Rove Installer       │"
    echo "  ╰──────────────────────────╯"
    echo ""

    detect_platform
    echo "  OS:     $(uname -s)"
    echo "  Arch:   $(uname -m)"
    echo "  Target: $TARGET"
    echo ""

    fetch_version
    download_binary
    verify_hash
    install_binary

    echo ""
    echo "Run 'rove setup' to configure."
    echo "Run 'rove doctor' to verify installation."
    echo ""
}

main
