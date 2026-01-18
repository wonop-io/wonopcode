# Environment Variables

Complete reference for wonopcode environment variables.

---

## API Keys

### AI Provider Keys

| Variable | Provider | Example |
|----------|----------|---------|
| `ANTHROPIC_API_KEY` | Anthropic Claude | `sk-ant-api03-...` |
| `OPENAI_API_KEY` | OpenAI | `sk-...` |
| `GOOGLE_API_KEY` | Google Gemini | `AIza...` |
| `OPENROUTER_API_KEY` | OpenRouter | `sk-or-...` |
| `AZURE_OPENAI_API_KEY` | Azure OpenAI | `...` |
| `XAI_API_KEY` | xAI Grok | `xai-...` |
| `MISTRAL_API_KEY` | Mistral | `...` |
| `GROQ_API_KEY` | Groq | `gsk_...` |

### AWS Bedrock

| Variable | Description |
|----------|-------------|
| `AWS_ACCESS_KEY_ID` | AWS access key |
| `AWS_SECRET_ACCESS_KEY` | AWS secret key |
| `AWS_REGION` | AWS region (default: `us-east-1`) |
| `AWS_PROFILE` | AWS profile name |

### Azure OpenAI

| Variable | Description |
|----------|-------------|
| `AZURE_OPENAI_API_KEY` | API key |
| `AZURE_OPENAI_ENDPOINT` | Resource endpoint |
| `AZURE_OPENAI_API_VERSION` | API version |
| `AZURE_OPENAI_DEPLOYMENT` | Deployment name |

---

## Configuration

### Core Settings

| Variable | Description | Default |
|----------|-------------|---------|
| `WONOPCODE_MODEL` | Default model | From config |
| `WONOPCODE_CONFIG` | Config file path | Auto-detected |
| `WONOPCODE_HOME` | Data directory | `~/.config/wonopcode` |

### Runtime Behavior

| Variable | Description | Default |
|----------|-------------|---------|
| `WONOPCODE_NO_SANDBOX` | Disable sandbox (set to `1`) | `0` |
| `WONOPCODE_SANDBOX_RUNTIME` | Sandbox runtime | `auto` |
| `WONOPCODE_TIMEOUT` | Default timeout (ms) | `120000` |

---

## Logging & Debug

| Variable | Description | Values |
|----------|-------------|--------|
| `WONOPCODE_LOG_LEVEL` | Log level | `debug`, `info`, `warn`, `error` |
| `WONOPCODE_LOG_FILE` | Log file path | None (stderr) |
| `WONOPCODE_DEBUG` | Enable debug mode | `0` or `1` |
| `RUST_LOG` | Rust logging filter | e.g., `wonopcode=debug` |
| `RUST_BACKTRACE` | Show backtraces | `0`, `1`, or `full` |

### Debug Logging Examples

```bash
# General debug logging
WONOPCODE_DEBUG=1 wonopcode

# Detailed Rust logging
RUST_LOG=wonopcode=debug wonopcode

# Module-specific logging
RUST_LOG=wonopcode_sandbox=trace,wonopcode_mcp=debug wonopcode

# Log to file
WONOPCODE_LOG_FILE=~/wonopcode.log wonopcode
```

---

## MCP Servers

MCP servers can reference environment variables in config using `{env:VAR}`:

```json
{
  "mcp": {
    "github": {
      "env": {
        "GITHUB_TOKEN": "{env:GITHUB_TOKEN}"
      }
    }
  }
}
```

### Common MCP Variables

| Variable | Used By |
|----------|---------|
| `GITHUB_TOKEN` | GitHub MCP server |
| `SLACK_TOKEN` | Slack MCP server |
| `DATABASE_URL` | PostgreSQL MCP server |

---

## Sandbox

| Variable | Description | Default |
|----------|-------------|---------|
| `DOCKER_HOST` | Docker daemon socket | Platform default |
| `DOCKER_CONFIG` | Docker config directory | `~/.docker` |
| `LIMA_INSTANCE` | Lima instance name | `default` |

---

## Network

| Variable | Description |
|----------|-------------|
| `HTTP_PROXY` | HTTP proxy URL |
| `HTTPS_PROXY` | HTTPS proxy URL |
| `NO_PROXY` | Hosts to bypass proxy |
| `ALL_PROXY` | Universal proxy URL |

```bash
# Use proxy for API requests
HTTPS_PROXY=http://proxy.company.com:8080 wonopcode
```

---

## TUI Display

| Variable | Description | Default |
|----------|-------------|---------|
| `TERM` | Terminal type | Auto-detected |
| `COLORTERM` | Color support | Auto-detected |
| `NO_COLOR` | Disable colors | Not set |
| `FORCE_COLOR` | Force colors | Not set |

```bash
# Disable colors
NO_COLOR=1 wonopcode

# Force true color
COLORTERM=truecolor wonopcode
```

---

## Shell Integration

| Variable | Used For |
|----------|----------|
| `SHELL` | Default shell for bash tool |
| `EDITOR` | External editor |
| `PAGER` | Pager for long output |
| `HOME` | Home directory |
| `USER` | Username |
| `PATH` | Executable search path |

---

## Setting Variables

### Temporary (Current Session)

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
wonopcode
```

### Permanent (Shell Config)

**Bash** (`~/.bashrc`):
```bash
export ANTHROPIC_API_KEY="sk-ant-..."
export WONOPCODE_MODEL="anthropic/claude-sonnet-4-5-20250929"
```

**Zsh** (`~/.zshrc`):
```bash
export ANTHROPIC_API_KEY="sk-ant-..."
export WONOPCODE_MODEL="anthropic/claude-sonnet-4-5-20250929"
```

**Fish** (`~/.config/fish/config.fish`):
```fish
set -gx ANTHROPIC_API_KEY "sk-ant-..."
set -gx WONOPCODE_MODEL "anthropic/claude-sonnet-4-5-20250929"
```

### Per-Command

```bash
WONOPCODE_MODEL=claude-haiku wonopcode -p "Quick question"
```

### In Config File

Reference env vars in config:

```json
{
  "provider": {
    "anthropic": {
      "api_key": "{env:ANTHROPIC_API_KEY}"
    }
  }
}
```

---

## Security

### Secret Management

**Don't** commit API keys to version control:

```bash
# ✗ Bad: hardcoded in config
{
  "api_key": "sk-ant-actual-key"
}

# ✓ Good: reference environment variable
{
  "api_key": "{env:ANTHROPIC_API_KEY}"
}
```

### Using Secret Managers

**1Password CLI**:
```bash
export ANTHROPIC_API_KEY=$(op read "op://Vault/Anthropic/api_key")
wonopcode
```

**AWS Secrets Manager**:
```bash
export ANTHROPIC_API_KEY=$(aws secretsmanager get-secret-value \
  --secret-id anthropic-key --query SecretString --output text)
wonopcode
```

**HashiCorp Vault**:
```bash
export ANTHROPIC_API_KEY=$(vault kv get -field=api_key secret/anthropic)
wonopcode
```

---

## Precedence

Environment variables have specific precedence in the configuration hierarchy:

1. **Default values** (lowest)
2. **Global config file**
3. **Project config file**
4. **Environment variables**
5. **CLI arguments** (highest)

Example:
```bash
# Config has model: "claude-sonnet"
# This overrides it:
WONOPCODE_MODEL=claude-haiku wonopcode
# This overrides both:
wonopcode --model gpt-4o
```

---

## Troubleshooting

### Check Variable Is Set

```bash
echo $ANTHROPIC_API_KEY
# Should show your key (or be empty if not set)
```

### Check Variable In Wonopcode

```bash
wonopcode --debug 2>&1 | grep -i "api_key\|model"
```

### Common Issues

**Variable not found**:
```bash
# Make sure it's exported
export ANTHROPIC_API_KEY="..."
# Not just assigned
ANTHROPIC_API_KEY="..."  # This doesn't export to child processes
```

**Variable in wrong shell config**:
```bash
# Check which shell you're using
echo $SHELL
# Edit the right config file
```

**Variable overridden by config**:
```bash
# CLI always wins
wonopcode --model my-model
```

---

## Complete Example

```bash
# ~/.bashrc or ~/.zshrc

# API Keys
export ANTHROPIC_API_KEY="sk-ant-..."
export OPENAI_API_KEY="sk-..."
export GITHUB_TOKEN="ghp_..."

# Wonopcode settings
export WONOPCODE_MODEL="anthropic/claude-sonnet-4-5-20250929"
export WONOPCODE_HOME="$HOME/.config/wonopcode"

# Debug (uncomment when needed)
# export WONOPCODE_DEBUG=1
# export RUST_LOG=wonopcode=debug

# Proxy (if needed)
# export HTTPS_PROXY="http://proxy:8080"
```

---

## See Also

- [Configuration](../CONFIGURATION.md) - Config file reference
- [CLI Reference](./cli.md) - Command-line options
- [Installation](../INSTALLATION.md) - Setting up API keys
