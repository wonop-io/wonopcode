# Coverage Plan: Achieving 90% with UX-Breaking Tests

## Overview

This plan focuses on writing tests that **break the user experience** when they fail. These are not just unit tests for code coverage, but tests that verify critical user-facing functionality.

## Current State (2026-01-17)

| Crate | Lines | Covered | Coverage | Status |
|-------|-------|---------|----------|--------|
| wonopcode | 7,465 | 388 | 5.20% | 🔴 |
| wonopcode-acp | 2,080 | 800 | 38.46% | 🔴 |
| wonopcode-auth | 322 | 290 | **90.06%** | ✅ |
| wonopcode-core | 9,158 | 8,299 | **90.62%** | ✅ |
| wonopcode-discover | 1,858 | 1,634 | 87.94% | 🟡 |
| wonopcode-lsp | 1,279 | 411 | 32.13% | 🔴 |
| wonopcode-mcp | 3,191 | 2,715 | 85.08% | 🟡 |
| wonopcode-protocol | 432 | 429 | **99.31%** | ✅ |
| wonopcode-provider | 4,604 | 1,755 | 38.12% | 🔴 |
| wonopcode-sandbox | 1,879 | 587 | 31.24% | 🔴 |
| wonopcode-server | 6,363 | 3,457 | 54.33% | 🟠 |
| wonopcode-snapshot | 718 | 685 | **95.40%** | ✅ |
| wonopcode-storage | 411 | 402 | **97.81%** | ✅ |
| wonopcode-test-utils | 1,787 | 1,623 | **90.82%** | ✅ |
| wonopcode-tools | 8,495 | 7,661 | **90.18%** | ✅ |
| wonopcode-tui | 2,598 | 0 | 0.00% | 🔴 |
| wonopcode-tui-core | 1,523 | 605 | 39.72% | 🔴 |
| wonopcode-tui-dialog | 3,533 | 0 | 0.00% | 🔴 |
| wonopcode-tui-messages | 2,055 | 243 | 11.82% | 🔴 |
| wonopcode-tui-render | 1,836 | 946 | 51.53% | 🟠 |
| wonopcode-tui-widgets | 3,552 | 844 | 23.76% | 🔴 |
| wonopcode-util | 1,633 | 1,490 | **91.24%** | ✅ |
| **TOTAL** | **66,772** | **35,264** | **52.81%** | 🔴 |

**Legend**: ✅ ≥90% (target) │ 🟡 ≥70% │ 🟠 ≥50% │ 🔴 <50%

### Crates at 90%+ (9 crates) ✅
- wonopcode-auth (90.06%)
- wonopcode-core (90.62%)
- wonopcode-protocol (99.31%)
- wonopcode-snapshot (95.40%)
- wonopcode-storage (97.81%)
- wonopcode-test-utils (90.82%)
- wonopcode-tools (90.18%)
- wonopcode-util (91.24%)

### Crates Needing Tests (13 crates)

#### High Priority (Large codebases, low coverage)
| Crate | Lines | Gap to 90% | Priority |
|-------|-------|------------|----------|
| wonopcode | 7,465 | ~6,300 lines | P0 |
| wonopcode-server | 6,363 | ~2,300 lines | P0 |
| wonopcode-provider | 4,604 | ~2,400 lines | P1 |
| wonopcode-acp | 2,080 | ~1,100 lines | P1 |

#### TUI Crates (New - need test infrastructure)
| Crate | Lines | Gap to 90% | Priority |
|-------|-------|------------|----------|
| wonopcode-tui-dialog | 3,533 | ~3,200 lines | P2 |
| wonopcode-tui-widgets | 3,552 | ~2,400 lines | P2 |
| wonopcode-tui | 2,598 | ~2,300 lines | P2 |
| wonopcode-tui-messages | 2,055 | ~1,600 lines | P2 |
| wonopcode-tui-core | 1,523 | ~800 lines | P2 |
| wonopcode-tui-render | 1,836 | ~700 lines | P2 |

#### Other Crates
| Crate | Lines | Gap to 90% | Priority |
|-------|-------|------------|----------|
| wonopcode-sandbox | 1,879 | ~1,100 lines | P1 |
| wonopcode-discover | 1,858 | ~40 lines | P3 |
| wonopcode-mcp | 3,191 | ~160 lines | P3 |
| wonopcode-lsp | 1,279 | ~740 lines | P2 |

---

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
8. **TUI Rendering** - UI is broken or unreadable

---

## Priority Matrix

| Crate | Lines | Coverage | UX Impact | Priority |
|-------|-------|----------|-----------|----------|
| wonopcode | 7,465 | 5.20% | Runner, Main loop | P0 |
| wonopcode-server | 6,363 | 54.33% | Git operations, API | P0 |
| wonopcode-provider | 4,604 | 38.12% | AI responses | P1 |
| wonopcode-acp | 2,080 | 38.46% | Agent protocol | P1 |
| wonopcode-sandbox | 1,879 | 31.24% | Secure execution | P1 |
| wonopcode-tui-dialog | 3,533 | 0.00% | Settings, Git UI | P2 |
| wonopcode-tui-widgets | 3,552 | 23.76% | Input, Sidebar | P2 |
| wonopcode-tui | 2,598 | 0.00% | Main app, Backend | P2 |
| wonopcode-tui-messages | 2,055 | 11.82% | Message display | P2 |
| wonopcode-tui-core | 1,523 | 39.72% | Theme, Keybinds | P2 |
| wonopcode-tui-render | 1,836 | 51.53% | Markdown, Syntax | P2 |
| wonopcode-lsp | 1,279 | 32.13% | Code intelligence | P2 |
| wonopcode-discover | 1,858 | 87.94% | Service discovery | P3 |
| wonopcode-mcp | 3,191 | 85.08% | MCP tools | P3 |

---

## Test Categories by UX Impact

### P0: Critical Path (Must not break)

#### 1. Main Application (wonopcode)
```
UX Impact: App doesn't start or crashes
Files: wonopcode/src/*.rs
Current: 5.20% coverage
Tests needed:
- [ ] Runner initialization
- [ ] Command parsing
- [ ] Session management
- [ ] Compaction logic
- [ ] GitHub integration
- [ ] Upgrade flow
```

#### 2. Server/API (wonopcode-server)
```
UX Impact: Git operations fail, API errors
Files: wonopcode-server/src/*.rs
Current: 54.33% coverage
Tests needed:
- [ ] Git status endpoint
- [ ] Git diff endpoint
- [ ] Git commit endpoint
- [ ] Session routes
- [ ] Error handling
```

### P1: Important Functionality

#### 3. Provider Integration (wonopcode-provider)
```
UX Impact: No AI responses
Files: wonopcode-provider/src/*.rs
Current: 38.12% coverage
Tests needed:
- [ ] Build request correctly
- [ ] Parse streaming response
- [ ] Handle rate limits
- [ ] Handle API errors
- [ ] Token counting
```

#### 4. Agent Protocol (wonopcode-acp)
```
UX Impact: Agent communication fails
Files: wonopcode-acp/src/*.rs
Current: 38.46% coverage
Tests needed:
- [ ] Session management
- [ ] Transport layer
- [ ] Message serialization
- [ ] Type conversions
```

#### 5. Sandbox Execution (wonopcode-sandbox)
```
UX Impact: Commands don't run in container
Files: wonopcode-sandbox/src/*.rs
Current: 31.24% coverage
Tests needed:
- [ ] Start sandbox container
- [ ] Execute command in sandbox
- [ ] Copy files to/from sandbox
- [ ] Handle sandbox failures
```

### P2: TUI Components

#### 6. TUI Dialogs (wonopcode-tui-dialog)
```
UX Impact: Settings/Git dialogs broken
Files: wonopcode-tui-dialog/src/*.rs
Current: 0.00% coverage
Tests needed:
- [ ] Settings dialog rendering
- [ ] Settings dialog navigation
- [ ] Git dialog rendering
- [ ] Command palette
- [ ] Permission dialog
- [ ] Input validation
```

#### 7. TUI Widgets (wonopcode-tui-widgets)
```
UX Impact: Input/sidebar/footer broken
Files: wonopcode-tui-widgets/src/*.rs
Current: 23.76% coverage
Tests needed:
- [ ] Input widget text handling
- [ ] Input widget cursor movement
- [ ] Sidebar rendering
- [ ] Footer status display
- [ ] Toast notifications
- [ ] Search widget
```

#### 8. TUI Main App (wonopcode-tui)
```
UX Impact: App/backend communication fails
Files: wonopcode-tui/src/*.rs
Current: 0.00% coverage
Tests needed:
- [ ] App state management
- [ ] Event handling
- [ ] Backend message routing
- [ ] Terminal setup/teardown
```

#### 9. TUI Messages (wonopcode-tui-messages)
```
UX Impact: Messages don't display correctly
Files: wonopcode-tui-messages/src/*.rs
Current: 11.82% coverage
Tests needed:
- [ ] Message rendering
- [ ] Code block display
- [ ] Tool call display
- [ ] Scroll handling
- [ ] Line wrapping
```

#### 10. TUI Core (wonopcode-tui-core)
```
UX Impact: Theme/keybinds don't work
Files: wonopcode-tui-core/src/*.rs
Current: 39.72% coverage
Tests needed:
- [ ] Theme loading
- [ ] Color parsing
- [ ] Keybind parsing
- [ ] Event handling
- [ ] Metrics collection
```

#### 11. TUI Render (wonopcode-tui-render)
```
UX Impact: Markdown/code not highlighted
Files: wonopcode-tui-render/src/*.rs
Current: 51.53% coverage
Tests needed:
- [ ] Markdown parsing
- [ ] Syntax highlighting
- [ ] Diff rendering
- [ ] Code region detection
```

#### 12. LSP Integration (wonopcode-lsp)
```
UX Impact: Code intelligence broken
Files: wonopcode-lsp/src/*.rs
Current: 32.13% coverage
Tests needed:
- [ ] LSP server startup
- [ ] Go to definition
- [ ] Find references
- [ ] Hover information
```

### P3: Near Target (Just needs polish)

#### 13. Service Discovery (wonopcode-discover)
```
UX Impact: Can't find local servers
Files: wonopcode-discover/src/*.rs
Current: 87.94% coverage (2.06% to target)
Tests needed:
- [ ] A few edge cases for browse/advertise
```

#### 14. MCP Integration (wonopcode-mcp)
```
UX Impact: External tools don't work
Files: wonopcode-mcp/src/*.rs
Current: 85.08% coverage (4.92% to target)
Tests needed:
- [ ] A few edge cases for OAuth, transport
```

---

## Execution Plan

### Phase 1: Critical Path (P0)
**Goal**: Get wonopcode and wonopcode-server to 90%

1. wonopcode-server (54% → 90%)
   - Focus on route handlers
   - Git operation tests

2. wonopcode (5% → 90%)
   - Runner tests
   - Command tests

### Phase 2: Important Features (P1)
**Goal**: Get provider, acp, sandbox to 90%

1. wonopcode-provider (38% → 90%)
2. wonopcode-acp (38% → 90%)
3. wonopcode-sandbox (31% → 90%)

### Phase 3: TUI Components (P2)
**Goal**: Get all TUI crates to 90%

1. wonopcode-tui-render (52% → 90%) - smallest gap
2. wonopcode-tui-core (40% → 90%)
3. wonopcode-tui-widgets (24% → 90%)
4. wonopcode-tui-messages (12% → 90%)
5. wonopcode-tui-dialog (0% → 90%)
6. wonopcode-tui (0% → 90%)
7. wonopcode-lsp (32% → 90%)

### Phase 4: Polish (P3)
**Goal**: Get discover and mcp to 90%

1. wonopcode-discover (88% → 90%)
2. wonopcode-mcp (85% → 90%)

---

## TUI Testing Strategy

TUI components require special testing approaches since they deal with terminal rendering.

### Approach 1: Logic Testing (Preferred)
Test the business logic separate from rendering:
```rust
#[test]
fn input_widget_handles_backspace() {
    let mut input = InputWidget::new();
    input.insert("hello");
    input.handle_key(KeyCode::Backspace);
    assert_eq!(input.text(), "hell");
}
```

### Approach 2: Snapshot Testing
Use insta or similar for rendered output:
```rust
#[test]
fn message_widget_renders_code_block() {
    let widget = MessagesWidget::new();
    widget.add_message(Message::with_code("rust", "fn main() {}"));
    let buffer = render_to_buffer(&widget, 80, 24);
    insta::assert_snapshot!(buffer.to_string());
}
```

### Approach 3: Mock Terminal
Test with a mock terminal backend:
```rust
#[test]
fn app_handles_resize() {
    let mut app = App::new(MockBackend::new(80, 24));
    app.handle_event(Event::Resize(100, 30));
    assert_eq!(app.size(), (100, 30));
}
```

---

## Commands

```bash
# Run all tests with coverage
just covstats

# Run tests for specific crate
cargo test -p wonopcode-tui-core

# Generate HTML coverage report
just coverage-html

# Watch tests during development
cargo watch -x "test -p wonopcode-tui-core"
```

---

## Success Criteria

- [ ] 90% line coverage overall
- [ ] All P0 crates at 90%+
- [ ] All P1 crates at 90%+
- [ ] All TUI crates at 90%+
- [ ] No regressions in existing functionality
- [ ] CI enforces coverage threshold
