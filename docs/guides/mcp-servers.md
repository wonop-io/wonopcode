# MCP Servers

Extend wonopcode's capabilities by connecting to Model Context Protocol (MCP) servers.

---

## What is MCP?

The **Model Context Protocol** is an open standard that allows AI assistants to connect to external tool servers. With MCP, you can:

- Add tools without modifying wonopcode
- Connect to existing MCP servers (GitHub, databases, etc.)
- Create custom tools for your workflow
- Share tool configurations across teams

---

## Quick Start

Add an MCP server to your `config.json`:

```json
{
  "mcp": {
    "filesystem": {
      "type": "local",
      "command": ["npx", "-y", "@modelcontextprotocol/server-filesystem", "/path/to/allowed/dir"]
    }
  }
}
```

Restart wonopcode. The server's tools are now available.

---

## Server Types

### Local Servers (stdio)

Local servers run as subprocesses and communicate via stdin/stdout. Use `"type": "local"`.

```json
{
  "mcp": {
    "my-server": {
      "type": "local",
      "command": ["path/to/server", "--flag", "value"],
      "environment": {
        "API_KEY": "{env:MY_API_KEY}"
      }
    }
  }
}
```

### Remote Servers (SSE)

Remote servers communicate over HTTP using Server-Sent Events. Use `"type": "remote"`.

```json
{
  "mcp": {
    "remote-server": {
      "type": "remote",
      "url": "https://mcp.example.com/sse",
      "headers": {
        "Authorization": "Bearer {env:API_TOKEN}"
      }
    }
  }
}
```

---

## Popular MCP Servers

### Filesystem Server

Access files in specified directories:

```json
{
  "mcp": {
    "filesystem": {
      "type": "local",
      "command": ["npx", "-y", "@modelcontextprotocol/server-filesystem", "~/documents", "~/projects"]
    }
  }
}
```

**Tools Provided**:
- `read_file` - Read file contents
- `write_file` - Write to files
- `list_directory` - List directory contents
- `search_files` - Search for files

---

### GitHub Server

Interact with GitHub repositories:

```json
{
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

**Tools Provided**:
- `create_issue` - Create GitHub issues
- `list_issues` - List repository issues
- `create_pull_request` - Open PRs
- `get_file_contents` - Read files from repos
- `search_code` - Search code on GitHub

**Setup**:
1. Create a GitHub personal access token
2. Set `GITHUB_TOKEN` environment variable
3. Add server configuration

---

### PostgreSQL Server

Query PostgreSQL databases:

```json
{
  "mcp": {
    "postgres": {
      "type": "local",
      "command": ["npx", "-y", "@modelcontextprotocol/server-postgres"],
      "environment": {
        "DATABASE_URL": "{env:DATABASE_URL}"
      }
    }
  }
}
```

**Tools Provided**:
- `query` - Execute SQL queries
- `list_tables` - List database tables
- `describe_table` - Get table schema

**Setup**:
1. Set `DATABASE_URL` (e.g., `postgres://user:pass@localhost/dbname`)
2. Add server configuration

---

### Slack Server

Send messages and interact with Slack:

```json
{
  "mcp": {
    "slack": {
      "type": "local",
      "command": ["npx", "-y", "@modelcontextprotocol/server-slack"],
      "environment": {
        "SLACK_TOKEN": "{env:SLACK_TOKEN}"
      }
    }
  }
}
```

**Tools Provided**:
- `send_message` - Post to channels
- `list_channels` - List available channels
- `search_messages` - Search message history

---

### Memory Server

Persistent key-value storage:

```json
{
  "mcp": {
    "memory": {
      "type": "local",
      "command": ["npx", "-y", "@modelcontextprotocol/server-memory"]
    }
  }
}
```

**Tools Provided**:
- `store` - Save data
- `retrieve` - Get data
- `list_keys` - List stored keys

---

## Configuration Options

### Local Server Configuration

```json
{
  "mcp": {
    "server-name": {
      "type": "local",
      "command": ["executable", "arg1", "arg2"],
      "environment": {
        "KEY": "value",
        "SECRET": "{env:ENV_VAR}"
      },
      "enabled": true,
      "timeout": 30000
    }
  }
}
```

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `type` | `"local"` | Yes | Server type identifier |
| `command` | `string[]` | Yes | Command and arguments as array |
| `environment` | `object` | No | Environment variables |
| `enabled` | `boolean` | No | Enable/disable server (default: true) |
| `timeout` | `number` | No | Connection timeout in ms |

### Remote Server Configuration

```json
{
  "mcp": {
    "server-name": {
      "type": "remote",
      "url": "https://mcp.example.com/sse",
      "headers": {
        "Authorization": "Bearer {env:API_TOKEN}"
      },
      "oauth": {
        "client_id": "your-client-id",
        "client_secret": "{env:CLIENT_SECRET}",
        "scope": "read write"
      },
      "enabled": true,
      "timeout": 30000
    }
  }
}
```

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `type` | `"remote"` | Yes | Server type identifier |
| `url` | `string` | Yes | Server URL (SSE endpoint) |
| `headers` | `object` | No | HTTP headers to send |
| `oauth` | `object` | No | OAuth configuration |
| `enabled` | `boolean` | No | Enable/disable server (default: true) |
| `timeout` | `number` | No | Connection timeout in ms |

### Environment Variable Expansion

Use `{env:VAR_NAME}` to reference environment variables:

```json
{
  "environment": {
    "API_KEY": "{env:MY_API_KEY}",
    "HOME": "{env:HOME}"
  }
}
```

---

## Managing MCP Servers

### View Connected Servers

```
/mcp
```

Shows:
```
MCP Servers:
  ✓ github (3 tools)
  ✓ postgres (2 tools)
  ✗ slack (disconnected)
```

### Reconnect a Server

```
/mcp reconnect github
```

### List Available Tools

```
/mcp tools github
```

Shows:
```
github tools:
  - create_issue: Create a new GitHub issue
  - list_issues: List issues in a repository
  - create_pull_request: Create a pull request
  ...
```

---

## Using MCP Tools

Once connected, MCP tools work like built-in tools:

```
Create a GitHub issue titled "Fix login bug" in the myorg/myrepo repository
```

The AI will use the `github:create_issue` tool:

```
┌─ github:create_issue ─────────────────────────────────────────┐
│ Repository: myorg/myrepo                                       │
│ Title: Fix login bug                                           │
│ Created: #42                                                   │
└────────────────────────────────────────────────────────────────┘
```

---

## Creating Custom MCP Servers

### Server Structure

An MCP server implements the Model Context Protocol:

1. **Initialize**: Handle connection setup
2. **List Tools**: Describe available tools
3. **Execute**: Handle tool calls
4. **Resources**: (Optional) Provide data resources

### Simple Python Server

```python
#!/usr/bin/env python3
import json
import sys

def handle_request(request):
    method = request.get("method")
    
    if method == "initialize":
        return {"capabilities": {"tools": True}}
    
    elif method == "tools/list":
        return {
            "tools": [{
                "name": "hello",
                "description": "Say hello",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Name to greet"}
                    },
                    "required": ["name"]
                }
            }]
        }
    
    elif method == "tools/call":
        tool_name = request["params"]["name"]
        args = request["params"]["arguments"]
        
        if tool_name == "hello":
            return {"content": [{"type": "text", "text": f"Hello, {args['name']}!"}]}
    
    return {"error": "Unknown method"}

# Main loop
for line in sys.stdin:
    request = json.loads(line)
    response = handle_request(request)
    response["id"] = request.get("id")
    print(json.dumps(response), flush=True)
```

### Using Your Server

```json
{
  "mcp": {
    "my-server": {
      "type": "local",
      "command": ["python3", "/path/to/my_server.py"]
    }
  }
}
```

### TypeScript Server (Recommended)

Use the official MCP SDK:

```bash
npm init -y
npm install @modelcontextprotocol/sdk
```

```typescript
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";

const server = new Server({
  name: "my-server",
  version: "1.0.0"
}, {
  capabilities: {
    tools: {}
  }
});

server.setRequestHandler("tools/list", async () => ({
  tools: [{
    name: "hello",
    description: "Say hello",
    inputSchema: {
      type: "object",
      properties: {
        name: { type: "string", description: "Name to greet" }
      },
      required: ["name"]
    }
  }]
}));

server.setRequestHandler("tools/call", async (request) => {
  const { name, arguments: args } = request.params;
  
  if (name === "hello") {
    return {
      content: [{ type: "text", text: `Hello, ${args.name}!` }]
    };
  }
  
  throw new Error(`Unknown tool: ${name}`);
});

const transport = new StdioServerTransport();
server.connect(transport);
```

---

## Authentication

### OAuth Servers

Some MCP servers require OAuth authentication:

```json
{
  "mcp": {
    "oauth-server": {
      "type": "remote",
      "url": "https://mcp.example.com/sse",
      "oauth": {
        "client_id": "your-client-id",
        "client_secret": "{env:CLIENT_SECRET}",
        "scope": "read write"
      }
    }
  }
}
```

On first connection, you'll be prompted to authenticate via browser.

### API Key Authentication

```json
{
  "mcp": {
    "api-server": {
      "type": "remote",
      "url": "https://mcp.example.com/sse",
      "headers": {
        "Authorization": "Bearer {env:API_KEY}"
      }
    }
  }
}
```

---

## Sandboxing and MCP

MCP servers run on the **host system**, not in the sandbox. This means:

- MCP tools can access host resources
- Be careful with file access permissions
- Network requests go through host network

If you need sandboxed MCP tools, run the MCP server itself in a container.

---

## Troubleshooting

### Server Won't Connect

```
Error: MCP server 'github' failed to connect
```

**Solutions**:
1. Check the command exists: `which npx`
2. Check environment variables are set
3. Run the command manually to see errors
4. Check network connectivity for remote servers

### Tool Not Found

```
Error: Tool 'github:unknown_tool' not found
```

**Solutions**:
1. Check tool name with `/mcp tools github`
2. Server may need updating
3. Reconnect with `/mcp reconnect github`

### Timeout Errors

```
Error: MCP server timed out
```

**Solutions**:
1. Increase timeout in config
2. Check server is responding
3. Check network latency for remote servers

### Permission Denied

```
Error: GITHUB_TOKEN not set
```

**Solutions**:
1. Set required environment variable
2. Check variable name matches config
3. Restart wonopcode after setting

---

## Best Practices

### 1. Use Environment Variables for Secrets

Never hardcode tokens in config:

```json
{
  "env": {
    "TOKEN": "{env:MY_TOKEN}"  // ✓ Good
    "TOKEN": "sk-secret123"   // ✗ Bad
  }
}
```

### 2. Limit Server Permissions

Only give servers access to what they need:

```json
{
  "mcp": {
    "filesystem": {
      "type": "local",
      "command": ["npx", "-y", "@modelcontextprotocol/server-filesystem", "~/specific-project"]
    }
  }
}
```

### 3. Disable Unused Servers

```json
{
  "mcp": {
    "unused-server": {
      "enabled": false
    }
  }
}
```

### 4. Monitor Server Activity

Watch MCP logs for issues:

```bash
RUST_LOG=wonopcode_mcp=debug wonopcode
```

---

## Resources

- [MCP Specification](https://spec.modelcontextprotocol.io/)
- [MCP TypeScript SDK](https://github.com/modelcontextprotocol/typescript-sdk)
- [Official MCP Servers](https://github.com/modelcontextprotocol/servers)

---

## Next Steps

- [Custom Agents](./custom-agents.md) - Configure MCP tools per agent
- [Configuration](../CONFIGURATION.md) - Full MCP settings
- [Tips & Tricks](./tips-and-tricks.md) - Advanced usage patterns
