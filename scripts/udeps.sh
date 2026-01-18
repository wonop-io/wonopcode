#!/usr/bin/env bash
# Check for unused dependencies using cargo-udeps
set -euo pipefail

# Check if cargo-udeps is installed
if ! cargo +nightly udeps --version &> /dev/null; then
    echo "cargo-udeps is not installed. Install it with:"
    echo "  cargo install cargo-udeps"
    echo ""
    echo "You also need the nightly toolchain:"
    echo "  rustup install nightly"
    exit 1
fi

echo "Checking for unused dependencies..."
cargo +nightly udeps --all-targets
