#!/usr/bin/env bash
# Run clippy linter
set -euo pipefail

MODE="${1:-check}"

case "$MODE" in
    check)
        echo "Running clippy..."
        cargo clippy --all-targets --all-features
        ;;
    strict)
        echo "Running clippy (strict - warnings as errors)..."
        cargo clippy --all-targets --all-features -- -D warnings
        ;;
    fix)
        echo "Running clippy with auto-fix..."
        cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged
        ;;
    *)
        echo "Usage: $0 [check|fix|strict]"
        echo "  check  - Run clippy (warnings allowed)"
        echo "  strict - Run clippy with warnings as errors"
        echo "  fix    - Auto-fix clippy suggestions"
        exit 1
        ;;
esac
