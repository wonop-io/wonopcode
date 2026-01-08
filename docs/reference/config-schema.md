# Configuration Schema

Complete reference for all wonopcode configuration options.

---

## Overview

Configuration is loaded from multiple sources and merged in order:

1. **Default configuration** (built-in)
2. **Global config** (`~/.config/wonopcode/config.json`)
3. **Environment variable** (`WONOPCODE_CONFIG_CONTENT`)
4. **Project config** (`wonopcode.json` or `wonopcode.jsonc` in project root)
5. **CLI arguments** (highest priority)

Supports JSONC (JSON with comments) and variable substitution.

---

## Full Schema

```json
{
  "$schema": "https://wonopcode.dev/config.json",
  
  // Core Settings
  "theme": "string",
  "log_level": "debug | info | warn | error",
  "model": "provider/model-name",
  "small_model": "provider/model-name",
  "default_agent": "build",
  "username": "string",
  "snapshot": true,
  "share": "manual | auto | disabled",
  "autoupdate": true | false | "notify",
  
  // Provider Lists
  "disabled_providers": ["provider-id"],
  "enabled_providers": ["provider-id"],
  
  // Nested Configurations
  "tui": { /* TUI settings */ },
  "server": { /* Server settings */ },
  "keybinds": { /* Keybind settings */ },
  "command": { /* Custom commands */ },
  "agent": { /* Agent configurations */ },
  "provider": { /* Provider configurations */ },
  "mcp": { /* MCP server configurations */ },
  "permission": { /* Permission settings */ },
  "tools": { /* Tool enable/disable */ },
  "instructions": ["path/to/file.md"],
  "compaction": { /* Compaction settings */ },
  "sandbox": { /* Sandbox settings */ },
  "enterprise": { /* Enterprise settings */ },
  "experimental": { /* Experimental features */ }
}
```

---

## Core Settings

### `theme`

UI color theme.

```json
{
  "theme": "tokyo-night"
}
```

**Type**: `string`  
**Default**: `"tokyo-night"`  
**Options**: `"tokyo-night"`, `"dark"`, `"light"`, `"dracula"`, `"nord"`

---

### `log_level`

Logging verbosity.

```json
{
  "log_level": "info"
}
```

**Type**: `string`  
**Default**: `"info"`  
**Options**: `"debug"`, `"info"`, `"warn"`, `"error"`

---

### `model`

Primary AI model in `provider/model` format.

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929"
}
```

**Type**: `string`  
**Required**: Yes (or set via environment)

---

### `small_model`

Lightweight model for quick tasks (titles, summaries).

```json
{
  "small_model": "anthropic/claude-haiku-3-5-20241022"
}
```

**Type**: `string`  
**Default**: Falls back to main model

---

### `default_agent`

Default agent to use.

```json
{
  "default_agent": "build"
}
```

**Type**: `string`  
**Default**: `"build"`  
**Options**: `"build"`, `"plan"`, or custom agent name

---

### `username`

Display name in conversations.

```json
{
  "username": "developer"
}
```

**Type**: `string`  
**Default**: System username

---

### `snapshot`

Enable file change tracking for undo/revert.

```json
{
  "snapshot": true
}
```

**Type**: `boolean`  
**Default**: `true`

---

### `share`

Session sharing mode.

```json
{
  "share": "manual"
}
```

**Type**: `string`  
**Default**: `"manual"`  
**Options**:
- `"manual"` - Share via `/share` command
- `"auto"` - Automatically share new sessions
- `"disabled"` - Disable sharing entirely

---

### `autoupdate`

Auto-update behavior.

```json
{
  "autoupdate": true
}
```

**Type**: `boolean | "notify"`  
**Default**: `true`  
**Options**:
- `true` - Automatically download updates
- `false` - Disable updates
- `"notify"` - Notify but don't download

---

### `disabled_providers`

Providers to disable even if credentials are available.

```json
{
  "disabled_providers": ["openai", "azure"]
}
```

**Type**: `array of string`

---

### `enabled_providers`

Allowlist of providers (others are ignored).

```json
{
  "enabled_providers": ["anthropic"]
}
```

**Type**: `array of string`  
**Note**: `disabled_providers` takes priority over `enabled_providers`

---

### `instructions`

Additional instruction files to include in system prompt.

```json
{
  "instructions": [
    "CONTRIBUTING.md",
    "docs/guidelines.md",
    ".cursor/rules/*.md"
  ]
}
```

**Type**: `array of string`  
**Supports**: Glob patterns

---

## TUI Settings

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
| `disabled` | boolean | `false` | Disable TUI, use basic mode |
| `mouse` | boolean | `true` | Enable mouse support |
| `paste` | string | `"bracketed"` | Paste mode: `"bracketed"` or `"direct"` |

---

## Server Settings

Server configuration for `wonopcode serve`.

```json
{
  "server": {
    "disabled": false,
    "port": 8080
  }
}
```

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `disabled` | boolean | `false` | Disable server |
| `port` | number | `8080` | Server port |

---

## Keybind Settings

Customize keyboard shortcuts.

```json
{
  "keybinds": {
    "leader": "ctrl+x",
    "app_exit": "ctrl+c,ctrl+d",
    "session_new": "<leader>n",
    "model_list": "<leader>m"
  }
}
```

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `leader` | string | `"ctrl+x"` | Leader key prefix |
| `app_exit` | string | `"ctrl+c,ctrl+d"` | Exit application |
| `editor_open` | string | `"<leader>e"` | Open external editor |
| `theme_list` | string | `"<leader>t"` | List themes |
| `sidebar_toggle` | string | `"<leader>b"` | Toggle sidebar |
| `session_new` | string | `"<leader>n"` | New session |
| `session_list` | string | `"<leader>l"` | List sessions |
| `model_list` | string | `"<leader>m"` | List models |
| `agent_list` | string | `"<leader>a"` | List agents |

See [Keybindings Reference](./keybindings.md) for full list.

---

## Command Settings

Custom slash commands.

```json
{
  "command": {
    "test": {
      "template": "Run the test suite with coverage",
      "description": "Run tests",
      "agent": "build",
      "model": "anthropic/claude-haiku-3-5-20241022",
      "subtask": false
    }
  }
}
```

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `template` | string | Yes | Command template (use `$ARGUMENTS` for args) |
| `description` | string | No | Description shown in command list |
| `agent` | string | No | Agent to use |
| `model` | string | No | Model override |
| `subtask` | boolean | No | Run as subtask |

---

## Agent Settings

Agent configurations and overrides.

```json
{
  "agent": {
    "build": {
      "model": "anthropic/claude-sonnet-4-5-20250929",
      "temperature": 0.7,
      "top_p": 0.9,
      "prompt": "Additional instructions...",
      "description": "Custom description",
      "mode": "primary",
      "color": "#ff5500",
      "max_steps": 50,
      "tools": {
        "*": true,
        "bash": false
      },
      "permission": {
        "edit": "allow",
        "bash": { "*": "ask" },
        "webfetch": "allow"
      },
      "sandbox": {
        "enabled": true,
        "workspace_writable": true,
        "network": "limited"
      },
      "disable": false
    }
  }
}
```

| Option | Type | Description |
|--------|------|-------------|
| `model` | string | Model override |
| `temperature` | number | Generation temperature (0.0-2.0) |
| `top_p` | number | Top-p sampling (0.0-1.0) |
| `prompt` | string | Additional system prompt |
| `description` | string | Agent description |
| `mode` | string | `"primary"`, `"subagent"`, or `"all"` |
| `color` | string | Display color (hex) |
| `max_steps` | number | Maximum steps per turn |
| `tools` | object | Tool enable/disable map |
| `permission` | object | Permission overrides |
| `sandbox` | object | Sandbox overrides |
| `disable` | boolean | Disable this agent |

---

## Provider Settings

Provider configurations.

```json
{
  "provider": {
    "anthropic": {
      "api": "anthropic",
      "name": "Anthropic",
      "env": ["ANTHROPIC_API_KEY"],
      "whitelist": ["claude-sonnet-4-5-*"],
      "blacklist": [],
      "models": {
        "claude-custom": {
          "name": "Custom Claude",
          "context_length": 200000,
          "max_tokens": 8192
        }
      },
      "options": {
        "api_key": "{env:ANTHROPIC_API_KEY}",
        "base_url": "https://api.anthropic.com",
        "timeout": 300000
      }
    }
  }
}
```

| Option | Type | Description |
|--------|------|-------------|
| `api` | string | API type |
| `name` | string | Display name |
| `env` | array | Environment variables for API key |
| `id` | string | Provider ID override |
| `whitelist` | array | Allowed models (patterns) |
| `blacklist` | array | Blocked models (patterns) |
| `models` | object | Model-specific overrides |
| `options` | object | Provider options |

### Provider Options

| Option | Type | Description |
|--------|------|-------------|
| `api_key` | string | API key (supports `{env:VAR}`) |
| `base_url` | string | API base URL |
| `timeout` | number/false | Request timeout in ms |

---

## MCP Settings

Model Context Protocol server configurations.

### Local Server (stdio)

```json
{
  "mcp": {
    "github": {
      "type": "local",
      "command": ["npx", "-y", "@modelcontextprotocol/server-github"],
      "environment": {
        "GITHUB_TOKEN": "{env:GITHUB_TOKEN}"
      },
      "enabled": true,
      "timeout": 30000
    }
  }
}
```

| Option | Type | Description |
|--------|------|-------------|
| `type` | string | `"local"` |
| `command` | array | Command and arguments |
| `environment` | object | Environment variables |
| `enabled` | boolean | Enable/disable |
| `timeout` | number | Timeout in ms |

### Remote Server (SSE)

```json
{
  "mcp": {
    "remote": {
      "type": "remote",
      "url": "https://mcp.example.com/sse",
      "headers": {
        "Authorization": "Bearer {env:TOKEN}"
      },
      "oauth": {
        "client_id": "...",
        "client_secret": "...",
        "scope": "read write"
      },
      "enabled": true,
      "timeout": 30000
    }
  }
}
```

| Option | Type | Description |
|--------|------|-------------|
| `type` | string | `"remote"` |
| `url` | string | Server URL |
| `headers` | object | HTTP headers |
| `oauth` | object | OAuth configuration |
| `enabled` | boolean | Enable/disable |
| `timeout` | number | Timeout in ms |

---

## Permission Settings

Global permission configuration.

```json
{
  "permission": {
    "edit": "allow",
    "bash": {
      "git *": "allow",
      "npm *": "ask",
      "rm *": "deny",
      "*": "ask"
    },
    "skill": {
      "*": "allow"
    },
    "webfetch": "allow",
    "external_directory": "ask"
  }
}
```

| Option | Type | Description |
|--------|------|-------------|
| `edit` | string | Edit file permission |
| `bash` | string/object | Bash command permissions |
| `skill` | string/object | Skill permissions |
| `webfetch` | string | Web fetch permission |
| `external_directory` | string | Access outside project |

**Permission Values**: `"allow"`, `"ask"`, `"deny"`

---

## Tool Settings

Enable or disable tools globally.

```json
{
  "tools": {
    "bash": false,
    "webfetch": true,
    "edit": true
  }
}
```

**Type**: `object` mapping tool names to `boolean`

---

## Compaction Settings

Context compaction behavior.

```json
{
  "compaction": {
    "auto": true,
    "prune": true
  }
}
```

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `auto` | boolean | `true` | Auto-compact when context is full |
| `prune` | boolean | `true` | Remove old tool outputs to save tokens |

---

## Sandbox Settings

Sandboxed execution configuration.

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
      "persist_caches": true,
      "workspace_path": "/workspace"
    },
    "bypass_tools": ["todoread", "todowrite"],
    "keep_alive": true
  }
}
```

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enabled` | boolean | `false` | Enable sandboxing |
| `runtime` | string | `"auto"` | `"auto"`, `"docker"`, `"podman"`, `"lima"`, `"none"` |
| `image` | string | `"wonopcode/sandbox:latest"` | Container image |
| `network` | string | `"limited"` | `"none"`, `"limited"`, `"full"` |
| `bypass_tools` | array | `[]` | Tools that run on host |
| `keep_alive` | boolean | `true` | Keep container running |

### Resource Limits

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `memory` | string | `"2G"` | Memory limit (e.g., `"512M"`, `"4G"`) |
| `cpus` | number | `2.0` | CPU limit |
| `pids` | number | `256` | Process limit |

### Mount Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `workspace_writable` | boolean | `true` | Allow writes to workspace |
| `persist_caches` | boolean | `true` | Persist npm/pip caches |
| `workspace_path` | string | `"/workspace"` | Path in container |

---

## Enterprise Settings

Enterprise configuration.

```json
{
  "enterprise": {
    "url": "https://enterprise.example.com"
  }
}
```

| Option | Type | Description |
|--------|------|-------------|
| `url` | string | Enterprise server URL |

---

## Experimental Settings

Experimental features (may change without notice).

```json
{
  "experimental": {
    "feature_flag": true
  }
}
```

**Type**: `object` with arbitrary keys

---

## Variable Substitution

### Environment Variables

Use `{env:VAR_NAME}` to substitute environment variables:

```json
{
  "provider": {
    "anthropic": {
      "options": {
        "api_key": "{env:ANTHROPIC_API_KEY}"
      }
    }
  }
}
```

### File Contents

Use `{file:path}` to substitute file contents:

```json
{
  "provider": {
    "openai": {
      "options": {
        "api_key": "{file:~/.secrets/openai-key}"
      }
    }
  }
}
```

**Paths**: Relative to config file or absolute (starting with `/` or `~`)

---

## See Also

- [Configuration Guide](../CONFIGURATION.md) - Practical examples
- [Sandboxing](../SANDBOXING.md) - Sandbox details
- [Environment Variables](./environment-variables.md) - All env vars
