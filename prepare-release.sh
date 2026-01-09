#!/bin/bash

# Configuration
VERSION="0.1.6"
DIST_DIR="dist"

echo "🎯 Preparing release assets for v$VERSION..."

# Create dist directory
mkdir -p $DIST_DIR

# Determine local target
ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')

if [ "$OS" == "darwin" ]; then
    if [ "$ARCH" == "arm64" ]; then
        TARGET="aarch64-apple-darwin"
    else
        TARGET="x86_64-apple-darwin"
    fi
elif [ "$OS" == "linux" ]; then
    TARGET="x86_64-unknown-linux-gnu"
else
    # Windows/MSVC
    TARGET="x86_64-pc-windows-msvc.exe"
fi

echo "📦 Detected local target: $TARGET"

# Copy and rename binaries
# Assumes 'cargo build --release' has been run
BINARY_CLI="target/release/ccm-cli"
BINARY_MCP="target/release/ccm-mcp"

# Check if binaries exist
if [ ! -f "$BINARY_CLI" ] || [ ! -f "$BINARY_MCP" ]; then
    echo "⚠️ Warning: Binaries not found in target/release/. Attempting to build..."
    cargo build --release
fi

if [ -f "$BINARY_CLI" ]; then
    cp "$BINARY_CLI" "$DIST_DIR/ccm-cli-$TARGET"
    echo "✓ Created: $DIST_DIR/ccm-cli-$TARGET"
fi

if [ -f "$BINARY_MCP" ]; then
    cp "$BINARY_MCP" "$DIST_DIR/ccm-mcp-$TARGET"
    echo "✓ Created: $DIST_DIR/ccm-mcp-$TARGET"
fi

echo ""
echo "🚀 Done! Upload the files in the '$DIST_DIR' folder to your GitHub Release v$VERSION."
