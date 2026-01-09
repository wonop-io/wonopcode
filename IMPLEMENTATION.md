# MCP HTTP Server Implementation for TUI Mode

## Summary

Successfully implemented HTTP/SSE MCP server support for normal `wonopcode` TUI mode (not just headless). Claude CLI now uses custom wonopcode tools in all modes.

## Changes Made

### 1. New Function: `start_mcp_server()`

Added before `create_mcp_http_state()` at line 2965 in `crates/wonopcode/src/main.rs`:

```rust
/// Start a background HTTP server for MCP tools.
///
/// This starts an HTTP server on a random available port that serves only the MCP endpoints.
/// Returns the MCP SSE URL and a server handle that can be used to shutdown the server.
async fn start_mcp_server(
    cwd: &std::path::Path,
) -> anyhow::Result<(String, tokio::task::JoinHandle<()>)> {
    use axum::Router;
    use tower_http::cors::{Any, CorsLayer};
    use wonopcode_mcp::create_mcp_router;

    // Bind to a random available port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let local_addr = listener.local_addr()?;

    info!(address = %local_addr, "Starting background MCP HTTP server");

    // Build the URL for the MCP message endpoint
    let mcp_message_url = format!("http://{}/mcp/message", local_addr);

    // Create MCP state
    let mcp_state = create_mcp_http_state(cwd, &mcp_message_url).await?;

    // Create router with just MCP endpoints
    let mcp_router = create_mcp_router(mcp_state);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .nest("/mcp", mcp_router)
        .layer(cors);

    // Build the SSE URL to return
    let mcp_sse_url = format!("http://{}/mcp/sse", local_addr);

    // Spawn the server in the background
    let server_handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(error = %e, "MCP HTTP server error");
        }
        info!("MCP HTTP server shutdown");
    });

    info!(mcp_url = %mcp_sse_url, "MCP HTTP server started");

    Ok((mcp_sse_url, server_handle))
}
```

### 2. Updated `run_interactive()` Function

**After line 1636** (after the closing brace of the auth check), add:

```rust
    // Start background MCP HTTP server for Claude CLI integration
    let (mcp_url, mcp_server_handle) = match start_mcp_server(cwd).await {
        Ok((url, handle)) => (Some(url), Some(handle)),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to start MCP server, Claude CLI will not use custom tools");
            (None, None)
        }
    };
```

**At line 1648**, change:
```rust
mcp_url: None, // No custom MCP tools in TUI mode
```
to:
```rust
mcp_url, // Use background MCP server for custom tools
```

**Before line 1682** (before `instance.dispose().await`), add:

```rust
    // Shutdown MCP server
    if let Some(handle) = mcp_server_handle {
        handle.abort();
        // Give it a moment to shutdown gracefully
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
```

### 3. Updated `run_command()` Function

**After line 613** (after the closing brace of the auth check), add:

```rust
    // Start background MCP HTTP server for Claude CLI integration
    let (mcp_url, mcp_server_handle) = match start_mcp_server(cwd).await {
        Ok((url, handle)) => (Some(url), Some(handle)),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to start MCP server, Claude CLI will not use custom tools");
            (None, None)
        }
    };
```

**At line 625**, change:
```rust
mcp_url: None, // No custom MCP tools in TUI mode
```
to:
```rust
mcp_url, // Use background MCP server for custom tools
```

**Before line 733** (before `instance.dispose().await`), add:

```rust
    // Shutdown MCP server
    if let Some(handle) = mcp_server_handle {
        handle.abort();
    }
```

## Architecture

### Before (Only Headless Mode)
```
wonopcode --headless → HTTP Server → MCP → Claude CLI
wonopcode (TUI)      → NO MCP     → Claude CLI (uses built-in tools)
```

### After (All Modes)
```
wonopcode --headless → HTTP Server → MCP → Claude CLI
wonopcode (TUI)      → Background HTTP Server → MCP → Claude CLI
wonopcode run        → Background HTTP Server → MCP → Claude CLI
```

## How It Works

1. When `wonopcode` starts in TUI or command mode, it:
   - Binds to `127.0.0.1:0` (random available port)
   - Starts a background HTTP server with only MCP endpoints
   - Gets the actual port (e.g., `127.0.0.1:54321`)
   - Creates MCP URL: `http://127.0.0.1:54321/mcp/sse`

2. When creating the Claude CLI provider:
   - Passes `mcp_url` to `RunnerConfig`
   - Claude CLI connects to this local MCP server
   - Uses wonopcode tools instead of built-in tools

3. On exit:
   - Aborts the background MCP server task
   - Cleans up resources

## Benefits

✅ Claude CLI uses custom tools in ALL modes (not just headless)
✅ Single process architecture - no child process spawning
✅ Automatic port selection - no configuration needed
✅ Clean shutdown on exit
✅ Consistent behavior across all modes

## Files Changed

- `crates/wonopcode/src/main.rs` - Added `start_mcp_server()`, updated `run_interactive()` and `run_command()`

## Testing

Build the project:
```bash
cargo build --release
```

Test in TUI mode:
```bash
./target/release/wonopcode
```

Test in command mode:
```bash
./target/release/wonopcode run "list files in current directory"
```

Check logs for:
- "Starting background MCP HTTP server"
- "MCP HTTP server started"
- "Using Claude CLI for subscription-based access with custom tools"

## Notes

- The MCP server runs on a random port (127.0.0.1:0) to avoid conflicts
- Only listens on localhost for security
- Uses the same `create_mcp_http_state()` as headless mode
- Gracefully handles startup failures (falls back to no custom tools)
