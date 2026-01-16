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
