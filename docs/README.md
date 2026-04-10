# Wonopcode Documentation

> **AI-powered coding assistant with secure sandboxed execution**

Wonopcode is a high-performance terminal-based AI coding assistant written in Rust. It's the only AI coding tool with **native sandboxed execution**, letting you safely run AI-generated code without risking your system.

---

## Why Wonopcode?

| Feature | Wonopcode | Other AI Coding Tools |
|---------|-----------|----------------------|
| **Sandboxed Execution** | ✅ Native support | ❌ Commands run directly on host |
| **Multi-Provider** | ✅ Anthropic, OpenAI, Google, and more | Often single-provider |
| **Terminal-Native** | ✅ Rich TUI experience | Mixed |
| **Open Source** | ✅ MIT Licensed | Often proprietary |
| **MCP Extensible** | ✅ Model Context Protocol | Limited |
| **Self-Hosted** | ✅ Your machine, your data | Cloud-dependent |

---

## Quick Start

```bash
# Install
cargo install wonopcode

# Set your API key
export ANTHROPIC_API_KEY="your-key"

# Run
wonopcode
```

**Enable sandboxing** (recommended):
```json
// ~/.config/wonopcode/config.json
{
  "sandbox": { "enabled": true }
}
```

→ [Full Getting Started Guide](./GETTING_STARTED.md)

---

## Documentation

### Getting Started
- [**Getting Started**](./GETTING_STARTED.md) - Up and running in 5 minutes
- [**Installation**](./INSTALLATION.md) - All installation methods
- [**Configuration**](./CONFIGURATION.md) - Complete config reference

### Key Feature
- [**Sandboxing**](./SANDBOXING.md) - Secure execution deep-dive ⭐

### Guides
- [Your First Session](./guides/first-session.md) - Walkthrough of the TUI
- [Tools Overview](./guides/tools-overview.md) - Built-in tool reference
- [MCP Servers](./guides/mcp-servers.md) - Extending with external tools
- [Custom Agents](./guides/custom-agents.md) - Creating agent personalities
- [IDE Integration](./guides/ide-integration.md) - VSCode, Zed, Cursor
- [Tips & Tricks](./guides/tips-and-tricks.md) - Power user guide

### Reference
- [CLI Reference](./reference/cli.md) - Command-line options
- [Slash Commands](./reference/slash-commands.md) - TUI commands
- [Keybindings](./reference/keybindings.md) - Keyboard shortcuts
- [Config Schema](./reference/config-schema.md) - Full configuration options
- [Environment Variables](./reference/environment-variables.md) - Env var reference

### Architecture
- [Overview](./architecture/overview.md) - System design
- [Crate Structure](./architecture/crate-structure.md) - Code organization
- [Security Model](./architecture/security-model.md) - Permission system
- [DAG Conversation Notes](./architecture/dag-conversation-notes.md) - Early DAG design notes
- [DAG Phase 0 Baseline](./architecture/dag-phase-0-baseline.md) - Migration scope, seams, compatibility, and parity matrix
- [DAG Phase 9 Cutover](./architecture/dag-phase-9-cutover.md) - DAG-first defaults, cutover diagnostics, and legacy retirement notes

### Contributing
- [Contributing Guide](./contributing/CONTRIBUTING.md) - How to contribute
- [Development Setup](./contributing/development-setup.md) - Building from source
- [Testing](./contributing/testing.md) - Running tests

---

## Features

### Sandboxed Execution
Run AI-generated code in isolated containers. No more worrying about `rm -rf` or destructive commands.

```json
{ "sandbox": { "enabled": true } }
```

### Multiple AI Providers
Works with your preferred AI provider:
- Anthropic Claude
- OpenAI GPT-4
- Google Gemini
- OpenRouter
- Azure OpenAI
- AWS Bedrock
- xAI Grok
- Mistral
- Groq

### Rich Terminal UI
Full-featured TUI with:
- Markdown rendering
- Syntax highlighting
- Split panes
- Session management
- Undo/redo

### Powerful Tools
Built-in tools for:
- File operations (read, write, edit)
- Code search (glob, grep)
- Shell execution (bash)
- Web fetching
- LSP integration
- Task management

### MCP Extensibility
Connect to MCP servers for:
- GitHub integration
- Database access
- Slack messaging
- Custom tools

### Snapshot System
Every file change is tracked. Revert any modification instantly.

---

## Supported Platforms

| Platform | Status |
|----------|--------|
| macOS (Apple Silicon) | ✅ Full support |
| macOS (Intel) | ✅ Full support |
| Linux (x86_64) | ✅ Full support |
| Linux (ARM64) | ✅ Full support |
| Windows | 🚧 Coming soon |

---

## License

MIT License - see [LICENSE](../LICENSE) for details.

---

## Links

- [GitHub Repository](https://github.com/wonop-io/wonopcode)
- [Issue Tracker](https://github.com/wonop-io/wonopcode/issues)
- [Changelog](../CHANGELOG.md)
