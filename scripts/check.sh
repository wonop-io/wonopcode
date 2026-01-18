#!/usr/bin/env bash
# Run all checks: format, lint, and test
set -euo pipefail

echo "=== Checking format ==="
cargo fmt --all -- --check

echo ""
echo "=== Running clippy ==="
cargo clippy --all-targets --all-features -- -D warnings

echo ""
echo "=== Running tests ==="
cargo test

echo ""
echo "All checks passed!"
