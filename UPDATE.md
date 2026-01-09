# Claude CLI Provider MCP Architecture

## Critical Requirements

The Claude CLI provider implementation has two absolute requirements:

1. **MUST use HTTP/SSE transport for MCP** - No stdio/child process spawning
2. **MUST disable default Claude CLI tools** - Only wonopcode tools via MCP should be available

## Current Architecture (Post-Commit 4696cde)

### Transport: HTTP/SSE Only

The `McpTransport` was changed from an enum (supporting both Stdio and Http) to a simple struct containing only a URL:

```rust
// crates/wonopcode-provider/src/claude_cli.rs:62-69
pub struct McpTransport {
    /// URL for the MCP SSE endpoint (e.g., "http://localhost:3000/mcp/sse").
    pub url: String,
}
```

This ensures the Claude CLI provider **only** connects to HTTP/SSE MCP servers, never spawns child processes.

### MCP Config Generation

When `use_custom_tools` is enabled, the provider generates an MCP configuration file that:

1. Points to the HTTP/SSE endpoint (crates/wonopcode-provider/src/claude_cli.rs:339-344):
```rust
mcp_servers.insert(
    "wonopcode-tools".to_string(),
    serde_json::json!({
        "type": "sse",
        "url": mcp_config.transport.url
    }),
);
```

2. Configures Claude CLI to use only MCP tools (crates/wonopcode-provider/src/claude_cli.rs:639-663):
```rust
// Add MCP config if using custom tools
if let Some(ref config_path) = mcp_config_path {
    args.push("--mcp-config".to_string());
    args.push(config_path.to_string_lossy().to_string());

    // Allow our MCP tools
    args.push("--allowedTools".to_string());
    args.push(self.get_allowed_tools_pattern());  // "mcp__wonopcode-tools__*"

    // Disallow Claude's built-in tools
    args.push("--disallowedTools".to_string());
    args.push(Self::builtin_tools_to_disable().to_string());

    // Use acceptEdits permission mode to auto-accept MCP tool calls
    args.push("--permission-mode".to_string());
    args.push("acceptEdits".to_string());
}
```

### Built-in Tools Disabled

The following Claude CLI built-in tools are explicitly disabled when using MCP (crates/wonopcode-provider/src/claude_cli.rs:406-415):

```rust
fn builtin_tools_to_disable() -> &'static str {
    "Bash,Read,Write,Edit,MultiEdit,Glob,Grep,WebSearch,WebFetch,Task,TodoRead,TodoWrite,AskUserQuestion,EnterPlanMode,ExitPlanMode"
}
```

**Rationale**:
- `AskUserQuestion` - Requires interactive stdin which doesn't work when spawned programmatically
- `EnterPlanMode/ExitPlanMode` - Wonopcode provides its own implementation via MCP
- All other tools - Replaced by wonopcode's MCP implementations

### HTTP MCP Server (Headless Mode)

When running in headless mode, wonopcode starts an HTTP server that includes MCP endpoints (crates/wonopcode/src/main.rs:1939-1953):

```rust
// Build MCP HTTP URL for headless mode
let mcp_sse_url = format!("http://{}/mcp/sse", address);

// Create runner config with MCP HTTP transport
let config = RunnerConfig {
    // ... other fields ...
    mcp_url: Some(mcp_sse_url), // Use HTTP transport for MCP
};
```

The MCP HTTP state is created with all wonopcode tools (crates/wonopcode/src/main.rs:2969-3069):

```rust
async fn create_mcp_http_state(
    cwd: &std::path::Path,
    message_url: &str,
) -> anyhow::Result<wonopcode_mcp::McpHttpState> {
    // ... setup permission manager, tool registry, etc ...

    // Create tool registry with all tools
    let mut tools = ToolRegistry::with_builtins();
    tools.register(Arc::new(wonopcode_tools::bash::BashTool));
    tools.register(Arc::new(wonopcode_tools::webfetch::WebFetchTool));
    // ... more tools ...

    // Return MCP state for HTTP serving
}
```

The router serves MCP at `/mcp/sse` and `/mcp/message` (crates/wonopcode-server/src/headless.rs:124-129):

```rust
// Add MCP routes if state is provided
if let Some(mcp) = mcp_state {
    let mcp_router = create_mcp_router(mcp);
    router = router.nest("/mcp", mcp_router);
    info!("MCP HTTP endpoints enabled at /mcp/sse and /mcp/message");
}
```

## Complete Flow

### 1. Start Headless Server
```bash
wonopcode --headless --address 127.0.0.1:3000
```

This:
- Creates an HTTP server on port 3000
- Exposes MCP endpoints at `http://127.0.0.1:3000/mcp/sse` and `/mcp/message`
- Creates a `RunnerConfig` with `mcp_url: Some("http://127.0.0.1:3000/mcp/sse")`

### 2. Create Claude CLI Provider
```rust
let provider = wonopcode_provider::claude_cli::with_custom_tools(
    model_info,
    "http://127.0.0.1:3000/mcp/sse".to_string(),
)?;
```

This creates a provider with:
- `mcp_config.use_custom_tools = true`
- `mcp_config.transport.url = "http://127.0.0.1:3000/mcp/sse"`

### 3. Generate Request
When the provider receives a `generate()` call:

a. It generates an MCP config file:
```json
{
  "mcpServers": {
    "wonopcode-tools": {
      "type": "sse",
      "url": "http://127.0.0.1:3000/mcp/sse"
    }
  }
}
```

b. It spawns Claude CLI with:
```bash
claude \
  -p "your prompt" \
  --output-format stream-json \
  --model claude-sonnet-4-5-20250929 \
  --mcp-config /tmp/wonopcode-mcp-12345-67890.json \
  --allowedTools "mcp__wonopcode-tools__*" \
  --disallowedTools "Bash,Read,Write,Edit,MultiEdit,Glob,Grep,WebSearch,WebFetch,Task,TodoRead,TodoWrite,AskUserQuestion,EnterPlanMode,ExitPlanMode" \
  --permission-mode acceptEdits
```

### 4. Claude CLI Execution

a. Claude CLI reads the MCP config file
b. Connects to `http://127.0.0.1:3000/mcp/sse` via SSE
c. Lists available tools via MCP protocol
d. Uses **only** MCP tools (wonopcode's implementations)
e. Built-in tools are **disabled** and unavailable

### 5. Tool Execution

When Claude wants to use a tool:
- It sends a tool call via MCP to the HTTP server
- The wonopcode HTTP MCP server executes the tool
- Results are returned via MCP protocol
- Claude CLI streams the results back to the provider

## Why This Architecture?

### HTTP/SSE Instead of Stdio

**Before (Stdio)**:
- Claude CLI spawned `wonopcode mcp-serve` as a child process
- Complex lifecycle management
- Shared state issues between TUI and MCP server
- Difficult to debug

**After (HTTP/SSE)**:
- Single wonopcode process serves both TUI and MCP
- MCP server runs in the same process as the runner
- Shared state (permissions, sandbox, tools) is natural
- Easier to debug (all in one process)
- Headless mode: HTTP server serves both TUI clients and MCP

### Disabled Built-in Tools

**Critical**: Claude CLI MUST NOT use its built-in tools when wonopcode is providing tools via MCP.

**Why?**
- **Consistency**: All tool execution goes through wonopcode's permission system
- **State Management**: File operations update wonopcode's snapshot store
- **Customization**: Wonopcode tools have features built-in tools don't (e.g., sandbox support)
- **Control**: Permission checks, sandbox isolation, etc.

## Verification Checklist

To verify the implementation is correct:

- [x] `McpTransport` is a struct with only a `url` field (no stdio option)
- [x] `generate_mcp_config()` only generates SSE configs (`"type": "sse"`)
- [x] `--disallowedTools` includes all built-in tools
- [x] `--allowedTools` restricts to `mcp__wonopcode-tools__*` pattern
- [x] `--permission-mode acceptEdits` is set
- [x] Headless mode creates `McpHttpState` and serves at `/mcp/sse`
- [x] `RunnerConfig` in headless mode sets `mcp_url`
- [x] No `mcp-serve` command in CLI (removed)
- [x] Client only supports SSE transport (stdio support removed from client.rs)

## Known Issues

### Local (Stdio) MCP Servers No Longer Supported

The commit removed support for local stdio MCP servers in the config:

```rust
// crates/wonopcode/src/runner.rs:451-454
McpConfig::Local(_local_config) => {
    // Local (stdio) MCP servers are no longer supported
    warn!(server = %name, "Local (stdio) MCP servers are no longer supported. Use remote (HTTP/SSE) servers instead.");
    continue;
}
```

Users with `mcpServers` config using `command` arrays will see a warning. They should migrate to remote (HTTP/SSE) servers.

## Files Changed in Commit 4696cde

### Core Changes
- `crates/wonopcode-provider/src/claude_cli.rs` - Removed stdio support, HTTP/SSE only
- `crates/wonopcode-mcp/src/client.rs` - Client only connects via SSE
- `crates/wonopcode-mcp/src/server.rs` - Server config is SSE only
- `crates/wonopcode/src/runner.rs` - Removed stdio MCP server support
- `crates/wonopcode/src/main.rs` - Removed `mcp-serve` command, headless uses HTTP MCP

### Architecture Simplification
- Removed ~1400 lines of code
- Single transport type (SSE) instead of two (stdio + SSE)
- Single process architecture instead of multi-process
- Shared state instead of IPC

## Conclusion

The implementation correctly satisfies both requirements:

1. ✅ **Uses HTTP/SSE transport for MCP** - All stdio code removed, only SSE supported
2. ✅ **Disables default Claude tools** - `--disallowedTools` explicitly disables built-in tools

The architecture is simpler, more maintainable, and avoids the complexity of multi-process IPC.
