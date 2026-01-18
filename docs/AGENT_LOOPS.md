# Agent Loops in Wonopcode

This document describes the agent loop architecture in wonopcode, including the public `wonopcode-agent-loop` crate and the private WASM-based extensions.

## Overview

An **agent loop** is the core execution engine that:
1. Receives user prompts
2. Sends them to an LLM provider
3. Processes streaming responses
4. Executes tool calls
5. Handles conversation history
6. Detects and prevents doom loops

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        wonopcode                             │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                      Runner                          │    │
│  │  - Manages TUI/CLI interface                        │    │
│  │  - Delegates to AgentLoop for prompt execution     │    │
│  └─────────────────────────────────────────────────────┘    │
│                            │                                 │
│                    Box<dyn AgentLoop>                        │
│                            │                                 │
│  ┌─────────────────────────┴─────────────────────────┐      │
│  │              wonopcode-agent-loop                  │      │
│  │  ┌─────────────────┐  ┌──────────────────────┐   │      │
│  │  │  StandardLoop   │  │   (trait AgentLoop)   │   │      │
│  │  │  - Built-in     │  │   - run_prompt()      │   │      │
│  │  │  - Fast         │  │   - on_start()        │   │      │
│  │  │  - No WASM      │  │   - on_complete()     │   │      │
│  │  └─────────────────┘  │   - on_cancel()       │   │      │
│  │                       │   - name()            │   │      │
│  │                       │   - capabilities()    │   │      │
│  │                       └──────────────────────┘   │      │
│  └───────────────────────────────────────────────────┘      │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│              wonopcode-pro (private)                         │
│  ┌─────────────────────────────────────────────────────┐    │
│  │       wonopcode-pro-wasm-orchestrator                │    │
│  │  ┌─────────────────────────────────────────────┐    │    │
│  │  │              WasmAgentLoop                   │    │    │
│  │  │  - Loads WASM component                     │    │    │
│  │  │  - Delegates to WASM for decisions          │    │    │
│  │  │  - Provides host functions                  │    │    │
│  │  └─────────────────────────────────────────────┘    │    │
│  └─────────────────────────────────────────────────────┘    │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              wonopcode-pro-tui                       │    │
│  │  - CLI with --loop-type flag                        │    │
│  │  - Module registry                                  │    │
│  │  - wonopcode-pro binary                             │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

## Public Crate: wonopcode-agent-loop

### AgentLoop Trait

```rust
#[async_trait]
pub trait AgentLoop: Send + Sync {
    /// Execute a single prompt turn, returning the final response.
    async fn run_prompt(
        &self,
        ctx: &mut LoopContext<'_>,
        user_input: &str,
    ) -> Result<String, LoopError>;

    /// Called when the loop starts (before first prompt).
    async fn on_start(&self, ctx: &mut LoopContext<'_>) -> Result<(), LoopError> {
        Ok(())
    }

    /// Called when the loop completes successfully.
    async fn on_complete(&self, ctx: &mut LoopContext<'_>, result: &str) -> Result<(), LoopError> {
        Ok(())
    }

    /// Called when the loop is cancelled.
    async fn on_cancel(&self, ctx: &mut LoopContext<'_>) -> Result<(), LoopError> {
        Ok(())
    }

    /// Get the name of this loop implementation.
    fn name(&self) -> &'static str;

    /// Get the capabilities of this loop implementation.
    fn capabilities(&self) -> LoopCapabilities {
        LoopCapabilities::default()
    }
}
```

### LoopContext

The context passed to the agent loop containing all necessary state:

```rust
pub struct LoopContext<'a> {
    pub cwd: &'a Path,                              // Working directory
    pub messages: &'a mut Vec<ProviderMessage>,     // Conversation history
    pub provider: &'a BoxedLanguageModel,           // LLM provider
    pub tools: &'a ToolRegistry,                    // Available tools
    pub tool_defs: Vec<ToolDefinition>,             // Tool definitions for LLM
    pub cancel: &'a CancellationToken,              // Cancellation signal
    pub snapshot_store: Option<&'a Arc<SnapshotStore>>,
    pub file_time: Arc<FileTimeState>,
    pub sandbox: Option<Arc<dyn SandboxRuntime>>,
    pub config: &'a LoopConfig,
    pub compaction_config: &'a CompactionConfig,
    pub update_tx: &'a mpsc::UnboundedSender<LoopUpdate>,
    pub session_id: String,
}
```

### LoopCapabilities

Describes what an agent loop implementation supports:

```rust
pub struct LoopCapabilities {
    pub streaming: bool,           // Supports streaming responses
    pub extended_thinking: bool,   // Supports Claude's thinking blocks
    pub parallel_tools: bool,      // Can execute multiple tools in parallel
    pub tool_observation: bool,    // Shows tool results to user before continuing
    pub max_context: usize,        // Maximum context window (0 = unlimited)
    pub auto_compaction: bool,     // Automatically compacts long conversations
    pub custom: HashMap<String, String>,  // Custom capability flags
}
```

### LoopError

Error types that can occur during loop execution:

```rust
pub enum LoopError {
    ProviderError(String),      // LLM provider error
    ToolError(String),          // Tool execution error
    PermissionDenied(String),   // Permission denied for operation
    DoomLoop(String),           // Detected repetitive behavior
    Cancelled,                  // User cancelled
    Internal(String),           // Internal error
}
```

### StandardLoop

The built-in implementation that provides:
- Streaming response processing
- Parallel tool execution
- Doom loop detection (threshold: 3 identical consecutive calls)
- Automatic message compaction
- Extended thinking support

## Private Crate: wonopcode-pro-wasm-orchestrator

### WasmAgentLoop

Loads and executes WASM components as agent loops:

```rust
// Load from file
let loop_impl = WasmAgentLoop::from_file("my-loop.wasm")?;

// Load from bytes
let loop_impl = WasmAgentLoop::from_bytes(&wasm_bytes)?;

// Validate without loading
WasmAgentLoop::validate("my-loop.wasm")?;
```

### WIT Interface

WASM components must implement the `agent-loop` interface:

```wit
interface agent-loop {
    run-prompt: func(input: string) -> result<string, loop-error>;
    on-start: func() -> result<_, loop-error>;
    on-complete: func(result: string) -> result<_, loop-error>;
    on-cancel: func() -> result<_, loop-error>;
    get-name: func() -> string;
    get-capabilities: func() -> loop-capabilities;
}
```

And can import host functions for:
- **config**: User preferences and configuration
- **providers**: LLM provider interaction and streaming
- **tools**: Tool listing, enabling/disabling, execution
- **ui**: Sending text, progress, and status to the user
- **storage**: Message history and local state
- **system**: Logging, timing, cancellation

## Private Crate: wonopcode-pro-tui

### CLI Flags

```bash
# Use standard loop (default)
wonopcode-pro

# Use embedded default WASM loop
wonopcode-pro --loop-type=wasm

# Use custom WASM module from file
wonopcode-pro --loop-type=custom --agent-loop=/path/to/my-loop.wasm

# Use named module from registry
wonopcode-pro --loop-type=custom --agent-loop-name=my-loop

# Validate a module without running
wonopcode-pro --validate-module --agent-loop=/path/to/my-loop.wasm

# List available modules
wonopcode-pro --list-loops
```

### Module Registry

Named WASM modules are discovered from:
1. `~/.config/wonopcode/agent-loops/registry.yaml`
2. `~/.config/wonopcode/agent-loops/*.wasm`
3. `~/.local/share/wonopcode/agent-loops/*.wasm`
4. `/usr/share/wonopcode/agent-loops/*.wasm`

Registry file format:
```yaml
modules:
  my-loop:
    path: /path/to/my-loop.wasm
    description: "My custom agent loop"
    version: "1.0.0"
```

## Design Decisions

### Why a Trait?

Using a trait for AgentLoop provides:
1. **Extensibility**: Custom implementations without modifying core code
2. **Testability**: Easy to mock for testing
3. **WASM support**: Clean boundary for WASM delegation

### Why WASM for Custom Loops?

1. **Sandboxing**: WASM provides memory and execution isolation
2. **Portability**: Single binary works across platforms
3. **Hot-loading**: Load new loops without recompiling
4. **Language flexibility**: Write loops in any language that compiles to WASM

### Doom Loop Detection

The doom loop detector tracks:
- Tool call signatures (name + arguments hash)
- Consecutive identical call count
- Threshold of 3 triggers error

This prevents the agent from getting stuck in infinite loops.

## Developing Custom WASM Loops

### Prerequisites

1. Install the WASM target: `rustup target add wasm32-unknown-unknown`
2. Install wasm-tools: `cargo install wasm-tools`
3. Add wit-bindgen to your project: `cargo add wit-bindgen`

### Creating a Custom Loop

1. Create a new Rust library crate:

```bash
cargo new --lib my-agent-loop
cd my-agent-loop
```

2. Update `Cargo.toml`:

```toml
[package]
name = "my-agent-loop"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
wit-bindgen = "0.41"
```

3. Implement the agent loop in `src/lib.rs`:

```rust
wit_bindgen::generate!({
    path: "path/to/wonopcode-pro-wasm-orchestrator/wit",
    world: "agent-loop-world",
});

use crate::exports::wonopcode::agent_loop::agent_loop::Guest;
use crate::wonopcode::agent_loop::types::{LoopCapabilities, LoopError};

pub struct MyLoop;

impl Guest for MyLoop {
    fn run_prompt(input: String) -> Result<String, LoopError> {
        use crate::wonopcode::agent_loop::{system, ui, config};
        
        // Log the input
        system::log(system::LogLevel::Info, &format!("Received: {}", input));
        
        // Send thinking update
        ui::send_thinking("Processing...");
        
        // Check for cancellation
        if system::is_cancelled() {
            return Err(LoopError::Cancelled);
        }
        
        // Your custom logic here
        let response = format!("Response to: {}", input);
        ui::send_text(&response);
        
        Ok(response)
    }

    fn on_start() -> Result<(), LoopError> { Ok(()) }
    fn on_complete(_response: String) -> Result<(), LoopError> { Ok(()) }
    fn on_cancel() -> Result<(), LoopError> { Ok(()) }
    fn get_name() -> String { "my-loop".to_string() }
    
    fn get_capabilities() -> LoopCapabilities {
        LoopCapabilities {
            streaming: true,
            extended_thinking: false,
            parallel_tools: false,
            tool_observation: false,
            max_context: 1000,
            auto_compaction: false,
            supports_sub_contexts: false,
            supports_custom_tools: false,
        }
    }
}

export!(MyLoop);
```

4. Build and convert to component:

```bash
cargo build --target wasm32-unknown-unknown --release
wasm-tools component new target/wasm32-unknown-unknown/release/my_agent_loop.wasm -o my-loop.wasm
```

5. Use with wonopcode-pro:

```bash
wonopcode-pro --loop-type=custom --agent-loop=./my-loop.wasm
```

### Available Host Functions

Your WASM loop can call these host functions:

| Interface | Functions |
|-----------|-----------|
| `config` | `get_user_preferred_provider()`, `get_user_preferred_model()`, `get_config(key)` |
| `system` | `log(level, message)`, `current_time_ms()`, `is_cancelled()`, `get_cwd()` |
| `ui` | `send_text(text)`, `send_thinking(text)`, `tool_start(id, name)`, `tool_result(id, output, is_error)`, `show_progress(msg, pct)`, `hide_progress()` |
| `tools` | `list_tools()`, `enable_tool(id)`, `disable_tool(id)`, `get_enabled_tools()`, `execute_tool(name, args_json)` |
| `providers` | `get_current_provider()`, `get_current_model()`, `set_provider(id)`, `set_model(id)`, `stream_prompt(messages, tools, options)`, `read_stream_chunk(handle)`, `close_stream(handle)` |
| `storage` | `get_messages()`, `add_message(msg)`, `clear_messages()`, `get_state(key)`, `set_state(key, value)`, `delete_state(key)` |

## Future Work

- [ ] Full provider streaming integration (currently placeholder)
- [ ] Default embedded WASM module equivalent to StandardLoop
- [ ] Integration with existing Runner in wonopcode crate
- [ ] Context spawning for recursive agents (max depth: 100)
- [ ] Custom tool registration from WASM

## See Also

- [WIT Interface](../../pro/crates/wonopcode-wasm-orchestrator/wit/wonopcode.wit)
- [Design Documents](../../../../specs/designs/)
- [Component Specs](../../../../specs/components/)
