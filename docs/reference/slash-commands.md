# Slash Commands

Complete reference for wonopcode TUI slash commands.

---

## Overview

Slash commands are typed in the input area starting with `/`. They provide quick access to wonopcode features without leaving the conversation.

```
> /help
```

Press `Tab` after `/` to see available commands.

---

## Session Commands

### `/new [name]`

Create a new session.

```
/new
/new auth-feature
/new "bug fix for issue #42"
```

### `/sessions`

List all sessions.

```
/sessions
```

Output:
```
Sessions:
  * current-session (active)
    auth-feature (2 hours ago)
    bug-fix-123 (yesterday)
    old-session (3 days ago)
```

### `/switch <name|id>`

Switch to a different session.

```
/switch auth-feature
/switch abc123
```

### `/delete <name|id>`

Delete a session.

```
/delete old-session
/delete abc123
```

### `/rename <new-name>`

Rename current session.

```
/rename jwt-implementation
```

### `/export [format]`

Export current session.

```
/export              # Default: markdown
/export markdown     # Markdown format
/export json         # JSON format
/export html         # HTML format
```

Exports to `.wonopcode/exports/`.

---

## Conversation Commands

### `/clear`

Clear conversation messages (keeps session).

```
/clear
```

### `/compact`

Compress conversation history to reduce tokens.

```
/compact
```

The AI summarizes the conversation while preserving context.

### `/undo`

Undo the last exchange (your message + AI response).

```
/undo
```

Also reverts file changes if snapshots enabled.

### `/redo`

Redo an undone exchange.

```
/redo
```

### `/history [n]`

Show conversation history.

```
/history        # Show all
/history 10     # Show last 10 messages
```

---

## Model Commands

### `/model [name]`

View or change the AI model.

```
/model                                    # Show current
/model claude-sonnet                      # Change model
/model anthropic/claude-sonnet-4-5-20250929  # Full name
```

### `/models`

List available models.

```
/models
```

Output:
```
Available models:
  Anthropic:
    claude-sonnet-4-5-20250929
    claude-haiku-3-5-20241022
    claude-opus-4-20250514
  OpenAI:
    gpt-4o
    gpt-4o-mini
  ...
```

---

## Agent Commands

### `/agent [name]`

View or switch agent.

```
/agent              # Show current
/agent explore      # Switch to explore
/agent reviewer     # Switch to reviewer
```

### `/agents`

List available agents.

```
/agents
```

Output:
```
Available agents:
* code (default) - General coding tasks
  explore - Codebase exploration
  build - Build and test operations
  reviewer - Code review specialist (custom)
```

---

## Sandbox Commands

### `/sandbox`

Show sandbox status.

```
/sandbox
```

Output:
```
Sandbox Status:
  Enabled: true
  Runtime: docker
  Status: running
  Container: wonopcode-abc123
  Memory: 245MB / 2GB
  Network: limited
```

### `/sandbox start`

Start the sandbox container.

```
/sandbox start
```

### `/sandbox stop`

Stop the sandbox container.

```
/sandbox stop
```

### `/sandbox restart`

Restart the sandbox.

```
/sandbox restart
```

### `/sandbox shell`

Open interactive shell in sandbox.

```
/sandbox shell
```

Opens a bash prompt inside the container.

---

## MCP Commands

### `/mcp`

Show MCP server status.

```
/mcp
```

Output:
```
MCP Servers:
  ✓ github (connected, 5 tools)
  ✓ postgres (connected, 3 tools)
  ✗ slack (disconnected)
```

### `/mcp connect <server>`

Connect to an MCP server.

```
/mcp connect github
```

### `/mcp disconnect <server>`

Disconnect from an MCP server.

```
/mcp disconnect slack
```

### `/mcp reconnect <server>`

Reconnect to an MCP server.

```
/mcp reconnect github
```

### `/mcp tools [server]`

List tools from MCP servers.

```
/mcp tools           # All servers
/mcp tools github    # Specific server
```

---

## Tool Commands

### `/tools`

List available tools.

```
/tools
```

Output:
```
Built-in Tools:
  read - Read file contents
  write - Create or overwrite files
  edit - Edit existing files
  glob - Find files by pattern
  grep - Search file contents
  bash - Execute shell commands
  ...

MCP Tools:
  github:create_issue
  github:list_issues
  ...
```

### `/tool <name>`

Show tool details.

```
/tool bash
```

Output:
```
Tool: bash
Execute shell commands

Parameters:
  command (required) - Shell command to execute
  description (required) - Human-readable description
  workdir (optional) - Working directory
  timeout (optional) - Timeout in ms (default: 120000)
```

---

## Status Commands

### `/status`

Show session status.

```
/status
```

Output:
```
Session Status:
  ID: abc123
  Name: feature-work
  Messages: 24
  Tokens: 15,432
  Cost: $0.23
  Duration: 45 minutes
  Files changed: 3
  Sandbox: running
```

### `/tokens`

Show token usage.

```
/tokens
```

Output:
```
Token Usage:
  Input: 12,543
  Output: 2,889
  Total: 15,432
  Context window: 200,000
  Used: 7.7%
```

### `/cost`

Show cost estimate.

```
/cost
```

Output:
```
Cost Estimate:
  This session: $0.23
  Today: $1.45
  This week: $8.92
```

---

## File Commands

### `/files`

Show files modified in session.

```
/files
```

Output:
```
Modified files:
  M src/auth.rs (3 changes)
  A src/jwt.rs (new)
  M tests/auth_test.rs (1 change)
```

### `/diff [file]`

Show changes made to files.

```
/diff                 # All changes
/diff src/auth.rs     # Specific file
```

### `/revert [file]`

Revert file changes.

```
/revert               # Revert all
/revert src/auth.rs   # Revert specific file
```

---

## Configuration Commands

### `/config`

Show current configuration.

```
/config
```

### `/config <key>`

Show specific config value.

```
/config model
/config sandbox.enabled
```

### `/set <key> <value>`

Set configuration temporarily.

```
/set model claude-haiku
/set sandbox.network full
```

Changes don't persist across sessions.

---

## UI Commands

### `/help [command]`

Show help.

```
/help              # General help
/help model        # Help for /model command
```

### `/theme [name]`

View or change theme.

```
/theme                  # Show current
/theme tokyo-night      # Change theme
/theme dark
/theme light
```

### `/refresh`

Refresh the display.

```
/refresh
```

### `/quit`

Exit wonopcode.

```
/quit
```

Also: `Ctrl+D` or `Ctrl+Q`

---

## Quick Reference

| Command | Description |
|---------|-------------|
| `/help` | Show help |
| `/new` | New session |
| `/clear` | Clear messages |
| `/compact` | Compress history |
| `/undo` | Undo last exchange |
| `/redo` | Redo exchange |
| `/model` | Change model |
| `/agent` | Switch agent |
| `/sandbox` | Sandbox status |
| `/mcp` | MCP status |
| `/status` | Session status |
| `/quit` | Exit |

---

## Command Completion

Press `Tab` after typing `/` to see suggestions:

```
> /m[Tab]
  /model
  /models
  /mcp
```

Press `Tab` again to cycle through options.

---

## See Also

- [Keybindings](./keybindings.md) - Keyboard shortcuts
- [CLI Reference](./cli.md) - Command-line options
- [Configuration](../CONFIGURATION.md) - Config settings
