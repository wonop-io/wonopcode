#!/usr/bin/env bash
# Install development tools
set -euo pipefail

echo "Installing Rust development tools..."

# Install nightly toolchain for udeps
echo "Installing nightly toolchain..."
rustup install nightly

# Development tools
TOOLS=(
    "cargo-watch"      # Watch for changes and run commands
    "cargo-udeps"      # Find unused dependencies
    "cargo-outdated"   # Find outdated dependencies
    "cargo-audit"      # Security vulnerability scanner
    "cargo-edit"       # Add/remove/upgrade dependencies
    "tokei"            # Count lines of code
    "just"             # Command runner (like make but better)
)

for tool in "${TOOLS[@]}"; do
    echo ""
    echo "Installing $tool..."
    cargo install "$tool" || echo "Warning: Failed to install $tool"
done

echo ""
echo "All tools installed!"
echo ""
echo "Optional: Install 'just' via your package manager for better performance:"
echo "  macOS:  brew install just"
echo "  Linux:  apt install just / dnf install just"
