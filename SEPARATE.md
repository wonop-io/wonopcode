# TUI Crate Separation Plan

This document outlines the strategy for splitting `wonopcode-tui` (~24,000 lines) into smaller, focused crates.

## Final Status: COMPLETE ✅

The TUI has been successfully split into 5 focused sub-crates plus the main crate.

### Crates Created

| Crate | Lines | Description |
|-------|-------|-------------|
| `wonopcode-tui-core` | ~2,400 | Theme, keybind, event, metrics, model_state |
| `wonopcode-tui-render` | ~2,700 | Markdown, syntax highlighting, diff rendering |
| `wonopcode-tui-widgets` | ~4,200 | Basic UI widgets (input, sidebar, footer, etc.) |
| `wonopcode-tui-dialog` | ~5,700 | Modal dialogs (settings, git, command palette, etc.) |
| `wonopcode-tui-messages` | ~3,000 | Message display widget |
| `wonopcode-tui` | ~4,300 | Main app (app.rs, backend.rs, re-exports) |
| **Total** | **~22,300** | |

### Line Count Summary
- **Before**: ~24,000 lines in single crate
- **After**: 6 crates, largest is 5,700 lines (`wonopcode-tui-dialog`)
- **Main crate reduced**: From ~24,000 to ~4,300 lines

### Architecture

```
                    ┌─────────────────────┐
                    │   wonopcode-tui     │ (main app: 4,256 lines)
                    │   app.rs + backend  │
                    └──────────┬──────────┘
                               │
         ┌─────────────────────┼─────────────────────┐
         │                     │                     │
         ▼                     ▼                     ▼
┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐
│wonopcode-tui-    │ │wonopcode-tui-    │ │wonopcode-tui-    │
│dialog (5,703)    │ │messages (3,037)  │ │widgets (4,200)   │
└────────┬─────────┘ └────────┬─────────┘ └────────┬─────────┘
         │                     │                     │
         └─────────────────────┼─────────────────────┘
                               │
                               ▼
                    ┌──────────────────┐
                    │wonopcode-tui-    │
                    │render (2,700)    │
                    │(md/syntax/diff)  │
                    └────────┬─────────┘
                               │
                               ▼
                    ┌──────────────────┐
                    │wonopcode-tui-    │
                    │core (2,400)      │
                    │(theme/keybind/   │
                    │ event/metrics)   │
                    └──────────────────┘
```

### Tests
- All workspace tests pass
- 52+ tests across TUI crates

### Re-export Pattern
The main `wonopcode-tui` crate re-exports from sub-crates for backwards compatibility:
```rust
// In wonopcode-tui/src/widgets/mod.rs
pub mod dialog {
    pub use wonopcode_tui_dialog::*;
}
pub mod messages {
    pub use wonopcode_tui_messages::*;
}
// etc.
```

---

## Original Analysis (Archived)

### Initial File Sizes
| File | Lines | Category |
|------|-------|----------|
| app.rs | 3,387 | Core Application |
| widgets/messages.rs | 3,037 | Rendering |
| widgets/dialog/settings.rs | 2,390 | Dialogs |
| widgets/input.rs | 1,736 | Widgets |
| widgets/markdown.rs | 970 | Rendering |
| widgets/sidebar.rs | 931 | Widgets |
| widgets/syntax.rs | 878 | Rendering |
| widgets/diff.rs | 876 | Rendering |
| theme.rs | 856 | Core |
| backend.rs | 726 | Backend |
| metrics.rs | 720 | Core |
| widgets/dialog/git.rs | 656 | Dialogs |
| keybind.rs | 574 | Core |
| Other widgets | ~6,000 | Various |
| **Total** | **~24,000** | |

---

## Benefits Achieved

1. **Smaller crates**: Largest crate is now 5,700 lines (vs 24,000 before)
2. **Faster compilation**: Changes to dialogs don't recompile rendering code
3. **Better testing**: Each crate can be tested in isolation
4. **Clearer ownership**: Each crate has a focused purpose
5. **Reusability**: Core and render crates could be used by other projects
