# Getting Started

Get up and running with wonopcode in under 5 minutes.

---

## 1. Install (30 seconds)

### From Source (Recommended)

```bash
# Clone the repository
git clone https://github.com/wonop-io/wonopcode
cd wonopcode

# Build in release mode
cargo build --release

# Add to your PATH (or move the binary)
export PATH="$PATH:$(pwd)/target/release"
```

### Prerequisites

- Rust 1.75 or later
- Docker (optional, for sandboxing)

---

## 2. Configure Your API Key (1 minute)

Wonopcode works with multiple AI providers. Set up at least one:

### Anthropic Claude (Recommended)

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

### OpenAI

```bash
export OPENAI_API_KEY="sk-..."
```

### Google Gemini

```bash
export GOOGLE_API_KEY="..."
```

### OpenRouter (Access Multiple Models)

```bash
export OPENROUTER_API_KEY="..."
```

> **Tip**: Add the export to your `~/.bashrc` or `~/.zshrc` to persist it.

---

## 3. Start Your First Session (2 minutes)

Navigate to a project directory and start wonopcode:

```bash
cd ~/my-project
wonopcode
```

You'll see the TUI (Terminal User Interface):

```
┌─────────────────────────────────────────────────────┐
│  wonopcode                              claude-sonnet │
├─────────────────────────────────────────────────────┤
│                                                      │
│  Welcome! I'm ready to help with your code.          │
│                                                      │
├─────────────────────────────────────────────────────┤
│ > Type your message...                               │
└─────────────────────────────────────────────────────┘
```

Try a simple prompt:

```
> What files are in this project?
```

The assistant will use the `glob` and `read` tools to explore your codebase.

---

## 4. Enable Sandboxing (30 seconds) ⭐

Sandboxing runs all AI-generated commands in an isolated container, protecting your system.

Create a config file:

```bash
mkdir -p ~/.config/wonopcode
cat > ~/.config/wonopcode/config.json << 'EOF'
{
  "sandbox": {
    "enabled": true
  }
}
EOF
```

That's it! Now all bash commands run safely in a container.

> **Requires**: Docker, Podman, or Lima (macOS)

→ [Learn more about sandboxing](./SANDBOXING.md)

---

## 5. Essential Commands

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Ctrl+C` | Cancel current operation |
| `Ctrl+L` | Clear screen |
| `Ctrl+D` | Quit |

### Slash Commands

Type `/` to see available commands:

| Command | Description |
|---------|-------------|
| `/help` | Show help |
| `/model` | Change AI model |
| `/clear` | Clear conversation |
| `/undo` | Undo last message |
| `/redo` | Redo undone message |
| `/compact` | Compress conversation history |

---

## What's Next?

### Explore the Tools

Wonopcode has powerful built-in tools:

- **File tools**: `read`, `write`, `edit`, `glob`
- **Search**: `grep` with regex support
- **Shell**: `bash` for command execution
- **Web**: `webfetch` for documentation

→ [Tools Overview](./guides/tools-overview.md)

### Configure Your Setup

Customize models, themes, and permissions:

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929",
  "theme": "tokyo-night",
  "sandbox": {
    "enabled": true,
    "network": "limited"
  }
}
```

→ [Configuration Guide](./CONFIGURATION.md)

### Add MCP Servers

Extend wonopcode with external tools:

```json
{
  "mcp": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"]
    }
  }
}
```

→ [MCP Servers Guide](./guides/mcp-servers.md)

---

## Troubleshooting

### "API key not found"

Make sure your API key is exported:

```bash
echo $ANTHROPIC_API_KEY
```

If empty, set it again and ensure it's in your shell config.

### "Docker not available"

Sandboxing requires Docker. Install it or disable sandboxing:

```json
{
  "sandbox": {
    "enabled": false
  }
}
```

### "Model not found"

Check your model string format: `provider/model-name`

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929"
}
```

→ [Full Troubleshooting Guide](./guides/tips-and-tricks.md#troubleshooting)

---

## Getting Help

- Type `/help` in wonopcode
- Check the [documentation](./README.md)
- [Open an issue](https://github.com/wonop-io/wonopcode/issues)
