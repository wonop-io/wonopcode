#!/usr/bin/env bash
# CI pipeline - runs all checks that would run in CI
set -euo pipefail

echo "========================================"
echo "  Wonopcode CI Check"
echo "========================================"
echo ""

# Track failures
FAILED=0

# Format check
echo "=== [1/3] Checking format ==="
if cargo fmt --all -- --check; then
    echo "Format: PASSED"
else
    echo "Format: FAILED"
    FAILED=1
fi
echo ""

# Clippy
echo "=== [2/3] Running clippy ==="
if cargo clippy --all-targets --all-features; then
    echo "Clippy: PASSED"
else
    echo "Clippy: FAILED"
    FAILED=1
fi
echo ""

# Tests
echo "=== [3/3] Running tests ==="
if cargo test; then
    echo "Tests: PASSED"
else
    echo "Tests: FAILED"
    FAILED=1
fi
echo ""

# Summary
echo "========================================"
if [ $FAILED -eq 0 ]; then
    echo "  All CI checks PASSED"
    echo "========================================"
    exit 0
else
    echo "  Some CI checks FAILED"
    echo "========================================"
    exit 1
fi
