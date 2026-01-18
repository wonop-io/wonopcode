# Configuration

Complete reference for configuring wonopcode.

---

## Configuration Files

Wonopcode loads configuration from multiple sources, merged in order:

1. **Default configuration** (built-in)
2. **Global configuration** (`~/.config/wonopcode/config.json`)
3. **Project configuration** (`.wonopcode/config.json` in project root)
4. **Environment variables** (override specific settings)
5. **CLI arguments** (highest priority)

### File Locations

| Platform | Global Config |
|----------|---------------|
| macOS | `~/.config/wonopcode/config.json` |
| Linux | `~/.config/wonopcode/config.json` |
| Windows | `%APPDATA%\wonopcode\config.json` |

Project configuration: `.wonopcode/config.json` in your project directory.

---

## Quick Start Configuration

### Minimal Config

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929"
}
```

### Recommended Config

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929",
  "theme": "tokyo-night",
  "sandbox": {
    "enabled": true
  }
}
```

### Full-Featured Config

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929",
  "small_model": "anthropic/claude-haiku-3-5-20241022",
  "theme": "tokyo-night",
  "username": "developer",
  "sandbox": {
    "enabled": true,
    "runtime": "auto",
    "network": "limited",
    "resources": {
      "memory": "4G",
      "cpus": 4
    }
  },
  "snapshot": true,
  "permission": {
    "edit": "allow",
    "bash": {
      "git *": "allow",
      "npm *": "ask"
    }
  },
  "mcp": {
    "github": {
      "type": "local",
      "command": ["npx", "-y", "@modelcontextprotocol/server-github"],
      "environment": {
        "GITHUB_TOKEN": "{env:GITHUB_TOKEN}"
      }
    }
  }
}
```

---

## Configuration Reference

### Core Settings

#### `model`

Default AI model for conversations.

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929"
}
```

**Format**: `provider/model-name`

**Available Providers**:
- `anthropic/` - Claude models
- `openai/` - GPT models
- `google/` - Gemini models
- `openrouter/` - Any OpenRouter model
- `azure/` - Azure OpenAI
- `bedrock/` - AWS Bedrock
- `xai/` - Grok models
- `mistral/` - Mistral models
- `groq/` - Groq-hosted models

**Examples**:
```json
"model": "anthropic/claude-sonnet-4-5-20250929"
"model": "openai/gpt-4o"
"model": "google/gemini-2.0-flash"
"model": "openrouter/anthropic/claude-3.5-sonnet"
```

#### `small_model`

Lightweight model for quick tasks (summarization, exploration).

```json
{
  "small_model": "anthropic/claude-haiku-3-5-20241022"
}
```

#### `theme`

UI color theme.

```json
{
  "theme": "tokyo-night"
}
```

**Available Themes**:
- `tokyo-night` (default)
- `dark`
- `light`
- `dracula`
- `nord`

#### `username`

Display name in conversations.

```json
{
  "username": "developer"
}
```

#### `snapshot`

Enable file change tracking for undo/revert.

```json
{
  "snapshot": true
}
```

---

### Sandbox Settings

Full sandbox configuration. See [Sandboxing](./SANDBOXING.md) for details.

```json
{
  "sandbox": {
    "enabled": true,
    "runtime": "auto",
    "image": "wonopcode/sandbox:latest",
    "resources": {
      "memory": "2G",
      "cpus": 2.0,
      "pids": 256
    },
    "network": "limited",
    "mounts": {
      "workspace_writable": true,
      "persist_caches": true
    },
    "bypass_tools": [],
    "keep_alive": true
  }
}
```

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enabled` | boolean | `false` | Enable sandboxing |
| `runtime` | string | `"auto"` | `"auto"`, `"docker"`, `"podman"`, `"lima"` |
| `image` | string | `"wonopcode/sandbox:latest"` | Container image |
| `resources.memory` | string | `"2G"` | Memory limit |
| `resources.cpus` | number | `2.0` | CPU limit |
| `resources.pids` | number | `256` | Process limit |
| `network` | string | `"limited"` | `"none"`, `"limited"`, `"full"` |
| `mounts.workspace_writable` | boolean | `true` | Allow writes |
| `mounts.persist_caches` | boolean | `true` | Persist npm/pip |
| `bypass_tools` | array | `[]` | Tools running on host |
| `keep_alive` | boolean | `true` | Keep container running |

---

### Permission Settings

Control which operations require approval. Permissions use three levels:
- `"allow"` - Execute without asking
- `"ask"` - Prompt user for approval (default)
- `"deny"` - Block execution

```json
{
  "permission": {
    "edit": "allow",
    "bash": "ask",
    "webfetch": "allow",
    "external_directory": "deny"
  }
}
```

#### Permission Types

| Permission | Description |
|------------|-------------|
| `edit` | File editing operations (Edit, Write, Patch tools) |
| `bash` | Shell command execution |
| `webfetch` | Fetching URLs from the web |
| `external_directory` | Accessing files outside workspace |

#### `bash` - Fine-Grained Control

Bash permissions can be a single value or a pattern map:

```json
{
  "permission": {
    "bash": {
      "ls": "allow",
      "cat": "allow",
      "grep": "allow",
      "find": "allow",
      "git *": "allow",
      "npm *": "ask",
      "cargo *": "ask",
      "rm *": "deny",
      "sudo *": "deny",
      "chmod *": "deny"
    }
  }
}
```

**Pattern Syntax**:
- `command` - Exact match
- `command *` - Command with any arguments
- `*pattern*` - Wildcard matching

#### Agent-Level Permissions

Agents can have additional permission options:

```json
{
  "agent": {
    "build": {
      "permission": {
        "edit": "allow",
        "bash": "ask",
        "skill": "allow",
        "doom_loop": "ask",
        "external_directory": "deny"
      }
    }
  }
}
```

| Permission | Description |
|------------|-------------|
| `skill` | Loading skill definitions |
| `doom_loop` | Allowing repeated similar operations |

---

### Provider Settings

Provider-specific configuration.

```json
{
  "provider": {
    "anthropic": {
      "api_key": "{env:ANTHROPIC_API_KEY}",
      "base_url": "https://api.anthropic.com"
    },
    "openai": {
      "api_key": "{env:OPENAI_API_KEY}",
      "base_url": "https://api.openai.com/v1"
    },
    "azure": {
      "api_key": "{env:AZURE_OPENAI_API_KEY}",
      "base_url": "https://your-resource.openai.azure.com",
      "api_version": "2024-02-15-preview",
      "deployment": "gpt-4o"
    }
  }
}
```

#### Environment Variable Expansion

Use `{env:VAR_NAME}` to reference environment variables:

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

### MCP Server Settings

Configure Model Context Protocol servers. See [MCP Servers Guide](./guides/mcp-servers.md) for details.

```json
{
  "mcp": {
    "filesystem": {
      "type": "local",
      "command": ["npx", "-y", "@modelcontextprotocol/server-filesystem", "~/projects"],
      "environment": {
        "HOME": "{env:HOME}"
      }
    },
    "github": {
      "type": "local",
      "command": ["npx", "-y", "@modelcontextprotocol/server-github"],
      "environment": {
        "GITHUB_TOKEN": "{env:GITHUB_TOKEN}"
      }
    }
  }
}
```

#### Local Server (stdio)

Local servers run as subprocesses. Requires `type: "local"`.

```json
{
  "mcp": {
    "my-server": {
      "type": "local",
      "command": ["/path/to/server", "--flag", "value"],
      "environment": {
        "KEY": "value"
      },
      "enabled": true,
      "timeout": 30000
    }
  }
}
```

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `type` | `"local"` | Yes | Server type |
| `command` | `string[]` | Yes | Command and arguments |
| `environment` | `object` | No | Environment variables |
| `enabled` | `boolean` | No | Enable/disable (default: true) |
| `timeout` | `number` | No | Timeout in ms |

#### Remote Server (SSE)

Remote servers communicate over HTTP. Requires `type: "remote"`.

```json
{
  "mcp": {
    "remote-server": {
      "type": "remote",
      "url": "https://mcp.example.com/sse",
      "headers": {
        "Authorization": "Bearer {env:API_TOKEN}"
      },
      "oauth": {
        "client_id": "your-client-id",
        "client_secret": "{env:CLIENT_SECRET}",
        "scope": "read write"
      }
    }
  }
}
```

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `type` | `"remote"` | Yes | Server type |
| `url` | `string` | Yes | Server URL |
| `headers` | `object` | No | HTTP headers |
| `oauth` | `object` | No | OAuth configuration |
| `enabled` | `boolean` | No | Enable/disable (default: true) |
| `timeout` | `number` | No | Timeout in ms |

---

### Agent Settings

Configure agent behavior and per-agent overrides.

```json
{
  "default_agent": "build",
  "agent": {
    "build": {
      "model": "anthropic/claude-sonnet-4-5-20250929",
      "sandbox": {
        "enabled": true,
        "network": "full"
      },
      "tools": {
        "*": true
      }
    },
    "plan": {
      "description": "Planning agent with read-only file access",
      "tools": {
        "edit": false,
        "write": false
      },
      "permission": {
        "bash": {
          "ls *": "allow",
          "git status": "allow",
          "git diff *": "allow",
          "*": "ask"
        }
      }
    },
    "explore": {
      "model": "anthropic/claude-haiku-3-5-20241022",
      "sandbox": {
        "enabled": true,
        "mounts": {
          "workspace_writable": false
        }
      },
      "tools": {
        "read": true,
        "glob": true,
        "grep": true,
        "write": false,
        "edit": false,
        "bash": false
      }
    }
  }
}
```

#### Per-Agent Tool Restrictions

```json
{
  "agent": {
    "readonly": {
      "tools": {
        "*": false,
        "read": true,
        "glob": true,
        "grep": true
      }
    }
  }
}
```

#### Per-Agent Bash Permissions

```json
{
  "agent": {
    "safe": {
      "permission": {
        "bash": {
          "*": "deny",
          "ls": "allow",
          "cat": "allow"
        }
      }
    }
  }
}
```

---

### TUI Settings

Terminal UI configuration.

```json
{
  "tui": {
    "disabled": false,
    "mouse": true,
    "paste": "bracketed"
  }
}
```

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `disabled` | boolean | `false` | Disable TUI and use basic mode |
| `mouse` | boolean | `true` | Enable mouse support |
| `paste` | string | `"bracketed"` | Paste mode: `"bracketed"` or `"direct"` |

#### Paste Modes

- `"bracketed"` - Uses bracketed paste mode for better multi-line paste handling (recommended)
- `"direct"` - Direct character input, may be needed for some terminal emulators

---

## Environment Variables

Environment variables override config file settings.

### API Keys

| Variable | Provider |
|----------|----------|
| `ANTHROPIC_API_KEY` | Anthropic Claude |
| `OPENAI_API_KEY` | OpenAI |
| `GOOGLE_API_KEY` | Google Gemini |
| `OPENROUTER_API_KEY` | OpenRouter |
| `AZURE_OPENAI_API_KEY` | Azure OpenAI |
| `AWS_ACCESS_KEY_ID` | AWS Bedrock |
| `AWS_SECRET_ACCESS_KEY` | AWS Bedrock |
| `XAI_API_KEY` | xAI Grok |
| `MISTRAL_API_KEY` | Mistral |
| `GROQ_API_KEY` | Groq |

### Configuration Overrides

| Variable | Description |
|----------|-------------|
| `WONOPCODE_MODEL` | Override default model |
| `WONOPCODE_CONFIG` | Custom config file path |
| `WONOPCODE_LOG_LEVEL` | Log level (debug, info, warn, error) |
| `WONOPCODE_NO_SANDBOX` | Disable sandbox (set to `1`) |

### Debug Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Rust logging filter |
| `WONOPCODE_DEBUG` | Enable debug mode |

---

## CLI Arguments

CLI arguments have highest priority.

```bash
wonopcode [OPTIONS] [COMMAND]
```

### Options

| Flag | Description |
|------|-------------|
| `-p, --prompt <TEXT>` | Run with initial prompt |
| `-m, --model <MODEL>` | Override model |
| `--cwd <PATH>` | Working directory |
| `-c, --config <FILE>` | Custom config file |
| `--no-sandbox` | Disable sandbox |
| `--debug` | Enable debug logging |
| `-v, --version` | Show version |
| `-h, --help` | Show help |

### Examples

```bash
# Run with specific model
wonopcode --model openai/gpt-4o

# Run in different directory
wonopcode --cwd ~/other-project

# Run with initial prompt
wonopcode -p "Explain this codebase"

# Use custom config
wonopcode --config ~/my-config.json
```

---

## Project-Level Configuration

Create `.wonopcode/config.json` in your project root for project-specific settings:

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929",
  "sandbox": {
    "enabled": true,
    "resources": {
      "memory": "8G"
    }
  },
  "permission": {
    "bash": {
      "npm *": "allow",
      "cargo *": "allow"
    }
  }
}
```

Project config merges with (and overrides) global config.

---

## Custom Agents

Create custom agents in `.wonopcode/agent/`:

```markdown
<!-- .wonopcode/agent/reviewer.md -->
---
name: reviewer
description: Code review specialist
model: anthropic/claude-sonnet-4-5-20250929
tools:
  read: true
  glob: true
  grep: true
  write: false
  bash: false
---

You are a code reviewer. Review code for:
- Bugs and potential issues
- Performance problems
- Security vulnerabilities
- Code style and best practices

Be thorough but constructive. Suggest specific improvements.
```

Use with `/agent reviewer` command.

---

## Configuration Validation

Wonopcode validates configuration on startup. Invalid config shows errors:

```
Error: Invalid configuration
  → sandbox.resources.memory: invalid format "2GB" (use "2G")
  → model: unknown provider "anthrpic" (did you mean "anthropic"?)
```

### Validate Without Running

```bash
wonopcode --validate-config
```

---

## Example Configurations

### Security-Focused

```json
{
  "sandbox": {
    "enabled": true,
    "network": "none",
    "mounts": {
      "workspace_writable": false
    }
  },
  "permission": {
    "auto_approve": [],
    "bash": {
      "*": "deny"
    }
  }
}
```

### Development Workflow

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929",
  "sandbox": {
    "enabled": true,
    "network": "full"
  },
  "permission": {
    "edit": "allow",
    "bash": {
      "git *": "allow",
      "npm *": "allow",
      "cargo *": "allow"
    }
  },
  "mcp": {
    "github": {
      "type": "local",
      "command": ["npx", "-y", "@modelcontextprotocol/server-github"],
      "environment": {
        "GITHUB_TOKEN": "{env:GITHUB_TOKEN}"
      }
    }
  }
}
```

### Multi-Provider Setup

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929",
  "small_model": "groq/llama-3.1-70b-versatile",
  "provider": {
    "anthropic": {
      "api_key": "{env:ANTHROPIC_API_KEY}"
    },
    "groq": {
      "api_key": "{env:GROQ_API_KEY}"
    }
  }
}
```

---

## Next Steps

- [Sandboxing](./SANDBOXING.md) - Secure execution details
- [MCP Servers](./guides/mcp-servers.md) - Extending with tools
- [Custom Agents](./guides/custom-agents.md) - Creating agents
