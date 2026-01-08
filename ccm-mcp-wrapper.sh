#!/bin/bash
# CCM MCP Server Wrapper
# ========================
# This script sets up the environment for the CCM MCP server.
# It should be referenced in your mcp_config.json file.
#
# Usage:
#   1. Update the paths below to match your system
#   2. Make executable: chmod +x ccm-mcp-wrapper.sh
#   3. Add to mcp_config.json with absolute path
#
# Example mcp_config.json entry:
# {
#   "mcpServers": {
#     "context-manager": {
#       "command": "/Users/yourname/path/to/ccm-mcp-wrapper.sh",
#       "args": [],
#       "env": {}
#     }
#   }
# }

# ============================================
# CONFIGURATION - Update these paths!
# ============================================
PROJECT_DIR="/Users/dogan/Desktop/context-manager"
BINARY="${PROJECT_DIR}/target/debug/ccm-mcp"
DB_PATH="${PROJECT_DIR}/data/ccm_mcp_db"
DEBUG_LOG="${PROJECT_DIR}/mcp_debug.log"

# ============================================
# EMBEDDING CONFIGURATION
# ============================================
export EMBEDDING_PROVIDER=ollama
export EMBEDDING_HOST=http://127.0.0.1:11434
export EMBEDDING_MODEL=mxbai-embed-large
export EMBEDDING_API_KEY=ollama

# ============================================
# SERVER CONFIGURATION
# ============================================
export CCM_DB_PATH="${DB_PATH}"
export RUST_LOG=info

# ============================================
# DEBUG LOGGING (Optional)
# Comment out the next line to disable debug logging
# ============================================
exec 2>>"${DEBUG_LOG}"
echo "=== CCM MCP Server Started at $(date) ===" >&2

# ============================================
# EXECUTE SERVER
# ============================================
cd "${PROJECT_DIR}"
exec "${BINARY}"
