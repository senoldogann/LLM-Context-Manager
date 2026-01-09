#!/bin/bash

# Configuration
VERSION="0.1.2"
PACKAGE_NAME="@ccm/context-manager"

echo "🚀 Preparing NPM package $PACKAGE_NAME v$VERSION..."

# 1. Build binaries for different targets (Should be run on a CI or multi-node setup)
# This is a template for the manual release process
echo "📝 Note: Ensure binaries are built and uploaded to GitHub Releases first."
echo "Expected URLs: https://github.com/senoldogann/LLM-Context-Manager/releases/download/v$VERSION/ccm-mcp-[target]"

# 2. Update version in package.json
cd npm
npm version $VERSION --no-git-tag-version

# 3. Publish to NPM
echo "📦 Publishing to NPM..."
# use --access public for scoped packages
npm publish --access public

echo "✅ Published successfully!"
