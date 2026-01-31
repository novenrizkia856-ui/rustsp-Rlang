#!/bin/bash
# ============================================================================
# RustS+ Installer for Linux
# The Programming Language with Effect Honesty
# Part of DSDN (Data Semi-Decentral Network) Project
# ============================================================================
# Usage: ./install.sh [OPTIONS]
# Run with --help for more information
# ============================================================================

set -e

# ----------------------------- Configuration -----------------------------
APP_NAME="RustS+"
APP_VERSION="1.0.0"
APP_DESCRIPTION="The Programming Language with Effect Honesty"
REPO_URL="https://github.com/novenrizkia856-ui/rustsp-Rlang"

# Default installation directories
DEFAULT_INSTALL_DIR="$HOME/.rustsp"
DEFAULT_BIN_DIR="$HOME/.local/bin"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color
BOLD='\033[1m'

# ----------------------------- Helper Functions -----------------------------

print_banner() {
    echo -e "${CYAN}"
    echo "╔═══════════════════════════════════════════════════════════════╗"
    echo "║                                                               ║"
    echo "║                     RustS+ Installer                          ║"
    echo "║         The Programming Language with Effect Honesty          ║"
    echo "║                                                               ║"
    echo "║   \"Rust prevents memory bugs. RustS+ prevents logic bugs.\"    ║"
    echo "║                                                               ║"
    echo "╚═══════════════════════════════════════════════════════════════╝"
    echo -e "${NC}"
    echo -e "${BOLD}Version:${NC} $APP_VERSION"
    echo -e "${BOLD}Repository:${NC} $REPO_URL"
    echo ""
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}!${NC} $1"
}

print_info() {
    echo -e "${BLUE}→${NC} $1"
}

print_step() {
    echo -e "\n${BOLD}${CYAN}[$1/$TOTAL_STEPS]${NC} $2"
}

show_help() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  -d, --dir DIR       Installation directory (default: $DEFAULT_INSTALL_DIR)"
    echo "  -b, --bin DIR       Binary symlink directory (default: $DEFAULT_BIN_DIR)"
    echo "  --system            Install system-wide to /usr/local (requires sudo)"
    echo "  --no-examples       Skip installing example files"
    echo "  --no-path           Don't modify PATH in shell config"
    echo "  --uninstall         Uninstall RustS+"
    echo "  -y, --yes           Skip confirmation prompts"
    echo "  -h, --help          Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0                      # Interactive installation"
    echo "  $0 --system             # System-wide installation"
    echo "  $0 -d ~/rustsp -y       # Install to custom dir, no prompts"
    echo "  $0 --uninstall          # Remove RustS+"
}

check_requirements() {
    print_step "1" "Checking requirements..."
    
    # Check if Rust is installed
    if command -v rustc &> /dev/null; then
        RUST_VERSION=$(rustc --version | cut -d' ' -f2)
        print_success "Rust toolchain found (rustc $RUST_VERSION)"
    else
        print_warning "Rust toolchain not found"
        print_info "RustS+ requires Rust. Install from: https://rustup.rs"
        if [ "$SKIP_CONFIRM" != "true" ]; then
            read -p "Continue anyway? [y/N] " -n 1 -r
            echo
            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                exit 1
            fi
        fi
    fi
    
    # Check for required binaries in source
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    
    if [ -f "$SCRIPT_DIR/files/rustsp" ] || [ -f "$SCRIPT_DIR/rustsp" ]; then
        print_success "RustS+ binaries found"
    else
        print_error "RustS+ binaries not found!"
        print_info "Expected files in: $SCRIPT_DIR/files/ or $SCRIPT_DIR/"
        exit 1
    fi
}

detect_shell() {
    if [ -n "$ZSH_VERSION" ]; then
        CURRENT_SHELL="zsh"
        SHELL_RC="$HOME/.zshrc"
    elif [ -n "$BASH_VERSION" ]; then
        CURRENT_SHELL="bash"
        SHELL_RC="$HOME/.bashrc"
    else
        CURRENT_SHELL="sh"
        SHELL_RC="$HOME/.profile"
    fi
}

add_to_path() {
    local bin_dir="$1"
    
    detect_shell
    
    # Check if already in PATH
    if [[ ":$PATH:" == *":$bin_dir:"* ]]; then
        print_info "Directory already in PATH"
        return 0
    fi
    
    # Add to shell config
    local path_export="export PATH=\"\$PATH:$bin_dir\""
    local marker="# RustS+ PATH"
    
    if grep -q "$marker" "$SHELL_RC" 2>/dev/null; then
        print_info "PATH entry already exists in $SHELL_RC"
    else
        echo "" >> "$SHELL_RC"
        echo "$marker" >> "$SHELL_RC"
        echo "$path_export" >> "$SHELL_RC"
        print_success "Added to PATH in $SHELL_RC"
    fi
}

remove_from_path() {
    detect_shell
    
    if [ -f "$SHELL_RC" ]; then
        # Remove RustS+ PATH entries
        sed -i '/# RustS+ PATH/d' "$SHELL_RC" 2>/dev/null || true
        sed -i '/\.rustsp\/bin/d' "$SHELL_RC" 2>/dev/null || true
        sed -i '/rustsp/d' "$SHELL_RC" 2>/dev/null || true
        print_success "Removed PATH entries from $SHELL_RC"
    fi
}

install_files() {
    print_step "2" "Installing files..."
    
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    
    # Determine source directory
    if [ -d "$SCRIPT_DIR/files" ]; then
        SRC_DIR="$SCRIPT_DIR/files"
    else
        SRC_DIR="$SCRIPT_DIR"
    fi
    
    # Create directories
    mkdir -p "$INSTALL_DIR/bin"
    print_success "Created $INSTALL_DIR/bin"
    
    # Copy binaries
    if [ -f "$SRC_DIR/rustsp" ]; then
        cp "$SRC_DIR/rustsp" "$INSTALL_DIR/bin/"
        chmod +x "$INSTALL_DIR/bin/rustsp"
        print_success "Installed rustsp"
    elif [ -f "$SRC_DIR/rustsp.exe" ]; then
        # Handle if someone accidentally includes Windows binary
        print_error "Found Windows binary (rustsp.exe), need Linux binary"
        exit 1
    fi
    
    if [ -f "$SRC_DIR/cargo-rustsp" ]; then
        cp "$SRC_DIR/cargo-rustsp" "$INSTALL_DIR/bin/"
        chmod +x "$INSTALL_DIR/bin/cargo-rustsp"
        print_success "Installed cargo-rustsp"
    fi
    
    # Copy LICENSE
    if [ -f "$SRC_DIR/LICENSE" ]; then
        cp "$SRC_DIR/LICENSE" "$INSTALL_DIR/"
        print_success "Installed LICENSE"
    fi
    
    # Copy examples (optional)
    if [ "$INSTALL_EXAMPLES" = "true" ] && [ -d "$SRC_DIR/examples" ]; then
        mkdir -p "$INSTALL_DIR/examples"
        cp -r "$SRC_DIR/examples/"* "$INSTALL_DIR/examples/" 2>/dev/null || true
        print_success "Installed examples to $INSTALL_DIR/examples"
    fi
}

create_symlinks() {
    print_step "3" "Creating symlinks..."
    
    mkdir -p "$BIN_DIR"
    
    # Create symlinks
    if [ -f "$INSTALL_DIR/bin/rustsp" ]; then
        ln -sf "$INSTALL_DIR/bin/rustsp" "$BIN_DIR/rustsp"
        print_success "Linked rustsp → $BIN_DIR/rustsp"
    fi
    
    if [ -f "$INSTALL_DIR/bin/cargo-rustsp" ]; then
        ln -sf "$INSTALL_DIR/bin/cargo-rustsp" "$BIN_DIR/cargo-rustsp"
        print_success "Linked cargo-rustsp → $BIN_DIR/cargo-rustsp"
    fi
}

setup_path() {
    print_step "4" "Configuring PATH..."
    
    if [ "$MODIFY_PATH" = "true" ]; then
        add_to_path "$BIN_DIR"
    else
        print_info "Skipping PATH modification (--no-path)"
    fi
}

show_completion() {
    echo ""
    echo -e "${GREEN}════════════════════════════════════════════════════════════════${NC}"
    echo -e "${GREEN}           RustS+ $APP_VERSION installed successfully!${NC}"
    echo -e "${GREEN}════════════════════════════════════════════════════════════════${NC}"
    echo ""
    echo -e "${BOLD}Installation directory:${NC} $INSTALL_DIR"
    echo -e "${BOLD}Binary directory:${NC} $BIN_DIR"
    echo ""
    
    if [ "$MODIFY_PATH" = "true" ]; then
        echo -e "${YELLOW}To start using RustS+, either:${NC}"
        echo "  1. Restart your terminal, or"
        echo "  2. Run: source $SHELL_RC"
        echo ""
    fi
    
    echo -e "${BOLD}Quick start:${NC}"
    echo "  rustsp --help              # Show help"
    echo "  rustsp hello.rss -o hello  # Compile a file"
    echo "  ./hello                    # Run the binary"
    echo ""
    
    if [ "$INSTALL_EXAMPLES" = "true" ]; then
        echo -e "${BOLD}Examples:${NC} $INSTALL_DIR/examples/"
    fi
    
    echo ""
    echo -e "Where Logic Safety Meets Memory Safety 🦀"
}

# ----------------------------- Uninstall -----------------------------

uninstall() {
    print_banner
    echo -e "${YELLOW}Uninstalling RustS+...${NC}"
    echo ""
    
    # Find installation
    if [ -d "$HOME/.rustsp" ]; then
        INSTALL_DIR="$HOME/.rustsp"
    elif [ -d "/usr/local/rustsp" ]; then
        INSTALL_DIR="/usr/local/rustsp"
        NEED_SUDO="true"
    else
        print_error "RustS+ installation not found"
        exit 1
    fi
    
    if [ "$SKIP_CONFIRM" != "true" ]; then
        echo "This will remove:"
        echo "  - $INSTALL_DIR"
        echo "  - Symlinks in ~/.local/bin or /usr/local/bin"
        echo "  - PATH entries in shell config"
        echo ""
        read -p "Continue? [y/N] " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 0
        fi
    fi
    
    # Remove symlinks
    rm -f "$HOME/.local/bin/rustsp" 2>/dev/null || true
    rm -f "$HOME/.local/bin/cargo-rustsp" 2>/dev/null || true
    rm -f "/usr/local/bin/rustsp" 2>/dev/null || true
    rm -f "/usr/local/bin/cargo-rustsp" 2>/dev/null || true
    print_success "Removed symlinks"
    
    # Remove installation directory
    if [ "$NEED_SUDO" = "true" ]; then
        sudo rm -rf "$INSTALL_DIR"
    else
        rm -rf "$INSTALL_DIR"
    fi
    print_success "Removed $INSTALL_DIR"
    
    # Remove from PATH
    remove_from_path
    
    echo ""
    echo -e "${GREEN}RustS+ has been uninstalled.${NC}"
}

# ----------------------------- Main -----------------------------

main() {
    # Default values
    INSTALL_DIR="$DEFAULT_INSTALL_DIR"
    BIN_DIR="$DEFAULT_BIN_DIR"
    INSTALL_EXAMPLES="true"
    MODIFY_PATH="true"
    SKIP_CONFIRM="false"
    SYSTEM_INSTALL="false"
    DO_UNINSTALL="false"
    TOTAL_STEPS="4"
    
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            -d|--dir)
                INSTALL_DIR="$2"
                shift 2
                ;;
            -b|--bin)
                BIN_DIR="$2"
                shift 2
                ;;
            --system)
                SYSTEM_INSTALL="true"
                INSTALL_DIR="/usr/local/rustsp"
                BIN_DIR="/usr/local/bin"
                shift
                ;;
            --no-examples)
                INSTALL_EXAMPLES="false"
                shift
                ;;
            --no-path)
                MODIFY_PATH="false"
                shift
                ;;
            --uninstall)
                DO_UNINSTALL="true"
                shift
                ;;
            -y|--yes)
                SKIP_CONFIRM="true"
                shift
                ;;
            -h|--help)
                show_help
                exit 0
                ;;
            *)
                print_error "Unknown option: $1"
                show_help
                exit 1
                ;;
        esac
    done
    
    # Handle uninstall
    if [ "$DO_UNINSTALL" = "true" ]; then
        uninstall
        exit 0
    fi
    
    # Print banner
    print_banner
    
    # Show installation plan
    echo -e "${BOLD}Installation Plan:${NC}"
    echo "  Install directory: $INSTALL_DIR"
    echo "  Binary directory:  $BIN_DIR"
    echo "  Install examples:  $INSTALL_EXAMPLES"
    echo "  Modify PATH:       $MODIFY_PATH"
    echo ""
    
    # Confirm
    if [ "$SKIP_CONFIRM" != "true" ]; then
        read -p "Proceed with installation? [Y/n] " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Nn]$ ]]; then
            echo "Installation cancelled."
            exit 0
        fi
    fi
    
    # Check for sudo if system install
    if [ "$SYSTEM_INSTALL" = "true" ]; then
        if [ "$EUID" -ne 0 ]; then
            print_error "System-wide installation requires sudo"
            print_info "Run: sudo $0 --system"
            exit 1
        fi
    fi
    
    echo ""
    
    # Run installation steps
    check_requirements
    install_files
    create_symlinks
    setup_path
    
    # Show completion
    detect_shell
    show_completion
}

# Run main
main "$@"
