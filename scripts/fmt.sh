#!/usr/bin/env bash
# Format code or check formatting
set -euo pipefail

MODE="${1:-fix}"

case "$MODE" in
    check)
        echo "Checking format..."
        cargo fmt --all -- --check
        ;;
    fix)
        echo "Formatting code..."
        cargo fmt --all
        ;;
    *)
        echo "Usage: $0 [check|fix]"
        exit 1
        ;;
esac
