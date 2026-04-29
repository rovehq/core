#!/usr/bin/env bash
# Build a Rove release binary.
#
# Same script used by local devs and CI (.github/workflows/ci.yml).
# Picks up Ed25519 public keys for `build.rs` embedding from env first, then
# falls back to core/manifest/*.bin committed in the repo.
#
# Usage:
#   scripts/release/build-release.sh [version]
#
# Env (all optional):
#   ROVE_CHANNEL                   dev (default) | stable
#                                   dev    → --features channel-dev
#                                   stable → --features production
#   ROVE_TEAM_OFFICIAL_PUBLIC_KEY  hex or base64-DER Ed25519 public key
#   ROVE_TEAM_COMMUNITY_PUBLIC_KEY hex or base64-DER Ed25519 public key
#   TARGET                         Rust target triple override (default: host)
#   CARGO_FLAGS                    extra flags passed to cargo build
#
# If either public-key env var is unset, build.rs falls back to
# core/manifest/team_{official,community}_public_key.bin. If both env and
# file are missing, build.rs uses a zero placeholder and --features production
# will reject signatures at runtime.
#
# In GitHub Actions the two keys come from repo secrets synced from Infisical.

set -euo pipefail

VERSION=${1:-$(git describe --tags --always 2>/dev/null || echo "0.0.0")}
CHANNEL=${ROVE_CHANNEL:-dev}
BUILD_DIR="target/release-builds"
DIST_DIR="dist"

case "$CHANNEL" in
    dev)    FEATURES="--features channel-dev" ;;
    stable) FEATURES="--features production" ;;
    *) echo "error: ROVE_CHANNEL must be 'dev' or 'stable' (got '$CHANNEL')" >&2; exit 1 ;;
esac

if [ -n "${TARGET:-}" ]; then
    RUST_TARGET="$TARGET"
else
    RUST_TARGET=$(rustc -vV | awk '/host:/{print $2}')
fi

EXT=""
case "$RUST_TARGET" in
    aarch64-apple-darwin)       PLAT=darwin-aarch64 ;;
    x86_64-apple-darwin)        PLAT=darwin-x86_64 ;;
    x86_64-unknown-linux-gnu)   PLAT=linux-x86_64 ;;
    aarch64-unknown-linux-gnu)  PLAT=linux-aarch64 ;;
    x86_64-pc-windows-msvc)     PLAT=windows-x86_64; EXT=.exe ;;
    aarch64-pc-windows-msvc)    PLAT=windows-aarch64; EXT=.exe ;;
    *) PLAT="$RUST_TARGET" ;;
esac

echo "🚀 Building Rove v${VERSION}"
echo "   channel:  ${CHANNEL}"
echo "   target:   ${RUST_TARGET}"
echo "   features: ${FEATURES}"
echo ""

# Key-source diagnostics
if [ -n "${ROVE_TEAM_OFFICIAL_PUBLIC_KEY:-}" ]; then
    echo "🔑 OFFICIAL key:  env var"
elif [ -f core/manifest/team_official_public_key.bin ]; then
    echo "🔑 OFFICIAL key:  core/manifest/team_official_public_key.bin"
else
    echo "⚠️  OFFICIAL key:  PLACEHOLDER — production builds will reject signatures"
fi
if [ -n "${ROVE_TEAM_COMMUNITY_PUBLIC_KEY:-}" ]; then
    echo "🔑 COMMUNITY key: env var"
elif [ -f core/manifest/team_community_public_key.bin ]; then
    echo "🔑 COMMUNITY key: core/manifest/team_community_public_key.bin"
else
    echo "⚠️  COMMUNITY key: PLACEHOLDER — production builds will reject signatures"
fi
echo ""

REPO_ROOT="$(pwd)"
rm -rf "$BUILD_DIR" "$DIST_DIR"
mkdir -p "$BUILD_DIR" "$DIST_DIR"

echo "📦 cargo build --release ${FEATURES} --target ${RUST_TARGET} -p engine ${CARGO_FLAGS:-}"
# shellcheck disable=SC2086
cargo build --release $FEATURES --target "$RUST_TARGET" -p engine ${CARGO_FLAGS:-}

BINARY_SRC="target/${RUST_TARGET}/release/rove${EXT}"
if [ ! -f "$BINARY_SRC" ]; then
    echo "error: binary not found at $BINARY_SRC" >&2
    exit 1
fi
BINARY_NAME="rove-${VERSION}-${PLAT}${EXT}"
cp "$BINARY_SRC" "$BUILD_DIR/$BINARY_NAME"

# Archive
tar -czf "${REPO_ROOT}/${DIST_DIR}/${BINARY_NAME}.tar.gz" -C "$BUILD_DIR" "$BINARY_NAME"

# BLAKE3 preferred (matches engine's CryptoModule::compute_hash), sha256 fallback
if command -v b3sum >/dev/null 2>&1; then
    b3sum "${DIST_DIR}/${BINARY_NAME}.tar.gz" > "${DIST_DIR}/${BINARY_NAME}.tar.gz.blake3"
    HASH_LINE=$(b3sum "${DIST_DIR}/${BINARY_NAME}.tar.gz")
    HASH_ALGO=BLAKE3
else
    shasum -a 256 "${DIST_DIR}/${BINARY_NAME}.tar.gz" > "${DIST_DIR}/${BINARY_NAME}.tar.gz.sha256"
    HASH_LINE=$(shasum -a 256 "${DIST_DIR}/${BINARY_NAME}.tar.gz")
    HASH_ALGO=SHA256
fi

echo ""
echo "✅ Build complete"
echo "   Binary:   $BUILD_DIR/$BINARY_NAME ($(du -h "$BUILD_DIR/$BINARY_NAME" | cut -f1))"
echo "   Archive:  ${DIST_DIR}/${BINARY_NAME}.tar.gz ($(du -h "${DIST_DIR}/${BINARY_NAME}.tar.gz" | cut -f1))"
echo "   ${HASH_ALGO}:   $HASH_LINE"
