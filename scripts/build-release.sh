#!/bin/bash
# Build release binaries for all platforms
# Usage: ./scripts/build-release.sh [version]

set -e

VERSION=${1:-$(git describe --tags --always)}
BUILD_DIR="target/release-builds"
DIST_DIR="dist"

echo "🚀 Building Rove v${VERSION}"
echo ""

# Clean previous builds
rm -rf "$BUILD_DIR" "$DIST_DIR"
mkdir -p "$BUILD_DIR" "$DIST_DIR"

# Build for current platform
echo "📦 Building for current platform..."
cargo build --release --bin rove

# Get current platform
PLATFORM=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$PLATFORM" in
    darwin)
        PLATFORM_NAME="macos"
        ;;
    linux)
        PLATFORM_NAME="linux"
        ;;
    *)
        PLATFORM_NAME="$PLATFORM"
        ;;
esac

case "$ARCH" in
    x86_64)
        ARCH_NAME="x64"
        ;;
    arm64|aarch64)
        ARCH_NAME="arm64"
        ;;
    *)
        ARCH_NAME="$ARCH"
        ;;
esac

BINARY_NAME="rove-${VERSION}-${PLATFORM_NAME}-${ARCH_NAME}"

# Copy binary
cp target/release/rove "$BUILD_DIR/$BINARY_NAME"

# Create tarball
echo "📦 Creating tarball..."
cd "$BUILD_DIR"
tar -czf "../$DIST_DIR/${BINARY_NAME}.tar.gz" "$BINARY_NAME"
cd ..

# Generate checksum
echo "🔐 Generating checksum..."
cd "$DIST_DIR"
shasum -a 256 "${BINARY_NAME}.tar.gz" > "${BINARY_NAME}.tar.gz.sha256"
cd ..

echo ""
echo "✅ Build complete!"
echo "   Binary: $BUILD_DIR/$BINARY_NAME"
echo "   Archive: $DIST_DIR/${BINARY_NAME}.tar.gz"
echo "   Checksum: $DIST_DIR/${BINARY_NAME}.tar.gz.sha256"
echo ""
echo "📊 Binary size: $(du -h "$BUILD_DIR/$BINARY_NAME" | cut -f1)"
echo "📊 Archive size: $(du -h "$DIST_DIR/${BINARY_NAME}.tar.gz" | cut -f1)"
