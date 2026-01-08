# Wonopcode

[![Build Status](https://img.shields.io/github/actions/workflow/status/wonop-io/wonopcode/ci.yml?branch=main)](https://github.com/wonop-io/wonopcode/actions)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A high-performance AI-powered coding assistant for the terminal, inspired by [OpenCode](https://github.com/sst/opencode), designed to enhance developer productivity with seamless integration of AI capabilities.

## Features

- **Multiple AI Providers**: Supports Anthropic Claude, OpenAI, Google Gemini, and many others.
- **Rich Terminal UI**: Includes markdown rendering, syntax highlighting, and split panes.
- **Tool Integration**: Comprehensive toolset including file operations, web fetching, and LSP support.
- **MCP Support**: Extensible tool integration via the Model Context Protocol (local and remote with OAuth).
- **Session Management**: Includes features like undo/redo, forking, and conversation compaction.
- **ACP Protocol**: IDE integration with VSCode, Zed, and Cursor.
- **Snapshot System**: Track and revert file changes efficiently.
- **Sandbox Mode**: Isolated container/VM execution for secure tool operations.

## Prerequisites

Before building wonopcode, ensure you have the following installed:

### Required

- **Rust** (stable toolchain): Install via [rustup](https://rustup.rs/)
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

- **just** (command runner): Used for build automation
  ```bash
  # macOS
  brew install just
  
  # Cargo
  cargo install just
  ```

### Optional (for development)

Install all development tools at once:
```bash
just install-tools
```

Or install individually:
- **cargo-watch**: Auto-rebuild on file changes (`cargo install cargo-watch`)
- **cargo-udeps**: Find unused dependencies (`cargo install cargo-udeps`)
- **cargo-outdated**: Check for outdated dependencies (`cargo install cargo-outdated`)
- **cargo-audit**: Security vulnerability scanner (`cargo install cargo-audit`)
- **tokei**: Lines of code counter (`brew install tokei` or `cargo install tokei`)

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/wonop-io/wonopcode
cd wonopcode

# Build in release mode
just release
# Or: cargo build --release

# The binary is at ./target/release/wonopcode

# Install locally
just install
```

### Configuration

Create a configuration file at `~/.config/wonopcode/config.json` or `wonopcode.json` in your project directory:

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929",
  "theme": "tokyo-night"
}
```

Set your API key via environment variable:

```bash
export ANTHROPIC_API_KEY="your-key-here"
# Or for other providers:
export OPENAI_API_KEY="your-key-here"
export OPENROUTER_API_KEY="your-key-here"
```

Or use the built-in authentication:

```bash
wonopcode auth login anthropic
```

## Usage

### CLI Options

```bash
wonopcode [OPTIONS] [COMMAND]

Options:
  --basic              Run in basic mode (no TUI)
  -p, --prompt <TEXT>  Prompt to send immediately
  -c, --continue       Continue the last session
  -r, --resume <ID>    Resume a specific session
  --json               Print output as JSON
  -v, --verbose        Enable verbose logging
  --provider <NAME>    Provider to use (default: anthropic)
  -m, --model <ID>     Model ID to use
```

### Examples

```bash
# Start interactive session
wonopcode

# Run with a prompt
wonopcode -p "Explain this codebase"

# Continue last session
wonopcode -c

# Use a specific model
wonopcode --model openai/gpt-4o

# Run non-interactively
wonopcode run "Fix the bug in main.rs"

# Run with JSON output
wonopcode run --format json "List all functions"
```

### Subcommands

| Command | Description |
|---------|-------------|
| `run` | Run with a message (non-interactive) |
| `serve` | Start the HTTP server |
| `models` | List available models |
| `config` | Show configuration |
| `version` | Print version information |
| `auth` | Authenticate with a provider (`login`, `logout`, `status`) |
| `session` | Manage sessions (`list`, `show`, `delete`) |
| `export` | Export session(s) to a file |
| `import` | Import session(s) from a file |
| `acp` | Start ACP server for IDE integration |
| `stats` | Show token usage and cost statistics |
| `web` | Start web UI server (headless mode) |
| `mcp` | Manage MCP servers (`add`, `list`, `auth`, `logout`) |
| `upgrade` | Upgrade to the latest version |
| `agent` | List available agents (`list`, `show`) |
| `mcp-serve` | Run as MCP server (for Claude CLI integration) |

### TUI Keybindings

The TUI uses a **leader key** system (default: `Ctrl+X`). Press the leader key followed by another key to trigger actions.

#### Global

| Keybinding | Action |
|------------|--------|
| `Ctrl+X Ctrl+C` | Exit the application |
| `Ctrl+P` | Open command palette |
| `?` | Toggle help overlay |
| `Escape` | Interrupt current operation / Cancel |

#### Leader Key Sequences (`Ctrl+X` + key)

| Keybinding | Action |
|------------|--------|
| `<leader> n` | Create a new session |
| `<leader> l` | List all sessions |
| `<leader> m` | List available models |
| `<leader> a` | List available agents |
| `<leader> t` | List available themes |
| `<leader> b` | Toggle sidebar |
| `<leader> e` | Open external editor |
| `<leader> x` | Export session |
| `<leader> c` | Compact the session |
| `<leader> g` | Show session timeline |
| `<leader> u` | Undo last message |
| `<leader> r` | Redo undone message |
| `<leader> y` | Copy last response |
| `<leader> z` | Revert to previous message |
| `<leader> Z` | Cancel revert |

#### Navigation

| Keybinding | Action |
|------------|--------|
| `PageUp` / `PageDown` | Scroll messages by page |
| `Ctrl+U` / `Ctrl+D` | Scroll messages by half page |
| `Home` / `End` | Go to first/last message |
| `j` / `Down` | Next message |
| `k` / `Up` | Previous message |

#### Input

| Keybinding | Action |
|------------|--------|
| `Enter` | Submit input |
| `Ctrl+J` / `Shift+Enter` | Insert newline |
| `Ctrl+C` | Cancel input |
| `Up` / `Down` | Navigate input history |

### Slash Commands

Type `/` in the input to access commands. Commands are organized by category:

#### Session Commands

| Command | Aliases | Description |
|---------|---------|-------------|
| `/new` | `/clear` | Create a new session |
| `/undo` | | Undo the last message |
| `/redo` | | Redo an undone message |
| `/compact` | `/summarize` | Compact conversation history |
| `/rename` | | Rename the current session |
| `/copy` | | Copy session transcript to clipboard |
| `/export` | | Export session transcript to file |
| `/timeline` | | Jump to a specific message |
| `/fork` | | Fork from a message |
| `/thinking` | | Toggle thinking visibility |
| `/share` | | Share the current session |
| `/unshare` | | Unshare a session |

#### Navigation Commands

| Command | Aliases | Description |
|---------|---------|-------------|
| `/sessions` | `/session`, `/resume`, `/continue` | List all sessions |
| `/models` | | List and select a model |
| `/agents` | `/agent` | List and select an agent |
| `/theme` | | Change the theme |
| `/status` | | Show configuration status |
| `/mcp` | | Toggle MCP servers |
| `/sandbox` | | Manage sandbox |
| `/connect` | | Connect to a provider |

#### UI Commands

| Command | Aliases | Description |
|---------|---------|-------------|
| `/editor` | | Open input in external editor |
| `/sidebar` | | Toggle the sidebar |
| `/commands` | | Show all commands |
| `/help` | | Show help |
| `/quit` | `/exit`, `/q` | Quit the application |

#### Built-in Custom Commands

| Command | Description |
|---------|-------------|
| `/init` | Initialize or update project configuration (creates AGENTS.md) |
| `/review` | Review code changes |
| `/explain` | Explain code or concepts |
| `/fix` | Fix issues or bugs |
| `/test` | Generate tests |
| `/doc` | Generate documentation |
| `/refactor` | Refactor code |

### Custom Commands

You can define custom commands in two ways:

1. **In configuration** (`wonopcode.json`):
```json
{
  "command": {
    "pr": {
      "template": "Create a PR description for these changes: $ARGUMENTS",
      "description": "Generate PR description",
      "agent": "plan"
    }
  }
}
```

2. **As markdown files** (`.wonopcode/command/*.md`):
```markdown
---
name: deploy
description: Deploy to production
agent: code
---

Deploy the application to production:
$ARGUMENTS

Follow these steps:
1. Run tests
2. Build for production
3. Deploy
```

Template variables:
- `$ARGUMENTS` - Full argument string
- `$1`, `$2`, etc. - Individual arguments (last one captures remaining)

## Crate Structure

| Crate | Description |
|-------|-------------|
| `wonopcode` | Main binary with CLI and TUI |
| `wonopcode-core` | Core types, session management, configuration |
| `wonopcode-provider` | AI provider implementations |
| `wonopcode-tools` | Tool implementations (bash, files, web, etc.) |
| `wonopcode-tui` | Terminal UI widgets and rendering |
| `wonopcode-server` | HTTP API server |
| `wonopcode-acp` | Agent Client Protocol for IDE integration |
| `wonopcode-mcp` | Model Context Protocol client |
| `wonopcode-lsp` | Language Server Protocol integration |
| `wonopcode-snapshot` | File change tracking and revert |
| `wonopcode-storage` | Persistent storage backends |
| `wonopcode-util` | Shared utilities |
| `wonopcode-auth` | Authentication and credential management |
| `wonopcode-sandbox` | Container/VM sandboxing for tool execution |

## Development

This project uses [just](https://github.com/casey/just) as a command runner. Run `just --list` to see all available commands.

### Building

```bash
# Debug build
just build

# Release build
just release

# Build with all features
just build-all

# Clean and rebuild
just rebuild
```

### Testing

```bash
# Run all tests
just test

# Run tests with output
just test-verbose

# Run a specific test
just test-one <test_name>

# Run tests for a specific crate
just test-crate wonopcode-core
```

### Linting & Formatting

```bash
# Format code
just fmt

# Check formatting (no changes)
just fmt-check

# Run clippy linter
just lint

# Run clippy and auto-fix issues
just lint-fix

# Run all checks (format, lint, test)
just check
```

### CI

```bash
# Run full CI checks (format, lint, test)
just ci

# Run strict checks (warnings as errors, for CI)
just check-strict

# Pre-commit hook check (format, lint only)
just pre-commit
```

### Documentation

```bash
# Generate documentation
just doc

# Generate and open in browser
just doc-open
```

### Development Helpers

```bash
# Watch for changes and run tests
just watch-test

# Watch for changes and run clippy
just watch-lint

# Run the application
just run [ARGS]

# Run in release mode
just run-release [ARGS]
```

### Dependency Management

```bash
# Check for outdated dependencies
just outdated

# Update dependencies
just update

# Audit for security vulnerabilities
just audit

# Check for unused dependencies (requires nightly)
just udeps
```

### Project Structure

```
crates/
├── wonopcode/           # Main binary
├── wonopcode-core/      # Core library
├── wonopcode-provider/  # AI providers
├── wonopcode-tools/     # Built-in tools
├── wonopcode-tui/       # Terminal UI
├── wonopcode-server/    # HTTP server
├── wonopcode-acp/       # IDE protocol
├── wonopcode-mcp/       # MCP client
├── wonopcode-lsp/       # LSP client
├── wonopcode-snapshot/  # File snapshots
├── wonopcode-storage/   # Storage layer
├── wonopcode-util/      # Utilities
├── wonopcode-auth/      # Authentication
└── wonopcode-sandbox/   # Sandboxing
```

## Configuration Options

Configuration is loaded from multiple sources (in order of precedence):
1. Global config: `~/.config/wonopcode/config.json`
2. Environment variable: `WONOPCODE_CONFIG_CONTENT`
3. Project config: `wonopcode.json` or `wonopcode.jsonc` in project directory

Supports JSONC (JSON with comments) and variable substitution:
- `{env:VAR_NAME}` - Substitute environment variable
- `{file:path}` - Substitute file contents

### Core Options

| Option | Type | Description |
|--------|------|-------------|
| `model` | string | Primary model (e.g., "anthropic/claude-sonnet-4-5-20250929") |
| `small_model` | string | Small/fast model for quick tasks |
| `theme` | string | UI theme (e.g., "tokyo-night", "dark", "light") |
| `log_level` | string | Log level: "debug", "info", "warn", "error" |
| `default_agent` | string | Default agent name |
| `username` | string | Display name in conversations |
| `snapshot` | boolean | Enable file change tracking |
| `share` | string | Share mode: "manual", "auto", "disabled" |
| `autoupdate` | boolean | Enable auto-updates |
| `instructions` | string[] | Additional instructions for the AI |

### Provider Options

| Option | Type | Description |
|--------|------|-------------|
| `disabled_providers` | string[] | Provider IDs to disable |
| `enabled_providers` | string[] | Provider IDs to enable (whitelist mode) |
| `provider` | object | Provider-specific configurations |

### TUI Options

| Option | Type | Description |
|--------|------|-------------|
| `tui.disabled` | boolean | Disable TUI and use basic mode |
| `tui.mouse` | boolean | Enable mouse support |
| `tui.paste` | string | Paste mode: "bracketed", "direct" |

### Server Options

| Option | Type | Description |
|--------|------|-------------|
| `server.disabled` | boolean | Disable the HTTP server |
| `server.port` | number | Server port |

### Keybind Options

| Option | Type | Description |
|--------|------|-------------|
| `keybinds.leader` | string | Leader key (default: "ctrl+x") |
| `keybinds.app_exit` | string | Exit keybinding |
| `keybinds.*` | string | Override any keybinding |

### Permission Options

| Option | Type | Description |
|--------|------|-------------|
| `permission.edit` | string | Edit permission: "ask", "allow", "deny" |
| `permission.bash` | string/object | Bash permission (can be pattern-based) |
| `permission.webfetch` | string | Web fetch permission |
| `permission.external_directory` | string | External directory access permission |

### MCP Configuration

```json
{
  "mcp": {
    "my-local-server": {
      "type": "local",
      "command": ["node", "server.js"],
      "environment": {"DEBUG": "true"},
      "enabled": true,
      "timeout": 30000
    },
    "my-remote-server": {
      "type": "remote",
      "url": "https://api.example.com/mcp",
      "headers": {"Authorization": "Bearer token"},
      "oauth": {
        "client_id": "...",
        "client_secret": "...",
        "scope": "read write"
      }
    }
  }
}
```

### Agent Configuration

```json
{
  "agent": {
    "my-agent": {
      "model": "anthropic/claude-sonnet-4-5-20250929",
      "temperature": 0.7,
      "prompt": "You are a helpful assistant...",
      "tools": {"bash": true, "edit": true},
      "max_steps": 50,
      "mode": "subagent"
    }
  }
}
```

### Sandbox Configuration

```json
{
  "sandbox": {
    "enabled": true,
    "runtime": "docker",
    "image": "ubuntu:22.04",
    "network": "limited",
    "resources": {
      "memory": "2G",
      "cpus": 2.0,
      "pids": 100
    },
    "mounts": {
      "workspace_writable": true,
      "persist_caches": true
    },
    "bypass_tools": ["read", "glob", "grep"],
    "keep_alive": true
  }
}
```

## License

MIT License - see LICENSE file for details.

## Acknowledgments

This project was inspired by [OpenCode](https://github.com/sst/opencode) by SST.

## Contributing

Contributions are welcome! Please read the [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on how to proceed.

For questions or discussions, feel free to open an issue or contact us via [GitHub Discussions](https://github.com/wonop-io/wonopcode/discussions).
