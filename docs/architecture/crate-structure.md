# Crate Structure

Detailed breakdown of wonopcode's workspace organization.

---

## Workspace Overview

```
wonopcode/
├── Cargo.toml                 # Workspace manifest
└── crates/
    ├── wonopcode/             # Main binary
    ├── wonopcode-core/        # Core business logic
    ├── wonopcode-provider/    # AI provider implementations
    ├── wonopcode-tools/       # Tool implementations
    ├── wonopcode-tui/         # Terminal UI
    ├── wonopcode-server/      # HTTP/ACP server
    ├── wonopcode-mcp/         # MCP client
    ├── wonopcode-lsp/         # LSP client
    ├── wonopcode-sandbox/     # Sandboxed execution
    ├── wonopcode-snapshot/    # File change tracking
    ├── wonopcode-storage/     # Persistence layer
    ├── wonopcode-auth/        # Authentication
    ├── wonopcode-acp/         # Agent Client Protocol
    └── wonopcode-util/        # Shared utilities
```

---

## Dependency Graph

```
wonopcode (binary)
├── wonopcode-core
│   ├── wonopcode-provider
│   │   └── wonopcode-util
│   ├── wonopcode-tools
│   │   ├── wonopcode-sandbox
│   │   └── wonopcode-util
│   ├── wonopcode-storage
│   │   └── wonopcode-util
│   ├── wonopcode-snapshot
│   │   └── wonopcode-util
│   └── wonopcode-util
├── wonopcode-tui
│   ├── wonopcode-core
│   └── wonopcode-util
├── wonopcode-server
│   ├── wonopcode-core
│   ├── wonopcode-acp
│   └── wonopcode-util
├── wonopcode-mcp
│   └── wonopcode-util
├── wonopcode-lsp
│   └── wonopcode-util
└── wonopcode-auth
    └── wonopcode-util
```

---

## Crate Details

### `wonopcode` (Main Binary)

**Purpose**: Entry point, CLI parsing, orchestration

**Key Files**:
```
src/
├── main.rs          # Entry point
├── runner.rs        # Core execution loop
├── compaction.rs    # Conversation compaction
├── stats.rs         # Usage statistics
└── github/          # GitHub integration
    ├── mod.rs
    ├── api.rs
    ├── pr.rs
    └── event.rs
```

**Dependencies**: All workspace crates

**Responsibilities**:
- Parse CLI arguments
- Initialize Instance
- Run TUI or serve mode
- Handle signals and shutdown

---

### `wonopcode-core`

**Purpose**: Core business logic and types

**Key Files**:
```
src/
├── lib.rs           # Re-exports
├── agent.rs         # Agent definitions
├── bus.rs           # Event bus
├── command.rs       # Slash commands
├── config.rs        # Configuration
├── error.rs         # Error types
├── format.rs        # Output formatting
├── hook.rs          # Lifecycle hooks
├── instance.rs      # Instance context
├── message.rs       # Message types
├── permission.rs    # Permission system
├── project.rs       # Project detection
├── prompt.rs        # Prompt building
├── retry.rs         # Retry logic
├── revert.rs        # File revert
├── session.rs       # Session management
├── share.rs         # Session sharing
└── system_prompt.rs # System prompts
```

**Key Types**:
```rust
pub struct Instance { ... }
pub struct Session { ... }
pub struct Agent { ... }
pub struct Config { ... }
pub struct Bus { ... }
pub struct Message { ... }
```

**Responsibilities**:
- Session lifecycle
- Configuration management
- Event distribution
- Permission enforcement

---

### `wonopcode-provider`

**Purpose**: AI provider implementations

**Key Files**:
```
src/
├── lib.rs           # Provider trait, re-exports
├── anthropic.rs     # Anthropic Claude
├── openai.rs        # OpenAI GPT
├── google.rs        # Google Gemini
├── openrouter.rs    # OpenRouter
├── azure.rs         # Azure OpenAI
├── bedrock.rs       # AWS Bedrock
├── xai.rs           # xAI Grok
├── mistral.rs       # Mistral
├── groq.rs          # Groq
└── streaming.rs     # Stream handling
```

**Key Traits**:
```rust
#[async_trait]
pub trait LanguageModel: Send + Sync {
    async fn generate(&self, ...) -> Result<impl Stream<...>>;
    fn model_id(&self) -> &str;
    fn capabilities(&self) -> ModelCapabilities;
}
```

**Responsibilities**:
- API communication
- Response streaming
- Token counting
- Rate limiting

---

### `wonopcode-tools`

**Purpose**: Built-in tool implementations

**Key Files**:
```
src/
├── lib.rs           # Tool trait, registry
├── registry.rs      # Tool registration
├── bash.rs          # Shell execution
├── read.rs          # File reading
├── write.rs         # File writing
├── edit.rs          # File editing
├── multiedit.rs     # Multi-file edits
├── patch.rs         # Patch application
├── glob.rs          # File finding
├── grep.rs          # Content search
├── list.rs          # Directory listing
├── webfetch.rs      # Web fetching
├── search.rs        # Web/code search
├── task.rs          # Subagent spawning
├── todo.rs          # Todo management
├── lsp.rs           # LSP operations
├── mcp.rs           # MCP tool wrapper
├── skill.rs         # Skill execution
└── batch.rs         # Batch operations
```

**Key Traits**:
```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn id(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput>;
}
```

**Responsibilities**:
- Tool execution
- Parameter validation
- Sandbox integration
- Permission checking

---

### `wonopcode-tui`

**Purpose**: Terminal user interface

**Key Files**:
```
src/
├── lib.rs           # App entry
├── app.rs           # Application state
├── event.rs         # Event handling
├── keybind.rs       # Key bindings
├── theme.rs         # Color themes
├── model_state.rs   # View state
└── widgets/
    ├── mod.rs
    ├── autocomplete.rs
    ├── dialog.rs
    ├── diff.rs
    ├── footer.rs
    ├── input.rs
    ├── logo.rs
    ├── markdown.rs
    ├── messages.rs
    ├── sidebar.rs
    ├── slash_commands.rs
    ├── spinner.rs
    ├── status.rs
    ├── syntax.rs
    ├── timeline.rs
    └── toast.rs
```

**Built with**: `ratatui`, `crossterm`

**Responsibilities**:
- Rendering
- Input handling
- Widget management
- Theme support

---

### `wonopcode-server`

**Purpose**: HTTP API and ACP server

**Key Files**:
```
src/
├── lib.rs           # Server setup
├── routes.rs        # API routes
├── handlers.rs      # Request handlers
├── sse.rs           # Server-sent events
└── ws.rs            # WebSocket support
```

**Built with**: `axum`, `tower`

**Responsibilities**:
- REST API
- ACP protocol
- SSE streaming
- IDE integration

---

### `wonopcode-mcp`

**Purpose**: Model Context Protocol client

**Key Files**:
```
src/
├── lib.rs           # Client entry
├── client.rs        # MCP client
├── protocol.rs      # Protocol types
├── transport.rs     # Transport layer
├── server.rs        # Server management
├── serve.rs         # MCP server mode
├── sse.rs           # SSE transport
├── oauth.rs         # OAuth support
├── callback.rs      # Callbacks
└── error.rs         # Error types
```

**Responsibilities**:
- Server connections
- Tool discovery
- Tool invocation
- OAuth flows

---

### `wonopcode-lsp`

**Purpose**: Language Server Protocol client

**Key Files**:
```
src/
├── lib.rs           # LSP client
├── client.rs        # Client implementation
├── config.rs        # Server configs
├── transport.rs     # Transport layer
└── error.rs         # Error types
```

**Responsibilities**:
- Language server communication
- Go to definition
- Find references
- Hover information

---

### `wonopcode-sandbox`

**Purpose**: Sandboxed execution environments

**Key Files**:
```
src/
├── lib.rs           # SandboxRuntime trait
├── config.rs        # Sandbox config
├── error.rs         # Error types
├── path.rs          # Path mapping
└── runtime/
    ├── mod.rs
    ├── docker.rs    # Docker runtime
    ├── podman.rs    # Podman runtime
    └── lima.rs      # Lima runtime
```

**Key Traits**:
```rust
#[async_trait]
pub trait SandboxRuntime: Send + Sync {
    async fn start(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    async fn execute(&self, cmd: &str, ...) -> Result<Output>;
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>>;
    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()>;
}
```

**Responsibilities**:
- Container lifecycle
- Command execution
- File operations
- Path translation

---

### `wonopcode-snapshot`

**Purpose**: File change tracking and revert

**Key Files**:
```
src/
├── lib.rs           # Snapshot store
├── store.rs         # Storage implementation
└── diff.rs          # Diff generation
```

**Responsibilities**:
- Track file changes
- Store snapshots
- Generate diffs
- Revert changes

---

### `wonopcode-storage`

**Purpose**: Persistent storage abstraction

**Key Files**:
```
src/
├── lib.rs           # Storage trait
├── json.rs          # JSON file storage
├── memory.rs        # In-memory storage
└── error.rs         # Error types
```

**Key Traits**:
```rust
#[async_trait]
pub trait Storage: Send + Sync {
    async fn read<T>(&self, key: &[&str]) -> Result<Option<T>>;
    async fn write<T>(&self, key: &[&str], value: &T) -> Result<()>;
    async fn delete(&self, key: &[&str]) -> Result<()>;
    async fn list(&self, prefix: &[&str]) -> Result<Vec<String>>;
}
```

**Responsibilities**:
- Data persistence
- Key-value storage
- Session storage
- Cache management

---

### `wonopcode-auth`

**Purpose**: Authentication and credential management

**Key Files**:
```
src/
├── lib.rs           # Auth management
├── storage.rs       # Credential storage
└── error.rs         # Error types
```

**Responsibilities**:
- API key management
- OAuth token storage
- Credential refresh

---

### `wonopcode-acp`

**Purpose**: Agent Client Protocol for IDE integration

**Key Files**:
```
src/
├── lib.rs           # Protocol types
├── messages.rs      # Message definitions
└── handlers.rs      # Message handlers
```

**Responsibilities**:
- ACP message handling
- IDE communication
- Capability negotiation

---

### `wonopcode-util`

**Purpose**: Shared utilities

**Key Files**:
```
src/
├── lib.rs           # Re-exports
├── bash_permission.rs  # Bash permission patterns
├── error.rs         # Common errors
├── file_time.rs     # File timestamps
├── id.rs            # ID generation
├── log.rs           # Logging setup
├── path.rs          # Path utilities
└── wildcard.rs      # Wildcard matching
```

**Responsibilities**:
- Common utilities
- Path handling
- ID generation
- Pattern matching

---

## Adding a New Crate

1. Create directory:
   ```bash
   mkdir -p crates/wonopcode-newcrate/src
   ```

2. Create `Cargo.toml`:
   ```toml
   [package]
   name = "wonopcode-newcrate"
   version.workspace = true
   edition.workspace = true
   license.workspace = true
   
   [dependencies]
   wonopcode-util = { workspace = true }
   # other deps...
   ```

3. Add to workspace in root `Cargo.toml`:
   ```toml
   [workspace]
   members = [
       # ...
       "crates/wonopcode-newcrate",
   ]
   
   [workspace.dependencies]
   wonopcode-newcrate = { path = "crates/wonopcode-newcrate" }
   ```

4. Create `src/lib.rs`:
   ```rust
   //! wonopcode-newcrate: Description
   
   mod error;
   
   pub use error::Error;
   ```

---

## See Also

- [Architecture Overview](./overview.md) - High-level design
- [Security Model](./security-model.md) - Permission system
- [Development Setup](../contributing/development-setup.md) - Building from source
