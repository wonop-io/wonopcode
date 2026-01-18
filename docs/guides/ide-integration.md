# IDE Integration

Connect wonopcode to your favorite code editor for seamless AI-assisted development.

---

## Overview

Wonopcode supports the **Agent Client Protocol (ACP)**, allowing integration with:

- Visual Studio Code
- Zed
- Cursor
- Neovim
- Other ACP-compatible editors

---

## How It Works

```
┌─────────────────────────────────────────────────────────────┐
│                       Your Editor                            │
│                                                              │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐  │
│  │   Editor     │    │     ACP      │    │  Wonopcode   │  │
│  │   Buffer     │◄──►│   Client     │◄──►│   Server     │  │
│  └──────────────┘    └──────────────┘    └──────────────┘  │
│                                                              │
│  Features:                                                   │
│  - Inline completions                                        │
│  - Code actions                                              │
│  - Chat panel                                                │
│  - File synchronization                                      │
└─────────────────────────────────────────────────────────────┘
```

The ACP protocol uses JSON-RPC 2.0 over stdio with newline-delimited JSON (ndjson).

---

## Starting the Server

### Automatic (Recommended)

Most editor extensions start the server automatically using the `acp` subcommand.

### Manual

```bash
# Start ACP server (stdio mode for IDE integration)
wonopcode acp

# Start HTTP server on specific port
wonopcode serve --port 3000

# With specific config
wonopcode serve --config ~/my-config.json
```

The `acp` command runs in stdio mode for direct IDE communication. The `serve` command runs an HTTP server for shared access.

---

## Visual Studio Code

### Installation

1. Install the wonopcode extension from VS Code marketplace
2. Or install from VSIX:
   ```bash
   code --install-extension wonopcode.vsix
   ```

### Configuration

Open VS Code settings (`Ctrl+,` / `Cmd+,`):

```json
{
  "wonopcode.serverPath": "/usr/local/bin/wonopcode",
  "wonopcode.model": "anthropic/claude-sonnet-4-5-20250929",
  "wonopcode.sandbox.enabled": true
}
```

### Features

| Feature | Shortcut | Description |
|---------|----------|-------------|
| Chat Panel | `Ctrl+Shift+W` | Open AI chat |
| Inline Suggest | `Tab` | Accept suggestion |
| Explain Code | `Ctrl+Shift+E` | Explain selection |
| Refactor | `Ctrl+Shift+R` | Refactor selection |
| Fix Error | `Ctrl+.` | Fix diagnostic |

### Commands

Open Command Palette (`Ctrl+Shift+P` / `Cmd+Shift+P`):

- `Wonopcode: Open Chat` - Open chat panel
- `Wonopcode: Explain Selection` - Explain highlighted code
- `Wonopcode: Refactor Selection` - Refactor highlighted code
- `Wonopcode: Generate Tests` - Generate tests for file
- `Wonopcode: Change Model` - Switch AI model
- `Wonopcode: Toggle Sandbox` - Enable/disable sandbox

---

## Zed

### Installation

Zed has built-in support for ACP-compatible assistants.

1. Open Zed settings (`Cmd+,`)
2. Add wonopcode configuration:

```json
{
  "assistant": {
    "provider": "wonopcode",
    "server_path": "/usr/local/bin/wonopcode",
    "model": "anthropic/claude-sonnet-4-5-20250929"
  }
}
```

### Features

| Feature | Shortcut | Description |
|---------|----------|-------------|
| Assistant Panel | `Cmd+?` | Open AI assistant |
| Inline Assist | `Cmd+Enter` | Inline completion |
| Transform | `Cmd+Shift+T` | Transform selection |

### Usage

1. Select code
2. Press `Cmd+Enter`
3. Type instruction
4. Press `Enter` to apply

---

## Cursor

Cursor has its own AI assistant, but you can use wonopcode as an alternative.

### Configuration

1. Open Cursor settings
2. Go to AI settings
3. Select "Custom" provider
4. Configure:

```json
{
  "ai.provider": "custom",
  "ai.customProvider.command": "wonopcode",
  "ai.customProvider.args": ["serve", "--port", "3001"]
}
```

### Features

Most Cursor AI features work with wonopcode:
- Tab completion
- Chat (`Ctrl+L`)
- Inline edits (`Ctrl+K`)
- Code generation

---

## Neovim

### Using nvim-acp

Install with your package manager:

```lua
-- lazy.nvim
{
  "wonopcode/nvim-acp",
  config = function()
    require("nvim-acp").setup({
      server_cmd = { "wonopcode", "serve" },
      model = "anthropic/claude-sonnet-4-5-20250929",
      sandbox = { enabled = true }
    })
  end
}
```

```vim
" vim-plug
Plug 'wonopcode/nvim-acp'
```

### Configuration

```lua
require("nvim-acp").setup({
  server_cmd = { "wonopcode", "serve" },
  model = "anthropic/claude-sonnet-4-5-20250929",
  
  -- Key mappings
  mappings = {
    chat_open = "<leader>wc",
    explain = "<leader>we",
    refactor = "<leader>wr",
    complete = "<C-x><C-a>",
  },
  
  -- Sandbox settings
  sandbox = {
    enabled = true,
    network = "limited"
  }
})
```

### Key Mappings

| Mapping | Mode | Description |
|---------|------|-------------|
| `<leader>wc` | n | Open chat window |
| `<leader>we` | n, v | Explain code |
| `<leader>wr` | n, v | Refactor code |
| `<C-x><C-a>` | i | Trigger completion |
| `<leader>wt` | n | Generate tests |

### Commands

```vim
:WonopChat          " Open chat window
:WonopExplain       " Explain current function
:WonopRefactor      " Refactor selection
:WonopTests         " Generate tests
:WonopModel         " Change model
```

---

## Using with Multiple Editors

Wonopcode can run as a shared server for multiple editors:

### Start Shared Server

```bash
# Start server in background
wonopcode serve --port 3000 &
```

### Connect Editors

Configure each editor to connect to the same port:

**VS Code**:
```json
{
  "wonopcode.serverUrl": "http://localhost:3000"
}
```

**Neovim**:
```lua
require("nvim-acp").setup({
  server_url = "http://localhost:3000"
})
```

### Benefits

- Shared session state
- Consistent context across editors
- Single server resource usage

---

## Terminal + Editor Workflow

Use wonopcode in terminal alongside your editor:

### Split Terminal Workflow

```
┌─────────────────────┬─────────────────────┐
│                     │                     │
│      Editor         │    Wonopcode TUI    │
│                     │                     │
│  - Edit files       │  - AI conversation  │
│  - Navigate code    │  - Run commands     │
│  - View diffs       │  - View changes     │
│                     │                     │
└─────────────────────┴─────────────────────┘
```

### tmux Setup

```bash
# Create split with editor and wonopcode
tmux new-session -d -s dev
tmux send-keys 'nvim .' C-m
tmux split-window -h
tmux send-keys 'wonopcode' C-m
tmux attach -t dev
```

### File Watching

Wonopcode automatically detects external file changes:

1. Edit file in your editor
2. Save
3. Wonopcode sees the change
4. AI can reference updated content

---

## Configuration Sync

Keep settings consistent across terminal and IDE:

### Shared Config

Use the same `config.json` for both:

```json
// ~/.config/wonopcode/config.json
{
  "model": "anthropic/claude-sonnet-4-5-20250929",
  "sandbox": {
    "enabled": true
  }
}
```

### Editor Overrides

Editors can override specific settings:

```json
// VS Code settings
{
  "wonopcode.config": "~/.config/wonopcode/config.json",
  "wonopcode.overrides": {
    "model": "anthropic/claude-haiku-3-5-20241022"  // Faster for completions
  }
}
```

---

## Troubleshooting

### Server Won't Start

```
Error: Failed to start wonopcode server
```

**Solutions**:
1. Check wonopcode is installed: `wonopcode --version`
2. Check port isn't in use: `lsof -i :3000`
3. Check logs: `wonopcode serve --debug`

### No Completions

**Solutions**:
1. Check API key is set
2. Check server is running
3. Check editor is connected
4. Try restarting editor

### Slow Responses

**Solutions**:
1. Use faster model for completions
2. Reduce context size
3. Check network connectivity
4. Try local server instead of remote

### Sandbox Conflicts

If sandbox causes issues with IDE integration:

```json
{
  "sandbox": {
    "bypass_tools": ["read", "glob", "grep"]  // Read operations on host
  }
}
```

---

## Performance Tips

### 1. Use Appropriate Models

- **Completions**: Fast model (Haiku, GPT-3.5)
- **Chat**: Full model (Sonnet, GPT-4)
- **Refactoring**: Full model

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929",
  "small_model": "anthropic/claude-haiku-3-5-20241022"
}
```

### 2. Local Server

Run server locally for lowest latency:

```bash
wonopcode serve --port 3000
```

### 3. Limit Context

Configure maximum context for completions:

```json
{
  "completion": {
    "max_context_lines": 100
  }
}
```

---

## ACP Protocol Reference

The Agent Client Protocol (ACP) enables communication between IDEs and wonopcode. This section documents the protocol for developers building integrations.

### Protocol Overview

ACP uses JSON-RPC 2.0 over stdio with newline-delimited JSON (ndjson):

1. **Initialize**: Client sends `initialize` request, agent responds with capabilities
2. **Authentication**: Client authenticates using available methods
3. **Session Management**: Create or load sessions with `session/new` or `session/load`
4. **Prompting**: Send prompts with `session/prompt`, receive streaming updates
5. **Permissions**: Agent requests permission for tool execution when needed

### Request/Response Flow

```
Client                                    Agent
  │                                         │
  │─────── initialize ─────────────────────►│
  │◄────── capabilities ───────────────────│
  │                                         │
  │─────── authenticate ───────────────────►│
  │◄────── success ────────────────────────│
  │                                         │
  │─────── session/new ────────────────────►│
  │◄────── session info ───────────────────│
  │                                         │
  │─────── session/prompt ─────────────────►│
  │◄────── session/update (streaming) ─────│
  │◄────── session/update (streaming) ─────│
  │◄────── prompt response ────────────────│
  │                                         │
```

### Methods

#### `initialize`

Initialize the connection and exchange capabilities.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocolVersion": 1,
    "clientCapabilities": {}
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": 1,
    "agentCapabilities": {
      "loadSession": true,
      "mcpCapabilities": { "http": true, "sse": true },
      "promptCapabilities": { "embeddedContext": true, "image": true }
    },
    "authMethods": [
      { "id": "api_key", "name": "API Key", "description": "Authenticate with API key" }
    ],
    "agentInfo": {
      "name": "wonopcode",
      "version": "0.1.0"
    }
  }
}
```

#### `session/new`

Create a new session.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "session/new",
  "params": {
    "cwd": "/path/to/project",
    "mcpServers": []
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "sessionId": "sess_abc123",
    "models": {
      "currentModelId": "anthropic/claude-sonnet-4-5-20250929",
      "availableModels": [
        { "modelId": "anthropic/claude-sonnet-4-5-20250929", "name": "Claude Sonnet" }
      ]
    },
    "modes": {
      "currentModeId": "build",
      "availableModes": [
        { "id": "build", "name": "Build", "description": "Default coding agent" },
        { "id": "plan", "name": "Plan", "description": "Planning agent" }
      ]
    }
  }
}
```

#### `session/prompt`

Send a prompt and receive streaming updates.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "session/prompt",
  "params": {
    "sessionId": "sess_abc123",
    "prompt": [
      { "type": "text", "text": "Explain this function" }
    ]
  }
}
```

**Streaming Updates (notifications):**
```json
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess_abc123",
    "update": {
      "sessionUpdate": "agent_message_chunk",
      "content": { "type": "text", "text": "This function..." }
    }
  }
}
```

**Final Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "stopReason": "end_turn"
  }
}
```

#### `session/cancel`

Cancel an ongoing prompt (notification, no response).

```json
{
  "jsonrpc": "2.0",
  "method": "session/cancel",
  "params": {
    "sessionId": "sess_abc123"
  }
}
```

### Session Updates

The agent sends `session/update` notifications during prompt processing:

| Update Type | Description |
|-------------|-------------|
| `agent_message_chunk` | Text output from the agent |
| `user_message_chunk` | User message (for replay) |
| `agent_thought_chunk` | Agent reasoning/thinking |
| `tool_call` | Tool execution started |
| `tool_call_update` | Tool execution progress/completion |
| `plan` | Todo list updates |
| `available_commands_update` | Available slash commands |

#### Tool Call Updates

```json
{
  "sessionUpdate": "tool_call",
  "toolCallId": "tc_123",
  "title": "Reading file",
  "kind": "read",
  "status": "pending",
  "locations": [{ "path": "/src/main.rs" }],
  "rawInput": { "filePath": "/src/main.rs" }
}
```

Tool kinds: `execute`, `fetch`, `edit`, `search`, `read`, `other`

Tool statuses: `pending`, `in_progress`, `completed`, `failed`

### Permission Requests

When the agent needs permission for an operation:

**Request (from agent):**
```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "method": "permission/request",
  "params": {
    "sessionId": "sess_abc123",
    "toolCall": {
      "toolCallId": "tc_456",
      "status": "pending",
      "title": "Execute command",
      "kind": "execute",
      "locations": [],
      "rawInput": { "command": "npm install" }
    },
    "options": [
      { "optionId": "allow_once", "kind": "allow_once", "name": "Allow Once" },
      { "optionId": "allow_always", "kind": "allow_always", "name": "Always Allow" },
      { "optionId": "reject", "kind": "reject_once", "name": "Reject" }
    ]
  }
}
```

**Response (from client):**
```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "result": {
    "outcome": {
      "outcome": "approved",
      "optionId": "allow_once"
    }
  }
}
```

### Prompt Parts

Prompts can include different content types:

```json
{
  "prompt": [
    { "type": "text", "text": "Explain this code:" },
    { "type": "resource_link", "uri": "file:///src/main.rs" },
    { "type": "image", "data": "base64...", "mimeType": "image/png" }
  ]
}
```

| Part Type | Description |
|-----------|-------------|
| `text` | Plain text content |
| `image` | Image with base64 data or URI |
| `resource_link` | Reference to a file/resource |
| `resource` | Embedded resource content |

---

## Building an ACP Client

To build your own ACP client:

1. **Spawn the agent**: `wonopcode acp`
2. **Communicate over stdio**: Send JSON-RPC requests, receive responses
3. **Handle streaming**: Process `session/update` notifications
4. **Handle permissions**: Respond to `permission/request` with user choices

### Example (Node.js)

```javascript
const { spawn } = require('child_process');
const readline = require('readline');

const agent = spawn('wonopcode', ['acp']);
const rl = readline.createInterface({ input: agent.stdout });

// Send request
function send(request) {
  agent.stdin.write(JSON.stringify(request) + '\n');
}

// Handle responses
rl.on('line', (line) => {
  const message = JSON.parse(line);
  if (message.method === 'session/update') {
    console.log('Update:', message.params.update);
  } else if (message.id) {
    console.log('Response:', message.result || message.error);
  }
});

// Initialize
send({
  jsonrpc: '2.0',
  id: 1,
  method: 'initialize',
  params: { protocolVersion: 1 }
});
```

---

## Next Steps

- [Configuration](../CONFIGURATION.md) - Full settings reference
- [Tools Overview](./tools-overview.md) - Available tools in editors
- [Tips & Tricks](./tips-and-tricks.md) - Productivity techniques
