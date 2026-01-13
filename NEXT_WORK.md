# Next Work - Codebase Improvement Plan

This document outlines areas for improvement identified through a comprehensive codebase analysis.

## 1. Large File Refactoring (High Priority)

Several files are excessively large and should be broken down into smaller, more focused modules:

| File | Lines | Recommendation |
|------|-------|----------------|
| `wonopcode-tui/src/widgets/dialog.rs` | ~5,500 | Split into separate dialog types (SelectDialog, ConfirmDialog, InputDialog, etc.) |
| `wonopcode/src/runner.rs` | ~4,400 | Extract into submodules: doom_loop_detector, provider_factory, sandbox_setup |
| `wonopcode/src/main.rs` | ~4,100 | Extract CLI argument handling, subcommands into separate modules |
| `wonopcode-tui/src/app.rs` | ~3,300 | Split into input_handling, state_management, event_processing |
| `wonopcode-tui/src/widgets/messages.rs` | ~2,700 | Split rendering logic from state management |
| `wonopcode-server/src/routes.rs` | ~2,600 | Group routes by resource (sessions, git, mcp, etc.) |

### Suggested approach for `dialog.rs`:
```
widgets/
  dialog/
    mod.rs           # Re-exports and shared types
    select.rs        # SelectDialog
    confirm.rs       # ConfirmDialog  
    input.rs         # InputDialog
    permission.rs    # PermissionDialog
    model_select.rs  # ModelSelectDialog
```

### Suggested approach for `runner.rs`:
```
runner/
  mod.rs              # Main Runner struct and core logic
  doom_loop.rs        # DoomLoopDetector
  provider.rs         # Provider factory and initialization
  sandbox.rs          # Sandbox setup and management
  compaction.rs       # Auto-compaction logic
```

---

## 2. Test Coverage Gaps (High Priority)

The codebase has inline tests (`#[cfg(test)]`) in ~90 files, but several critical areas lack adequate coverage.

### Missing integration tests

No `tests/` directory exists for integration testing. Create:

```
tests/
  cli_test.rs         # End-to-end CLI workflow tests
  provider_test.rs    # Provider integration tests (mock server)
  mcp_test.rs         # MCP server/client communication tests
  tool_test.rs        # Tool execution integration tests
```

### Crates with minimal test coverage

- `wonopcode-protocol` - Protocol types need serialization/deserialization tests
- `wonopcode-discover` - Browser automation needs mocking tests
- `wonopcode/src/runner.rs` - Core runner logic needs unit tests
- `wonopcode/src/main.rs` - CLI argument parsing needs tests

### Recommended test utilities

Consider adding a `wonopcode-test-utils` crate with:
- Mock providers
- Test fixtures
- Common test helpers

---

## 3. Documentation Improvements (Medium Priority)

### Strengths
- Most `lib.rs` files have excellent module-level documentation with examples
- Good use of `//!` doc comments for module overviews

### Gaps to address

1. **Public function documentation**: Many public functions lack doc comments
2. **Crate README files**: Add `README.md` to each crate explaining:
   - Purpose
   - Usage examples
   - API overview
3. **Missing examples**: Several modules could benefit from `# Examples` sections

### Action items
- Run `cargo doc --document-private-items` to identify undocumented items
- Add `#![warn(missing_docs)]` to crates incrementally
- Create crate-level README files

---

## 4. Error Handling Improvements (Medium Priority)

### Current strengths
- Proper use of `thiserror` across all error modules
- Each crate has dedicated error types
- Good error hierarchy (CoreError, ConfigError, SessionError, etc.)

### Issues to fix

Replace unsafe `.unwrap()` calls in production code:

```rust
// runner.rs:4348 - metadata.unwrap() in non-test code
// claude_cli.rs:420 - config write unwrap
// discover/browse.rs:67 - servers.lock().unwrap()
```

**Recommendation**: Replace with:
- `.expect("clear error message")` for invariants
- Proper `?` propagation for fallible operations
- `if let` / `match` for optional handling

---

## 5. Incomplete Features (Medium Priority)

Several modules are marked as incomplete and should be either completed or removed:

### GitHub Actions Integration
Files with `#![allow(dead_code)]` and TODO comments:
- `crates/wonopcode/src/github/api.rs`
- `crates/wonopcode/src/github/pr.rs`
- `crates/wonopcode/src/github/event.rs`

**Decision needed**: Complete the GitHub Actions integration or remove these files to reduce maintenance burden.

### WebSocket TODOs
- `crates/wonopcode-server/src/ws.rs:126` - Event filtering not implemented
- `crates/wonopcode-server/src/ws.rs:130` - Event filtering not implemented

---

## 6. Dependency Management (Low Priority)

### Current strengths
- Workspace dependencies are well-organized
- Version pinning is consistent
- `deny.toml` present for security auditing

### Minor improvements

Move non-workspace dependencies to workspace for consistency:

```toml
# In wonopcode-provider/Cargo.toml, these should move to workspace:
hmac = "0.12"
hex = "0.4"
urlencoding = "2.1"
```

---

## 7. Code Pattern Standardization (Low Priority)

### Builder pattern inconsistency
Some structs use builder pattern, others use `with_*` methods. Consider standardizing:
- Use `Builder` suffix for builder types
- Use `new()` + `with_*` chain for simple cases
- Use separate `FooBuilder` for complex construction

### Async trait migration
Currently using `async-trait` crate. Consider migrating to native async traits (Rust 1.75+) for:
- Better error messages
- Reduced compile time
- No heap allocation for futures

---

## Summary Priority Matrix

| Priority | Category | Effort | Impact |
|----------|----------|--------|--------|
| **High** | Split large files (dialog.rs, runner.rs, main.rs) | Medium | High maintainability |
| **High** | Add integration test suite | High | High reliability |
| **Medium** | Complete GitHub Actions integration or remove | Low | Cleaner codebase |
| **Medium** | Replace production `.unwrap()` calls | Low | Better error handling |
| **Medium** | Add crate-level README files | Low | Better discoverability |
| **Low** | Standardize builder patterns | Low | Consistency |
| **Low** | Move remaining deps to workspace | Low | Consistency |

---

## Getting Started

Recommended order of work:

1. **Week 1-2**: Split `dialog.rs` into submodules (most complex, highest impact)
2. **Week 3**: Split `runner.rs` into submodules
3. **Week 4**: Add integration test infrastructure
4. **Ongoing**: Address `.unwrap()` calls as encountered
5. **As needed**: Document public APIs incrementally
