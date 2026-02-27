#!/usr/bin/env bash
# nudl development environment setup script
# Installs LLVM 18, Polly, and all dependencies needed to build nudl.
#
# Usage:
#   chmod +x setup.sh
#   ./setup.sh
#
# Supports: Ubuntu/Debian, Fedora/RHEL, macOS (Homebrew)

set -euo pipefail

echo "=== nudl development setup ==="

# Detect OS
if [[ -f /etc/os-release ]]; then
    . /etc/os-release
    OS_ID="${ID:-unknown}"
    OS_LIKE="${ID_LIKE:-$OS_ID}"
elif [[ "$(uname)" == "Darwin" ]]; then
    OS_ID="macos"
    OS_LIKE="macos"
else
    OS_ID="unknown"
    OS_LIKE="unknown"
fi

echo "Detected OS: $OS_ID ($OS_LIKE)"

install_ubuntu_debian() {
    echo ""
    echo "--- Installing LLVM 18 + dev headers + Polly (Ubuntu/Debian) ---"

    # Add LLVM apt repository for latest packages
    sudo apt-get update -y
    sudo apt-get install -y wget lsb-release software-properties-common gnupg

    # Install LLVM 18 from apt (Ubuntu 24.04+ has it in universe)
    sudo apt-get install -y \
        llvm-18 \
        llvm-18-dev \
        libllvm18 \
        libpolly-18-dev \
        clang-18 \
        lld-18 \
        zlib1g-dev \
        libzstd-dev \
        build-essential \
        pkg-config

    # Set LLVM config path for llvm-sys crate
    LLVM_PREFIX="/usr/lib/llvm-18"
    echo ""
    echo "LLVM 18 installed at: $LLVM_PREFIX"
    echo ""
    echo "Add to your shell profile:"
    echo "  export LLVM_SYS_181_PREFIX=$LLVM_PREFIX"
    export LLVM_SYS_181_PREFIX="$LLVM_PREFIX"
}

install_fedora_rhel() {
    echo ""
    echo "--- Installing LLVM 18 + dev headers + Polly (Fedora/RHEL) ---"

    sudo dnf install -y \
        llvm18 \
        llvm18-devel \
        llvm18-libs \
        clang18 \
        lld18 \
        polly18-devel \
        zlib-devel \
        libzstd-devel \
        gcc \
        gcc-c++ \
        make

    LLVM_PREFIX="/usr"
    echo ""
    echo "LLVM 18 installed."
    echo "Add to your shell profile:"
    echo "  export LLVM_SYS_181_PREFIX=$LLVM_PREFIX"
    export LLVM_SYS_181_PREFIX="$LLVM_PREFIX"
}

install_macos() {
    echo ""
    echo "--- Installing LLVM 18 + Polly via Homebrew (macOS) ---"

    if ! command -v brew &>/dev/null; then
        echo "ERROR: Homebrew not found. Install it from https://brew.sh"
        exit 1
    fi

    brew install llvm@18

    LLVM_PREFIX="$(brew --prefix llvm@18)"
    echo ""
    echo "LLVM 18 installed at: $LLVM_PREFIX"
    echo ""
    echo "Add to your shell profile:"
    echo "  export LLVM_SYS_181_PREFIX=$LLVM_PREFIX"
    export LLVM_SYS_181_PREFIX="$LLVM_PREFIX"
}

# Install Rust if not present
install_rust() {
    if command -v rustc &>/dev/null; then
        echo "Rust already installed: $(rustc --version)"
    else
        echo ""
        echo "--- Installing Rust ---"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
        echo "Rust installed: $(rustc --version)"
    fi
}

# Main install
case "$OS_ID" in
    ubuntu|debian|pop|linuxmint)
        install_ubuntu_debian
        ;;
    fedora|rhel|centos|rocky|alma)
        install_fedora_rhel
        ;;
    macos)
        install_macos
        ;;
    *)
        if [[ "$OS_LIKE" == *"debian"* ]]; then
            install_ubuntu_debian
        elif [[ "$OS_LIKE" == *"fedora"* ]] || [[ "$OS_LIKE" == *"rhel"* ]]; then
            install_fedora_rhel
        else
            echo "ERROR: Unsupported OS: $OS_ID"
            echo "Manually install LLVM 18 with development headers and set LLVM_SYS_181_PREFIX."
            exit 1
        fi
        ;;
esac

install_rust

echo ""
echo "--- Verifying build ---"
echo "LLVM_SYS_181_PREFIX=${LLVM_SYS_181_PREFIX:-<not set>}"

if cargo check --workspace 2>&1 | tail -1 | grep -q "Finished"; then
    echo ""
    echo "=== Setup complete! nudl builds successfully. ==="
else
    echo ""
    echo "Build check had issues. Make sure LLVM_SYS_181_PREFIX is set correctly."
    echo "Try: export LLVM_SYS_181_PREFIX=/usr/lib/llvm-18"
    echo "Then: cargo build --workspace"
fi

echo ""
echo "Quick start:"
echo "  cargo build                     # Build nudl-cli and nudl-lsp"
echo "  cargo test --workspace          # Run all tests"
echo "  cargo run --bin nudl-cli        # Run the CLI"
