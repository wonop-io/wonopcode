# Architecture Overview

High-level architecture and design of wonopcode.

---

## System Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                           User Interface                             │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────┐  │
│  │   TUI (ratatui) │  │   CLI (clap)    │  │   ACP Server (axum) │  │
│  └────────┬────────┘  └────────┬────────┘  └──────────┬──────────┘  │
│           └────────────────────┼──────────────────────┘              │
│                                │                                     │
│  ┌─────────────────────────────▼─────────────────────────────────┐  │
│  │                        wonopcode-core                          │  │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌─────────────────┐   │  │
│  │  │ Session  │ │  Agent   │ │ Instance │ │   Permission    │   │  │
│  │  └──────────┘ └──────────┘ └──────────┘ └─────────────────┘   │  │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌─────────────────┐   │  │
│  │  │  Config  │ │   Bus    │ │ Snapshot │ │     Share       │   │  │
│  │  └──────────┘ └──────────┘ └──────────┘ └─────────────────┘   │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                │                                     │
│         ┌──────────────────────┼──────────────────────┐             │
│         │                      │                      │              │
│  ┌──────▼──────┐        ┌──────▼──────┐        ┌──────▼──────┐      │
│  │  Provider   │        │    Tools    │        │   Storage   │      │
│  │  (AI APIs)  │        │  (actions)  │        │   (JSON)    │      │
│  └─────────────┘        └──────┬──────┘        └─────────────┘      │
│                                │                                     │
│         ┌──────────────────────┼──────────────────────┐             │
│         │                      │                      │              │
│  ┌──────▼──────┐        ┌──────▼──────┐        ┌──────▼──────┐      │
│  │     MCP     │        │   Sandbox   │        │     LSP     │      │
│  │  (external) │        │  (Docker)   │        │   (code)    │      │
│  └─────────────┘        └─────────────┘        └─────────────┘      │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Core Components

### Instance

The `Instance` is the central context for a wonopcode session:

```rust
pub struct Instance {
    pub directory: PathBuf,       // Project root
    pub project: Project,         // Project metadata
    pub config: Config,           // Configuration
    pub bus: Bus,                 // Event bus
    pub storage: Box<dyn Storage>, // Persistence
}
```

Each project directory has one Instance. It's shared across sessions and agents.

### Session

A `Session` represents a conversation with the AI:

```rust
pub struct Session {
    pub id: String,
    pub name: Option<String>,
    pub messages: Vec<Message>,
    pub parent_id: Option<String>,  // For subagent sessions
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

Sessions are persistent and can be resumed.

### Agent

An `Agent` defines AI behavior:

```rust
pub struct Agent {
    pub name: String,
    pub system_prompt: String,
    pub model: Option<String>,
    pub tools: HashMap<String, bool>,
    pub permission: PermissionConfig,
    pub sandbox: Option<SandboxConfig>,
}
```

Built-in agents: `code`, `explore`, `build`, `plan`.

### Bus

The `Bus` enables event-driven communication:

```rust
pub struct Bus {
    session_tx: broadcast::Sender<SessionEvent>,
    message_tx: broadcast::Sender<MessageEvent>,
    permission_tx: broadcast::Sender<PermissionEvent>,
}
```

Components subscribe to events and react without direct coupling.

---

## Data Flow

### User Input to AI Response

```
User Input
    │
    ▼
┌──────────────────┐
│   Parse Input    │  Detect slash commands vs prompts
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Resolve Context  │  Get session, agent, instance
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│  Build Prompt    │  System prompt + messages + context
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│   AI Provider    │  Stream response from API
└────────┬─────────┘
         │
         ├──── Text chunks ──────► Display to user
         │
         ├──── Tool calls ───┐
         │                   │
         │    ┌──────────────▼──────────────┐
         │    │      Tool Execution          │
         │    │  1. Check permission         │
         │    │  2. Execute (sandbox/host)   │
         │    │  3. Return result            │
         │    └──────────────┬──────────────┘
         │                   │
         └───────────────────┘
                   │
                   ▼
            Save to Session
```

### Tool Execution

```
Tool Call from AI
        │
        ▼
┌───────────────────┐
│ Permission Check  │────► Denied ────► Return error
└────────┬──────────┘
         │ Allowed
         ▼
┌───────────────────┐
│   Sandbox Check   │
└────────┬──────────┘
         │
    ┌────┴────┐
    │         │
    ▼         ▼
Sandboxed   Direct
    │         │
    ▼         ▼
┌─────────┐ ┌─────────┐
│Container│ │  Host   │
│ Exec    │ │  Exec   │
└────┬────┘ └────┬────┘
     └─────┬─────┘
           │
           ▼
┌───────────────────┐
│  Return Result    │
└───────────────────┘
```

---

## Event System

### Event Types

```rust
// Session lifecycle
pub enum SessionEvent {
    Created(Session),
    Updated(Session),
    Deleted(String),
}

// Message updates (streaming)
pub enum MessageEvent {
    Updated(Message),
    PartUpdated { message_id: String, part: MessagePart },
    Removed(String),
}

// Permission requests
pub enum PermissionEvent {
    Requested(PermissionRequest),
    Responded(PermissionResponse),
}

// Status changes
pub enum StatusEvent {
    Idle,
    Busy(String),
    Error(String),
}
```

### Event Flow

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Runner    │────►│     Bus     │────►│     TUI     │
└─────────────┘     └──────┬──────┘     └─────────────┘
                          │
                          ├────────────►│   Storage   │
                          │             └─────────────┘
                          │
                          └────────────►│   Server    │
                                        └─────────────┘
```

---

## Provider Architecture

### Provider Trait

```rust
#[async_trait]
pub trait LanguageModel: Send + Sync {
    async fn generate(
        &self,
        messages: Vec<Message>,
        tools: Vec<Tool>,
        options: GenerateOptions,
    ) -> Result<impl Stream<Item = StreamChunk>>;
    
    fn model_id(&self) -> &str;
    fn capabilities(&self) -> ModelCapabilities;
}
```

### Supported Providers

| Provider | Implementation |
|----------|---------------|
| Anthropic | `AnthropicProvider` |
| OpenAI | `OpenAIProvider` |
| Google | `GoogleProvider` |
| OpenRouter | `OpenRouterProvider` |
| Azure | `AzureProvider` |
| Bedrock | `BedrockProvider` |
| xAI | `XAIProvider` |
| Mistral | `MistralProvider` |
| Groq | `GroqProvider` |

### Streaming Response

```rust
pub enum StreamChunk {
    Text(String),
    Reasoning(String),
    ToolCall { id: String, name: String, args: Value },
    ToolResult { id: String, result: Value },
    Usage(TokenUsage),
    Done,
}
```

---

## Tool System

### Tool Trait

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn id(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    
    async fn execute(
        &self,
        args: Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError>;
}
```

### Tool Context

```rust
pub struct ToolContext {
    pub session_id: String,
    pub message_id: String,
    pub agent: String,
    pub abort: CancellationToken,
    pub root_dir: PathBuf,
    pub cwd: PathBuf,
    pub sandbox: Option<Arc<dyn SandboxRuntime>>,
    pub snapshot: Option<Arc<SnapshotStore>>,
}
```

### Built-in Tools

| Tool | Category |
|------|----------|
| `read`, `write`, `edit`, `patch`, `multiedit` | File Operations |
| `glob`, `grep`, `list` | Search |
| `bash` | Execution |
| `webfetch`, `websearch`, `codesearch` | Web |
| `task` | Subagent |
| `todoread`, `todowrite` | Task Management |
| `lsp` | Code Intelligence |

---

## Sandbox Architecture

### SandboxRuntime Trait

```rust
#[async_trait]
pub trait SandboxRuntime: Send + Sync {
    async fn start(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    async fn execute(&self, command: &str, workdir: &Path) -> Result<Output>;
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>>;
    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()>;
}
```

### Runtime Implementations

```
SandboxRuntime (trait)
    │
    ├── DockerRuntime
    │   └── Uses bollard for Docker API
    │
    ├── PodmanRuntime
    │   └── Uses podman CLI
    │
    └── LimaRuntime
        └── Uses limactl for macOS VMs
```

### Path Mapping

```
Host: /Users/dev/project/src/main.rs
           │
           ▼
    PathMapper.to_sandbox()
           │
           ▼
Sandbox: /workspace/src/main.rs
```

---

## Storage Layer

### Storage Trait

```rust
#[async_trait]
pub trait Storage: Send + Sync {
    async fn read<T: DeserializeOwned>(&self, key: &[&str]) -> Result<Option<T>>;
    async fn write<T: Serialize>(&self, key: &[&str], value: &T) -> Result<()>;
    async fn delete(&self, key: &[&str]) -> Result<()>;
    async fn list(&self, prefix: &[&str]) -> Result<Vec<String>>;
}
```

### JSON Storage

Default implementation stores data as JSON files:

```
~/.config/wonopcode/
├── state/
│   ├── sessions/
│   │   ├── abc123.json
│   │   └── def456.json
│   ├── auth/
│   │   └── tokens.json
│   └── cache/
│       └── ...
```

---

## MCP Integration

### MCP Client

```rust
pub struct McpClient {
    servers: HashMap<String, McpServer>,
}

pub struct McpServer {
    name: String,
    transport: McpTransport,
    tools: Vec<McpTool>,
}
```

### Transport Types

```rust
pub enum McpTransport {
    Stdio { process: Child },
    Sse { url: String, client: reqwest::Client },
}
```

### Tool Registration

MCP tools are registered with prefixed names:

```
github:create_issue
github:list_issues
postgres:query
```

---

## Concurrency Model

### Async Runtime

Wonopcode uses Tokio for async operations:

- **Main thread**: TUI rendering
- **Async tasks**: AI streaming, tool execution
- **Background**: File watching, MCP servers

### Cancellation

Operations support cancellation via `tokio_util::CancellationToken`:

```rust
let token = CancellationToken::new();

tokio::select! {
    result = operation() => result,
    _ = token.cancelled() => Err(Cancelled),
}
```

### State Sharing

Shared state uses `Arc` and async-safe primitives:

```rust
pub struct SharedState {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    config: Arc<Config>,
    bus: Arc<Bus>,
}
```

---

## Error Handling

### Error Types

Each crate defines its own error type:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Validation error: {0}")]
    Validation(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
}
```

### Error Propagation

Errors bubble up through the call chain:

```
Tool Error
    │
    ▼
Runner catches, formats for AI
    │
    ▼
AI interprets, may retry
    │
    ▼
User sees result
```

---

## Configuration Loading

```
Defaults
    │
    ▼
Global Config (~/.config/wonopcode/config.json)
    │
    ▼
Project Config (.wonopcode/config.json)
    │
    ▼
Environment Variables
    │
    ▼
CLI Arguments
    │
    ▼
Final Config
```

---

## Next Steps

- [Crate Structure](./crate-structure.md) - Detailed crate breakdown
- [Security Model](./security-model.md) - Permission system
- [Contributing](../contributing/CONTRIBUTING.md) - How to contribute
