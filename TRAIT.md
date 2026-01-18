# Agentic Loop Trait Analysis

This document analyzes the feasibility of introducing a trait to abstract the main agentic loop in wonopcode, enabling different implementations (e.g., standard, Claude CLI native, custom providers, **WASM-based orchestration**).

## Executive Summary

**Feasibility: YES - Highly Feasible**

The current architecture already has clear separation between the UI layer, action handling, and the core agentic loop. Introducing an `AgentLoop` trait would be a clean refactoring that enables:
- Provider-specific optimizations (e.g., native Claude CLI tool handling)
- Alternative loop implementations (e.g., ReAct, Tree of Thoughts)
- Testing and mocking of the agentic loop
- Plugin-based extensions
- **WASM-based customizable orchestration** (using Wasmtime)
- **Recursive context stack** for hierarchical agent delegation

## Current Architecture

### Key Components

1. **`Runner` struct** (`crates/wonopcode/src/runner.rs`)
   - Owns the main event loop via `run()` method
   - Manages conversation history, tools, permissions, sandbox, etc.
   - Dispatches `AppAction` events and sends `AppUpdate` events

2. **`run()` method** (lines 835-1515)
   - Main action handling loop: receives `AppAction` from TUI/backend
   - Dispatches to handlers (SendPrompt, Cancel, ChangeModel, etc.)

3. **`run_prompt()` method** (lines 1759-2945)
   - **THE CORE AGENTIC LOOP**
   - Handles message building, streaming, tool execution, and response processing
   - Contains the actual "agent" behavior

### The Agentic Loop (`run_prompt`)

Location: `crates/wonopcode/src/runner.rs:1759-2945`

The agentic loop follows this pattern:

```rust
async fn run_prompt(&self, user_input: &str, cwd: &Path, update_tx: &...) -> Result<String, ...> {
    // 1. Initialize: reset doom loop detector, get history
    // 2. Check compaction needs (token/message limits)
    // 3. Build system prompt with context
    // 4. Add user message to history
    
    // 5. THE MAIN LOOP:
    loop {
        // 5a. Stream from provider
        let stream = provider.generate_stream(&messages, &tools, &options);
        
        // 5b. Process chunks (text deltas, tool calls, thinking, etc.)
        while let Some(chunk) = stream.next() {
            match chunk {
                StreamChunk::TextDelta { .. } => { /* stream text */ }
                StreamChunk::ToolCallStart { .. } => { /* record tool call */ }
                StreamChunk::ToolCallDelta { .. } => { /* accumulate args */ }
                StreamChunk::ToolCallEnd { .. } => { /* finalize tool */ }
                StreamChunk::ThinkingDelta { .. } => { /* extended thinking */ }
                // ... etc
            }
        }
        
        // 5c. Execute tool calls (parallel)
        if !tool_calls.is_empty() {
            // Permission checking
            // Doom loop detection  
            // Parallel tool execution
            // Add tool results to messages
            continue; // Loop back for model response
        }
        
        // 5d. No more tools - done
        break;
    }
    
    // 6. Store final assistant message
    Ok(final_text)
}
```

## Proposed Trait Design

### Core Trait

```rust
/// Core trait for implementing agentic loops.
/// 
/// An agentic loop handles the interaction pattern between:
/// - User prompts
/// - LLM provider streaming
/// - Tool execution
/// - Response generation
#[async_trait]
pub trait AgentLoop: Send + Sync {
    /// Execute a single prompt through the agentic loop.
    /// 
    /// This is the main entry point that implements the agent's behavior.
    /// Different implementations can use different strategies:
    /// - Standard: stream -> tool calls -> stream -> ...
    /// - ReAct: reasoning -> action -> observation -> ...
    /// - Tree of Thoughts: branch exploration with backtracking
    /// - WASM: externally orchestrated via WebAssembly module
    async fn run_prompt(
        &self,
        ctx: &mut LoopContext,
        user_input: &str,
        update_tx: &mpsc::UnboundedSender<AppUpdate>,
    ) -> Result<String, LoopError>;

    /// Called before the loop starts (setup).
    async fn on_start(&self, _ctx: &mut LoopContext) -> Result<(), LoopError> {
        Ok(())
    }

    /// Called after the loop completes (cleanup).
    async fn on_complete(&self, _ctx: &mut LoopContext, _result: &str) -> Result<(), LoopError> {
        Ok(())
    }

    /// Called when the loop is cancelled.
    async fn on_cancel(&self, _ctx: &mut LoopContext) -> Result<(), LoopError> {
        Ok(())
    }

    /// Get the name of this loop implementation.
    fn name(&self) -> &'static str;

    /// Get capabilities of this loop implementation.
    fn capabilities(&self) -> LoopCapabilities {
        LoopCapabilities::default()
    }
}
```

### Context Struct

```rust
/// Shared context for the agentic loop.
/// 
/// Contains all state needed to execute the loop, passed by reference
/// to allow the Runner to maintain ownership.
pub struct LoopContext<'a> {
    /// Working directory
    pub cwd: &'a Path,
    
    /// Conversation history (mutable for the loop to update)
    pub messages: &'a mut Vec<ProviderMessage>,
    
    /// The LLM provider
    pub provider: &'a BoxedLanguageModel,
    
    /// Available tools
    pub tools: &'a ToolRegistry,
    
    /// Cancellation token
    pub cancel: &'a CancellationToken,
    
    /// Permission manager
    pub permission_manager: &'a PermissionManager,
    
    /// Sandbox manager (if enabled)
    pub sandbox_manager: Option<&'a SandboxManager>,
    
    /// Snapshot store for file versioning
    pub snapshot_store: Option<&'a SnapshotStore>,
    
    /// File time tracker
    pub file_time: &'a FileTimeState,
    
    /// Todo store for task tracking
    pub todo_store: &'a dyn TodoStore,
    
    /// Doom loop detector
    pub doom_loop_detector: &'a mut DoomLoopDetector,
    
    /// Runner config
    pub config: &'a RunnerConfig,
    
    /// Compaction config
    pub compaction_config: &'a CompactionConfig,
    
    /// Event bus
    pub bus: &'a Bus,
}
```

### Capabilities

```rust
/// Describes what an agent loop implementation can do.
#[derive(Debug, Clone, Default)]
pub struct LoopCapabilities {
    /// Supports streaming text output
    pub streaming: bool,
    
    /// Supports extended thinking/reasoning
    pub extended_thinking: bool,
    
    /// Supports parallel tool execution
    pub parallel_tools: bool,
    
    /// Supports tool call observation (without execution)
    pub tool_observation: bool,
    
    /// Maximum supported context length (0 = unlimited)
    pub max_context: u32,
    
    /// Supports automatic compaction
    pub auto_compaction: bool,
    
    /// Custom capabilities (for extensions)
    pub custom: HashMap<String, bool>,
}
```

### Error Type

```rust
/// Errors that can occur during agentic loop execution.
#[derive(Debug, thiserror::Error)]
pub enum LoopError {
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),
    
    #[error("Tool execution failed: {0}")]
    ToolExecution(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Doom loop detected: {0}")]
    DoomLoop(String),
    
    #[error("Cancelled")]
    Cancelled,
    
    #[error("Context limit exceeded")]
    ContextLimitExceeded,
    
    #[error("Internal error: {0}")]
    Internal(String),
}
```

---

# WASM-Based Orchestration Architecture

## Design Philosophy: Inverted Control Model

The WASM boundary is **NOT** for security sandboxing. It's for **customizability**.

In this model:
- **WASM module = The Brain** (orchestrator, decision maker)
- **Host = The Hands** (provides capabilities, executes actions)

The WASM module has full control over:
- Which provider to use for each request
- What tools are available to the model
- How to process responses
- When to loop vs when to finish
- Custom agentic strategies (ReAct, Tree of Thoughts, etc.)
- **Spawning sub-contexts with different WASM modules**

---

# Orchestrator Capabilities

## 1. Provider & Model Selection

### 1a. Reading User Preferences

The orchestrator can read what the user has configured as their desired provider/model:

```wit
/// Configuration interface - read user preferences
interface config {
    /// User's preferred provider (from config file or CLI)
    get-user-preferred-provider: func() -> option<string>;
    
    /// User's preferred model
    get-user-preferred-model: func() -> option<string>;
    
    /// Get any configuration value
    get-config: func(key: string) -> option<string>;
    
    /// Get all user preferences as key-value pairs
    get-user-preferences: func() -> list<tuple<string, string>>;
}
```

### 1b. Independent Provider/Model Selection

The orchestrator can override user preferences when appropriate:

```rust
// In WASM module
fn run_prompt(input: String) -> Result<String, LoopError> {
    // Read what user wants
    let user_provider = config::get_user_preferred_provider();
    let user_model = config::get_user_preferred_model();
    
    // But we can choose differently based on the task
    let (provider, model) = match analyze_task(&input) {
        TaskType::SimpleQuestion => {
            // User preference is fine for simple stuff
            (user_provider.unwrap_or("anthropic".into()), 
             user_model.unwrap_or("claude-3-haiku".into()))
        }
        TaskType::ComplexReasoning => {
            // Override to use best reasoning model
            ("anthropic".into(), "claude-opus-4".into())
        }
        TaskType::CodeGeneration => {
            // Use specialized code model
            ("deepseek".into(), "deepseek-coder-v2".into())
        }
        TaskType::LargeContext => {
            // Use model with large context window
            ("google".into(), "gemini-pro-1.5".into())
        }
    };
    
    providers::set_provider(&provider)?;
    providers::set_model(&model)?;
    
    // ... rest of loop
}
```

### 1c. Tool Enable/Disable Control

```wit
/// Extended tools interface with enable/disable
interface tools {
    // ... existing tool-info, execute, etc ...
    
    /// Enable a tool for the current context
    enable-tool: func(tool-id: string) -> result<_, string>;
    
    /// Disable a tool for the current context
    disable-tool: func(tool-id: string) -> result<_, string>;
    
    /// Get currently enabled tools
    get-enabled-tools: func() -> list<string>;
    
    /// Set the complete list of enabled tools (replaces current)
    set-enabled-tools: func(tool-ids: list<string>) -> result<_, string>;
    
    /// Check if a tool is currently enabled
    is-tool-enabled: func(tool-id: string) -> bool;
}
```

Example usage:

```rust
fn configure_tools_for_task(task: &TaskType) {
    // Start with minimal tools
    tools::set_enabled_tools(&["read", "glob", "grep"])?;
    
    match task {
        TaskType::ReadOnly => {
            // Already have what we need
        }
        TaskType::CodeModification => {
            tools::enable_tool("write")?;
            tools::enable_tool("edit")?;
            tools::enable_tool("patch")?;
        }
        TaskType::SystemAdmin => {
            tools::enable_tool("bash")?;
        }
        TaskType::Research => {
            tools::enable_tool("websearch")?;
            tools::enable_tool("webfetch")?;
        }
    }
}
```

### 1d. Custom Tool Definition

WASM modules can define their own tools that the LLM can call:

```wit
/// Custom tool registration
interface custom-tools {
    /// Definition for a custom tool
    record custom-tool-definition {
        /// Unique tool ID
        id: string,
        /// Display name
        name: string,
        /// Description for the LLM
        description: string,
        /// JSON Schema for parameters
        parameters-schema: string,
        /// Whether this tool is enabled by default
        enabled-by-default: bool,
    }
    
    /// Register a custom tool implemented by this WASM module
    register-tool: func(definition: custom-tool-definition) -> result<_, string>;
    
    /// Unregister a custom tool
    unregister-tool: func(tool-id: string) -> result<_, string>;
    
    /// List all custom tools registered by this module
    list-custom-tools: func() -> list<string>;
}

/// The WASM module must implement this to handle custom tool calls
interface custom-tool-handler {
    /// Called when the LLM invokes a custom tool
    handle-tool-call: func(tool-id: string, arguments: string) -> result<string, string>;
}
```

Example WASM module with custom tools:

```rust
impl Guest for MyAgentLoop {
    fn on_start() -> Result<(), LoopError> {
        // Register custom tools
        custom_tools::register_tool(CustomToolDefinition {
            id: "analyze_architecture".into(),
            name: "analyze_architecture".into(),
            description: "Deeply analyze the architecture of a codebase, \
                         identifying patterns, dependencies, and potential issues".into(),
            parameters_schema: r#"{
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Root path to analyze" },
                    "depth": { "type": "string", "enum": ["shallow", "deep"], "default": "shallow" }
                },
                "required": ["path"]
            }"#.into(),
            enabled_by_default: true,
        })?;
        
        custom_tools::register_tool(CustomToolDefinition {
            id: "delegate_to_specialist".into(),
            name: "delegate_to_specialist".into(),
            description: "Delegate a subtask to a specialist agent".into(),
            parameters_schema: r#"{
                "type": "object",
                "properties": {
                    "specialist": { "type": "string", "enum": ["coder", "researcher", "reviewer"] },
                    "task": { "type": "string", "description": "The task to delegate" }
                },
                "required": ["specialist", "task"]
            }"#.into(),
            enabled_by_default: true,
        })?;
        
        Ok(())
    }
}

impl CustomToolHandler for MyAgentLoop {
    fn handle_tool_call(tool_id: String, arguments: String) -> Result<String, String> {
        match tool_id.as_str() {
            "analyze_architecture" => {
                let args: AnalyzeArgs = serde_json::from_str(&arguments)?;
                
                // Use host tools to gather information
                let files = tools::execute_tool("glob", 
                    &json!({"pattern": format!("{}/**/*.rs", args.path)}).to_string())?;
                
                // Spawn a sub-context with an analysis-focused agent
                let result = context::spawn(ContextParams {
                    module: ContextModule::Named("analyzer.wasm".into()),
                    task: format!("Analyze the architecture of these files: {}", files.content),
                    tools: Some(vec!["read".into(), "grep".into()]),  // Read-only tools
                    ..Default::default()
                })?;
                
                Ok(result.response)
            }
            
            "delegate_to_specialist" => {
                let args: DelegateArgs = serde_json::from_str(&arguments)?;
                
                let module = match args.specialist.as_str() {
                    "coder" => "coder.wasm",
                    "researcher" => "researcher.wasm", 
                    "reviewer" => "reviewer.wasm",
                    _ => return Err(format!("Unknown specialist: {}", args.specialist)),
                };
                
                let result = context::spawn(ContextParams {
                    module: ContextModule::Named(module.into()),
                    task: args.task,
                    ..Default::default()
                })?;
                
                Ok(result.response)
            }
            
            _ => Err(format!("Unknown custom tool: {}", tool_id)),
        }
    }
}
```

---

# Recursive Context Stack

## Overview

The recursive context stack allows WASM orchestrators to spawn sub-contexts, creating a hierarchy of agents that can delegate work to each other.

```
┌─────────────────────────────────────────────────────────────────┐
│                         Context Stack                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │ [2] Analysis Context (CURRENT)                             │  │
│  │     Module: analyzer.wasm                                  │  │
│  │     Task: "Analyze architecture of src/parser"             │  │
│  │     Provider: claude-haiku (cheap, fast analysis)          │  │
│  │     Tools: [read, grep, glob]                              │  │
│  │     Messages: [user: "Analyze...", assistant: "..."]       │  │
│  │     Custom Tools: []                                        │  │
│  └───────────────────────────────────────────────────────────┘  │
│                              ▲                                   │
│                              │ spawned by                        │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │ [1] Coding Context (SUSPENDED)                             │  │
│  │     Module: coder.wasm                                     │  │
│  │     Task: "Implement new parser"                           │  │
│  │     Provider: deepseek-coder                                │  │
│  │     Tools: [read, write, edit, bash]                       │  │
│  │     Messages: [user: "Implement...", assistant: "...", ...] │  │
│  │     Custom Tools: [analyze_architecture]                   │  │
│  │     *** WAITING for context [2] to return ***              │  │
│  └───────────────────────────────────────────────────────────┘  │
│                              ▲                                   │
│                              │ spawned by                        │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │ [0] Root Context (SUSPENDED)                               │  │
│  │     Module: orchestrator.wasm                              │  │
│  │     Task: User's original request                          │  │
│  │     Provider: claude-sonnet (user's preference)            │  │
│  │     Tools: [all]                                           │  │
│  │     Messages: [user: "Build me a parser", ...]             │  │
│  │     Custom Tools: [delegate_to_specialist]                 │  │
│  │     *** WAITING for context [1] to return ***              │  │
│  └───────────────────────────────────────────────────────────┘  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## What's Shared vs Isolated

| Aspect | Shared Across Contexts | Isolated Per Context |
|--------|------------------------|----------------------|
| Host tools (read, write, bash) | ✓ | |
| File system | ✓ | |
| Provider connections (API keys) | ✓ | |
| Cancellation (parent can cancel children) | ✓ | |
| User preferences (read-only) | ✓ | |
| **WASM linear memory** | | ✓ |
| **Conversation history** | | ✓ |
| **Selected provider/model** | | ✓ |
| **Enabled tools** | | ✓ |
| **Custom tools** | | ✓ |
| **Local state (key-value)** | | ✓ |

## WIT Interface for Context Management

```wit
/// Context management - spawn and manage sub-contexts
interface context {
    /// Parameters for spawning a new context
    record context-params {
        /// Which WASM module to use
        module: context-module,
        
        /// The task/prompt for the new context
        task: string,
        
        /// Tool restrictions (if None, inherits from parent)
        allowed-tools: option<list<string>>,
        
        /// Tools to explicitly deny (applied after allowed-tools)
        denied-tools: option<list<string>>,
        
        /// Preferred provider (child can still override)
        preferred-provider: option<string>,
        
        /// Preferred model (child can still override)
        preferred-model: option<string>,
        
        /// Maximum iterations before forced termination
        max-iterations: option<u32>,
        
        /// Maximum tokens (input + output) before termination
        max-tokens: option<u64>,
        
        /// Timeout in milliseconds
        timeout-ms: option<u64>,
        
        /// Whether to inherit parent's conversation context
        inherit-messages: bool,
        
        /// Custom data (JSON) passed to the child module
        custom-data: option<string>,
        
        /// System prompt override for the child
        system-prompt: option<string>,
    }
    
    /// How to specify which module to use
    variant context-module {
        /// Use the same WASM module as the current context
        same,
        /// Load a different module by name (from configured module path)
        named(string),
        /// Load a module from a specific file path
        path(string),
    }
    
    /// Result from a completed child context
    record context-result {
        /// The final response text
        response: string,
        
        /// Structured data returned by the child (JSON)
        data: option<string>,
        
        /// Token usage for the entire child context
        usage: token-usage,
        
        /// How the context completed
        status: context-status,
        
        /// Files modified during the context
        modified-files: list<string>,
        
        /// Duration in milliseconds
        duration-ms: u64,
    }
    
    record token-usage {
        input-tokens: u64,
        output-tokens: u64,
        total-cost-microcents: option<u64>,
    }
    
    /// How a context completed
    variant context-status {
        /// Normal completion
        completed,
        /// Cancelled by parent or user
        cancelled,
        /// Hit max iterations
        max-iterations-reached,
        /// Hit token limit
        token-limit-reached,
        /// Hit timeout
        timeout,
        /// Error occurred
        error(string),
    }
    
    /// Spawn a new child context and wait for it to complete
    /// The current WASM execution is suspended until the child returns
    spawn: func(params: context-params) -> result<context-result, string>;
    
    /// Get information about the current context
    get-current-context: func() -> context-info;
    
    /// Get the depth of the current context (0 = root)
    get-context-depth: func() -> u32;
    
    /// Get parameters that were passed to this context
    get-context-params: func() -> option<context-params>;
    
    record context-info {
        /// Unique ID for this context
        id: string,
        /// Depth in the stack (0 = root)
        depth: u32,
        /// Module name/path
        module: string,
        /// Parent context ID (None for root)
        parent-id: option<string>,
        /// When this context started (unix ms)
        started-at: u64,
    }
}
```

## How Context Spawning Works

Since wasmtime supports async host functions, the implementation is straightforward:

```rust
// Host-side implementation of context::spawn

linker.func_wrap_async("context", "spawn",
    |caller: Caller<'_, HostState>, params: WasmContextParams| {
        Box::new(async move {
            let host = caller.data();
            
            // 1. Validate and prepare
            if host.context_depth >= MAX_CONTEXT_DEPTH {
                return Err("Maximum context depth exceeded".into());
            }
            
            // 2. Load the child WASM module
            let child_module = match &params.module {
                ContextModule::Same => host.current_module.clone(),
                ContextModule::Named(name) => {
                    host.module_registry.load(name).await?
                }
                ContextModule::Path(path) => {
                    Module::from_file(&host.engine, path)?
                }
            };
            
            // 3. Create child context state
            let child_state = HostState {
                // Shared resources
                engine: host.engine.clone(),
                providers: host.providers.clone(),
                base_tools: host.base_tools.clone(),
                cancel: host.cancel.child_token(),
                cwd: host.cwd.clone(),
                
                // Isolated state
                messages: if params.inherit_messages {
                    host.messages.clone()
                } else {
                    Vec::new()
                },
                enabled_tools: params.allowed_tools.clone()
                    .unwrap_or_else(|| host.enabled_tools.clone()),
                custom_tools: HashMap::new(),  // Fresh custom tools
                local_state: HashMap::new(),   // Fresh state
                context_depth: host.context_depth + 1,
                context_params: Some(params.clone()),
                current_module: child_module.clone(),
                
                // ... other fields
            };
            
            // 4. Apply tool restrictions
            if let Some(denied) = &params.denied_tools {
                for tool_id in denied {
                    child_state.enabled_tools.retain(|t| t != tool_id);
                }
            }
            
            // 5. Create child store and instance
            let mut child_store = Store::new(&host.engine, child_state);
            
            // Apply limits
            if let Some(timeout) = params.timeout_ms {
                child_store.set_epoch_deadline(timeout / EPOCH_INTERVAL_MS);
            }
            
            let child_instance = host.linker
                .instantiate_async(&mut child_store, &child_module)
                .await?;
            
            // 6. Call child's on_start
            let on_start = child_instance
                .get_typed_func::<(), Result<(), WasmLoopError>>(&mut child_store, "on-start")?;
            on_start.call_async(&mut child_store, ()).await??;
            
            // 7. Run the child's main loop
            let run_prompt = child_instance
                .get_typed_func::<String, Result<String, WasmLoopError>>(&mut child_store, "run-prompt")?;
            
            let start_time = Instant::now();
            let result = run_prompt.call_async(&mut child_store, params.task.clone()).await;
            let duration = start_time.elapsed();
            
            // 8. Call child's on_complete or on_cancel
            let child_data = child_store.data();
            
            let (response, status) = match result {
                Ok(Ok(response)) => {
                    let on_complete = child_instance
                        .get_typed_func::<String, Result<(), WasmLoopError>>(&mut child_store, "on-complete")?;
                    let _ = on_complete.call_async(&mut child_store, response.clone()).await;
                    (response, ContextStatus::Completed)
                }
                Ok(Err(loop_error)) => {
                    let status = match &loop_error {
                        WasmLoopError::Cancelled => ContextStatus::Cancelled,
                        WasmLoopError::DoomLoop(_) => ContextStatus::MaxIterationsReached,
                        WasmLoopError::ContextLimitExceeded => ContextStatus::TokenLimitReached,
                        e => ContextStatus::Error(e.to_string()),
                    };
                    (String::new(), status)
                }
                Err(trap) => {
                    // WASM trap (timeout, OOM, etc.)
                    if trap.to_string().contains("epoch") {
                        (String::new(), ContextStatus::Timeout)
                    } else {
                        (String::new(), ContextStatus::Error(trap.to_string()))
                    }
                }
            };
            
            // 9. Build result
            Ok(ContextResult {
                response,
                data: child_store.data().local_state.get("__result_data__").cloned(),
                usage: child_store.data().token_usage.clone(),
                status,
                modified_files: child_store.data().modified_files.clone(),
                duration_ms: duration.as_millis() as u64,
            })
        })
    }
)?;
```

## Example: Hierarchical Agent System

Here's an example of an orchestrator that delegates to specialist agents:

```rust
// orchestrator.wasm - The main entry point

wit_bindgen::generate!({
    world: "agent-loop-world",
    exports: {
        "wonopcode:agent-loop/agent-loop": Orchestrator,
    },
});

struct Orchestrator;

impl Guest for Orchestrator {
    fn on_start() -> Result<(), LoopError> {
        // Register our delegation tool
        custom_tools::register_tool(CustomToolDefinition {
            id: "delegate".into(),
            name: "delegate".into(),
            description: "Delegate a task to a specialist agent. Use this for complex \
                         subtasks that require focused expertise.".into(),
            parameters_schema: r#"{
                "type": "object",
                "properties": {
                    "specialist": {
                        "type": "string",
                        "enum": ["coder", "researcher", "reviewer", "planner"],
                        "description": "Which specialist to delegate to"
                    },
                    "task": {
                        "type": "string", 
                        "description": "Detailed description of the subtask"
                    },
                    "context": {
                        "type": "string",
                        "description": "Additional context from our conversation"
                    }
                },
                "required": ["specialist", "task"]
            }"#.into(),
            enabled_by_default: true,
        })?;
        
        Ok(())
    }
    
    fn run_prompt(input: String) -> Result<String, LoopError> {
        // Read user preferences
        let user_provider = config::get_user_preferred_provider()
            .unwrap_or_else(|| "anthropic".into());
        let user_model = config::get_user_preferred_model()
            .unwrap_or_else(|| "claude-sonnet-4".into());
        
        // Use user's preference for the orchestrator (it's user-facing)
        providers::set_provider(&user_provider)?;
        providers::set_model(&user_model)?;
        
        // Enable all tools for the orchestrator
        let all_tools = tools::list_tools();
        tools::set_enabled_tools(&all_tools.iter().map(|t| t.id.clone()).collect::<Vec<_>>())?;
        
        // Standard agentic loop
        let mut messages = storage::get_messages();
        messages.push(Message {
            role: MessageRole::User,
            content: input,
            ..Default::default()
        });
        
        loop {
            if system::is_cancelled() {
                return Err(LoopError::Cancelled);
            }
            
            // Get enabled tools (includes our custom "delegate" tool)
            let enabled = tools::get_enabled_tools();
            
            let handle = providers::stream_prompt(&messages, &enabled, StreamOptions {
                max_tokens: Some(4096),
                temperature: Some(0.7),
                thinking_budget: None,
            })?;
            
            let (text, tool_calls) = process_stream(handle)?;
            
            messages.push(Message {
                role: MessageRole::Assistant,
                content: text.clone(),
                tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls.clone()) },
                ..Default::default()
            });
            
            if tool_calls.is_empty() {
                return Ok(text);
            }
            
            // Execute tool calls
            let results = execute_tool_calls(&tool_calls)?;
            messages.push(Message {
                role: MessageRole::Tool,
                tool_results: Some(results),
                ..Default::default()
            });
        }
    }
}

impl CustomToolHandler for Orchestrator {
    fn handle_tool_call(tool_id: String, arguments: String) -> Result<String, String> {
        if tool_id != "delegate" {
            return Err(format!("Unknown tool: {}", tool_id));
        }
        
        let args: DelegateArgs = serde_json::from_str(&arguments)
            .map_err(|e| e.to_string())?;
        
        // Map specialist to module and configuration
        let (module, tools, provider, model) = match args.specialist.as_str() {
            "coder" => (
                "coder.wasm",
                vec!["read", "write", "edit", "patch", "bash", "glob", "grep"],
                Some("deepseek"),
                Some("deepseek-coder-v2"),
            ),
            "researcher" => (
                "researcher.wasm",
                vec!["read", "glob", "grep", "websearch", "webfetch"],
                Some("anthropic"),
                Some("claude-sonnet-4"),
            ),
            "reviewer" => (
                "reviewer.wasm",
                vec!["read", "glob", "grep"],  // Read-only
                Some("anthropic"),
                Some("claude-opus-4"),  // Best for careful review
            ),
            "planner" => (
                "planner.wasm",
                vec!["read", "glob"],  // Minimal tools
                Some("anthropic"),
                Some("claude-opus-4"),
            ),
            _ => return Err(format!("Unknown specialist: {}", args.specialist)),
        };
        
        // Build the task with context
        let full_task = if let Some(ctx) = args.context {
            format!("{}\n\nContext:\n{}", args.task, ctx)
        } else {
            args.task
        };
        
        // Spawn the specialist context
        let result = context::spawn(ContextParams {
            module: ContextModule::Named(module.into()),
            task: full_task,
            allowed_tools: Some(tools.iter().map(|s| s.to_string()).collect()),
            denied_tools: None,
            preferred_provider: provider.map(Into::into),
            preferred_model: model.map(Into::into),
            max_iterations: Some(30),
            max_tokens: Some(100_000),
            timeout_ms: Some(300_000),  // 5 minutes
            inherit_messages: false,  // Fresh context
            custom_data: None,
            system_prompt: None,
        }).map_err(|e| e)?;
        
        // Format the result
        match result.status {
            ContextStatus::Completed => {
                let usage_info = format!(
                    "\n\n[Specialist used {} input + {} output tokens in {}ms]",
                    result.usage.input_tokens,
                    result.usage.output_tokens,
                    result.duration_ms,
                );
                Ok(format!("{}{}", result.response, usage_info))
            }
            ContextStatus::Cancelled => {
                Err("Specialist task was cancelled".into())
            }
            ContextStatus::MaxIterationsReached => {
                Ok(format!(
                    "{}\n\n[Warning: Specialist hit iteration limit]",
                    result.response
                ))
            }
            ContextStatus::Timeout => {
                Err("Specialist task timed out".into())
            }
            ContextStatus::Error(e) => {
                Err(format!("Specialist error: {}", e))
            }
            _ => Ok(result.response),
        }
    }
}

#[derive(Deserialize)]
struct DelegateArgs {
    specialist: String,
    task: String,
    context: Option<String>,
}
```

## Specialist Agent Example (coder.wasm)

```rust
// coder.wasm - Specialist for coding tasks

struct Coder;

impl Guest for Coder {
    fn on_start() -> Result<(), LoopError> {
        // Get our task context
        let params = context::get_context_params();
        let depth = context::get_context_depth();
        
        system::log(LogLevel::Info, &format!(
            "Coder agent starting at depth {} with task: {}",
            depth,
            params.as_ref().map(|p| &p.task[..50.min(p.task.len())]).unwrap_or("?")
        ));
        
        // We can also register sub-tools for our own use
        custom_tools::register_tool(CustomToolDefinition {
            id: "run_tests".into(),
            name: "run_tests".into(),
            description: "Run tests and analyze failures".into(),
            parameters_schema: r#"{"type": "object", "properties": {}}"#.into(),
            enabled_by_default: true,
        })?;
        
        Ok(())
    }
    
    fn run_prompt(input: String) -> Result<String, LoopError> {
        // Use the preferred provider/model from our spawn params
        // (or override if we know better)
        let input_lower = input.to_lowercase();
        
        if input_lower.contains("test") || input_lower.contains("debug") {
            // For test-related work, might want to use a different model
            providers::set_model("claude-sonnet-4")?;
        }
        // Otherwise, use whatever was set for us (probably deepseek-coder)
        
        // Standard coding loop with our tools
        // ... implementation ...
        
        Ok("Coding complete".into())
    }
}

impl CustomToolHandler for Coder {
    fn handle_tool_call(tool_id: String, arguments: String) -> Result<String, String> {
        match tool_id.as_str() {
            "run_tests" => {
                // Execute tests using bash
                let result = tools::execute_tool("bash", 
                    &json!({"command": "cargo test 2>&1"}).to_string())?;
                
                if !result.success {
                    // Parse failures and provide analysis
                    Ok(format!("Tests failed:\n{}\n\nAnalyzing failures...", result.content))
                } else {
                    Ok("All tests passed!".into())
                }
            }
            _ => Err(format!("Unknown tool: {}", tool_id)),
        }
    }
}
```

---

# Complete WIT Interface Definition

```wit
// wonopcode.wit - Complete interface definition with context support

package wonopcode:agent-loop@0.2.0;

// =============================================================================
// EXPORTS - What WASM modules must implement
// =============================================================================

/// The main interface that WASM modules must implement
interface agent-loop {
    use types.{loop-error, loop-capabilities};
    
    /// Run a single prompt through the agent loop
    run-prompt: func(input: string) -> result<string, loop-error>;
    
    /// Called when starting (after spawn, before first run-prompt)
    on-start: func() -> result<_, loop-error>;
    
    /// Called when completing normally
    on-complete: func(result: string) -> result<_, loop-error>;
    
    /// Called when cancelled
    on-cancel: func() -> result<_, loop-error>;
    
    /// Get loop metadata
    get-name: func() -> string;
    get-capabilities: func() -> loop-capabilities;
}

/// Handler for custom tools defined by this module
interface custom-tool-handler {
    /// Called when the LLM invokes a custom tool registered by this module
    handle-tool-call: func(tool-id: string, arguments: string) -> result<string, string>;
}

// =============================================================================
// SHARED TYPES
// =============================================================================

interface types {
    /// Error types
    variant loop-error {
        provider-error(string),
        tool-error(string),
        permission-denied(string),
        doom-loop(string),
        cancelled,
        context-limit-exceeded,
        context-spawn-failed(string),
        internal(string),
    }
    
    /// Capabilities declaration
    record loop-capabilities {
        streaming: bool,
        extended-thinking: bool,
        parallel-tools: bool,
        tool-observation: bool,
        max-context: u32,
        auto-compaction: bool,
        supports-sub-contexts: bool,
        supports-custom-tools: bool,
    }
}

// =============================================================================
// IMPORTS - What the host provides to WASM
// =============================================================================

/// User configuration and preferences
interface config {
    /// Get user's preferred provider
    get-user-preferred-provider: func() -> option<string>;
    
    /// Get user's preferred model
    get-user-preferred-model: func() -> option<string>;
    
    /// Get arbitrary config value
    get-config: func(key: string) -> option<string>;
    
    /// Get all user preferences
    get-user-preferences: func() -> list<tuple<string, string>>;
}

/// Provider management
interface providers {
    record provider-info {
        id: string,
        name: string,
        models: list<string>,
        supports-streaming: bool,
        supports-tools: bool,
        supports-thinking: bool,
    }
    
    list-providers: func() -> list<provider-info>;
    get-current-provider: func() -> option<string>;
    get-current-model: func() -> option<string>;
    set-provider: func(provider-id: string) -> result<_, string>;
    set-model: func(model-id: string) -> result<_, string>;
    
    // Streaming
    stream-prompt: func(
        messages: list<message>,
        tools: list<string>,
        options: stream-options,
    ) -> result<stream-handle, string>;
    
    read-stream-chunk: func(handle: stream-handle) -> option<stream-chunk>;
    close-stream: func(handle: stream-handle);
    
    // Message types
    record message {
        role: message-role,
        content: string,
        tool-calls: option<list<tool-call>>,
        tool-results: option<list<tool-result>>,
    }
    
    enum message-role { system, user, assistant, tool }
    
    record tool-call {
        id: string,
        name: string,
        arguments: string,
    }
    
    record tool-result {
        call-id: string,
        content: string,
        is-error: bool,
    }
    
    record stream-options {
        max-tokens: option<u32>,
        temperature: option<f32>,
        thinking-budget: option<u32>,
    }
    
    type stream-handle = u64;
    
    variant stream-chunk {
        text-delta(string),
        tool-call-start(tool-call-start-data),
        tool-call-delta(tool-call-delta-data),
        tool-call-end(string),
        thinking-delta(string),
        usage(usage-data),
        done,
        error(string),
    }
    
    record tool-call-start-data { id: string, name: string }
    record tool-call-delta-data { id: string, arguments-delta: string }
    record usage-data { input-tokens: u32, output-tokens: u32 }
}

/// Tool management with enable/disable support
interface tools {
    record tool-info {
        id: string,
        name: string,
        description: string,
        parameters-schema: string,
        category: tool-category,
        requires-permission: bool,
        is-custom: bool,
    }
    
    enum tool-category {
        file-system,
        process,
        network,
        mcp,
        internal,
        custom,
    }
    
    // Listing
    list-tools: func() -> list<tool-info>;
    get-tools-by-category: func(category: tool-category) -> list<tool-info>;
    
    // Enable/disable
    enable-tool: func(tool-id: string) -> result<_, string>;
    disable-tool: func(tool-id: string) -> result<_, string>;
    get-enabled-tools: func() -> list<string>;
    set-enabled-tools: func(tool-ids: list<string>) -> result<_, string>;
    is-tool-enabled: func(tool-id: string) -> bool;
    
    // Execution
    execute-tool: func(name: string, arguments: string) -> result<tool-execution-result, string>;
    execute-tools-parallel: func(calls: list<tool-execution-request>) -> list<tool-execution-result>;
    
    record tool-execution-request {
        id: string,
        name: string,
        arguments: string,
    }
    
    record tool-execution-result {
        call-id: string,
        success: bool,
        content: string,
        error: option<string>,
        duration-ms: u64,
    }
    
    // Permissions
    check-permission: func(tool-name: string, arguments: string) -> permission-status;
    request-permission: func(tool-name: string, arguments: string) -> bool;
    
    enum permission-status { allowed, denied, needs-approval }
}

/// Custom tool registration
interface custom-tools {
    record custom-tool-definition {
        id: string,
        name: string,
        description: string,
        parameters-schema: string,
        enabled-by-default: bool,
    }
    
    register-tool: func(definition: custom-tool-definition) -> result<_, string>;
    unregister-tool: func(tool-id: string) -> result<_, string>;
    list-custom-tools: func() -> list<string>;
}

/// Context management for spawning sub-contexts
interface context {
    record context-params {
        module: context-module,
        task: string,
        allowed-tools: option<list<string>>,
        denied-tools: option<list<string>>,
        preferred-provider: option<string>,
        preferred-model: option<string>,
        max-iterations: option<u32>,
        max-tokens: option<u64>,
        timeout-ms: option<u64>,
        inherit-messages: bool,
        custom-data: option<string>,
        system-prompt: option<string>,
    }
    
    variant context-module {
        same,
        named(string),
        path(string),
    }
    
    record context-result {
        response: string,
        data: option<string>,
        usage: token-usage,
        status: context-status,
        modified-files: list<string>,
        duration-ms: u64,
    }
    
    record token-usage {
        input-tokens: u64,
        output-tokens: u64,
        total-cost-microcents: option<u64>,
    }
    
    variant context-status {
        completed,
        cancelled,
        max-iterations-reached,
        token-limit-reached,
        timeout,
        error(string),
    }
    
    record context-info {
        id: string,
        depth: u32,
        module: string,
        parent-id: option<string>,
        started-at: u64,
    }
    
    /// Spawn a child context (suspends current execution)
    spawn: func(params: context-params) -> result<context-result, string>;
    
    /// Get info about current context
    get-current-context: func() -> context-info;
    get-context-depth: func() -> u32;
    get-context-params: func() -> option<context-params>;
}

/// UI updates
interface ui {
    send-text: func(text: string);
    send-thinking: func(text: string);
    tool-start: func(call-id: string, tool-name: string);
    tool-result: func(call-id: string, result: string, is-error: bool);
    show-progress: func(message: string, progress: option<f32>);
    hide-progress: func();
    show-message: func(level: message-level, message: string);
    
    enum message-level { info, warning, error }
    
    request-input: func(prompt: string) -> option<string>;
    request-confirmation: func(message: string) -> bool;
}

/// Storage
interface storage {
    use providers.{message};
    
    get-messages: func() -> list<message>;
    add-message: func(message: message);
    clear-messages: func();
    get-token-count: func() -> u64;
    
    compact-messages: func(strategy: compaction-strategy) -> result<u32, string>;
    
    variant compaction-strategy {
        keep-last(u32),
        token-limit(u32),
        summarize,
    }
    
    get-state: func(key: string) -> option<string>;
    set-state: func(key: string, value: string);
    delete-state: func(key: string);
}

/// System utilities
interface system {
    log: func(level: log-level, message: string);
    enum log-level { trace, debug, info, warn, error }
    
    current-time-ms: func() -> u64;
    is-cancelled: func() -> bool;
    get-cwd: func() -> string;
    sleep-ms: func(ms: u64);
}

// =============================================================================
// WORLD DEFINITION
// =============================================================================

world agent-loop-world {
    // WASM module exports
    export agent-loop;
    export custom-tool-handler;
    
    // Host provides
    import config;
    import providers;
    import tools;
    import custom-tools;
    import context;
    import ui;
    import storage;
    import system;
}
```

---

# Implementation Strategy

## Phase 1: Core Trait (Native Only)

1. Create `crates/wonopcode-core/src/agent_loop/mod.rs`
2. Define `AgentLoop` trait, `LoopContext`, `LoopError`
3. Implement `StandardLoop` by extracting from `runner.rs`
4. Wire up `Runner` to use trait

## Phase 2: WASM Support (No Context Stack)

1. Add WIT definitions to `crates/wonopcode-core/src/wit/`
2. Add `wasmtime` + `wit-bindgen` dependencies
3. Implement `WasmAgentLoop` with basic host functions
4. Support provider/model selection and tool enable/disable
5. Test with simple WASM modules

## Phase 3: Custom Tools

1. Add `custom-tools` and `custom-tool-handler` interfaces
2. Implement tool registration in host state
3. Wire custom tools into tool execution path
4. Test with WASM modules that define tools

## Phase 4: Recursive Context Stack

1. Add `context` interface
2. Implement context spawn with proper isolation
3. Add module registry for loading WASM modules by name
4. Implement token/iteration/timeout limits
5. Test with hierarchical agent examples

## Phase 5: Production Hardening

1. Hot reloading of WASM modules
2. Telemetry and observability
3. Cost tracking across contexts
4. Context caching for repeated patterns

---

# Open Design Questions

1. **Module Discovery**: How do we discover and list available WASM modules?
   - File system scan of a modules directory?
   - Registry in config file?
   - Remote module repository?

2. **Module Versioning**: How do we handle WIT interface evolution?
   - Version in package name (`wonopcode:agent-loop@0.2.0`)
   - Compatibility checking at load time
   - Graceful degradation for missing functions?

3. **Cost Attribution**: How do we track costs across context hierarchy?
   - Each context tracks its own usage
   - Roll up to parent on completion
   - Global cost limits?

4. **State Persistence**: Should WASM module state persist across sessions?
   - Currently isolated per context
   - Could offer opt-in persistence via storage interface

5. **Debugging**: How do we debug complex context hierarchies?
   - Structured logging with context IDs
   - Trace visualization
   - Breakpoints in WASM?

6. **Security Boundaries**: Even though not for sandboxing, should there be any limits?
   - Prevent infinite spawn recursion
   - Resource quotas (memory, CPU)
   - Network access control?

---

# Conclusion

The proposed architecture provides:

1. **Full orchestrator control** over provider/model selection with access to user preferences
2. **Dynamic tool management** with enable/disable and custom tool definition
3. **Recursive context stack** for hierarchical agent delegation
4. **Clean isolation** between contexts while sharing host capabilities
5. **Flexible module system** supporting same-module recursion and specialist modules

This design enables building sophisticated multi-agent systems entirely in WASM, with the host providing a stable, capable runtime environment.
