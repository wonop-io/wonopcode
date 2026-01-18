# Integration Tests

This directory contains integration tests that exercise multiple crates together.

## Test Organization

- `cli_tests.rs` - End-to-end CLI command tests
- `provider_tests.rs` - Provider integration tests (requires API keys)
- `mcp_tests.rs` - MCP server integration tests
- `session_tests.rs` - Session persistence tests

## Running Tests

```bash
# Run all integration tests
cargo test --test '*'

# Run specific test file
cargo test --test cli_tests

# Run with verbose output
cargo test --test '*' -- --nocapture
```

## Test Environment

Some tests require environment variables:
- `ANTHROPIC_API_KEY` - For Anthropic provider tests
- `OPENAI_API_KEY` - For OpenAI provider tests
- `WONOPCODE_TEST_ENABLED` - Enable slow/expensive tests

## Writing Tests

Integration tests should:
1. Be independent and not rely on external state
2. Clean up any created resources
3. Use temporary directories for file operations
4. Be skipped by default if they require API keys

Example:
```rust
#[test]
fn test_config_loading() {
    let temp = tempfile::tempdir().unwrap();
    // ... test code
}

#[test]
#[ignore] // Requires API key
fn test_provider_connection() {
    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        return;
    }
    // ... test code
}
```
