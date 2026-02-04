#!/bin/bash

# RustS+ v1.0.0 Linux Installation Script
# Bro, script ini untuk install RustS+ binary ke sistem

set -e

# Colors untuk output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Default install path
INSTALL_PATH="/usr/local/bin"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo -e "${GREEN}================================${NC}"
echo -e "${GREEN}  RustS+ v1.0.0 Linux Installer${NC}"
echo -e "${GREEN}================================${NC}"
echo ""

# Check if running as root for system-wide install
if [ "$INSTALL_PATH" = "/usr/local/bin" ] && [ ! -w "$INSTALL_PATH" ]; then
    echo -e "${YELLOW}⚠ Memerlukan akses sudo untuk install ke $INSTALL_PATH${NC}"
    echo "Kamu bisa run dengan: sudo $0"
    echo ""
    echo "Atau install ke home directory:"
    INSTALL_PATH="$HOME/.local/bin"
    mkdir -p "$INSTALL_PATH"
    echo -e "${YELLOW}Installing ke: $INSTALL_PATH${NC}"
fi

echo -e "${YELLOW}📦 Install path: $INSTALL_PATH${NC}"
echo ""

# Check if binary exist
if [ ! -f "$SCRIPT_DIR/rustsp" ]; then
    echo -e "${RED}❌ Error: rustsp binary tidak ditemukan di $SCRIPT_DIR${NC}"
    exit 1
fi

# Install rustsp
echo -e "${YELLOW}📝 Installing rustsp binary...${NC}"
cp "$SCRIPT_DIR/rustsp" "$INSTALL_PATH/rustsp" || sudo cp "$SCRIPT_DIR/rustsp" "$INSTALL_PATH/rustsp"
chmod +x "$INSTALL_PATH/rustsp"
echo -e "${GREEN}✓ rustsp installed ke $INSTALL_PATH/rustsp${NC}"

# Install cargo-rustsp (if exist)
if [ -f "$SCRIPT_DIR/cargo-rustsp" ]; then
    echo -e "${YELLOW}📝 Installing cargo-rustsp...${NC}"
    cp "$SCRIPT_DIR/cargo-rustsp" "$INSTALL_PATH/cargo-rustsp" || sudo cp "$SCRIPT_DIR/cargo-rustsp" "$INSTALL_PATH/cargo-rustsp"
    chmod +x "$INSTALL_PATH/cargo-rustsp"
    echo -e "${GREEN}✓ cargo-rustsp installed ke $INSTALL_PATH/cargo-rustsp${NC}"
fi

echo ""
echo -e "${GREEN}================================${NC}"
echo -e "${GREEN}✓ Installation berhasil!${NC}"
echo -e "${GREEN}================================${NC}"
echo ""

# Verify installation
if command -v rustsp &> /dev/null; then
    RUSTSP_VERSION=$(rustsp --version 2>/dev/null || echo "v1.0.0")
    echo -e "${GREEN}✓ RustS+ siap digunakan: $RUSTSP_VERSION${NC}"
    echo ""
    echo "Coba jalankan: rustsp --help"
else
    echo -e "${YELLOW}⚠ rustsp belum accessible dari PATH${NC}"
    if [ "$INSTALL_PATH" != "/usr/local/bin" ]; then
        echo -e "${YELLOW}  Tambahkan ke PATH kamu:${NC}"
        echo -e "${YELLOW}  export PATH=\"$INSTALL_PATH:\$PATH\"${NC}"
    fi
fi

echo ""
echo -e "${YELLOW}📖 Documentation: https://github.com/yourusername/rustsp${NC}"