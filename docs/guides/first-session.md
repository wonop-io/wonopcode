# Your First Session

A walkthrough of using wonopcode for your first coding session.

---

## Starting Wonopcode

Navigate to your project directory and launch wonopcode:

```bash
cd ~/my-project
wonopcode
```

You'll see the Terminal User Interface (TUI):

```
┌─────────────────────────────────────────────────────────────────────┐
│  wonopcode                                         claude-sonnet     │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Welcome! I'm ready to help with your code.                          │
│                                                                      │
│  I have access to tools for reading, writing, and editing files,     │
│  searching your codebase, and running shell commands.                │
│                                                                      │
│  What would you like to work on?                                     │
│                                                                      │
├─────────────────────────────────────────────────────────────────────┤
│ ⬡ Sandbox (docker) │ Tokens: 0 │ Cost: $0.00                        │
├─────────────────────────────────────────────────────────────────────┤
│ > Type your message...                                               │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Understanding the Interface

### Main Areas

| Area | Description |
|------|-------------|
| **Header** | Model name and session info |
| **Messages** | Conversation history |
| **Footer** | Status indicators (sandbox, tokens, cost) |
| **Input** | Where you type messages |

### Status Indicators

- **⬡ Sandbox (docker)** - Sandbox is active (green = running)
- **Tokens** - Token count for current session
- **Cost** - Estimated API cost

---

## Your First Prompt

Let's start with something simple. Type:

```
What files are in this project?
```

Press `Enter` to send.

### What Happens

1. **Tool Selection**: The AI decides to use the `glob` tool
2. **Permission Check**: If auto-approved, it executes immediately
3. **Execution**: The tool runs (in sandbox if enabled)
4. **Response**: You see the file list and AI analysis

```
┌─────────────────────────────────────────────────────────────────────┐
│ You                                                                  │
├─────────────────────────────────────────────────────────────────────┤
│ What files are in this project?                                      │
├─────────────────────────────────────────────────────────────────────┤
│ Assistant                                                            │
├─────────────────────────────────────────────────────────────────────┤
│ I'll look at the project structure.                                  │
│                                                                      │
│ ┌─ glob ─────────────────────────────────────────────────────────┐  │
│ │ Pattern: **/*                                                   │  │
│ │ Found 42 files                                                  │  │
│ └─────────────────────────────────────────────────────────────────┘  │
│                                                                      │
│ This appears to be a Rust project with:                              │
│ - `Cargo.toml` - Project manifest                                    │
│ - `src/` - Source code directory                                     │
│ - `tests/` - Test files                                              │
│ ...                                                                  │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Reading Code

Ask the AI to examine a specific file:

```
Show me the main function in src/main.rs
```

The AI uses the `read` tool to fetch the file content:

```
┌─ read ────────────────────────────────────────────────────────────┐
│ File: src/main.rs                                                  │
│ Lines: 1-50                                                        │
├────────────────────────────────────────────────────────────────────┤
│     1│  fn main() {                                                │
│     2│      println!("Hello, world!");                             │
│     3│  }                                                          │
└────────────────────────────────────────────────────────────────────┘
```

---

## Making Changes

Ask the AI to modify code:

```
Add a CLI argument parser using clap
```

### Permission Prompt

For write operations, you may see a permission prompt:

```
┌─ Permission Required ─────────────────────────────────────────────┐
│                                                                    │
│ The assistant wants to:                                            │
│   Edit file: src/main.rs                                           │
│                                                                    │
│ Changes:                                                           │
│   - Add clap dependency handling                                   │
│   - Add argument parsing to main()                                 │
│                                                                    │
│ [A]llow  [D]eny  [V]iew diff                                       │
└────────────────────────────────────────────────────────────────────┘
```

Press `A` to allow, `D` to deny, or `V` to see the diff first.

### Viewing the Diff

If you press `V`:

```
┌─ Diff: src/main.rs ───────────────────────────────────────────────┐
│                                                                    │
│   use clap::Parser;                                                │
│                                                                    │
│ + #[derive(Parser)]                                                │
│ + struct Args {                                                    │
│ +     #[arg(short, long)]                                          │
│ +     name: String,                                                │
│ + }                                                                │
│                                                                    │
│   fn main() {                                                      │
│ +     let args = Args::parse();                                    │
│ -     println!("Hello, world!");                                   │
│ +     println!("Hello, {}!", args.name);                           │
│   }                                                                │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
```

---

## Running Commands

Ask the AI to run shell commands:

```
Run the tests
```

With sandbox enabled, the command runs safely in a container:

```
┌─ bash ────────────────────────────────────────────────────────────┐
│ Command: cargo test                                                │
│ Sandbox: ✓ Enabled                                                 │
├────────────────────────────────────────────────────────────────────┤
│ running 3 tests                                                    │
│ test tests::test_add ... ok                                        │
│ test tests::test_sub ... ok                                        │
│ test tests::test_mul ... ok                                        │
│                                                                    │
│ test result: ok. 3 passed; 0 failed                                │
└────────────────────────────────────────────────────────────────────┘
```

---

## Using Slash Commands

Type `/` to see available commands:

```
┌─ Commands ────────────────────────────────────────────────────────┐
│                                                                    │
│ /help      - Show help                                             │
│ /model     - Change AI model                                       │
│ /agent     - Switch agent                                          │
│ /clear     - Clear conversation                                    │
│ /compact   - Compress conversation history                         │
│ /undo      - Undo last message                                     │
│ /redo      - Redo undone message                                   │
│ /status    - Show session status                                   │
│ /export    - Export session                                        │
│ /mcp       - Manage MCP servers                                    │
│ /sandbox   - Control sandbox                                       │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
```

### Common Commands

| Command | Description |
|---------|-------------|
| `/help` | Show all available commands |
| `/model gpt-4o` | Switch to GPT-4o |
| `/clear` | Clear conversation history |
| `/undo` | Undo the last exchange |
| `/compact` | Compress history to save tokens |

---

## Keyboard Shortcuts

### Essential Shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Ctrl+C` | Cancel current operation |
| `Ctrl+L` | Clear screen |
| `Ctrl+D` | Quit wonopcode |

### Navigation

| Key | Action |
|-----|--------|
| `↑` / `↓` | Scroll messages |
| `Page Up` / `Page Down` | Scroll faster |
| `Home` / `End` | Jump to start/end |

### Leader Key Sequences

Press `Ctrl+X` followed by another key:

| Sequence | Action |
|----------|--------|
| `Ctrl+X, N` | New session |
| `Ctrl+X, S` | Switch session |
| `Ctrl+X, M` | Change model |
| `Ctrl+X, U` | Undo |
| `Ctrl+X, R` | Redo |

---

## Working with Multiple Files

Ask the AI to work across files:

```
Refactor the error handling in src/lib.rs to use thiserror
```

The AI will:
1. Read the current file
2. Identify error handling patterns
3. Create/modify error types
4. Update all affected code

You'll see multiple tool calls:

```
┌─ read ─────────────────────────────────────────────────────────┐
│ File: src/lib.rs                                                │
└─────────────────────────────────────────────────────────────────┘

┌─ edit ─────────────────────────────────────────────────────────┐
│ File: src/lib.rs                                                │
│ Adding thiserror derive macro                                   │
└─────────────────────────────────────────────────────────────────┘

┌─ edit ─────────────────────────────────────────────────────────┐
│ File: src/error.rs                                              │
│ Creating error module                                           │
└─────────────────────────────────────────────────────────────────┘
```

---

## Undoing Changes

Made a mistake? Use undo:

### Via Slash Command

```
/undo
```

### Via Keyboard

Press `Ctrl+X, U`

### What Gets Undone

- The AI's last message is removed
- File changes are reverted (if snapshot enabled)
- You can redo with `/redo` or `Ctrl+X, R`

---

## Session Management

### Viewing Session Status

```
/status
```

Shows:
- Session ID
- Message count
- Token usage
- File changes made

### Compacting History

Long conversations use many tokens. Compress them:

```
/compact
```

The AI summarizes the conversation while preserving context.

### Exporting Session

Save your session for later reference:

```
/export
```

Exports to a markdown file in `.wonopcode/exports/`.

---

## Ending Your Session

### Quit

- Press `Ctrl+D`, or
- Press `Ctrl+Q`, or
- Type `/quit`

### Sessions Are Persistent

Your session is automatically saved. Next time you run `wonopcode` in the same directory, you can resume where you left off.

### Starting Fresh

To start a new session:

```bash
wonopcode --new
```

Or use the command:

```
/new
```

---

## Tips for Effective Sessions

### Be Specific

Instead of:
```
Fix the bug
```

Try:
```
Fix the null pointer exception in src/parser.rs line 42 when input is empty
```

### Provide Context

```
I'm building a REST API with axum. Add authentication middleware using JWT tokens.
```

### Ask for Explanations

```
Explain what this regex does: ^(?=.*[A-Z])(?=.*\d).{8,}$
```

### Iterate

```
That's good, but can you also handle the edge case where the list is empty?
```

---

## Common Workflows

### Code Review

```
Review src/api/handlers.rs for security issues and best practices
```

### Documentation

```
Add documentation comments to all public functions in src/lib.rs
```

### Debugging

```
I'm getting this error when running the app:
[paste error]
Help me debug it.
```

### Refactoring

```
Refactor this function to be more readable and testable
```

---

## Troubleshooting

### AI Not Responding

- Check your API key: `echo $ANTHROPIC_API_KEY`
- Check network connectivity
- Press `Ctrl+C` to cancel and try again

### Permission Denied

- Check sandbox is running: `/sandbox status`
- Check file permissions in your project

### Slow Responses

- Use `/compact` to reduce context size
- Switch to a faster model: `/model claude-haiku`

### Tool Errors

- Check the error message in the tool output
- The AI will usually explain and retry

---

## Next Steps

- [Tools Overview](./tools-overview.md) - Learn about all available tools
- [MCP Servers](./mcp-servers.md) - Extend with external tools
- [Tips & Tricks](./tips-and-tricks.md) - Power user techniques
