#!/usr/bin/env bash
# Rove installer (POSIX / macOS / Linux).
# Usage:
#   curl -sSfL https://rove.sh/install.sh | sh
#   ROVE_CHANNEL=dev curl -sSfL https://rove.sh/install.sh | sh
#
# Env:
#   ROVE_CHANNEL       stable (default) | dev
#   ROVE_REGISTRY_BASE override registry URL (default https://registry.roveai.co)
#   ROVE_INSTALL_DIR   override install dir (default /usr/local/bin or $HOME/.local/bin)

set -eu

CHANNEL="${ROVE_CHANNEL:-stable}"
REGISTRY_BASE="${ROVE_REGISTRY_BASE:-https://registry.roveai.co}"
REGISTRY_BASE="${REGISTRY_BASE%/}"

case "$CHANNEL" in
    stable|dev) ;;
    *) echo "error: ROVE_CHANNEL must be 'stable' or 'dev' (got '$CHANNEL')" >&2; exit 1 ;;
esac

BIN_NAME="rove"
[ "$CHANNEL" = "dev" ] && BIN_NAME="rove-dev"

# --- platform detect ---
uname_s="$(uname -s)"
uname_m="$(uname -m)"
case "$uname_s" in
    Linux)  os="linux" ;;
    Darwin) os="darwin" ;;
    *) echo "error: unsupported OS '$uname_s'" >&2; exit 1 ;;
esac
case "$uname_m" in
    x86_64|amd64) arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    *) echo "error: unsupported arch '$uname_m'" >&2; exit 1 ;;
esac
target="${os}-${arch}"

case "$target" in
    darwin-aarch64) asset="rove-aarch64-apple-darwin" ;;
    darwin-x86_64)  asset="rove-x86_64-apple-darwin" ;;
    linux-x86_64)   asset="rove-x86_64-unknown-linux-gnu" ;;
    linux-aarch64)  asset="rove-aarch64-unknown-linux-gnu" ;;
    *) echo "error: no published build for $target" >&2; exit 1 ;;
esac

# --- deps ---
if command -v curl >/dev/null 2>&1; then
    dl() { curl -sSfL "$1" -o "$2"; }
    dl_text() { curl -sSfL "$1"; }
elif command -v wget >/dev/null 2>&1; then
    dl() { wget -q -O "$2" "$1"; }
    dl_text() { wget -q -O - "$1"; }
else
    echo "error: need curl or wget" >&2; exit 1
fi

if command -v b3sum >/dev/null 2>&1; then
    hash_cmd="b3sum"
else
    hash_cmd=""
    echo "warn: b3sum not found; skipping payload hash verification" >&2
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# --- fetch manifest + signature ---
manifest_url="${REGISTRY_BASE}/${CHANNEL}/engine/manifest.json"
sig_url="${REGISTRY_BASE}/${CHANNEL}/engine/manifest.sig"

echo "Fetching manifest: $manifest_url"
dl "$manifest_url" "$tmp/manifest.json"
dl "$sig_url" "$tmp/manifest.sig" || echo "warn: signature fetch failed" >&2

# Minimal manifest parse with shell + sed (no jq dependency).
# Extract: entries.latest.version, entries.latest.platforms.<target>.url / blake3 / size_bytes
channel_in_manifest="$(sed -n 's/.*"channel"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$tmp/manifest.json" | head -n1)"
if [ "$channel_in_manifest" != "$CHANNEL" ]; then
    echo "error: manifest channel mismatch (expected $CHANNEL got '$channel_in_manifest')" >&2
    exit 1
fi

version="$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$tmp/manifest.json" | head -n1)"
[ -n "$version" ] || { echo "error: cannot parse version from manifest" >&2; exit 1; }

# Extract platform block.
python_parse() {
    cat <<PY | /usr/bin/env python3 -
import json, sys
m = json.load(open("$tmp/manifest.json"))
p = m["entries"]["latest"]["platforms"]["$target"]
print(p.get("url",""))
print(p.get("fallback_url",""))
print(p.get("blake3",""))
print(p.get("size_bytes",0))
PY
}

if command -v python3 >/dev/null 2>&1; then
    parse_out="$(python_parse)"
    url="$(echo "$parse_out" | sed -n '1p')"
    fallback="$(echo "$parse_out" | sed -n '2p')"
    expected_hash="$(echo "$parse_out" | sed -n '3p')"
else
    echo "error: python3 required for manifest parse" >&2
    exit 1
fi

[ -n "$url" ] || { echo "error: no url for target $target in manifest" >&2; exit 1; }

# --- download binary ---
echo "Downloading $asset ($version, $CHANNEL channel)..."
if ! dl "$url" "$tmp/$asset"; then
    if [ -n "$fallback" ]; then
        echo "Primary download failed, trying fallback..." >&2
        dl "$fallback" "$tmp/$asset"
    else
        exit 1
    fi
fi

# --- verify ---
if [ -n "$hash_cmd" ] && [ -n "$expected_hash" ]; then
    actual="$($hash_cmd "$tmp/$asset" | awk '{print $1}')"
    if [ "$actual" != "$expected_hash" ]; then
        echo "error: BLAKE3 mismatch" >&2
        echo "  expected: $expected_hash" >&2
        echo "  actual:   $actual" >&2
        exit 1
    fi
    echo "BLAKE3 verified."
fi

chmod 0755 "$tmp/$asset"

# --- install ---
if [ -n "${ROVE_INSTALL_DIR:-}" ]; then
    dest_dir="$ROVE_INSTALL_DIR"
elif [ -w "/usr/local/bin" ] 2>/dev/null; then
    dest_dir="/usr/local/bin"
elif [ "$(id -u)" = "0" ]; then
    dest_dir="/usr/local/bin"
else
    dest_dir="$HOME/.local/bin"
    mkdir -p "$dest_dir"
fi

dest="$dest_dir/$BIN_NAME"
if [ ! -w "$dest_dir" ] 2>/dev/null; then
    echo "Installing to $dest (sudo)..."
    sudo install -m 0755 "$tmp/$asset" "$dest"
else
    install -m 0755 "$tmp/$asset" "$dest"
fi

echo ""
echo "Installed: $dest (v$version, $CHANNEL)"
echo "Data dir:  $HOME/.rove$( [ "$CHANNEL" = "dev" ] && echo "-dev" )"
echo ""
echo "Next: $BIN_NAME init"
