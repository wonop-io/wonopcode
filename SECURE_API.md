# Secure API Implementation

This document describes the secret key authentication feature for protecting server endpoints.

## Overview

The wonopcode server can be protected with a secret key. When enabled, clients must provide the key via HTTP headers to authenticate their requests. This protects **all server endpoints** (except `/health` for monitoring).

## Protected Endpoints

When secret authentication is enabled, the following endpoints require authentication:

### Headless Server Endpoints
- `/state` - Get current state
- `/events` - SSE event stream
- `/action/*` - All action endpoints (prompt, cancel, model, etc.)

### MCP Endpoints (if enabled)
- `/mcp/sse` - MCP SSE connection
- `/mcp/message` - MCP message endpoint

### Public Endpoints (no auth required)
- `/health` - Health check for monitoring systems

## Setting the Secret

The secret can be configured via (in order of priority):

1. **CLI argument**: `--secret <key>`
2. **Environment variable**: `WONOPCODE_SECRET`
3. **Config file**: `server.api_key` in wonopcode.json

### Using CLI Argument

```bash
# Interactive mode
wonopcode --secret "your-secret-key"

# Headless mode
wonopcode --headless --secret "your-secret-key"

# Run command
wonopcode run --secret "your-secret-key" "Hello"

# Connect to a protected server
wonopcode --connect :3000 --secret "your-secret-key"
```

### Using Environment Variable

```bash
export WONOPCODE_SECRET="your-secret-key"
wonopcode
```

### Using Config File (wonopcode.json)

```jsonc
{
  "server": {
    "port": 8080,
    // Use environment variable substitution
    "api_key": "{env:MY_SECRET_KEY}"
  }
}
```

Or with a direct value:

```jsonc
{
  "server": {
    "port": 8080,
    "api_key": "your-secret-api-key-here"
  }
}
```

## Client Configuration

When connecting to a secured server, clients must provide the secret via headers.

### Remote MCP Server Configuration

```jsonc
{
  "mcp": {
    "my-secure-server": {
      "type": "remote",
      "url": "https://example.com/mcp/sse",
      "headers": {
        "X-API-Key": "{env:MY_SERVER_SECRET}"
      }
    }
  }
}
```

Or using Authorization header:

```jsonc
{
  "mcp": {
    "my-secure-server": {
      "type": "remote",
      "url": "https://example.com/mcp/sse",
      "headers": {
        "Authorization": "Bearer {env:MY_SERVER_SECRET}"
      }
    }
  }
}
```

### Connecting to a Remote Headless Server

When using `wonopcode --connect` to connect to a remote headless server with authentication:

```bash
# Via CLI argument
wonopcode --connect "http://example.com:8080" --secret "your-secret-key"

# Via environment variable
export WONOPCODE_SECRET="your-secret-key"
wonopcode --connect "http://example.com:8080"
```

## Authentication Protocol

The server accepts secrets via two header formats:

1. **X-API-Key header**: `X-API-Key: <your-secret>`
2. **Authorization header**: `Authorization: Bearer <your-secret>`

If a secret is configured but not provided by the client, the server returns:
- HTTP 401 Unauthorized
- `{"error": "Authentication required"}`

If an invalid secret is provided:
- HTTP 401 Unauthorized  
- `{"error": "Invalid API key"}`

## Security Considerations

1. **Use HTTPS**: Secrets should only be transmitted over HTTPS to prevent interception.

2. **Secure Key Generation**: Generate strong random keys:
   ```bash
   openssl rand -base64 32
   ```

3. **Environment Variables**: Prefer environment variables over config files for sensitive keys.

4. **Constant-Time Comparison**: The server uses constant-time comparison to prevent timing attacks.

5. **Never Log Keys**: Secrets are never logged, only authentication failures are reported.

## Implementation Details

### Server Side

- **`McpHttpState.with_api_key(key)`**: Sets the secret for MCP authentication
- **`create_mcp_router(state)`**: Automatically adds auth middleware if secret is configured
- **`create_headless_router_with_options(state, mcp_state, secret)`**: Creates router with full authentication
- Uses `subtle::ConstantTimeEq` for secure key comparison

### Client Side

- **`RemoteBackend.with_api_key(address, secret)`**: Creates backend with authentication
- **`SseConfig.headers`**: Custom headers sent with all requests
- **`ServerConfig.headers`**: Headers configured in MCP server config
- Both `X-API-Key` and `Authorization: Bearer` formats are supported

### Files Modified

| File | Description |
|------|-------------|
| `crates/wonopcode-mcp/src/http_serve.rs` | Auth middleware for MCP endpoints |
| `crates/wonopcode-mcp/src/sse.rs` | Custom headers support in SseConfig |
| `crates/wonopcode-mcp/src/client.rs` | Header propagation to transport |
| `crates/wonopcode-core/src/config.rs` | api_key field in ServerConfig |
| `crates/wonopcode-server/src/headless.rs` | Auth middleware for all headless endpoints |
| `crates/wonopcode-tui/src/backend.rs` | API key support in RemoteBackend |
| `crates/wonopcode/src/main.rs` | CLI flag and wiring |
