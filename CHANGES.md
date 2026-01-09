# Implementation Complete: MCP HTTP Server for TUI Mode

## Summary

✅ **Successfully implemented** HTTP/SSE MCP server support for normal `wonopcode` TUI mode.

Claude CLI now uses wonopcode's custom tools via HTTP/SSE in **all modes**:
- `wonopcode` (TUI mode)
- `wonopcode run` (command mode)
- `wonopcode --headless` (headless mode - already worked)

## What Changed

### File Modified
- `crates/wonopcode/src/main.rs` (+82 lines, -2 lines)

### Changes Made

#### 1. Added `start_mcp_server()` Function (Line 2965)
New async function that:
- Binds to `127.0.0.1:0` (random available port)
- Creates MCP HTTP state with all wonopcode tools
- Starts background HTTP server serving `/mcp/sse` and `/mcp/message` endpoints
- Returns tuple of `(mcp_sse_url, server_handle)`
- Uses simplified router (no CORS needed for localhost)

#### 2. Updated `run_command()` Function
**Added after line 613:**
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

**Changed line 626:**
```rust
// Before:
mcp_url: None, // No custom MCP tools in TUI mode

// After:
mcp_url, // Use background MCP server for custom tools
```

**Added at line 743 (shutdown section):**
```rust
// Shutdown MCP server
if let Some(handle) = mcp_server_handle {
    handle.abort();
}
```

#### 3. Updated `run_interactive()` Function
Same three changes as `run_command()`:
- MCP server startup after line 1644
- Updated `mcp_url` field at line 1667
- MCP server shutdown at line 1706 (with graceful 100ms delay)

## How It Works

### Startup Flow
1. User runs `wonopcode` or `wonopcode run "..."`
2. `start_mcp_server(cwd)` is called
3. Server binds to random port (e.g., `127.0.0.1:54321`)
4. Returns URL: `http://127.0.0.1:54321/mcp/sse`
5. URL is passed to `RunnerConfig` as `mcp_url`
6. When Claude CLI provider is created, it uses this URL
7. Claude CLI connects via HTTP/SSE to wonopcode's MCP server
8. Claude CLI uses wonopcode tools (not built-in tools)

### Shutdown Flow
1. User exits wonopcode
2. MCP server handle is aborted
3. (Optional) 100ms grace period for TUI mode
4. Instance cleanup proceeds

## Architecture

### Before
```
┌────────────────┐           ┌──────────────┐
│   wonopcode    │  NO MCP   │  Claude CLI  │
│   (TUI mode)   │ ────────► │ (built-in    │
│                │           │   tools)     │
└────────────────┘           └──────────────┘
```

### After
```
┌────────────────┐           ┌──────────────────┐           ┌──────────────┐
│   wonopcode    │  spawns   │  Background HTTP │  HTTP/SSE │  Claude CLI  │
│   (TUI mode)   │ ────────► │   MCP Server     │ ◄───────► │  (wonopcode  │
│                │           │  (random port)   │           │    tools)    │
└────────────────┘           └──────────────────┘           └──────────────┘
                                     │
                                     │ serves
                                     ▼
                             ┌──────────────┐
                             │ /mcp/sse     │
                             │ /mcp/message │
                             └──────────────┘
```

## Testing

### Build
```bash
cargo build --release
```
✅ Compiles successfully with no warnings

### Run
```bash
./target/release/wonopcode
```

Expected log output:
```
Starting background MCP HTTP server address=127.0.0.1:xxxxx
MCP HTTP server started mcp_url="http://127.0.0.1:xxxxx/mcp/sse"
```

When using Anthropic provider with Claude CLI auth:
```
Using Claude CLI for subscription-based access with custom tools
```

## Benefits

✅ **Unified Architecture** - Same HTTP/SSE MCP server used in all modes
✅ **No Configuration** - Automatic port selection, works out of the box
✅ **Clean Shutdown** - Proper cleanup on exit
✅ **Graceful Fallback** - If MCP server fails to start, continues without custom tools
✅ **Localhost Only** - Binds to 127.0.0.1 for security
✅ **No CORS Complexity** - Simplified for localhost-only use

## Files Created

- `UPDATE.md` - Original architecture documentation
- `IMPLEMENTATION.md` - Detailed implementation guide
- `CHANGES.md` - This file, summary of completed work
- `mcp-tui-mode.patch` - Patch file (for reference)

## Verification

Run these commands to verify:

```bash
# Check that it compiles
cargo build --release

# Check line count changes
git diff --stat crates/wonopcode/src/main.rs
# Expected: 1 file changed, 82 insertions(+), 2 deletions(-)

# Check that start_mcp_server exists
grep -c "async fn start_mcp_server" crates/wonopcode/src/main.rs
# Expected: 1

# Check that mcp_url is used (not None)
grep "mcp_url, // Use background MCP server" crates/wonopcode/src/main.rs | wc -l
# Expected: 2

# Check that shutdown code exists
grep -c "// Shutdown MCP server" crates/wonopcode/src/main.rs
# Expected: 2
```

## Next Steps

To use in production:
```bash
cargo install --path crates/wonopcode --locked
```

The Claude CLI provider will now automatically use wonopcode's custom tools via the background MCP server in all modes.
