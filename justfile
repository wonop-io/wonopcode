# Wonopcode Development Commands
# Run `just --list` to see all available commands

# Default recipe: run checks
default: check

# === Building ===

# Build in debug mode
build:
    cargo build

# Build in release mode
release:
    cargo build --release

# Build with all features
build-all:
    cargo build --all-features

# === Testing ===

# Run all tests
test:
    cargo test

# Run tests with output
test-verbose:
    cargo test -- --nocapture

# Run a specific test
test-one NAME:
    cargo test {{NAME}} -- --nocapture

# Run tests for a specific crate
test-crate CRATE:
    cargo test -p {{CRATE}}

# === Code Coverage ===

# Generate code coverage report (requires cargo-llvm-cov)
coverage:
    cargo llvm-cov --all-features --workspace \
        --ignore-filename-regex '(tests/|test\.rs|mock\.rs)'

# Generate coverage report as HTML
coverage-html:
    cargo llvm-cov --all-features --workspace \
        --ignore-filename-regex '(tests/|test\.rs|mock\.rs)' \
        --html --output-dir coverage

# Generate coverage report as LCOV for CI
coverage-lcov:
    cargo llvm-cov --all-features --workspace \
        --ignore-filename-regex '(tests/|test\.rs|mock\.rs)' \
        --lcov --output-path lcov.info

# Open coverage report in browser
coverage-open: coverage-html
    open coverage/html/index.html

# Show coverage statistics summary per crate
# Usage: just covstats [--sort crate|lines|covered|coverage|status]
covstats *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    
    # Parse arguments
    SORT_BY="crate"  # Default sort
    for arg in {{ARGS}}; do
        case "$arg" in
            --sort)
                shift_next=true
                ;;
            crate|lines|covered|coverage|status)
                if [[ "${shift_next:-false}" == "true" ]]; then
                    SORT_BY="$arg"
                    shift_next=false
                fi
                ;;
            --sort=*)
                SORT_BY="${arg#--sort=}"
                ;;
            -h|--help)
                echo "Usage: just covstats [--sort <field>]"
                echo ""
                echo "Sort options:"
                echo "  crate     - Sort by crate name (default)"
                echo "  lines     - Sort by total lines (descending)"
                echo "  covered   - Sort by covered lines (descending)"
                echo "  coverage  - Sort by coverage percentage (descending)"
                echo "  status    - Sort by status (worst first: 🔴 → 🟠 → 🟡 → ✅)"
                exit 0
                ;;
        esac
    done
    
    echo "📊 Running tests and collecting coverage..."
    echo ""
    
    # Run coverage once and save the output
    COVERAGE_OUTPUT=$(cargo llvm-cov --all-features --workspace \
        --ignore-filename-regex '(tests/|test\.rs|mock\.rs)' 2>&1)
    
    # Collect all crate data into a temp file for sorting
    TEMP_DATA=$(mktemp)
    trap "rm -f $TEMP_DATA" EXIT
    
    # Get unique crate names and process each
    echo "$COVERAGE_OUTPUT" | grep -E "^wonop(code)?[a-z-]*/src" | \
        sed 's|/src/.*||' | sort -u | \
    while read -r crate; do
        # Sum up lines for this crate
        CRATE_DATA=$(echo "$COVERAGE_OUTPUT" | grep "^${crate}/src" | \
            awk '{total+=$8; missed+=$9} END {
                if(total>0) {
                    covered = total - missed;
                    pct = (covered/total)*100;
                    printf "%d %d %.2f", total, covered, pct;
                } else {
                    print "0 0 0";
                }
            }')
        
        TOTAL_LINES=$(echo "$CRATE_DATA" | awk '{print $1}')
        COVERED=$(echo "$CRATE_DATA" | awk '{print $2}')
        PCT=$(echo "$CRATE_DATA" | awk '{print $3}')
        
        # Determine status (numeric for sorting: 1=red, 2=orange, 3=yellow, 4=green)
        if (( $(echo "$PCT >= 90" | bc -l) )); then
            STATUS_NUM=4
            STATUS="✅"
        elif (( $(echo "$PCT >= 70" | bc -l) )); then
            STATUS_NUM=3
            STATUS="🟡"
        elif (( $(echo "$PCT >= 50" | bc -l) )); then
            STATUS_NUM=2
            STATUS="🟠"
        else
            STATUS_NUM=1
            STATUS="🔴"
        fi
        
        # Output: crate|lines|covered|coverage|status_num|status_emoji
        echo "${crate}|${TOTAL_LINES}|${COVERED}|${PCT}|${STATUS_NUM}|${STATUS}" >> "$TEMP_DATA"
    done
    
    # Sort the data based on the selected field
    case "$SORT_BY" in
        crate)
            SORTED_DATA=$(sort -t'|' -k1 "$TEMP_DATA")
            ;;
        lines)
            SORTED_DATA=$(sort -t'|' -k2 -rn "$TEMP_DATA")
            ;;
        covered)
            SORTED_DATA=$(sort -t'|' -k3 -rn "$TEMP_DATA")
            ;;
        coverage)
            SORTED_DATA=$(sort -t'|' -k4 -rn "$TEMP_DATA")
            ;;
        status)
            # Sort by status (ascending = worst first), then by coverage (ascending)
            SORTED_DATA=$(sort -t'|' -k5 -n -k4 -n "$TEMP_DATA")
            ;;
        *)
            echo "Unknown sort field: $SORT_BY"
            echo "Valid options: crate, lines, covered, coverage, status"
            exit 1
            ;;
    esac
    
    # Print header
    echo "╔══════════════════════════════════════════════════════════════════════╗"
    echo "║                    WONOPCODE COVERAGE SUMMARY                        ║"
    echo "╠══════════════════════════════════════════════════════════════════════╣"
    echo "║ Crate                      │ Lines    │ Covered  │ Coverage │ Status ║"
    echo "╠════════════════════════════╪══════════╪══════════╪══════════╪════════╣"
    
    # Print sorted rows
    echo "$SORTED_DATA" | while IFS='|' read -r crate lines covered pct status_num status; do
        CRATE_FMT=$(printf "%-26s" "$crate")
        LINES_FMT=$(printf "%8d" "$lines")
        COV_FMT=$(printf "%8d" "$covered")
        PCT_FMT=$(printf "%7.2f%%" "$pct")
        echo "║ ${CRATE_FMT} │ ${LINES_FMT} │ ${COV_FMT} │ ${PCT_FMT} │   ${status}   ║"
    done
    
    echo "╠════════════════════════════╪══════════╪══════════╪══════════╪════════╣"
    
    # Parse total line
    TOTAL_LINE=$(echo "$COVERAGE_OUTPUT" | grep "^TOTAL")
    TOTAL_LINES=$(echo "$TOTAL_LINE" | awk '{print $8}')
    MISSED=$(echo "$TOTAL_LINE" | awk '{print $9}')
    COVERED=$((TOTAL_LINES - MISSED))
    PCT=$(echo "$TOTAL_LINE" | awk '{print $10}' | tr -d '%')
    
    if (( $(echo "$PCT >= 90" | bc -l) )); then
        STATUS="✅"
    elif (( $(echo "$PCT >= 70" | bc -l) )); then
        STATUS="🟡"
    else
        STATUS="🔴"
    fi
    
    LINES_FMT=$(printf "%8d" "$TOTAL_LINES")
    COV_FMT=$(printf "%8d" "$COVERED")
    PCT_FMT=$(printf "%7.2f%%" "$PCT")
    
    echo "║ TOTAL                      │ ${LINES_FMT} │ ${COV_FMT} │ ${PCT_FMT} │   ${STATUS}   ║"
    echo "╚══════════════════════════════════════════════════════════════════════╝"
    echo ""
    echo "Legend: ✅ ≥90% (target) │ 🟡 ≥70% │ 🟠 ≥50% │ 🔴 <50%"
    echo ""
    echo "Sorted by: $SORT_BY | Target: 90% coverage"

# === Linting & Formatting ===

# Run all checks (format, lint, test)
check: fmt-check lint test
    @echo "All checks passed!"

# Run strict checks (for CI - warnings as errors)
check-strict: fmt-check lint-strict test
    @echo "All strict checks passed!"

# Run clippy linter
lint:
    cargo clippy --all-targets --all-features

# Run clippy linter (strict - warnings as errors)
lint-strict:
    cargo clippy --all-targets --all-features -- -D warnings

# Run clippy and fix what it can
lint-fix:
    cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Format all code
fmt:
    cargo fmt --all

# === Dependency Management ===

# Check for unused dependencies (requires cargo-udeps)
udeps:
    @echo "Running cargo-udeps (requires nightly)..."
    cargo +nightly udeps --all-targets

# Check dependencies for issues (requires cargo-deny)
deny:
    cargo deny check

# Check for outdated dependencies
outdated:
    cargo outdated -R

# Update dependencies
update:
    cargo update

# Audit dependencies for security vulnerabilities
audit:
    cargo audit

# === Cleaning ===

# Clean build artifacts
clean:
    cargo clean

# Clean and rebuild
rebuild: clean build

# Clean coverage artifacts
clean-coverage:
    rm -rf coverage lcov.info

# === Documentation ===

# Generate documentation
doc:
    cargo doc --no-deps --all-features

# Generate and open documentation
doc-open:
    cargo doc --no-deps --all-features --open

# === Development Helpers ===

# Watch for changes and run tests
watch-test:
    cargo watch -x test

# Watch for changes and run clippy
watch-lint:
    cargo watch -x clippy

# Run the application
run *ARGS:
    cargo run -- {{ARGS}}

# Run in release mode
run-release *ARGS:
    cargo run --release -- {{ARGS}}

# === CI/Pre-commit ===

# Full CI check (what CI would run)
ci: fmt-check lint-strict test deny udeps
    @echo "CI checks passed!"

# Pre-commit hook check
pre-commit: fmt-check lint
    @echo "Pre-commit checks passed!"

# === Installation ===

# Install development tools
install-tools:
    @echo "Installing development tools..."
    cargo install cargo-watch
    cargo install cargo-udeps
    cargo install cargo-outdated
    cargo install cargo-audit
    cargo install cargo-deny
    cargo install cargo-llvm-cov
    @echo "Tools installed!"

# Install the application locally
install:
    cargo install --path crates/wonopcode --locked

# === Profiling & Analysis ===

# Show dependency tree
tree:
    cargo tree

# Show duplicate dependencies
tree-dupes:
    cargo tree --duplicate

# Count lines of code (requires tokei)
loc:
    tokei --exclude target

# Show code complexity metrics
complexity:
    cargo clippy --all-targets --all-features -- -W clippy::cognitive_complexity 2>&1 | grep -E "cognitive_complexity|warning:"

# === Release ===

# Prepare a release build with optimizations
release-build:
    cargo build --release --locked

# Check that the release builds correctly
release-check:
    cargo check --release --locked
