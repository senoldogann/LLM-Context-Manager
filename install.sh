#!/usr/bin/env bash
#
# CCM (Cognitive Codebase Matrix) - One-Click Installer
# https://github.com/senoldogann/LLM-Context-Manager
#
# Usage: curl -sSL https://raw.githubusercontent.com/senoldogann/LLM-Context-Manager/main/install.sh | bash
#        OR: ./install.sh (if already cloned)
#
set -e

# ============================================
# Configuration
# ============================================
REPO_URL="https://github.com/senoldogann/LLM-Context-Manager.git"
INSTALL_DIR="$HOME/.ccm"
EMBEDDING_MODEL="mxbai-embed-large"

# ============================================
# Colors & Helpers
# ============================================
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_banner() {
    echo -e "${BLUE}"
    echo "╔══════════════════════════════════════════════════════════════╗"
    echo "║       🧠 CCM - Cognitive Codebase Matrix Installer           ║"
    echo "║              Context Provider for AI Agents                  ║"
    echo "╚══════════════════════════════════════════════════════════════╝"
    echo -e "${NC}"
}

info() { echo -e "${BLUE}[INFO]${NC} $1"; }
success() { echo -e "${GREEN}[✓]${NC} $1"; }
warn() { echo -e "${YELLOW}[!]${NC} $1"; }
error() { echo -e "${RED}[✗]${NC} $1"; exit 1; }

check_command() {
    command -v "$1" >/dev/null 2>&1
}

# ============================================
# OS Detection
# ============================================
detect_os() {
    case "$(uname -s)" in
        Darwin*)    OS="macos" ;;
        Linux*)     OS="linux" ;;
        MINGW*|MSYS*|CYGWIN*) OS="windows" ;;
        *)          error "Unsupported operating system" ;;
    esac
    info "Detected OS: $OS"
}

# ============================================
# Step 1: Install Rust
# ============================================
install_rust() {
    if check_command rustc; then
        RUST_VERSION=$(rustc --version | awk '{print $2}')
        success "Rust is already installed (v$RUST_VERSION)"
    else
        info "Installing Rust..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
        success "Rust installed successfully"
    fi
}

# ============================================
# Step 2: Install Ollama
# ============================================
install_ollama() {
    if check_command ollama; then
        success "Ollama is already installed"
    else
        info "Installing Ollama..."
        case "$OS" in
            macos)
                if check_command brew; then
                    brew install ollama
                else
                    curl -fsSL https://ollama.com/install.sh | sh
                fi
                ;;
            linux)
                curl -fsSL https://ollama.com/install.sh | sh
                ;;
            windows)
                warn "Please install Ollama manually from https://ollama.com/download"
                warn "After installation, run this script again."
                exit 1
                ;;
        esac
        success "Ollama installed successfully"
    fi
}

# ============================================
# Step 3: Start Ollama & Pull Model
# ============================================
setup_ollama() {
    info "Ensuring Ollama is running..."
    
    # Check if Ollama is running
    if ! curl -s http://127.0.0.1:11434/api/tags >/dev/null 2>&1; then
        info "Starting Ollama service..."
        case "$OS" in
            macos)
                # On macOS, Ollama runs as a background app
                open -a Ollama 2>/dev/null || ollama serve &
                ;;
            linux)
                ollama serve &
                ;;
        esac
        sleep 3
    fi
    
    # Check if model exists
    if ollama list 2>/dev/null | grep -q "$EMBEDDING_MODEL"; then
        success "Embedding model '$EMBEDDING_MODEL' is already available"
    else
        info "Pulling embedding model '$EMBEDDING_MODEL' (this may take a few minutes)..."
        ollama pull "$EMBEDDING_MODEL"
        success "Embedding model pulled successfully"
    fi
}

# ============================================
# Step 4: Clone or Update Repository
# ============================================
setup_repository() {
    # Check if we're already in the repo directory
    if [ -f "Cargo.toml" ] && grep -q "ccm-core" Cargo.toml 2>/dev/null; then
        info "Running from existing CCM directory"
        INSTALL_DIR="$(pwd)"
    elif [ -d "$INSTALL_DIR" ]; then
        info "Updating existing installation..."
        cd "$INSTALL_DIR"
        git pull origin main
    else
        info "Cloning CCM repository..."
        git clone "$REPO_URL" "$INSTALL_DIR"
        cd "$INSTALL_DIR"
    fi
    
    success "Repository ready at: $INSTALL_DIR"
}

# ============================================
# Step 5: Build Project
# ============================================
build_project() {
    info "Building CCM (release mode)..."
    cd "$INSTALL_DIR"
    
    # Ensure cargo is in PATH
    source "$HOME/.cargo/env" 2>/dev/null || true
    
    cargo build --release --workspace
    
    success "Build completed successfully"
    success "Binaries available at: $INSTALL_DIR/target/release/"
}

# ============================================
# Step 6: Create .env Configuration
# ============================================
# ============================================
# Step 6: Create .env Configuration
# ============================================
create_env() {
    # Always use ~/.ccm/.env for global config
    mkdir -p "$HOME/.ccm"
    ENV_FILE="$HOME/.ccm/.env"
    
    if [ -f "$ENV_FILE" ]; then
        warn ".env config already exists at $ENV_FILE, skipping..."
    else
        info "Creating global .env configuration..."
        cat > "$ENV_FILE" << EOF
# CCM Configuration
# Generated by install.sh on $(date)

# Embedding Provider (ollama or openai)
EMBEDDING_PROVIDER=ollama
EMBEDDING_HOST=http://127.0.0.1:11434
EMBEDDING_MODEL=$EMBEDDING_MODEL
EMBEDDING_API_KEY=ollama

# Logging
RUST_LOG=info
EOF
        success "Global config created at $ENV_FILE"
    fi
}

# ============================================
# Step 7: Setup Wrapper Script
# ============================================
setup_wrapper() {
    # No longer needed for MCP config since we point to binary directly in README
    # But useful for backward compat or custom setups
    WRAPPER_FILE="$INSTALL_DIR/ccm-mcp-wrapper.sh"
    
    info "Creating helper wrapper..."
    
    cat > "$WRAPPER_FILE" << EOF
#!/bin/bash
# Helper script to launch CCM MCP Server
export RUST_LOG=info
exec "$INSTALL_DIR/target/release/ccm-mcp" "\$@"
EOF
    
    chmod +x "$WRAPPER_FILE"
    
    success "Wrapper created at $WRAPPER_FILE"
}

# ============================================
# Step 8: Add to PATH (Optional)
# ============================================
add_to_path() {
    BIN_DIR="$INSTALL_DIR/target/release"
    
    # Detect shell config file
    if [ -f "$HOME/.zshrc" ]; then
        SHELL_RC="$HOME/.zshrc"
    elif [ -f "$HOME/.bashrc" ]; then
        SHELL_RC="$HOME/.bashrc"
    else
        SHELL_RC=""
    fi
    
    if [ -n "$SHELL_RC" ]; then
        if ! grep -q "CCM_PATH" "$SHELL_RC" 2>/dev/null; then
            echo "" >> "$SHELL_RC"
            echo "# CCM (Cognitive Codebase Matrix)" >> "$SHELL_RC"
            echo "export CCM_PATH=\"$INSTALL_DIR\"" >> "$SHELL_RC"
            echo "export PATH=\"\$CCM_PATH/target/release:\$PATH\"" >> "$SHELL_RC"
            success "Added CCM to PATH in $SHELL_RC"
        fi
    fi
}

# ============================================
# Step 9: Interactive Project Indexing
# ============================================
index_project() {
    echo ""
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}📂 Project Indexing${NC}"
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    echo "CCM needs to index your codebase to provide intelligent context."
    echo ""
    
    # Ask if user wants to index now
    read -p "Would you like to index a project now? (y/n): " INDEX_NOW
    
    if [[ "$INDEX_NOW" =~ ^[Yy]$ ]]; then
        # Get project path
        echo ""
        read -p "Enter the full path to your project: " PROJECT_PATH
        
        # Expand ~ to $HOME
        PROJECT_PATH="${PROJECT_PATH/#\~/$HOME}"
        
        # Validate path
        if [ ! -d "$PROJECT_PATH" ]; then
            warn "Directory not found: $PROJECT_PATH"
            warn "You can index later with: ccm-cli index --path /your/project"
            return
        fi
        
        info "Indexing project: $PROJECT_PATH"
        echo ""
        
        # Source env for cargo
        source "$HOME/.cargo/env" 2>/dev/null || true
        
        # Run indexing
        if "$INSTALL_DIR/target/release/ccm-cli" index --path "$PROJECT_PATH"; then
            success "Project indexed successfully!"
        else
            warn "Indexing failed. You can retry later with:"
            echo "    ccm-cli index --path $PROJECT_PATH"
        fi
    else
        info "Skipping indexing. You can index later with:"
        echo "    $INSTALL_DIR/target/release/ccm-cli index --path /your/project"
    fi
}

# ============================================
# Print Final Instructions
# ============================================
print_success() {
    echo ""
    echo -e "${GREEN}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║              ✓ CCM Installation Complete!                    ║${NC}"
    echo -e "${GREEN}╚══════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    echo -e "${BLUE}📁 Installation Directory:${NC} $INSTALL_DIR"
    echo ""
    echo -e "${YELLOW}🚀 Quick Start:${NC}"
    echo ""
    echo "   1. Index your project:"
    echo "      ${GREEN}$INSTALL_DIR/target/release/ccm-cli index --path /path/to/your/project${NC}"
    echo ""
    echo "   2. Add to your AI editor (Antigravity/Claude Desktop):"
    echo "      Edit your MCP config and add:"
    echo ""
    echo -e "      ${BLUE}\"context-manager\": {"
    echo "        \"command\": \"$INSTALL_DIR/ccm-mcp-wrapper.sh\","
    echo "        \"args\": []"
    echo -e "      }${NC}"
    echo ""
    echo "   3. Restart your AI editor and start asking questions!"
    echo ""
    echo -e "${YELLOW}📖 Documentation:${NC} https://github.com/senoldogann/LLM-Context-Manager"
    echo ""
}

# ============================================
# Main Execution
# ============================================
main() {
    print_banner
    detect_os
    
    echo ""
    info "Starting installation..."
    echo ""
    
    install_rust
    install_ollama
    setup_repository
    setup_ollama
    build_project
    create_env
    setup_wrapper
    add_to_path
    index_project
    
    print_success
}

main "$@"
