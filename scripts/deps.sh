#!/usr/bin/env bash
# Dependency management utilities
set -euo pipefail

CMD="${1:-help}"

case "$CMD" in
    outdated)
        echo "Checking for outdated dependencies..."
        if ! command -v cargo-outdated &> /dev/null; then
            echo "Installing cargo-outdated..."
            cargo install cargo-outdated
        fi
        cargo outdated -R
        ;;
    audit)
        echo "Auditing dependencies for vulnerabilities..."
        if ! command -v cargo-audit &> /dev/null; then
            echo "Installing cargo-audit..."
            cargo install cargo-audit
        fi
        cargo audit
        ;;
    update)
        echo "Updating dependencies..."
        cargo update
        ;;
    tree)
        echo "Dependency tree:"
        cargo tree
        ;;
    dupes)
        echo "Duplicate dependencies:"
        cargo tree --duplicate
        ;;
    help|*)
        echo "Dependency management utilities"
        echo ""
        echo "Usage: $0 <command>"
        echo ""
        echo "Commands:"
        echo "  outdated  - Check for outdated dependencies"
        echo "  audit     - Audit for security vulnerabilities"
        echo "  update    - Update dependencies"
        echo "  tree      - Show dependency tree"
        echo "  dupes     - Show duplicate dependencies"
        ;;
esac
