# Coverage Plan: Achieving 90% with UX-Breaking Tests

## Overview

This plan focuses on writing tests that **break the user experience** when they fail. These are not just unit tests for code coverage, but tests that verify critical user-facing functionality.

## Current State

- **Total Coverage**: ~29.67%
- **Target**: 90%
- **Gap**: ~60%

## Philosophy: UX-Breaking Tests

Every test should answer: **"If this test fails, what user experience breaks?"**

### Categories of UX-Critical Functionality

1. **Session Management** - Users lose their work
2. **Configuration Loading** - App won't start correctly  
3. **Tool Execution** - Core coding assistant functionality
4. **Permission System** - Security-critical decisions
5. **Message Handling** - Conversation doesn't work
6. **Provider Integration** - AI responses fail
7. **File Operations** - Code changes not applied

## Priority Matrix

| Crate | Lines | Current Tests | UX Impact | Priority |
|-------|-------|---------------|-----------|----------|
| wonopcode-core | 9,518 | 51 | Session, Config, Permissions | P0 |
| wonopcode-tools | 9,627 | 34 | All tool execution | P0 |
| wonopcode | 11,976 | 23 | Runner, Main loop | P0 |
| wonopcode-provider | 11,410 | 29 | AI responses | P1 |
| wonopcode-sandbox | 4,473 | 21 | Secure execution | P1 |
| wonopcode-server | 5,207 | 3 | Git operations | P1 |
| wonopcode-mcp | 3,051 | 28 | MCP tools | P2 |
| wonopcode-tui | 24,102 | 57 | UI rendering | P2 |
| wonopcode-auth | 567 | 0 | Authentication | P2 |
| wonopcode-storage | 598 | 0 | Data persistence | P2 |
| wonopcode-snapshot | 741 | 0 | Undo/Redo | P2 |
| wonopcode-protocol | 576 | 0 | Wire protocol | P3 |

## Test Categories by UX Impact

### P0: Critical Path (Must not break)

#### 1. Configuration Loading
```
UX Impact: App fails to start or uses wrong settings
Files: wonopcode-core/src/config.rs
Tests needed:
- [ ] Load valid config from all sources
- [ ] Handle missing config gracefully
- [ ] Variable substitution ({env:VAR})
- [ ] JSONC comment stripping
- [ ] MCP server config parsing
- [ ] Config merge priority (global < project < env)
```

#### 2. Session Management
```
UX Impact: Users lose conversation history
Files: wonopcode-core/src/session.rs
Tests needed:
- [ ] Create new session
- [ ] Load existing session
- [ ] Save session changes
- [ ] List sessions
- [ ] Delete session
- [ ] Handle corrupt session data
```

#### 3. Message Handling
```
UX Impact: Conversation doesn't work
Files: wonopcode-core/src/message.rs
Tests needed:
- [ ] Create user message
- [ ] Create assistant message
- [ ] Serialize/deserialize messages
- [ ] Message parts (text, tool calls, tool results)
- [ ] File diff handling
```

#### 4. Tool Execution - Core Tools
```
UX Impact: Can't read/write/execute code
Files: wonopcode-tools/src/*.rs
Tests needed:
- [ ] Bash: Execute command successfully
- [ ] Bash: Handle timeout
- [ ] Bash: Handle errors
- [ ] Read: Read existing file
- [ ] Read: Handle missing file
- [ ] Read: Block sensitive files
- [ ] Write: Create new file
- [ ] Write: Overwrite existing
- [ ] Edit: Apply edit successfully
- [ ] Edit: Fail on no match
- [ ] Glob: Find files by pattern
- [ ] Grep: Search file contents
```

#### 5. Permission System
```
UX Impact: Security decisions wrong or app hangs
Files: wonopcode-core/src/permission.rs
Tests needed:
- [ ] Check permission for allowed command
- [ ] Check permission for denied command
- [ ] Permission caching
- [ ] Wildcard pattern matching
- [ ] Path normalization
```

### P1: Important Functionality

#### 6. Provider Integration
```
UX Impact: No AI responses
Files: wonopcode-provider/src/*.rs
Tests needed:
- [ ] Build request correctly
- [ ] Parse streaming response
- [ ] Handle rate limits
- [ ] Handle API errors
- [ ] Token counting
```

#### 7. Sandbox Execution
```
UX Impact: Commands don't run in container
Files: wonopcode-sandbox/src/*.rs
Tests needed:
- [ ] Start sandbox container
- [ ] Execute command in sandbox
- [ ] Copy files to/from sandbox
- [ ] Handle sandbox failures
```

#### 8. Git Operations
```
UX Impact: Can't show/commit changes
Files: wonopcode-server/src/*.rs
Tests needed:
- [ ] Get git status
- [ ] Stage files
- [ ] Create commit
- [ ] Get diff
```

### P2: Enhanced Functionality

#### 9. MCP Integration
```
UX Impact: External tools don't work
Files: wonopcode-mcp/src/*.rs
Tests needed:
- [ ] Connect to MCP server
- [ ] Call MCP tool
- [ ] Handle server disconnection
```

#### 10. Authentication
```
UX Impact: Can't use service
Files: wonopcode-auth/src/*.rs
Tests needed:
- [ ] Store token
- [ ] Retrieve token
- [ ] Validate token
- [ ] Handle expired token
```

## Execution Order

### Week 1: P0 Core Infrastructure
1. Configuration tests
2. Session tests  
3. Message tests

### Week 2: P0 Tools
1. Bash tool tests
2. Read tool tests
3. Write/Edit tool tests
4. Glob/Grep tool tests

### Week 3: P1 Integration
1. Provider tests
2. Sandbox tests
3. Git operation tests

### Week 4: P2 Enhancement
1. MCP tests
2. Auth tests
3. Storage tests

## Test Writing Guidelines

### 1. Name tests by UX scenario
```rust
#[test]
fn user_cannot_read_env_files_containing_secrets() { ... }

#[test]
fn session_persists_across_restarts() { ... }
```

### 2. Test error paths
```rust
#[test]
fn bash_returns_helpful_error_when_command_not_found() { ... }
```

### 3. Use the test-utils crate
```rust
use wonopcode_test_utils::{TestProject, MockSandbox};
```

### 4. Test with realistic data
```rust
let project = TestProject::new()
    .with_file("src/main.rs", "fn main() {}")
    .with_file("Cargo.toml", r#"[package]\nname = "test""#)
    .build();
```

## Commands

```bash
# Run all tests with coverage
just covstats

# Run tests for specific crate
just test-crate wonopcode-core

# Generate HTML coverage report
just coverage-html

# Watch tests during development
just watch-test
```

## Success Criteria

- [ ] 90% line coverage
- [ ] All P0 tests passing
- [ ] All P1 tests passing
- [ ] No regressions in existing functionality
- [ ] CI enforces coverage threshold
