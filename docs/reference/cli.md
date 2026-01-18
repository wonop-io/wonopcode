# CLI Reference

Complete reference for wonopcode command-line options.

---

## Synopsis

```bash
wonopcode [OPTIONS] [COMMAND]
```

---

## Commands

### `wonopcode` (default)

Start interactive TUI session.

```bash
wonopcode
wonopcode --model claude-sonnet
wonopcode --cwd ~/project
```

### `wonopcode serve`

Start the ACP server for IDE integration.

```bash
wonopcode serve
wonopcode serve --port 3000
wonopcode serve --host 0.0.0.0
```

**Options**:
| Option | Description |
|--------|-------------|
| `--port <PORT>` | Server port (default: 3000) |
| `--host <HOST>` | Bind address (default: 127.0.0.1) |

### `wonopcode version`

Show version information.

```bash
wonopcode version
wonopcode --version
wonopcode -v
```

### `wonopcode help`

Show help information.

```bash
wonopcode help
wonopcode --help
wonopcode -h
wonopcode help serve  # Help for specific command
```

---

## Global Options

### `-p, --prompt <TEXT>`

Run with an initial prompt. Executes the prompt and exits (non-interactive).

```bash
wonopcode -p "Explain this codebase"
wonopcode --prompt "List all TODO comments"
```

**With output**:
```bash
# Output goes to stdout
wonopcode -p "What files are in src/?" > files.txt
```

### `-m, --model <MODEL>`

Override the default model.

```bash
wonopcode -m anthropic/claude-sonnet-4-5-20250929
wonopcode --model openai/gpt-4o
wonopcode -m claude-haiku  # Short form (provider inferred)
```

**Format**: `provider/model-name` or `model-name`

### `--cwd <PATH>`

Set working directory.

```bash
wonopcode --cwd ~/projects/myapp
wonopcode --cwd /absolute/path
wonopcode --cwd ../relative/path
```

### `-c, --config <FILE>`

Use a custom configuration file.

```bash
wonopcode -c ~/my-config.json
wonopcode --config ./project-config.json
```

### `--agent <AGENT>`

Start with a specific agent.

```bash
wonopcode --agent explore
wonopcode --agent reviewer
```

### `--session <ID>`

Resume a specific session.

```bash
wonopcode --session abc123
wonopcode --session "my-feature-work"
```

### `--new`

Start a new session (don't resume previous).

```bash
wonopcode --new
```

---

## Sandbox Options

### `--no-sandbox`

Disable sandboxed execution.

```bash
wonopcode --no-sandbox
```

### `--sandbox-runtime <RUNTIME>`

Override sandbox runtime.

```bash
wonopcode --sandbox-runtime docker
wonopcode --sandbox-runtime podman
wonopcode --sandbox-runtime lima
```

---

## Output Options

### `--json`

Output in JSON format (for scripting).

```bash
wonopcode -p "List files" --json
```

**Output**:
```json
{
  "response": "...",
  "tools_used": ["glob"],
  "tokens": 150,
  "cost": 0.001
}
```

### `--quiet, -q`

Suppress non-essential output.

```bash
wonopcode -p "Fix the bug" --quiet
```

### `--verbose`

Enable verbose output.

```bash
wonopcode --verbose
```

---

## Debug Options

### `--debug`

Enable debug logging.

```bash
wonopcode --debug
```

### `--log-level <LEVEL>`

Set log level.

```bash
wonopcode --log-level debug
wonopcode --log-level info
wonopcode --log-level warn
wonopcode --log-level error
```

### `--log-file <FILE>`

Write logs to file.

```bash
wonopcode --log-file ~/wonopcode.log
```

### `--validate-config`

Validate configuration and exit.

```bash
wonopcode --validate-config
wonopcode -c custom.json --validate-config
```

---

## Environment Variables

CLI behavior can be modified via environment variables:

| Variable | Description |
|----------|-------------|
| `WONOPCODE_MODEL` | Default model |
| `WONOPCODE_CONFIG` | Config file path |
| `WONOPCODE_LOG_LEVEL` | Log level |
| `WONOPCODE_NO_SANDBOX` | Disable sandbox (set to `1`) |
| `WONOPCODE_DEBUG` | Enable debug mode |

```bash
WONOPCODE_MODEL=claude-haiku wonopcode
WONOPCODE_NO_SANDBOX=1 wonopcode -p "Run tests"
```

---

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | General error |
| `2` | Configuration error |
| `3` | API error |
| `4` | Permission denied |
| `5` | Timeout |
| `130` | Interrupted (Ctrl+C) |

---

## Examples

### Basic Usage

```bash
# Start interactive session
wonopcode

# Run single prompt
wonopcode -p "Explain main.rs"

# Work on different project
wonopcode --cwd ~/other-project
```

### Model Selection

```bash
# Use specific model
wonopcode -m anthropic/claude-sonnet-4-5-20250929

# Use fast model for quick tasks
wonopcode -m claude-haiku -p "What does this function do?"
```

### Session Management

```bash
# Start fresh session
wonopcode --new

# Resume specific session
wonopcode --session feature-auth

# Use specific agent
wonopcode --agent reviewer
```

### Scripting

```bash
# Get JSON output
result=$(wonopcode -p "Count lines of code" --json)
echo $result | jq '.response'

# Pipe input
cat error.log | wonopcode -p "Analyze this error"

# Quiet mode for scripts
wonopcode -p "Fix linting errors" --quiet
```

### Debugging

```bash
# Verbose output
wonopcode --verbose

# Debug logging
wonopcode --debug --log-file debug.log

# Validate configuration
wonopcode --validate-config
```

### Server Mode

```bash
# Start ACP server
wonopcode serve

# Custom port
wonopcode serve --port 8080

# Background server
wonopcode serve --port 3000 &
```

---

## Configuration Precedence

Options are applied in this order (later overrides earlier):

1. Default values
2. Global config (`~/.config/wonopcode/config.json`)
3. Project config (`.wonopcode/config.json`)
4. Environment variables
5. CLI arguments

---

## Combining Options

Options can be combined:

```bash
wonopcode \
  --cwd ~/project \
  --model claude-sonnet \
  --agent reviewer \
  --no-sandbox \
  -p "Review the latest changes"
```

---

## Shell Completion

Generate shell completions:

```bash
# Bash
wonopcode completions bash > /etc/bash_completion.d/wonopcode

# Zsh
wonopcode completions zsh > ~/.zsh/completions/_wonopcode

# Fish
wonopcode completions fish > ~/.config/fish/completions/wonopcode.fish
```

---

## See Also

- [Configuration](../CONFIGURATION.md) - Full config options
- [Environment Variables](./environment-variables.md) - All env vars
- [Slash Commands](./slash-commands.md) - TUI commands
