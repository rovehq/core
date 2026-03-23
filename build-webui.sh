#!/bin/bash
# Rove WebUI Next.js Build Script
# Installs dependencies and builds the Next.js WebUI

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WEBUI_DIR="$SCRIPT_DIR/webui"

echo "🌐 Rove WebUI Build Script"
echo "=========================="
echo ""

# Check if Node.js is installed
if ! command -v node &> /dev/null; then
    echo "❌ Node.js is not installed"
    echo ""
    echo "Please install Node.js:"
    echo "  macOS:  brew install node"
    echo "  Linux:  curl -fsSL https://deb.nodesource.com/setup_lts.x | sudo -E bash - && sudo apt-get install -y nodejs"
    echo "  Windows: Download from https://nodejs.org/"
    exit 1
fi

echo "✅ Node.js: $(node --version)"
echo "✅ npm: $(npm --version)"
echo ""

# Navigate to webui directory
cd "$WEBUI_DIR"

# Install dependencies
echo "📦 Installing dependencies..."
npm install

# Build
echo "🔨 Building WebUI..."
npm run build

echo ""
echo "✅ Build complete!"
echo ""
echo "The built files are in: $WEBUI_DIR/dist"
echo ""
echo "To serve the WebUI:"
echo "  1. Build the Rove daemon: cd engine && cargo build"
echo "  2. Start the daemon: ROVE_OPENAI_API_KEY=test ./target/debug/rove daemon --port 47630"
echo "  3. Open: https://app.roveai.co"
echo ""
