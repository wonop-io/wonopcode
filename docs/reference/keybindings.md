# Keybindings

Complete reference for wonopcode keyboard shortcuts.

---

## Input Mode

Default mode when typing messages.

### Basic Input

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Shift+Enter` | New line (multi-line input) |
| `Esc` | Cancel input / Clear line |
| `Tab` | Trigger completion |

### Line Editing

| Key | Action |
|-----|--------|
| `Ctrl+A` | Move to start of line |
| `Ctrl+E` | Move to end of line |
| `Ctrl+B` / `←` | Move back one character |
| `Ctrl+F` / `→` | Move forward one character |
| `Alt+B` | Move back one word |
| `Alt+F` | Move forward one word |

### Text Manipulation

| Key | Action |
|-----|--------|
| `Ctrl+W` | Delete word before cursor |
| `Alt+D` | Delete word after cursor |
| `Ctrl+U` | Delete from cursor to start |
| `Ctrl+K` | Delete from cursor to end |
| `Ctrl+Y` | Paste deleted text (yank) |
| `Ctrl+T` | Transpose characters |

### History

| Key | Action |
|-----|--------|
| `↑` / `Ctrl+P` | Previous input |
| `↓` / `Ctrl+N` | Next input |
| `Ctrl+R` | Search history |

---

## Navigation Mode

When viewing messages (press `Esc` from input).

### Scrolling

| Key | Action |
|-----|--------|
| `↑` / `k` | Scroll up |
| `↓` / `j` | Scroll down |
| `Page Up` / `Ctrl+U` | Page up |
| `Page Down` / `Ctrl+D` | Page down |
| `Home` / `g` | Go to top |
| `End` / `G` | Go to bottom |

### Message Navigation

| Key | Action |
|-----|--------|
| `[` | Previous message |
| `]` | Next message |
| `{` | Previous user message |
| `}` | Next user message |

### Return to Input

| Key | Action |
|-----|--------|
| `i` | Return to input mode |
| `Enter` | Return to input mode |
| `/` | Open command input |

---

## Global Shortcuts

Work in any mode.

### Session Control

| Key | Action |
|-----|--------|
| `Ctrl+C` | Cancel current operation |
| `Ctrl+L` | Clear screen |
| `Ctrl+D` | Quit wonopcode |
| `Ctrl+Q` | Quit wonopcode |
| `Ctrl+Z` | Suspend (background) |

### Leader Key Sequences

Press `Ctrl+X` (leader key) followed by another key:

| Sequence | Action |
|----------|--------|
| `<leader> n` | New session |
| `<leader> l` | List sessions |
| `<leader> m` | List models |
| `<leader> a` | List agents |
| `<leader> u` | Undo last message |
| `<leader> r` | Redo undone message |
| `<leader> z` | Revert to previous message |
| `<leader> Z` | Cancel revert |
| `<leader> c` | Compact conversation |
| `<leader> x` | Export session |
| `<leader> e` | Open external editor |
| `<leader> t` | List themes |
| `<leader> b` | Toggle sidebar |
| `<leader> g` | Show session timeline |
| `<leader> y` | Copy last response |

---

## Permission Prompts

When asked to approve tool execution.

| Key | Action |
|-----|--------|
| `A` / `Y` | Allow (approve) |
| `D` / `N` | Deny (reject) |
| `V` | View details/diff |
| `Esc` | Cancel |

---

## Dialogs

When a dialog is open.

| Key | Action |
|-----|--------|
| `Tab` | Next option |
| `Shift+Tab` | Previous option |
| `Enter` | Select/Confirm |
| `Esc` | Cancel/Close |
| `↑` / `↓` | Navigate list |

---

## Sidebar

When sidebar is focused.

| Key | Action |
|-----|--------|
| `↑` / `k` | Previous session |
| `↓` / `j` | Next session |
| `Enter` | Switch to session |
| `D` | Delete session |
| `R` | Rename session |
| `Esc` | Close sidebar |
| `Tab` | Return to main |

---

## Tool Output

When viewing tool output.

| Key | Action |
|-----|--------|
| `↑` / `↓` | Scroll output |
| `Page Up/Down` | Page scroll |
| `q` | Close expanded view |
| `y` | Copy output |
| `Enter` | Collapse |

---

## Diff View

When viewing file diffs.

| Key | Action |
|-----|--------|
| `↑` / `↓` | Scroll diff |
| `[` / `]` | Previous/Next hunk |
| `a` | Accept changes |
| `r` | Reject changes |
| `q` / `Esc` | Close diff |

---

## Search

When search is active.

| Key | Action |
|-----|--------|
| `Ctrl+R` | Search backward |
| `Ctrl+S` | Search forward |
| `Enter` | Accept result |
| `Esc` | Cancel search |
| `↑` / `↓` | Navigate results |

---

## Command Palette

When command input is open (after `/`).

| Key | Action |
|-----|--------|
| `Tab` | Complete command |
| `↑` / `↓` | Navigate suggestions |
| `Enter` | Execute command |
| `Esc` | Cancel |
| `Ctrl+U` | Clear input |

---

## Vim-Style Navigation

Available when navigation mode is active.

| Key | Action |
|-----|--------|
| `h` | Scroll left |
| `j` | Scroll down |
| `k` | Scroll up |
| `l` | Scroll right |
| `gg` | Go to top |
| `G` | Go to bottom |
| `Ctrl+F` | Page forward |
| `Ctrl+B` | Page backward |
| `/` | Start search |
| `n` | Next search result |
| `N` | Previous search result |

---

## Quick Reference Card

```
┌─────────────────────────────────────────────────────────────┐
│                    Wonopcode Keybindings                     │
├─────────────────────────────────────────────────────────────┤
│ BASICS                                                       │
│   Enter       Send message      Ctrl+C    Cancel/Quit        │
│   Ctrl+D      Quit              Escape    Interrupt          │
│                                                              │
│ EDITING                                                      │
│   Ctrl+A/E    Start/End         Ctrl+W    Delete word        │
│   Ctrl+U/K    Delete line       Ctrl+Y    Paste              │
│   Ctrl+J      New line          Shift+Enter  New line        │
│                                                              │
│ NAVIGATION                                                   │
│   j/k         Scroll down/up    Page Up/Down  Page scroll    │
│   Home/End    Top/Bottom        Ctrl+U/D  Half page scroll   │
│                                                              │
│ LEADER (Ctrl+X, then...)                                     │
│   n  New session               l  List sessions              │
│   m  List models               a  List agents                │
│   u  Undo                      r  Redo                       │
│   z  Revert                    c  Compact                    │
│   e  External editor           t  List themes                │
│   b  Toggle sidebar            g  Timeline                   │
│                                                              │
│ PERMISSIONS                                                  │
│   A/Y  Allow                   D/N  Deny                     │
│   V    View diff               Esc  Cancel                   │
│                                                              │
│ OTHER                                                        │
│   Ctrl+P  Command palette      ?  Toggle help                │
└─────────────────────────────────────────────────────────────┘
```

---

## Customization

Keybindings can be customized in your configuration file:

```json
{
  "keybinds": {
    "leader": "ctrl+x",
    "app_exit": "ctrl+c,ctrl+d",
    "session_new": "<leader>n",
    "session_list": "<leader>l",
    "model_list": "<leader>m",
    "agent_list": "<leader>a",
    "edit_undo": "<leader>u",
    "edit_redo": "<leader>r",
    "command_palette": "ctrl+p",
    "help_toggle": "?"
  }
}
```

---

## Terminal Compatibility

Some keybindings may not work in all terminals:

| Terminal | Notes |
|----------|-------|
| iTerm2 | Full support |
| Terminal.app | Most keys work |
| Alacritty | Full support |
| Kitty | Full support |
| Windows Terminal | Most keys work |
| VS Code Terminal | Some conflicts |

If a keybinding doesn't work, try the alternative or use slash commands.

---

## See Also

- [Slash Commands](./slash-commands.md) - Command reference
- [First Session](../guides/first-session.md) - Using the TUI
- [Tips & Tricks](../guides/tips-and-tricks.md) - Productivity tips
