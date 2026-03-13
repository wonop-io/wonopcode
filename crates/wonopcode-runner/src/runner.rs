//! Runner module - connects the TUI to the AI prompt loop.
// @ace:implements COMP-T90R9Q-8J4

/// Default capacity for the action channel.
/// This provides backpressure when the runner is overwhelmed with incoming actions.
pub const DEFAULT_ACTION_CHANNEL_CAPACITY: usize = 256;

use async_trait::async_trait;
use futures::future::join_all;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};
use wonopcode_agent_loop::{
    BoxedAgentLoop, CompactionConfig as LoopCompactionConfig, LoopConfig, LoopContext, LoopError,
    LoopUpdate, MemoryState, ObservationalMemoryConfig, PermissionCheckRequest, PermissionChecker,
    SystemPromptSource, TokenStateMachine, persistence::ObservationPersistence,
};
use wonopcode_core::bus::{
    Bus, PermissionRequest as BusPermissionRequest, PermissionResponse as BusPermissionResponse,
    SandboxState, SandboxStatusChanged,
};
use wonopcode_core::config::{McpConfig, McpRemoteConfig, SandboxConfig as CoreSandboxConfig};
use wonopcode_core::permission::{Decision, PermissionManager};
use wonopcode_core::system_prompt;
use wonopcode_core::Instance;
use wonopcode_core::SessionService;
use wonopcode_mcp::{McpClient, ServerConfig as McpServerConfig};
use wonopcode_provider::{
    anthropic::AnthropicProvider, claude_cli::ClaudeCliProvider, model::ModelInfo,
    openai::OpenAIProvider, BoxedLanguageModel, Message as ProviderMessage, ToolDefinition,
};
use wonopcode_sandbox::{SandboxConfig, SandboxManager, SandboxRuntime, SandboxRuntimeType};
use wonopcode_server::GitOperations;
use wonopcode_snapshot::{SnapshotConfig, SnapshotStore};
use wonopcode_tools::{
    ace::state::WorkstreamState, mcp::McpToolsBuilder, mcp_todo_adapter, todo, ToolEvent,
    ToolRegistry,
};
use wonopcode_tui::{
    AppAction, AppUpdate, GitCommitUpdate, GitFileUpdate, GitStatusUpdate, McpStatusUpdate,
    PermissionRequestUpdate, PhaseUpdate, SaveScope, TodoUpdate,
};
use wonopcode_util::FileTimeState;

use crate::compaction;
use crate::compaction::{CompactionConfig, CompactionProgress, CompactionResult, ProgressCallback};

/// Adapter that implements `PermissionChecker` for `PermissionManager`.
///
/// This allows the agent loop to check permissions without directly depending
/// on the full `wonopcode-core` permission system.
pub struct PermissionCheckerAdapter {
    permission_manager: Arc<PermissionManager>,
}

impl PermissionCheckerAdapter {
    /// Create a new adapter wrapping a permission manager.
    pub fn new(permission_manager: Arc<PermissionManager>) -> Self {
        Self { permission_manager }
    }
}

#[async_trait]
impl PermissionChecker for PermissionCheckerAdapter {
    async fn check_permission(
        &self,
        session_id: &str,
        request: PermissionCheckRequest,
        timeout: Option<std::time::Duration>,
    ) -> bool {
        // Convert the request to PermissionCheck format used by PermissionManager
        let check = wonopcode_core::permission::PermissionCheck {
            id: request.id.clone(),
            tool: request.tool.clone(),
            action: request.action.clone(),
            path: request.path.clone(),
            description: request.description.clone(),
            details: request.details.clone(),
        };

        let has_sandbox = self.permission_manager.is_sandbox_running();
        let permission_future =
            self.permission_manager
                .check_with_sandbox(session_id, check, has_sandbox);

        // Apply timeout if specified
        if let Some(timeout_duration) = timeout {
            match tokio::time::timeout(timeout_duration, permission_future).await {
                Ok(result) => result,
                Err(_) => {
                    // Timeout expired - cancel the pending request and deny
                    tracing::warn!(
                        request_id = %request.id,
                        tool = %request.tool,
                        timeout_secs = timeout_duration.as_secs(),
                        "Permission request timed out (provider timeout)"
                    );
                    // Clean up the pending request and notify clients to dismiss the dialog
                    self.permission_manager
                        .cleanup_timed_out_request(&request.id)
                        .await;
                    false
                }
            }
        } else {
            // No timeout - wait indefinitely (use PermissionManager's internal timeout)
            permission_future.await
        }
    }

    fn is_sandbox_running(&self) -> bool {
        self.permission_manager.is_sandbox_running()
    }
}

/// Adapter that implements `TsPermissionChecker` for `PermissionManager`.
///
/// This allows TypeScript tools to check permissions through the same
/// permission system used by the rest of the agent.
pub struct TsPermissionCheckerAdapter {
    permission_manager: Arc<PermissionManager>,
}

impl TsPermissionCheckerAdapter {
    /// Create a new adapter wrapping a permission manager.
    pub fn new(permission_manager: Arc<PermissionManager>) -> Self {
        Self { permission_manager }
    }
}

#[async_trait]
impl wonopcode_tools::TsPermissionChecker for TsPermissionCheckerAdapter {
    async fn check(
        &self,
        session_id: &str,
        request: wonopcode_tools::TsPermissionRequest,
    ) -> bool {
        // Convert the request to PermissionCheck format used by PermissionManager
        let check = wonopcode_core::permission::PermissionCheck {
            id: request.id,
            tool: request.tool,
            action: request.action,
            path: request.path,
            description: request.description,
            details: request.details.unwrap_or(serde_json::Value::Null),
        };

        let has_sandbox = self.permission_manager.is_sandbox_running();
        self.permission_manager
            .check_with_sandbox(session_id, check, has_sandbox)
            .await
    }

    async fn cleanup_request(&self, request_id: &str) {
        self.permission_manager
            .cleanup_timed_out_request(request_id)
            .await
    }
}

/// Adapter that implements `SystemPromptSource` for dynamic system prompt rendering.
///
/// Re-renders the system prompt from a Tera template before every LLM invocation,
/// ensuring fresh values for date, time, git branch, and AGENTS.md content.
struct RunnerSystemPromptSource {
    cwd: std::path::PathBuf,
    hms_service: Option<wonopcode_tools::SharedHmsService>,
    renderer: system_prompt::SystemPromptRenderer,
    model_name: String,
    provider_name: String,
}

impl RunnerSystemPromptSource {
    fn new(
        cwd: std::path::PathBuf,
        provider_name: String,
        model_name: String,
        hms_service: Option<wonopcode_tools::SharedHmsService>,
    ) -> Result<Self, String> {
        Ok(Self {
            cwd,
            hms_service,
            renderer: system_prompt::SystemPromptRenderer::new()?,
            model_name,
            provider_name,
        })
    }
}

#[async_trait]
impl SystemPromptSource for RunnerSystemPromptSource {
    async fn render_system_prompt(&self) -> Option<String> {
        // 1. Get AGENTS.md content - try HMS first, fall back to disk
        let agent_md = if let Some(ref hms) = self.hms_service {
            // Re-render AGENTS.md from HMS (memory.yaml + template)
            match hms.write().await.generate(&self.cwd).await {
                Ok(content) => Some(content),
                Err(e) => {
                    debug!("HMS generate failed, falling back to disk: {}", e);
                    system_prompt::load_custom_instructions(&self.cwd)
                }
            }
        } else {
            system_prompt::load_custom_instructions(&self.cwd)
        };

        // 2. Build fresh variables from current environment
        let mut vars = system_prompt::SystemPromptVars::from_env(
            &self.cwd,
            &self.model_name,
            &self.provider_name,
        );
        vars.agent_md = agent_md;

        // 3. Render the template
        match self.renderer.render(&vars) {
            Ok(rendered) => Some(rendered),
            Err(e) => {
                warn!("Failed to render system prompt template: {}", e);
                None
            }
        }
    }
}

/// Helper to send updates to the TUI with proper error logging.
/// This replaces `let _ = update_tx.send(...)` to avoid silent failures.
fn send_update(update_tx: &mpsc::UnboundedSender<AppUpdate>, update: AppUpdate) {
    if let Err(e) = update_tx.send(update) {
        warn!("Failed to send update to TUI (channel closed): {}", e);
    }
}

/// Create a progress callback that sends CompactionProgress events to the TUI.
fn create_progress_callback(update_tx: mpsc::UnboundedSender<AppUpdate>) -> ProgressCallback {
    Arc::new(move |progress: CompactionProgress| {
        if let Err(e) = update_tx.send(AppUpdate::CompactionProgress {
            current_chunk: progress.current_chunk,
            total_chunks: progress.total_chunks,
            phase: progress.phase,
        }) {
            warn!("Failed to send compaction progress update: {}", e);
        }
    })
}

/// Convert PhasedTodos to TUI update format.
fn convert_phased_todos_to_updates(
    phased: &todo::PhasedTodos,
) -> (Vec<PhaseUpdate>, Vec<TodoUpdate>) {
    let phases: Vec<PhaseUpdate> = phased
        .phases
        .iter()
        .map(|p| PhaseUpdate {
            id: p.id.clone(),
            name: p.name.clone(),
            status: p.status().as_str().to_string(),
            todos: p
                .todos
                .iter()
                .map(|t| TodoUpdate {
                    id: t.id.clone(),
                    content: t.content.clone(),
                    status: t.status.as_str().to_string(),
                    priority: t.priority.as_str().to_string(),
                    phase_id: Some(p.id.clone()),
                    parents: t.parents.clone(),
                })
                .collect(),
        })
        .collect();

    // Also build flat list for backward compatibility
    let todos: Vec<TodoUpdate> = phased
        .phases
        .iter()
        .flat_map(|p| {
            p.todos.iter().map(move |t| TodoUpdate {
                id: t.id.clone(),
                content: t.content.clone(),
                status: t.status.as_str().to_string(),
                priority: t.priority.as_str().to_string(),
                phase_id: Some(p.id.clone()),
                parents: t.parents.clone(),
            })
        })
        .collect();

    (phases, todos)
}

/// Convert LoopUpdate observation snapshot to TUI observation update.
fn convert_observation_snapshot(
    snapshot: wonopcode_agent_loop::ObservationSnapshot,
) -> wonopcode_tui::ObservationUpdate {
    wonopcode_tui::ObservationUpdate {
        id: snapshot.id,
        priority: snapshot.priority,
        timestamp: snapshot.timestamp,
        content: snapshot.content,
        children: snapshot.children.into_iter()
            .map(convert_observation_snapshot)
            .collect(),
        pinned: snapshot.pinned,
    }
}

/// Convert an Observation from persistence to ObservationSnapshot for UI display.
/// This is used when loading observations from disk on startup.
fn observation_to_snapshot(obs: &wonopcode_agent_loop::Observation) -> wonopcode_agent_loop::ObservationSnapshot {
    use wonopcode_agent_loop::Priority;
    wonopcode_agent_loop::ObservationSnapshot {
        id: obs.id.clone(),
        priority: match obs.priority {
            Priority::High => "high".to_string(),
            Priority::Medium => "medium".to_string(),
            Priority::Low => "low".to_string(),
        },
        timestamp: obs.observation_date.format("%H:%M").to_string(),
        content: obs.content.clone(),
        children: obs.children.iter()
            .map(observation_to_snapshot)
            .collect(),
        pinned: obs.pinned,
    }
}

/// Parse MCP TODO tool output and convert it to PhasedTodos.
/// This is a simplified parser that handles the common markdown output format from the todowrite tool.
fn parse_mcp_todo_output_simple(output: &str) -> Result<todo::PhasedTodos, serde_json::Error> {
    // Try to parse as JSON first (may be embedded in markdown response)
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(output) {
        // Try direct PhasedTodos format
        if let Ok(phased_todos) = serde_json::from_value::<todo::PhasedTodos>(value.clone()) {
            return Ok(phased_todos);
        }
        // Try parsing phases array from JSON
        if let Some(phases_value) = value.get("phases") {
            if let Ok(phases) = serde_json::from_value::<Vec<todo::Phase>>(phases_value.clone()) {
                let mut phased_todos = todo::PhasedTodos::new();
                for phase in phases {
                    phased_todos.add_phase(phase);
                }
                return Ok(phased_todos);
            }
        }
    }

    // Parse markdown format (the most common output from todowrite)
    // Format:
    // ## ○ Phase Name (0/3 done)
    //   [ ] [high] Task description (task_id)
    //   [>] [medium] In progress task (task_id_2)
    //   [x] [low] Completed task (task_id_3)
    let mut phased_todos = todo::PhasedTodos::new();
    let mut current_phase: Option<todo::Phase> = None;

    for line in output.lines() {
        let trimmed = line.trim();

        // Phase header: ## ○ Phase Name (0/3 done)
        if trimmed.starts_with("##") {
            // Save previous phase if exists
            if let Some(phase) = current_phase.take() {
                phased_todos.add_phase(phase);
            }

            // Extract phase name
            let phase_line = trimmed.trim_start_matches("##").trim();
            let phase_name = if let Some(pos) = phase_line.find('(') {
                phase_line[..pos].trim()
            } else {
                phase_line
            };

            // Remove status icon if present (○, ◐, ●)
            let phase_name = phase_name.trim_start_matches(['○', '◐', '●']).trim();

            current_phase = Some(todo::Phase::new(
                format!("phase_{}", phased_todos.phases.len() + 1),
                phase_name,
            ));
        }
        // Todo item: [ ] [priority] description (id)
        else if trimmed.starts_with('[') {
            if let Some(ref mut phase) = current_phase {
                if let Some(todo_item) = parse_markdown_todo_line(trimmed) {
                    phase.add_todo(todo_item);
                }
            }
        }
    }

    // Save final phase
    if let Some(phase) = current_phase {
        phased_todos.add_phase(phase);
    }

    // If no phases were parsed, return error
    if phased_todos.phases.is_empty() {
        return Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "No valid TODO phases found in output",
        )));
    }

    Ok(phased_todos)
}

/// Parse a single markdown TODO line into a TodoItem.
fn parse_markdown_todo_line(line: &str) -> Option<todo::TodoItem> {
    let line = line.trim();

    // Extract status icon: [ ], [>], [x], [-]
    let status = if line.starts_with("[ ]") {
        todo::TodoStatus::Pending
    } else if line.starts_with("[>]") {
        todo::TodoStatus::InProgress
    } else if line.starts_with("[x]") {
        todo::TodoStatus::Completed
    } else if line.starts_with("[-]") {
        todo::TodoStatus::Cancelled
    } else {
        return None;
    };

    // Remove status part
    let rest = line[3..].trim();

    // Extract priority if present: [high], [medium], [low]
    let (priority, rest) = if rest.starts_with('[') {
        if let Some(end) = rest.find(']') {
            let priority_str = &rest[1..end];
            let priority = match priority_str {
                "high" => todo::TodoPriority::High,
                "medium" => todo::TodoPriority::Medium,
                "low" => todo::TodoPriority::Low,
                _ => todo::TodoPriority::Medium,
            };
            (priority, rest[end + 1..].trim())
        } else {
            (todo::TodoPriority::Medium, rest)
        }
    } else {
        (todo::TodoPriority::Medium, rest)
    };

    // Extract ID from parentheses at the end
    let (content, id) = if let Some(start) = rest.rfind('(') {
        if let Some(end) = rest.rfind(')') {
            if end > start {
                let id = rest[start + 1..end].trim().to_string();
                let content = rest[..start].trim().to_string();
                (content, id)
            } else {
                // Generate a simple ID based on content hash if no ID found
                let hash =
                    rest.len() as u64 * 31 + rest.as_bytes().iter().map(|&b| b as u64).sum::<u64>();
                (rest.to_string(), format!("todo_{:x}", hash))
            }
        } else {
            let hash =
                rest.len() as u64 * 31 + rest.as_bytes().iter().map(|&b| b as u64).sum::<u64>();
            (rest.to_string(), format!("todo_{:x}", hash))
        }
    } else {
        let hash = rest.len() as u64 * 31 + rest.as_bytes().iter().map(|&b| b as u64).sum::<u64>();
        (rest.to_string(), format!("todo_{:x}", hash))
    };

    Some(todo::TodoItem {
        id,
        content,
        status,
        priority,
        parents: vec![],
    })
}

/// Wrapper to store `Arc<dyn SandboxRuntime>` as `Arc<dyn Any + Send + Sync>`.
/// This allows sharing sandbox runtime through permission manager without circular deps.
pub struct SandboxRuntimeWrapper(pub Arc<dyn SandboxRuntime>);

/// Context usage level for UI display and compaction decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextLevel {
    /// < 50% - plenty of room
    Comfortable,
    /// 50-80% - normal usage
    Normal,
    /// 80-95% - approaching limit, compaction may occur
    High,
    /// > 95% - critical, compaction imminent
    Critical,
}

impl ContextLevel {
    /// Determine context level from usage percentage.
    pub fn from_percent(percent: u8) -> Self {
        match percent {
            0..=49 => Self::Comfortable,
            50..=79 => Self::Normal,
            80..=94 => Self::High,
            _ => Self::Critical,
        }
    }

    /// Get a human-readable description.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Comfortable => "Comfortable",
            Self::Normal => "Normal",
            Self::High => "High - compaction may occur",
            Self::Critical => "Critical - compaction imminent",
        }
    }
}

/// Token tracking state for context management.
///
/// This struct tracks token usage to enable token-based compaction decisions
/// rather than just message count. It maintains both estimated tokens (from
/// message content) and actual tokens (from provider responses).
#[derive(Debug, Clone, Default)]
pub struct ContextState {
    /// Estimated tokens from current message history (calculated from content).
    pub estimated_tokens: u32,
    /// Last actual input tokens reported by the provider (per-turn).
    pub last_input_tokens: u32,
    /// Last actual output tokens reported by the provider (per-turn).
    pub last_output_tokens: u32,
    /// Cumulative input tokens for the entire session.
    pub session_input_tokens: u32,
    /// Cumulative output tokens for the entire session.
    pub session_output_tokens: u32,
    /// Cumulative cost for the entire session.
    pub session_cost: f64,
    /// Context limit from the model.
    pub context_limit: u32,
    /// Output token reserve (tokens reserved for model response).
    pub output_reserve: u32,
}

impl ContextState {
    /// Create a new context state with a given context limit.
    pub fn new(context_limit: u32) -> Self {
        Self {
            context_limit,
            output_reserve: compaction::OUTPUT_TOKEN_MAX,
            ..Default::default()
        }
    }

    /// Calculate usable context (limit minus output reserve).
    pub fn usable_context(&self) -> u32 {
        self.context_limit.saturating_sub(self.output_reserve)
    }

    /// Calculate available context (usable minus estimated used).
    pub fn available(&self) -> u32 {
        self.usable_context().saturating_sub(self.estimated_tokens)
    }

    /// Calculate usage percentage (0-100).
    pub fn usage_percent(&self) -> u8 {
        let usable = self.usable_context();
        if usable == 0 {
            return 0;
        }
        ((self.estimated_tokens as u64 * 100) / usable as u64).min(100) as u8
    }

    /// Get the current context level.
    pub fn level(&self) -> ContextLevel {
        ContextLevel::from_percent(self.usage_percent())
    }

    /// Check if compaction is needed (>80% threshold).
    pub fn needs_compaction(&self) -> bool {
        self.usage_percent() >= 80
    }

    /// Check if context is critical (>95% threshold).
    pub fn is_critical(&self) -> bool {
        self.usage_percent() >= 95
    }

    /// Update estimated tokens from message history.
    pub fn update_estimated(&mut self, messages: &[ProviderMessage]) {
        self.estimated_tokens = compaction::estimate_messages_tokens(messages);
    }

    /// Update with actual tokens from provider response.
    pub fn update_actual(&mut self, input_tokens: u32, output_tokens: u32) {
        self.last_input_tokens = input_tokens;
        self.last_output_tokens = output_tokens;
        // Update estimated to match actual input (more accurate)
        // The actual input is what's in context, plus we need to account
        // for the output which will become input on next turn
        self.estimated_tokens = input_tokens + output_tokens;
    }

    /// Update context limit (e.g., when model changes).
    pub fn set_context_limit(&mut self, limit: u32) {
        self.context_limit = limit;
    }
}

/// Configuration for the runner.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Provider to use (anthropic, openai, openrouter).
    pub provider: String,
    /// Model ID.
    pub model_id: String,
    /// API key for the provider.
    pub api_key: String,
    /// System prompt.
    pub system_prompt: Option<String>,
    /// Maximum tokens.
    pub max_tokens: Option<u32>,
    /// Temperature.
    pub temperature: Option<f32>,
    /// Doom loop permission (ask, allow, deny).
    /// Default is "ask" which prompts the user when a doom loop is detected.
    pub doom_loop: Decision,
    /// Test provider settings (only used when provider is "test").
    pub test_provider_settings: Option<wonopcode_provider::test::TestProviderSettings>,
    /// Force allow all tool executions without permission prompts.
    /// Used in headless mode where there's no UI to prompt.
    pub allow_all: bool,
    /// Allow all tool executions when running inside a sandbox.
    /// Default is true - sandbox isolation makes unrestricted execution safe.
    pub allow_all_in_sandbox: bool,
    /// MCP HTTP URL for headless mode.
    /// When set, the Claude CLI provider will connect to this URL instead of
    /// spawning a child process for MCP tools.
    pub mcp_url: Option<String>,
    /// Secret for MCP server authentication.
    /// When set, the Claude CLI provider will include this in requests to the MCP server.
    pub mcp_secret: Option<String>,
    /// External MCP servers (local/stdio) to pass to Claude CLI.
    /// These are servers configured in .mcp.json that use command/args format.
    pub external_mcp_servers: HashMap<String, (Vec<String>, HashMap<String, String>)>,
    /// Working directory for the provider (used by Claude CLI).
    /// This sets the current working directory when spawning external processes.
    pub working_directory: Option<std::path::PathBuf>,
    /// Observational Memory configuration.
    /// When enabled, uses OM for context compression instead of legacy compaction.
    pub observational_memory: ObservationalMemoryConfig,
    /// Maximum number of agent loop iterations before stopping.
    /// None means unlimited (default).
    pub max_iterations: Option<u32>,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model_id: "claude-sonnet-4-5-20250929".to_string(),
            api_key: String::new(),
            system_prompt: None,
            max_tokens: Some(8192),
            temperature: Some(0.7),
            doom_loop: Decision::Ask,
            test_provider_settings: None,
            allow_all: false,
            allow_all_in_sandbox: true,
            mcp_url: None,
            mcp_secret: None,
            external_mcp_servers: HashMap::new(),
            working_directory: None,
            observational_memory: ObservationalMemoryConfig::enabled(), // OM enabled by default
            max_iterations: None, // Unlimited by default
        }
    }
}

/// The runner connects the TUI to the AI.
pub struct Runner {
    /// The agent loop implementation that handles prompt execution.
    agent_loop: tokio::sync::Mutex<BoxedAgentLoop>,
    config: Arc<RwLock<RunnerConfig>>,
    instance: Instance,
    provider: Arc<RwLock<BoxedLanguageModel>>,
    tools: Arc<ToolRegistry>,
    /// Cancellation token for the current operation. Reset after each prompt.
    cancel: Arc<RwLock<CancellationToken>>,
    /// Conversation history.
    history: RwLock<Vec<ProviderMessage>>,
    /// Compaction configuration.
    compaction_config: CompactionConfig,
    /// Snapshot store for file versioning.
    snapshot_store: Option<Arc<SnapshotStore>>,
    /// MCP client for external tools.
    mcp_client: Option<Arc<McpClient>>,
    /// External MCP servers (local/stdio) passed to Claude CLI.
    /// These work through the Claude CLI provider, not wonopcode's MCP client.
    external_mcp_server_names: Vec<String>,
    /// Unsupported/disabled MCP servers that should be shown in sidebar.
    unsupported_mcp_servers: Vec<(String, String)>, // (name, reason)
    /// Permission manager for tool execution control.
    permission_manager: Arc<PermissionManager>,
    /// Event bus for permission and other events.
    bus: Bus,
    /// File time tracker for detecting external modifications.
    file_time: Arc<FileTimeState>,
    /// Sandbox manager for isolated execution.
    sandbox_manager: Option<Arc<SandboxManager>>,
    /// Todo store for cross-process task tracking (shared with MCP server via temp file).
    todo_store: Arc<todo::SharedFileTodoStore>,
    /// Shared LSP client for status reporting.
    lsp_client: Arc<wonopcode_lsp::LspClient>,
    /// MCP TODO adapter for bridging MCP TODO tools to native events.
    mcp_todo_adapter: Option<mcp_todo_adapter::McpTodoAdapter>,
    /// Session service for history persistence.
    /// Optional for backward compatibility - if None, uses in-memory only.
    session_service: Option<Arc<SessionService>>,
    /// Context state for token tracking and compaction decisions.
    /// This enables token-based compaction rather than just message count.
    context_state: RwLock<ContextState>,
    /// Optional ticket service for ticket management tools.
    /// When set, ticket tools can access configured issue trackers.
    ticket_service: Option<Arc<dyn wonopcode_tools::TicketService>>,
    /// Optional memory service for memory tools.
    /// When set, memory tools can store and retrieve information across scopes.
    memory_service: Option<wonopcode_tools::SharedMemoryService>,
    /// Optional HMS service for hierarchical memory system.
    /// When set, HMS tools can access memory.yaml files and render AGENTS.md.
    hms_service: Option<wonopcode_tools::SharedHmsService>,
    // =========================================================================
    // Observational Memory (OM) state
    // =========================================================================
    /// Observational Memory configuration.
    om_config: ObservationalMemoryConfig,
    /// Memory state for OM (observations and statistics).
    /// Only initialized when OM is enabled.
    memory_state: Option<RwLock<MemoryState>>,
    /// Token state machine for OM threshold management.
    /// Only initialized when OM is enabled.
    token_state_machine: Option<RwLock<TokenStateMachine>>,
    /// TypeScript executor for in-process V8 execution.
    typescript_executor: Option<wonopcode_tools::SharedTypescriptExecutor>,
}

impl Runner {
    /// Create a new runner.
    #[allow(dead_code)]
    pub fn new(
        config: RunnerConfig,
        instance: Instance,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_bus(config, instance, None, None)
    }

    /// Create a new runner with optional shared Bus and PermissionManager.
    /// If not provided, creates new ones.
    pub fn new_with_bus(
        config: RunnerConfig,
        instance: Instance,
        shared_bus: Option<Bus>,
        shared_permission_manager: Option<Arc<PermissionManager>>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_agent_loop(
            config,
            instance,
            shared_bus,
            shared_permission_manager,
            None,
        )
    }

    /// Create a new runner with optional shared Bus, PermissionManager, and custom AgentLoop.
    ///
    /// This is the primary constructor that allows full customization of the agent loop.
    /// If `agent_loop` is None, a default `StandardLoop` is created.
    ///
    /// # Arguments
    /// * `config` - Runner configuration
    /// * `instance` - Project instance
    /// * `shared_bus` - Optional shared event bus
    /// * `shared_permission_manager` - Optional shared permission manager
    /// * `agent_loop` - Optional custom agent loop (e.g., WasmAgentLoop from Pro)
    pub fn new_with_agent_loop(
        config: RunnerConfig,
        instance: Instance,
        shared_bus: Option<Bus>,
        shared_permission_manager: Option<Arc<PermissionManager>>,
        agent_loop: Option<BoxedAgentLoop>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Create provider (sandbox state not known yet, will be determined by MCP server config)
        // For initial creation, use allow_all=false since we don't know sandbox state yet.
        // Provider will be recreated later with correct settings once sandbox is initialized.
        let provider = create_provider(&config, None, false)?;

        // Create shared file todo store for cross-process communication with MCP server.
        // This sets WONOPCODE_TODO_FILE env var which the MCP server will inherit.
        let todo_store = Arc::new(todo::SharedFileTodoStore::from_env_or_create());

        // Create shared LSP client for status reporting
        let lsp_client = Arc::new(wonopcode_lsp::LspClient::with_defaults());

        // Create tool registry with execute_typescript only
        // All other tools (bash, webfetch, lsp, ACE, tickets, memory) are now accessible
        // through the wonop.* API inside TypeScript code
        // Agents (task, plan_mode) are accessible via agents.* API in TypeScript
        // Skills are accessible via skills.* namespace in TypeScript
        let tools = ToolRegistry::with_builtins();

        // Use shared bus/permission_manager or create new ones
        let bus = shared_bus.unwrap_or_default();
        let permission_manager = shared_permission_manager
            .unwrap_or_else(|| Arc::new(PermissionManager::new(bus.clone())));

        // Create file time tracker
        let file_time = Arc::new(FileTimeState::new());

        // Use provided agent loop or create default StandardLoop with config max_iterations
        let max_iterations = config.max_iterations;
        let agent_loop: BoxedAgentLoop =
            agent_loop.unwrap_or_else(|| Box::new(wonopcode_agent_loop::StandardLoop::with_max_iterations(max_iterations)));

        debug!(
            loop_name = agent_loop.name(),
            "Runner created with agent loop"
        );

        // Get context limit from provider for initial context state
        let context_limit = provider.model_info().limit.context;

        // Extract OM config before wrapping in Arc<RwLock<>>
        let om_config = config.observational_memory.clone();

        // Initialize Observational Memory state if enabled
        let (memory_state, token_state_machine) = if om_config.enabled {
            info!(
                context_limit,
                observer_ratio = om_config.thresholds.observer_ratio,
                reflector_ratio = om_config.thresholds.reflector_ratio,
                project_dir = ?om_config.project_dir,
                "Observational Memory enabled"
            );
            let session_id = format!("session-{}", uuid::Uuid::new_v4());
            
            // Try to load observations from disk if project_dir is configured
            let memory_state = if let Some(ref project_dir) = om_config.project_dir {
                info!(
                    project_dir = %project_dir.display(),
                    "OM: Attempting to load observations from project directory"
                );
                let persistence = ObservationPersistence::new(project_dir);
                match persistence.load_into_state(&session_id, context_limit) {
                    Ok((state, loaded_date)) => {
                        if state.loaded_from_previous_session {
                            info!(
                                observations = state.observations.len(),
                                loaded_date = ?loaded_date,
                                "Loaded observations from previous session"
                            );
                        }
                        state
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to load observations from disk, starting fresh");
                        MemoryState::new(session_id, context_limit)
                    }
                }
            } else {
                info!("OM: No project_dir configured, starting with fresh memory state");
                MemoryState::new(session_id, context_limit)
            };
            
            let token_state_machine =
                TokenStateMachine::from_config(&om_config.thresholds, context_limit);
            (
                Some(RwLock::new(memory_state)),
                Some(RwLock::new(token_state_machine)),
            )
        } else {
            debug!("Observational Memory disabled, using legacy compaction");
            (None, None)
        };

        Ok(Self {
            agent_loop: tokio::sync::Mutex::new(agent_loop),
            config: Arc::new(RwLock::new(config)),
            instance,
            provider: Arc::new(RwLock::new(provider)),
            tools: Arc::new(tools),
            cancel: Arc::new(RwLock::new(CancellationToken::new())),
            history: RwLock::new(Vec::new()),
            compaction_config: CompactionConfig::default(),
            snapshot_store: None, // Will be initialized async in new_with_features
            mcp_client: None,     // Will be initialized async if configured
            external_mcp_server_names: Vec::new(), // Will be populated by initialize_mcp
            unsupported_mcp_servers: Vec::new(), // Will be populated by initialize_mcp
            permission_manager,
            bus,
            file_time,
            sandbox_manager: None, // Will be initialized async in new_with_features
            todo_store,
            lsp_client,
            mcp_todo_adapter: None, // Will be initialized when MCP tools are loaded
            session_service: None,  // Will be set by new_with_session
            context_state: RwLock::new(ContextState::new(context_limit)),
            ticket_service: None, // Will be set by new_with_shared
            memory_service: None, // Will be set by new_with_shared
            hms_service: None, // Will be set by new_with_shared
            // Observational Memory state
            om_config,
            memory_state,
            token_state_machine,
            typescript_executor: None,
        })
    }

    /// Create a new runner with snapshot support.
    #[allow(dead_code)]
    pub async fn new_with_snapshots(
        config: RunnerConfig,
        instance: Instance,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_features(config, instance, None).await
    }

    /// Create a new runner with full feature support (snapshots, MCP, skills).
    #[allow(dead_code)]
    pub async fn new_with_features(
        config: RunnerConfig,
        instance: Instance,
        mcp_configs: Option<HashMap<String, McpConfig>>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_shared(config, instance, mcp_configs, None, None, None, None, None, None, None).await
    }

    /// Create a new runner with optional shared Bus, PermissionManager, SessionService, and AgentLoop.
    /// This allows sharing permission state with external components like MCP servers,
    /// persisting conversation history, and using custom agent loop implementations.
    ///
    /// When using a shared PermissionManager, the caller is responsible for initializing
    /// permission rules before calling this function. The runner will skip rule initialization.
    ///
    /// When providing a SessionService, the runner will:
    /// - Load existing conversation history on startup
    /// - Persist user messages when prompts are received
    /// - Persist assistant messages when responses complete
    ///
    /// When providing an AgentLoop, the runner will use that instead of the default StandardLoop.
    /// This is how Pro injects WasmAgentLoop for WASM-based agent behavior.
    #[allow(clippy::too_many_arguments)]
    pub async fn new_with_shared(
        mut config: RunnerConfig,
        instance: Instance,
        mcp_configs: Option<HashMap<String, McpConfig>>,
        shared_bus: Option<Bus>,
        shared_permission_manager: Option<Arc<PermissionManager>>,
        session_service: Option<Arc<SessionService>>,
        agent_loop: Option<BoxedAgentLoop>,
        ticket_service: Option<Arc<dyn wonopcode_tools::TicketService>>,
        memory_service: Option<wonopcode_tools::SharedMemoryService>,
        hms_service: Option<wonopcode_tools::SharedHmsService>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Track whether we're using a shared permission manager
        let using_shared_pm = shared_permission_manager.is_some();

        // Extract external (local/stdio) MCP servers BEFORE creating the provider
        // These need to be in the config so create_provider() can pass them to Claude CLI
        let mut external_server_names: Vec<String> = Vec::new();
        if let Some(ref configs) = mcp_configs {
            for (name, mcp_config) in configs {
                if let McpConfig::Local(local_config) = mcp_config {
                    if local_config.enabled != Some(false) {
                        let env = local_config.environment.clone().unwrap_or_default();
                        config
                            .external_mcp_servers
                            .insert(name.clone(), (local_config.command.clone(), env));
                        external_server_names.push(name.clone());
                    }
                }
            }
        }

        let mut runner = Self::new_with_agent_loop(
            config,
            instance,
            shared_bus,
            shared_permission_manager,
            agent_loop,
        )?;

        // Store external server names for status reporting
        runner.external_mcp_server_names = external_server_names;

        // Load core config for permission and sandbox initialization
        let core_config = runner.instance.config().await;

        // Only initialize permission rules if NOT using a shared permission manager.
        // When shared, the caller is responsible for initializing rules.
        if !using_shared_pm {
            // Initialize default permission rules
            for rule in PermissionManager::default_rules() {
                runner.permission_manager.add_rule(rule).await;
            }

            // Load permission rules from config (these take precedence over defaults)
            if let Some(perm_config) = &core_config.permission {
                for rule in PermissionManager::rules_from_config(perm_config) {
                    runner.permission_manager.add_rule(rule).await;
                }
                debug!("Permission manager initialized with default rules and config-based rules");
            } else {
                debug!("Permission manager initialized with default rules");
            }
        }

        // Initialize snapshot store with proper directory
        // Clone to avoid borrow issues when later mutably borrowing runner
        let cwd = runner.instance.directory().to_path_buf();
        let snapshot_dir = cwd.join(".wonopcode").join("snapshots");

        match SnapshotStore::new(snapshot_dir, cwd.to_path_buf(), SnapshotConfig::default()).await {
            Ok(store) => {
                debug!("Snapshot store initialized");
                runner.snapshot_store = Some(Arc::new(store));
            }
            Err(e) => {
                warn!("Failed to initialize snapshot store: {}", e);
            }
        }

        // Initialize sandbox manager if configured (using lazy detection for faster startup)
        debug!(sandbox_config = ?core_config.sandbox, "Checking sandbox configuration");
        if let Some(sandbox_cfg) = &core_config.sandbox {
            debug!(enabled = ?sandbox_cfg.enabled, runtime = ?sandbox_cfg.runtime, "Sandbox config found");
            if sandbox_cfg.enabled.unwrap_or(false) {
                let sandbox_config = convert_sandbox_config(sandbox_cfg);
                // Use lazy initialization to avoid blocking startup with runtime detection
                let manager = SandboxManager::new_lazy(sandbox_config, cwd.to_path_buf());

                // Check availability asynchronously in the background
                let runtime_type = manager.runtime_type();
                if let Some(rt) = runtime_type {
                    // Runtime was explicitly configured (not Auto)
                    if !matches!(rt, SandboxRuntimeType::None) {
                        debug!(runtime = ?rt, "Sandbox manager initialized");
                        runner
                            .bus
                            .publish(SandboxStatusChanged {
                                state: SandboxState::Stopped,
                                runtime_type: Some(format!("{rt:?}")),
                                error: None,
                            })
                            .await;
                        runner.sandbox_manager = Some(Arc::new(manager));
                    } else {
                        warn!("Sandbox enabled but runtime set to None");
                        runner
                            .bus
                            .publish(SandboxStatusChanged {
                                state: SandboxState::Disabled,
                                runtime_type: None,
                                error: Some("Sandbox runtime set to None".to_string()),
                            })
                            .await;
                    }
                } else {
                    // Auto mode - defer detection, assume available for now
                    // Actual detection will happen when sandbox is first used
                    debug!("Sandbox manager initialized with lazy runtime detection");
                    runner
                        .bus
                        .publish(SandboxStatusChanged {
                            state: SandboxState::Stopped,
                            runtime_type: Some("Auto".to_string()),
                            error: None,
                        })
                        .await;
                    runner.sandbox_manager = Some(Arc::new(manager));
                }
            } else {
                debug!("Sandbox not enabled in configuration (enabled = false or None)");
            }
        } else {
            debug!("No sandbox configuration found in config");
        }

        // Note: Sandbox permission rules are now checked dynamically at permission-check time,
        // based on whether the sandbox is actually running (not just configured).
        // See check_with_sandbox() in PermissionManager.
        if runner.sandbox_manager.is_some() {
            let allow_all = core_config
                .permission
                .as_ref()
                .and_then(|p| p.allow_all_in_sandbox)
                .unwrap_or(true);

            if allow_all {
                debug!("Sandbox configured with allow_all_in_sandbox=true (rules applied when sandbox is running)");
            } else {
                debug!(
                    "Sandbox configured but allow_all_in_sandbox=false, write operations will prompt"
                );
            }
        }

        // Now that we know the sandbox state, recreate the provider if using Claude CLI
        // This is needed because the provider was created before sandbox was initialized
        {
            let config = runner.config.read().await;
            if config.provider == "anthropic" && config.api_key.is_empty() {
                // Using Claude CLI - recreate provider with correct sandbox state
                // Check if sandbox is RUNNING, not just available
                // At startup, sandbox is in "stopped" state, so we pass false
                let sandbox_enabled = if let Some(ref manager) = runner.sandbox_manager {
                    manager.is_ready().await
                } else {
                    false
                };

                // Determine allow_all for MCP server:
                // - If config.allow_all is set (e.g., headless mode), use that
                // - Else if sandbox is enabled and allow_all_in_sandbox is true, allow all
                // - Otherwise, use project-scoped permissions (deny operations needing prompts)
                let allow_all_for_mcp = if config.allow_all {
                    true // Explicit allow_all override (headless mode)
                } else if sandbox_enabled {
                    core_config
                        .permission
                        .as_ref()
                        .and_then(|p| p.allow_all_in_sandbox)
                        .unwrap_or(true) // Default to true for sandbox
                } else {
                    false // Outside sandbox, use strict permissions
                };

                debug!(
                    sandbox_enabled = sandbox_enabled,
                    allow_all_for_mcp = allow_all_for_mcp,
                    "Recreating Claude CLI provider with sandbox state"
                );

                match create_provider(&config, Some(sandbox_enabled), allow_all_for_mcp) {
                    Ok(new_provider) => {
                        drop(config); // Release read lock before acquiring write lock
                        let mut provider = runner.provider.write().await;
                        *provider = new_provider;
                        debug!(
                            sandbox_enabled = sandbox_enabled,
                            allow_all_for_mcp = allow_all_for_mcp,
                            "Recreated Claude CLI provider with sandbox state"
                        );
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to recreate provider with sandbox state");
                    }
                }
            }
        }

        // Note: Skills are now accessible through the skills.* namespace in TypeScript
        // via the execute_typescript tool, not as a separate MCP tool.

        // Store memory service BEFORE MCP initialization
        // Also initialize workstream for memory service based on project directory
        // Note: Memory tools are registered in ToolRegistry::with_builtins() and access
        // the service through ctx.memory_service at execution time (same pattern as ticket tools)
        if let Some(ref mem_svc) = memory_service {
            let workstream_id = get_workstream_id(&cwd);
            if let Err(e) = mem_svc.set_workstream(cwd.to_path_buf(), workstream_id.clone()) {
                warn!(error = %e, workstream_id = %workstream_id, "Failed to initialize workstream memory");
            } else {
                info!(workstream_id = %workstream_id, "Workstream memory initialized");
            }
        }
        runner.memory_service = memory_service.clone();
        runner.hms_service = hms_service.clone();

        // Create TypeScript executor for in-process V8 execution
        // This is for the CLI version where WebKit is not present
        {
            use wonopcode_tools::{CodemodeServiceHandles, InProcessTypescriptExecutor, TicketServiceAdapter, HmsServiceAdapter, MemoryServiceAdapter, FileAceService};
            
            // Build service handles from the runner's services using adapters
            let mut codemode_services = CodemodeServiceHandles::new();
            
            // Wire up ticket service if available
            if let Some(ref ticket_svc) = runner.ticket_service {
                let adapter = TicketServiceAdapter::new(ticket_svc.clone());
                codemode_services.tickets = Some(std::sync::Arc::new(adapter));
                debug!("TypeScript executor: TicketService wired");
            }
            
            // Wire up HMS service if available
            if let Some(ref hms_svc) = runner.hms_service {
                let adapter = HmsServiceAdapter::for_project(cwd.clone());
                codemode_services.hms = Some(std::sync::Arc::new(adapter));
                debug!("TypeScript executor: HmsService wired");
            }
            
            // Wire up memory service if available
            if let Some(ref mem_svc) = memory_service {
                let adapter = MemoryServiceAdapter::new(mem_svc.clone());
                codemode_services.memory = Some(std::sync::Arc::new(adapter));
                debug!("TypeScript executor: MemoryService wired");
            }
            
            // Wire up ACE service - FileAceService directly implements codemode AceService
            {
                let ace_svc = FileAceService::new(cwd.clone());
                codemode_services.ace = Some(std::sync::Arc::new(ace_svc));
                debug!("TypeScript executor: AceService wired");
            }
            
            // Note: Web, LSP, and Agent services need additional adapters
            // For now, those will return "service not available" errors in TypeScript
            
            let executor = InProcessTypescriptExecutor::new(codemode_services);
            runner.typescript_executor = Some(std::sync::Arc::new(executor));
            debug!("TypeScript executor initialized (in-process V8)");
        }

        // Initialize MCP client if configured
        // NOTE: This may replace the entire tool registry, so memory tools are also added there
        if let Some(configs) = mcp_configs {
            if !configs.is_empty() {
                runner.initialize_mcp(configs).await;
            }
        }

        // Set up session service for history persistence
        if let Some(ref svc) = session_service {
            // Load existing history from session service
            match svc.load_history().await {
                Ok(history) => {
                    if !history.is_empty() {
                        debug!(
                            message_count = history.len(),
                            "Loaded conversation history from session"
                        );
                        // Estimate tokens in loaded history and update context state
                        let estimated_tokens = compaction::estimate_messages_tokens(&history);
                        {
                            let mut state = runner.context_state.write().await;
                            state.estimated_tokens = estimated_tokens;
                        }
                        debug!(
                            estimated_tokens = estimated_tokens,
                            "Estimated tokens from loaded history"
                        );

                        let mut runner_history = runner.history.write().await;
                        *runner_history = history;
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to load conversation history from session");
                }
            }
            runner.session_service = session_service;
        }

        // Store ticket service if provided
        runner.ticket_service = ticket_service;

        Ok(runner)
    }

    /// Initialize MCP client and connect to configured servers.
    async fn initialize_mcp(&mut self, configs: HashMap<String, McpConfig>) {
        let mcp_client = Arc::new(McpClient::new());

        // Collect enabled remote server configs for parallel connection
        // Note: Local (stdio) servers were already extracted in new_with_shared and
        // stored in config.external_mcp_servers for the Claude CLI provider
        let mut server_configs: Vec<(String, McpServerConfig)> = Vec::new();
        // Track disabled servers for display in sidebar
        let mut unsupported: Vec<(String, String)> = Vec::new();

        for (name, config) in configs {
            match &config {
                McpConfig::Local(local_config) => {
                    // Local (stdio) MCP servers are already handled via config.external_mcp_servers
                    // Just track disabled ones for the sidebar
                    if local_config.enabled == Some(false) {
                        debug!(server = %name, "MCP server disabled");
                        unsupported.push((name, "disabled".to_string()));
                    }
                    // Note: enabled local servers are already in self.external_mcp_server_names
                }
                McpConfig::Remote(remote_config) => {
                    // Check if enabled
                    if remote_config.enabled == Some(false) {
                        debug!(server = %name, "MCP server disabled, skipping");
                        unsupported.push((name, "disabled".to_string()));
                        continue;
                    }
                    // Convert to McpServerConfig for wonopcode's MCP client
                    let server_config = convert_mcp_remote_config(&name, remote_config);
                    server_configs.push((name, server_config));
                }
            }
        }

        // Store disabled servers for status reporting
        self.unsupported_mcp_servers = unsupported;

        // Connect to all servers in parallel
        let connection_futures: Vec<_> = server_configs
            .into_iter()
            .map(|(name, server_config)| {
                let client = mcp_client.clone();
                async move {
                    let result = client.add_server(server_config).await;
                    (name, result)
                }
            })
            .collect();

        let results = join_all(connection_futures).await;

        let mut connected_servers = 0;
        for (name, result) in results {
            match result {
                Ok(()) => {
                    debug!(server = %name, "MCP server connected");
                    connected_servers += 1;
                }
                Err(e) => {
                    warn!(server = %name, error = %e, "Failed to connect MCP server");
                }
            }
        }

        if connected_servers > 0 {
            // Register MCP tools with server-specific prefixes to avoid name conflicts
            // Use "mcp" prefix to distinguish from native tools
            let builder = McpToolsBuilder::new(mcp_client.clone()).with_prefix("mcp");
            let mcp_tools = builder.build_all().await;

            debug!(
                servers = connected_servers,
                tools = mcp_tools.len(),
                "MCP initialized"
            );

            // Check for MCP TODO tools and create adapter
            let mcp_todo_adapter = mcp_todo_adapter::detect_mcp_todo_tools(&mcp_tools);
            let has_mcp_todo_tools = mcp_todo_adapter.has_mcp_todo_tools();

            // For now, default to preferring MCP TODO tools when available
            // TODO: Add proper config integration later
            let prefer_mcp_todo = true;

            if has_mcp_todo_tools {
                debug!(
                    mcp_todo_tools = %mcp_todo_adapter.get_summary(),
                    prefer_mcp = prefer_mcp_todo,
                    "Detected MCP TODO tools"
                );
            }

            // We need a mutable tools registry - create a new one with MCP tools
            // All standard tools (bash, webfetch, lsp) are now accessible via wonop.* API
            // Agents (task, plan_mode) are now accessible via agents.* API in TypeScript
            // Skills are accessible via skills.* namespace in TypeScript
            let mut new_tools = ToolRegistry::with_builtins();

            // Register MCP tools
            for tool in mcp_tools {
                new_tools.register(tool);
            }

            self.tools = Arc::new(new_tools);
            self.mcp_client = Some(mcp_client);

            // Store the MCP TODO adapter for event bridging
            if has_mcp_todo_tools && prefer_mcp_todo {
                self.mcp_todo_adapter = Some(mcp_todo_adapter);
                debug!("MCP TODO adapter initialized for event bridging");
            }
        }
    }

    /// Get the current cancellation token.
    async fn get_cancel_token(&self) -> CancellationToken {
        self.cancel.read().await.clone()
    }

    /// Reset the cancellation token (create a new one).
    async fn reset_cancel_token(&self) {
        let mut guard = self.cancel.write().await;
        *guard = CancellationToken::new();
    }

    /// Get a snapshot of the current context state.
    pub async fn get_context_state(&self) -> ContextState {
        self.context_state.read().await.clone()
    }

    /// Update context state with estimated tokens from current history.
    async fn update_context_estimate(&self, messages: &[ProviderMessage]) {
        let mut state = self.context_state.write().await;
        state.update_estimated(messages);
    }

    /// Update context state with actual tokens from provider response.
    #[allow(dead_code)]
    async fn update_context_actual(&self, input_tokens: u32, output_tokens: u32) {
        let mut state = self.context_state.write().await;
        state.update_actual(input_tokens, output_tokens);
    }

    /// Check if compaction is needed based on current context state.
    #[allow(dead_code)]
    async fn needs_compaction(&self, messages: &[ProviderMessage]) -> bool {
        let mut state = self.context_state.write().await;
        state.update_estimated(messages);
        state.needs_compaction()
    }

    /// Update context limit (called when model changes).
    async fn set_context_limit(&self, limit: u32) {
        let mut state = self.context_state.write().await;
        state.set_context_limit(limit);
    }

    /// Get the session service, if configured.
    ///
    /// Used by Workstream to access conversation history for client queries.
    pub fn session_service(&self) -> Option<Arc<SessionService>> {
        self.session_service.clone()
    }

    /// Set a custom agent loop implementation.
    ///
    /// This allows replacing the default StandardLoop with a custom implementation
    /// (e.g., WasmAgentLoop for WASM-based loops).
    ///
    /// Must be called before `run()` to take effect.
    pub async fn set_agent_loop(&self, agent_loop: BoxedAgentLoop) {
        let mut guard = self.agent_loop.lock().await;
        *guard = agent_loop;
    }

    /// Get the name of the current agent loop implementation.
    pub async fn agent_loop_name(&self) -> String {
        let guard = self.agent_loop.lock().await;
        guard.name().to_string()
    }

    /// Run a prompt using the configured agent loop.
    ///
    /// This method delegates to the configured `AgentLoop` implementation. It handles:
    /// - Pre-prompt compaction if context is too large
    /// - Session persistence (user and assistant messages)
    /// - Building the `LoopContext` from Runner's state
    /// - Forwarding `LoopUpdate` events to `AppUpdate`
    /// - Post-processing (history update, TODO sync)
    async fn run_prompt_via_agent_loop(
        &self,
        user_input: &str,
        cwd: &std::path::Path,
        update_tx: &mpsc::UnboundedSender<AppUpdate>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.run_prompt_via_agent_loop_with_images(user_input, cwd, update_tx, Vec::new())
            .await
    }

    /// Run a prompt through the agent loop with optional image attachments.
    ///
    /// This is the full implementation that supports both text-only and multi-modal prompts.
    async fn run_prompt_via_agent_loop_with_images(
        &self,
        user_input: &str,
        cwd: &std::path::Path,
        update_tx: &mpsc::UnboundedSender<AppUpdate>,
        images: Vec<wonopcode_agent_loop::PromptImage>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        use std::time::Instant;

        // Get the cancellation token for this prompt
        let cancel = self.get_cancel_token().await;

        // Get mutable access to conversation history
        let mut messages = {
            let history = self.history.read().await;
            history.clone()
        };

        // === PRE-PROMPT COMPACTION ===
        // Skip legacy compaction when Observational Memory is enabled.
        // OM handles context management via observations/reflections.
        let om_enabled = self.om_config.enabled;
        
        // Check if compaction is needed based on TOKEN count (primary) or message count (fallback)
        let context_limit = {
            let provider = self.provider.read().await;
            provider.model_info().limit.context
        };

        // Update context state with current limit
        self.set_context_limit(context_limit).await;

        // Estimate tokens in current history
        let estimated_tokens = compaction::estimate_token_usage(&messages);
        let usable_context = context_limit.saturating_sub(compaction::OUTPUT_TOKEN_MAX);
        let usage_percent = if usable_context > 0 {
            ((estimated_tokens.total() as u64 * 100) / usable_context as u64).min(100) as u8
        } else {
            0
        };

        // Compaction thresholds (only used when OM is disabled)
        const TOKEN_COMPACTION_THRESHOLD_PERCENT: u8 = 80; // Compact at 80% context usage
        const MESSAGE_COMPACTION_THRESHOLD: usize = 100; // Fallback: also compact at 100+ messages

        let needs_token_compaction = usage_percent >= TOKEN_COMPACTION_THRESHOLD_PERCENT;
        let needs_message_compaction = messages.len() > MESSAGE_COMPACTION_THRESHOLD;
        // Only trigger legacy compaction when OM is disabled
        let needs_compaction = !om_enabled && (needs_token_compaction || needs_message_compaction);

        if needs_compaction {
            let reason = if needs_token_compaction {
                format!(
                    "Context usage at {}% ({} tokens / {} usable)",
                    usage_percent,
                    estimated_tokens.total(),
                    usable_context
                )
            } else {
                format!("Message count {} exceeds threshold", messages.len())
            };

            info!(
                estimated_tokens = estimated_tokens.total(),
                usage_percent = usage_percent,
                messages = messages.len(),
                reason = %reason,
                "Triggering automatic compaction"
            );
            let compact_start = Instant::now();
            let messages_before = messages.len();
            let tokens_before = estimated_tokens.total();

            // Send CompactionStarted event so UI can show running indicator
            send_update(
                update_tx,
                AppUpdate::CompactionStarted {
                    compaction_type: wonopcode_tui::CompactionType::Automatic,
                    messages_before,
                },
            );

            let provider = self.provider.read().await;
            let progress_callback = create_progress_callback(update_tx.clone());
            match compaction::compact(
                &mut messages,
                &provider,
                &self.compaction_config,
                &estimated_tokens,
                context_limit,
                false,
                Some(progress_callback),
            )
            .await
            {
                CompactionResult::Compacted {
                    messages: new_messages,
                    summary,
                    messages_summarized,
                } => {
                    let duration = compact_start.elapsed();
                    let tokens_after = compaction::estimate_messages_tokens(&new_messages);
                    info!(
                        messages_before = messages_before,
                        messages_after = new_messages.len(),
                        tokens_before = tokens_before,
                        tokens_after = tokens_after,
                        messages_summarized = messages_summarized,
                        duration_ms = duration.as_millis(),
                        "Auto-compaction successful"
                    );
                    messages = new_messages;

                    // Store compaction summary in memory for future retrieval
                    if !summary.is_empty() {
                        if let Some(ref mem_svc) = self.memory_service {
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let store_result = mem_svc
                                .store(wonop_memory::MemoryStoreParams {
                                    key: format!("compaction_summary_{}", timestamp),
                                    content: summary.clone(),
                                    scope: Some("workstream".to_string()),
                                    tags: Some(vec![
                                        "compaction".to_string(),
                                        "context".to_string(),
                                        "summary".to_string(),
                                    ]),
                                    index: Some(true),
                                    metadata: Some(
                                        serde_json::json!({
                                            "messages_summarized": messages_summarized,
                                            "tokens_before": tokens_before,
                                            "tokens_after": tokens_after,
                                        })
                                        .as_object()
                                        .cloned()
                                        .unwrap_or_default()
                                        .into_iter()
                                        .collect(),
                                    ),
                                })
                                .await;
                            match store_result {
                                Ok(entry) => {
                                    debug!(
                                        id = %entry.id,
                                        "Stored compaction summary in workstream memory"
                                    );
                                }
                                Err(e) => {
                                    warn!(error = %e, "Failed to store compaction summary in memory");
                                }
                            }
                        }
                    }

                    // Update history with compacted messages
                    {
                        let mut history = self.history.write().await;
                        *history = messages.clone();
                    }

                    // Update context state with new token estimate
                    self.update_context_estimate(&messages).await;

                    // CRITICAL: Reset stateful provider sessions after compaction
                    // Stateful providers (like Claude CLI with --resume) maintain their own
                    // conversation history. After compaction, we must reset the session so the
                    // provider starts fresh with the compacted message history.
                    {
                        let provider = self.provider.read().await;
                        if provider.is_stateful() {
                            provider.reset_session().await;
                        }
                    }

                    // Send compaction performed event to UI
                    send_update(
                        update_tx,
                        AppUpdate::CompactionPerformed {
                            compaction_type: wonopcode_tui::CompactionType::Automatic,
                            messages_before,
                            messages_after: messages.len(),
                            tokens_before,
                            tokens_after,
                            summary: if !summary.is_empty() {
                                Some(summary.clone())
                            } else {
                                None
                            },
                        },
                    );

                    // Persist compacted messages to session for history reload
                    // This replaces all existing messages with the new compacted history
                    if let Some(ref svc) = self.session_service {
                        match svc.replace_with_compacted_messages(
                            &messages,
                            "automatic",
                            messages_before,
                            tokens_before,
                            tokens_after,
                            if !summary.is_empty() { Some(summary.clone()) } else { None },
                        ).await {
                            Ok(_saved_count) => {}
                            Err(e) => {
                                warn!(error = %e, "Failed to persist compacted messages to session");
                            }
                        }
                    }

                    // Send context status update
                    let new_usage_percent = if usable_context > 0 {
                        ((tokens_after as u64 * 100) / usable_context as u64).min(100) as u8
                    } else {
                        0
                    };
                    send_update(
                        update_tx,
                        AppUpdate::ContextStatus {
                            estimated_tokens: tokens_after,
                            context_limit,
                            usage_percent: new_usage_percent,
                            needs_compaction: false,
                        },
                    );
                }
                CompactionResult::NotNeeded | CompactionResult::InsufficientMessages => {
                    debug!("Compaction not needed or insufficient messages");
                    // Send event so UI can clear any in-progress indicator
                    send_update(
                        update_tx,
                        AppUpdate::CompactionNotNeeded,
                    );
                }
                CompactionResult::Failed(err) => {
                    warn!(
                        "Auto-compaction failed: {}, continuing without compaction",
                        err
                    );
                    // Send CompactionNotNeeded to reset the UI state
                    // (this will unblock input and clear the in-progress indicator)
                    send_update(
                        update_tx,
                        AppUpdate::CompactionNotNeeded,
                    );
                    // Send warning to user
                    send_update(
                        update_tx,
                        AppUpdate::Status(format!("Warning: Compaction failed - {}", err)),
                    );
                }
            }
        } else {
            // Send context status update even when not compacting
            send_update(
                update_tx,
                AppUpdate::ContextStatus {
                    estimated_tokens: estimated_tokens.total(),
                    context_limit,
                    usage_percent,
                    needs_compaction: false,
                },
            );
        }

        // === SESSION PERSISTENCE: Save user message ===
        let user_msg = ProviderMessage::user(user_input);
        let user_msg_id = if let Some(ref svc) = self.session_service {
            match svc.save_user_message(&user_msg).await {
                Ok(msg_id) => {
                    debug!(message_id = %msg_id, "Persisted user message to session");
                    Some(msg_id)
                }
                Err(e) => {
                    warn!(error = %e, "Failed to persist user message to session");
                    None
                }
            }
        } else {
            None
        };

        // Track how many messages existed before the loop runs
        // This helps us identify which assistant messages are new (for session persistence)
        let messages_count_before_loop = messages.len();

        // Create a channel for LoopUpdate events
        let (loop_update_tx, mut loop_update_rx) = mpsc::unbounded_channel::<LoopUpdate>();

        // Create a channel for ToolEvents (like TodosUpdated from ACE tools)
        let (tool_event_tx, mut tool_event_rx) = mpsc::unbounded_channel::<ToolEvent>();

        // Spawn a task to forward ToolEvents to AppUpdates
        let update_tx_for_tool_events = update_tx.clone();
        let tool_event_forward_task = tokio::spawn(async move {
            while let Some(event) = tool_event_rx.recv().await {
                match event {
                    ToolEvent::TodosUpdated(phased_todos) => {
                        debug!("Forwarding TodosUpdated from tool event channel");
                        let (phases, todos) = convert_phased_todos_to_updates(&phased_todos);
                        let _ = update_tx_for_tool_events
                            .send(AppUpdate::TodosUpdated { phases, todos });
                    }
                    // Other events are logged but not forwarded yet
                    ToolEvent::ArtifactCreated { id, artifact_type } => {
                        debug!("Tool event: ArtifactCreated {} ({})", id, artifact_type);
                    }
                    ToolEvent::ArtifactUpdated { id } => {
                        debug!("Tool event: ArtifactUpdated {}", id);
                    }
                    ToolEvent::TaskStatusChanged {
                        id,
                        old_status,
                        new_status,
                    } => {
                        debug!(
                            "Tool event: TaskStatusChanged {} ({} -> {})",
                            id, old_status, new_status
                        );
                    }
                    ToolEvent::PhaseCompleted { phase } => {
                        debug!("Tool event: PhaseCompleted {}", phase);
                    }
                    ToolEvent::CheckpointRequested { checkpoint } => {
                        debug!("Tool event: CheckpointRequested {}", checkpoint);
                    }
                    ToolEvent::WorkflowComplete => {
                        debug!("Tool event: WorkflowComplete");
                    }
                }
            }
        });

        // Get MCP TODO tool mappings for intercepting TODO tool completions
        let mcp_todo_mappings = self.mcp_todo_adapter.as_ref().map(|a| a.tool_mappings());

        // Collector for messages that need to be persisted.
        // The OM observer drains messages from ctx.messages, so we need to collect
        // new messages BEFORE they're drained. The agent loop emits MessagesForPersistence
        // with these messages.
        let messages_for_persistence: Arc<std::sync::Mutex<Vec<ProviderMessage>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let messages_for_persistence_clone = Arc::clone(&messages_for_persistence);

        // Spawn a task to forward LoopUpdate events to AppUpdate
        let update_tx_clone = update_tx.clone();
        let forward_task = tokio::spawn(async move {
            while let Some(update) = loop_update_rx.recv().await {
                let app_update = match update {
                    LoopUpdate::TextDelta(text) => AppUpdate::TextDelta(text),
                    LoopUpdate::ThinkingDelta(_) => continue, // TUI doesn't have this variant yet
                    LoopUpdate::ToolStarted { id, name, input } => {
                        AppUpdate::ToolStarted { id, name, input }
                    }
                    LoopUpdate::ToolCompleted {
                        id,
                        name,
                        success,
                        output,
                        metadata,
                    } => {
                        // Check if this is a legacy TODO tool (from MCP) and emit TodosUpdated if so.
                        // Note: ACE tools (todowrite, ace_todo_update) emit events directly
                        // through the tool_event_tx channel, so they don't need interception here.
                        let is_legacy_todo_tool = if let Some(ref mappings) = mcp_todo_mappings {
                            // Check registered MCP TODO tools
                            mappings.contains_key(&name)
                        } else {
                            // Fallback: check by tool name pattern for external MCP tools
                            // This is essential for Claude CLI mode where tools are executed externally
                            mcp_todo_adapter::McpTodoAdapter::is_mcp_todo_write_tool_static(&name)
                        };

                        if is_legacy_todo_tool {
                            debug!(tool = %name, "Intercepting legacy TODO tool completion");
                            // Parse the output as TODO data
                            if let Ok(phased_todos) = parse_mcp_todo_output_simple(&output) {
                                let (phases, todos) =
                                    convert_phased_todos_to_updates(&phased_todos);
                                let _ =
                                    update_tx_clone.send(AppUpdate::TodosUpdated { phases, todos });
                            }
                        }
                        AppUpdate::ToolCompleted {
                            id,
                            success,
                            output,
                            metadata,
                        }
                    }
                    LoopUpdate::ResponseComplete { text } => AppUpdate::Completed { text },
                    LoopUpdate::TokenUsage {
                        input,
                        output,
                        cost,
                        context_limit,
                        accumulated_input,
                        accumulated_output,
                        accumulated_cost,
                        last_request_input,
                        last_request_output,
                        last_request_cache_read,
                    } => AppUpdate::TokenUsage {
                        input,
                        output,
                        cost,
                        context_limit,
                        accumulated_input,
                        accumulated_output,
                        accumulated_cost,
                        last_request_input,
                        last_request_output,
                        last_request_cache_read,
                    },
                    LoopUpdate::ContextUpdate {
                        estimated_tokens,
                        context_limit,
                        usage_percent,
                        needs_compaction,
                    } => AppUpdate::ContextStatus {
                        estimated_tokens,
                        context_limit,
                        usage_percent,
                        needs_compaction,
                    },
                    LoopUpdate::Status(status) => AppUpdate::Status(status),
                    LoopUpdate::Error(error) => AppUpdate::Error(error),
                    LoopUpdate::ObservationalMemoryUpdate(snapshot) => {
                        tracing::info!(
                            "🧠 [Runner] Converting LoopUpdate::ObservationalMemoryUpdate: {} observations, {} tokens, enabled={}",
                            snapshot.observations.len(),
                            snapshot.observation_tokens,
                            snapshot.enabled
                        );
                        AppUpdate::ObservationalMemoryUpdate(
                            wonopcode_tui::ObservationalMemoryStateUpdate {
                                enabled: snapshot.enabled,
                                observations: snapshot.observations.into_iter()
                                    .map(convert_observation_snapshot)
                                    .collect(),
                                observation_tokens: snapshot.observation_tokens,
                                reflector_threshold: snapshot.reflector_threshold,
                                message_tokens: snapshot.message_tokens,
                                observer_threshold: snapshot.observer_threshold,
                                system_tokens: snapshot.system_tokens,
                                total_observations: snapshot.total_observations,
                                reflections_count: snapshot.reflections_count,
                                avg_compression: snapshot.avg_compression,
                                cache_savings: snapshot.cache_savings,
                                loaded_from_previous_session: snapshot.loaded_from_previous_session,
                                loaded_session_date: snapshot.loaded_session_date.clone(),
                            }
                        )
                    }
                    LoopUpdate::MessagesForPersistence(msgs) => {
                        // Collect messages for session persistence.
                        // The OM observer drains ctx.messages, so these are emitted BEFORE
                        // the drain to preserve them for persistence.
                        // Deduplicate to prevent duplicate messages when OM runs multiple times.
                        tracing::info!(
                            "🧠 [Runner] Received MessagesForPersistence: {} messages",
                            msgs.len()
                        );
                        if let Ok(mut guard) = messages_for_persistence_clone.lock() {
                            for msg in msgs {
                                if !guard.iter().any(|existing| *existing == msg) {
                                    guard.push(msg);
                                }
                            }
                        }
                        continue; // Don't forward to AppUpdate
                    }
                    LoopUpdate::CompletionRecorded {
                        id,
                        timestamp,
                        model,
                        input_tokens,
                        output_tokens,
                        cache_read_tokens,
                        cost,
                        latency_ms,
                        total_duration_ms,
                        finish_reason,
                        request,
                        response,
                    } => AppUpdate::CompletionRecorded {
                        id,
                        timestamp,
                        model,
                        input_tokens,
                        output_tokens,
                        cache_read_tokens,
                        cost,
                        latency_ms,
                        total_duration_ms,
                        finish_reason,
                        request,
                        response,
                    },
                };
                let _ = update_tx_clone.send(app_update);
            }
        });

        // Build tool definitions
        let tool_defs: Vec<ToolDefinition> = self
            .tools
            .all()
            .map(|t| ToolDefinition {
                name: t.id().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters_schema(),
            })
            .collect();

        // Build loop config
        let loop_config = {
            let config = self.config.read().await;
            LoopConfig {
                system_prompt: config.system_prompt.clone().or_else(|| {
                    Some(build_system_prompt_for_session(
                        &config.provider,
                        &config.model_id,
                        cwd,
                    ))
                }),
                max_tokens: config.max_tokens,
                temperature: config.temperature,
                max_iterations: Some(50),
                include_tool_docs: false,
            }
        };

        // Build compaction config
        let compaction_config = LoopCompactionConfig::default();

        // Get sandbox runtime if available
        let sandbox = if let Some(ref manager) = self.sandbox_manager {
            if manager.is_ready().await {
                manager.runtime().await.ok()
            } else {
                None
            }
        } else {
            None
        };

        // Get provider (read lock)
        let provider = self.provider.read().await;

        // Create permission checker adapters
        let permission_checker: Arc<dyn PermissionChecker> = Arc::new(
            PermissionCheckerAdapter::new(self.permission_manager.clone()),
        );
        let ts_permission_checker: Arc<dyn wonopcode_tools::TsPermissionChecker> = Arc::new(
            TsPermissionCheckerAdapter::new(self.permission_manager.clone()),
        );

        // Create system prompt source for dynamic re-rendering before each LLM call
        let system_prompt_source: Option<Arc<dyn SystemPromptSource>> = {
            let config = self.config.read().await;
            match RunnerSystemPromptSource::new(
                cwd.to_path_buf(),
                config.provider.clone(),
                config.model_id.clone(),
                self.hms_service.clone(),
            ) {
                Ok(source) => Some(Arc::new(source)),
                Err(e) => {
                    warn!("Failed to create system prompt source: {}. Using static prompt.", e);
                    None
                }
            }
        };

        // Load workstream state for default tracker resolution
        let (workstream_ticket_id, workstream_default_tracker_id) =
            Self::load_workstream_context(cwd);

        // Acquire OM locks if enabled - these must live for the duration of run_prompt
        // We use .write().await since we're in an async context (not blocking_write which panics in async)
        let mut memory_state_guard = if self.om_config.enabled {
            match self.memory_state.as_ref() {
                Some(ms) => Some(ms.write().await),
                None => None,
            }
        } else {
            None
        };
        let mut token_state_machine_guard = if self.om_config.enabled {
            match self.token_state_machine.as_ref() {
                Some(tsm) => Some(tsm.write().await),
                None => None,
            }
        } else {
            None
        };

        // Build LoopContext with OM state references
        let mut ctx = LoopContext {
            cwd,
            messages: &mut messages,
            provider: &provider,
            tools: &self.tools,
            tool_defs,
            cancel: &cancel,
            snapshot_store: self.snapshot_store.as_ref(),
            file_time: self.file_time.clone(),
            sandbox,
            config: &loop_config,
            compaction_config: &compaction_config,
            update_tx: &loop_update_tx,
            session_id: "default".to_string(),
            tool_event_tx: Some(tool_event_tx),
            permission_checker: Some(permission_checker),
            ticket_service: self.ticket_service.clone(),
            memory_service: self.memory_service.clone(),
            hms_service: self.hms_service.clone(),
            system_prompt_source,
            ts_permission_checker: Some(ts_permission_checker),
            workstream_ticket_id,
            workstream_default_tracker_id,
            prompt_images: images,
            // Observational Memory state - enabled if config says so and guards are acquired
            memory_state: memory_state_guard.as_mut().map(|g| &mut **g),
            token_state_machine: token_state_machine_guard.as_mut().map(|g| &mut **g),
            om_enabled: self.om_config.enabled,
            // Track how many messages existed at start for session persistence
            messages_count_at_start: messages_count_before_loop,
            // Project directory for continuous OM persistence
            om_project_dir: self.om_config.project_dir.clone(),
            // TypeScript executor for in-process V8 execution
            typescript_executor: self.typescript_executor.clone(),
        };

        // Run the agent loop with emergency compaction on context overflow
        // If the provider returns "prompt too long" or similar, we compact and retry once.
        const MAX_OVERFLOW_RETRIES: usize = 1;
        let mut overflow_retries = 0;
        let result: Result<String, LoopError>;

        // Send Started event now that pre-prompt compaction is complete.
        // This ensures the compaction message appears BEFORE the streaming message in the UI.
        send_update(update_tx, AppUpdate::Started);

        loop {
            let loop_result = {
                let agent_loop = self.agent_loop.lock().await;
                agent_loop.run_prompt(&mut ctx, user_input).await
            };

            match &loop_result {
                Err(LoopError::ContextOverflow) if overflow_retries < MAX_OVERFLOW_RETRIES => {
                    overflow_retries += 1;
                    
                    // When OM is enabled, context overflow is unexpected (OM should manage context).
                    // Skip legacy compaction and report the error - OM needs investigation.
                    if om_enabled {
                        warn!(
                            retry = overflow_retries,
                            "Context overflow detected with Observational Memory enabled - this is unexpected. \
                             OM should have compressed context before overflow. Skipping legacy compaction."
                        );
                        result = loop_result;
                        break;
                    }
                    
                    warn!(
                        retry = overflow_retries,
                        "Context overflow detected, performing emergency compaction"
                    );

                    // Send CompactionStarted event so UI can show running indicator
                    let messages_before = ctx.messages.len();
                    send_update(
                        update_tx,
                        AppUpdate::CompactionStarted {
                            compaction_type: wonopcode_tui::CompactionType::Emergency,
                            messages_before,
                        },
                    );

                    // Perform emergency compaction
                    let provider = self.provider.read().await;
                    let estimated_tokens = compaction::estimate_token_usage(ctx.messages);
                    let progress_callback = create_progress_callback(update_tx.clone());

                    match compaction::compact(
                        ctx.messages,
                        &provider,
                        &self.compaction_config,
                        &estimated_tokens,
                        context_limit,
                        true, // force compaction
                        Some(progress_callback),
                    )
                    .await
                    {
                        CompactionResult::Compacted {
                            messages: new_messages,
                            summary,
                            messages_summarized,
                        } => {
                            let messages_before = ctx.messages.len();
                            let tokens_before = estimated_tokens.total();
                            let tokens_after = compaction::estimate_messages_tokens(&new_messages);
                            info!(
                                messages_before = messages_before,
                                messages_after = new_messages.len(),
                                tokens_after = tokens_after,
                                messages_summarized = messages_summarized,
                                "Emergency compaction successful"
                            );

                            // Update context with compacted messages
                            *ctx.messages = new_messages.clone();

                            // Update history with compacted messages
                            {
                                let mut history = self.history.write().await;
                                *history = ctx.messages.clone();
                            }

                            // Update context state
                            self.update_context_estimate(ctx.messages).await;

                            // CRITICAL: Reset stateful provider sessions after emergency compaction
                            if provider.is_stateful() {
                                provider.reset_session().await;
                            }

                            // Send emergency compaction event to UI
                            send_update(
                                update_tx,
                                AppUpdate::CompactionPerformed {
                                    compaction_type: wonopcode_tui::CompactionType::Emergency,
                                    messages_before,
                                    messages_after: new_messages.len(),
                                    tokens_before,
                                    tokens_after,
                                    summary: if !summary.is_empty() {
                                        Some(summary.clone())
                                    } else {
                                        None
                                    },
                                },
                            );

                            // Persist compacted messages to session for history reload
                            // This replaces all existing messages with the new compacted history
                            if let Some(ref svc) = self.session_service {
                                match svc.replace_with_compacted_messages(
                                    ctx.messages,
                                    "emergency",
                                    messages_before,
                                    tokens_before,
                                    tokens_after,
                                    if !summary.is_empty() { Some(summary.clone()) } else { None },
                                ).await {
                                    Ok(_saved_count) => {}
                                    Err(e) => {
                                        warn!(error = %e, "Failed to persist compacted messages to session (emergency)");
                                    }
                                }
                            }

                            // Retry the prompt (loop continues)
                            continue;
                        }
                        CompactionResult::Failed(err) => {
                            error!(error = %err, "Emergency compaction failed");
                            // Send CompactionNotNeeded to reset the UI state
                            // (this will unblock input and clear the in-progress indicator)
                            send_update(
                                update_tx,
                                AppUpdate::CompactionNotNeeded,
                            );
                            send_update(
                                update_tx,
                                AppUpdate::Status(format!("Emergency compaction failed: {}", err)),
                            );
                            result = loop_result;
                            break;
                        }
                        _ => {
                            // NotNeeded or InsufficientMessages - shouldn't happen after overflow
                            warn!("Emergency compaction returned unexpected result");
                            result = loop_result;
                            break;
                        }
                    }
                }
                _ => {
                    // Either success, non-overflow error, or we've exhausted retries
                    result = loop_result;
                    break;
                }
            }
        }

        // Drop the update channel to signal the forward task to stop
        drop(loop_update_tx);
        let _ = forward_task.await;

        // Abort the tool event forward task (sender was moved to ctx and dropped)
        tool_event_forward_task.abort();

        // === SESSION PERSISTENCE: Save assistant messages (with tool calls) ===
        // IMPORTANT: We save the actual messages from the agent loop, not synthetic text-only messages.
        // This ensures tool calls (ContentPart::ToolUse) are persisted and can be reconstructed
        // when the conversation is reloaded (e.g., switching workstreams).
        if result.is_ok() {
            if let Some(ref svc) = self.session_service {
                if let Some(ref parent_id) = user_msg_id {
                    // Get new messages for persistence.
                    // PRIORITY 1: Use messages collected from OM's MessagesForPersistence (emitted BEFORE drain)
                    // PRIORITY 2: Fallback to ctx.messages if OM didn't run (no drain happened)
                    let collected_messages = messages_for_persistence
                        .lock()
                        .map(|g| g.clone())
                        .unwrap_or_default();
                    
                    let new_messages: Vec<_> = if !collected_messages.is_empty() {
                        // Use messages collected from OM before the drain
                        info!(
                            "SESSION PERSISTENCE: Using {} messages from MessagesForPersistence collector",
                            collected_messages.len()
                        );
                        collected_messages.iter().collect()
                    } else {
                        // Fallback: OM didn't run or didn't emit messages, use ctx.messages
                        // (this happens when OM is disabled or threshold wasn't reached)
                        messages.iter().skip(messages_count_before_loop).collect()
                    };

                    trace!(
                        messages_before = messages_count_before_loop,
                        messages_after = messages.len(),
                        new_message_count = new_messages.len(),
                        collected_count = collected_messages.len(),
                        "SESSION PERSISTENCE: Processing new messages for storage"
                    );

                    let mut last_saved_id = parent_id.clone();
                    let mut assistant_count = 0;
                    let mut tool_count = 0;

                    // Track the last saved assistant message ID for updating tool results
                    let mut current_assistant_msg_id: Option<String> = None;
                    let mut tool_results_updated = 0;

                    for msg in new_messages {
                        match msg.role {
                            wonopcode_provider::Role::Assistant => {
                                assistant_count += 1;
                                // Count tool use parts in this message
                                let tools_in_msg = msg
                                    .content
                                    .iter()
                                    .filter(|c| {
                                        matches!(c, wonopcode_provider::ContentPart::ToolUse { .. })
                                    })
                                    .count();
                                tool_count += tools_in_msg;

                                trace!(
                                    content_parts = msg.content.len(),
                                    tool_use_parts = tools_in_msg,
                                    "SESSION PERSISTENCE: Saving assistant message"
                                );

                                // This message may contain ContentPart::ToolUse parts
                                // which will be converted to MessagePart::Tool by save_assistant_message
                                match svc.save_assistant_message(msg, &last_saved_id).await {
                                    Ok(msg_id) => {
                                        trace!(
                                            message_id = %msg_id,
                                            parts = msg.content.len(),
                                            "SESSION PERSISTENCE: Persisted assistant message with {} content parts",
                                            msg.content.len()
                                        );
                                        current_assistant_msg_id = Some(msg_id.clone());
                                        last_saved_id = msg_id;
                                    }
                                    Err(e) => {
                                        warn!(error = %e, "Failed to persist assistant message to session");
                                    }
                                }
                            }
                            wonopcode_provider::Role::Tool => {
                                // Tool result messages update the state of existing tool parts
                                // These come from both internal tool execution and observed (CLI) tools
                                if let Some(ref assistant_msg_id) = current_assistant_msg_id {
                                    for content in &msg.content {
                                        if let wonopcode_provider::ContentPart::ToolResult {
                                            tool_use_id,
                                            content: result_content,
                                            is_error,
                                        } = content
                                        {
                                            let output = result_content.clone();
                                            let success = !is_error.unwrap_or(false);

                                            debug!(
                                                assistant_msg_id = %assistant_msg_id,
                                                tool_use_id = %tool_use_id,
                                                success = %success,
                                                output_len = output.len(),
                                                "SESSION PERSISTENCE: Updating tool result"
                                            );

                                            if let Err(e) = svc
                                                .update_tool_result(
                                                    assistant_msg_id,
                                                    tool_use_id,
                                                    output,
                                                    success,
                                                    None,
                                                )
                                                .await
                                            {
                                                warn!(error = %e, tool_use_id = %tool_use_id, "Failed to update tool result");
                                            } else {
                                                tool_results_updated += 1;
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {
                                // User and System messages are handled separately
                            }
                        }
                    }

                    trace!(
                        assistant_messages_saved = assistant_count,
                        total_tool_parts = tool_count,
                        tool_results_updated = tool_results_updated,
                        "SESSION PERSISTENCE: Completed saving messages"
                    );

                    // Signal that turn messages have been persisted.
                    // This allows the workstream server to safely clear streaming state
                    // without losing the last message when clients reconnect.
                    let _ = update_tx.send(AppUpdate::TurnPersisted);
                    trace!("SESSION PERSISTENCE: Sent TurnPersisted event");
                }
            }
        }

        // Update history with the new messages
        {
            let mut history = self.history.write().await;
            *history = messages;
        }

        // Convert LoopError to Box<dyn Error>
        match result {
            Ok(text) => Ok(text),
            Err(e) => Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    }

    /// Change the model at runtime.
    async fn change_model(
        &self,
        model_spec: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Parse model spec (provider/model or just model)
        let (provider_name, model_id) = if let Some((p, m)) = model_spec.split_once('/') {
            (p.to_string(), m.to_string())
        } else {
            // Infer provider from model name
            let provider = infer_provider_from_model(model_spec).unwrap_or("anthropic");
            (provider.to_string(), model_spec.to_string())
        };

        // Load API key for the new provider (may be empty for CLI-based auth)
        let api_key = load_api_key(&provider_name).unwrap_or_default();

        // Check if we have authentication
        if api_key.is_empty() {
            // For anthropic-cli, use Claude CLI authentication (no API key needed)
            if provider_name == "anthropic-cli" {
                if !ClaudeCliProvider::is_available() {
                    return Err("Claude CLI not found. Install with: npm install -g @anthropic-ai/claude-code".into());
                }
                if !ClaudeCliProvider::is_authenticated() {
                    return Err("Claude CLI not authenticated. Run 'claude login' to authenticate.".into());
                }
                info!("Using Claude CLI subscription for model change");
            } else if provider_name == "anthropic"
                && ClaudeCliProvider::is_available()
                && ClaudeCliProvider::is_authenticated()
            {
                // For "anthropic" provider without API key, allow CLI-based subscription auth as fallback
                info!("Using Claude CLI subscription for model change (fallback from anthropic provider)");
            } else if provider_name == "openai-codex"
                && wonopcode_provider::codex::CodexProvider::has_credentials()
            {
                // Codex supports both API key and ChatGPT subscription via `codex login`
                info!("Using Codex authentication (API key or subscription) for model change");
            } else if provider_name == "test" {
                // Test provider doesn't need an API key
                info!("Using test provider (no API key required)");
            } else {
                return Err(format!("No API key found for provider '{provider_name}'. Set the environment variable or run 'wonopcode auth login {provider_name}'").into());
            }
        }

        // Create new config
        let new_config = {
            let old_config = self.config.read().await;
            RunnerConfig {
                provider: provider_name.clone(),
                model_id: model_id.clone(),
                api_key,
                system_prompt: old_config.system_prompt.clone(),
                max_tokens: old_config.max_tokens,
                temperature: old_config.temperature,
                doom_loop: old_config.doom_loop,
                test_provider_settings: old_config.test_provider_settings.clone(),
                allow_all: old_config.allow_all,
                allow_all_in_sandbox: old_config.allow_all_in_sandbox,
                mcp_url: old_config.mcp_url.clone(),
                mcp_secret: old_config.mcp_secret.clone(),
                external_mcp_servers: old_config.external_mcp_servers.clone(),
                working_directory: old_config.working_directory.clone(),
                observational_memory: old_config.observational_memory.clone(),
                max_iterations: old_config.max_iterations,
            }
        };

        // Create new provider with current sandbox state
        let sandbox_enabled = self.sandbox_manager.as_ref().map(|m| m.is_available());

        // Determine allow_all based on config override, sandbox state and permission config
        let allow_all_for_mcp = if new_config.allow_all {
            true // Explicit allow_all override (headless mode)
        } else if sandbox_enabled.unwrap_or(false) {
            let core_config = self.instance.config().await;
            core_config
                .permission
                .as_ref()
                .and_then(|p| p.allow_all_in_sandbox)
                .unwrap_or(true)
        } else {
            false
        };

        // Get the current CLI session ID and model info before recreating the provider
        // We only preserve the session if staying with the SAME model on the same provider,
        // because Claude CLI sessions are tied to specific models
        let (old_provider_id, old_model_id) = {
            let provider = self.provider.read().await;
            (
                provider.provider_id().to_string(),
                provider.model_info().id.clone(),
            )
        };
        let cli_session_id = if old_provider_id == "anthropic-cli"
            && provider_name == "anthropic-cli"
            && old_model_id == model_id
        {
            // Staying with same Claude CLI provider AND same model - preserve session
            let provider = self.provider.read().await;
            provider.get_cli_session_id().await
        } else {
            // Changing providers or models - start fresh session
            // Claude CLI sessions are model-specific, so we can't reuse them across models
            if old_model_id != model_id {
                info!(old_model = %old_model_id, new_model = %model_id, "Model changed, starting fresh session");
            }
            None
        };

        let new_provider = create_provider(&new_config, sandbox_enabled, allow_all_for_mcp)?;

        // Restore CLI session ID if we preserved it
        if cli_session_id.is_some() {
            new_provider
                .set_cli_session_id(cli_session_id.clone())
                .await;
            debug!(cli_session_id = ?cli_session_id, "Restored CLI session ID after model change");
        }

        // Update config and provider
        {
            let mut config = self.config.write().await;
            *config = new_config;
        }
        {
            let mut provider = self.provider.write().await;
            *provider = new_provider;
        }

        info!(provider = %provider_name, model = %model_id, cli_session_id = ?cli_session_id, "Model changed successfully");
        Ok(())
    }

    /// Run the action handler loop.
    ///
    /// This is the primary interface for running the action handler, accepting
    /// unbounded channels for compatibility with the TUI which uses unbounded
    /// channels internally.
    ///
    /// For server/headless mode where backpressure control is needed, the caller
    /// should create a bounded channel and spawn a forwarding task from the
    /// bounded channel to an unbounded one.
    ///
    /// # Arguments
    /// * `action_rx` - Unbounded receiver for incoming actions.
    /// * `update_tx` - Unbounded sender for outgoing updates.
    pub async fn run(
        self,
        mut action_rx: mpsc::UnboundedReceiver<AppAction>,
        update_tx: mpsc::UnboundedSender<AppUpdate>,
    ) {
        let cwd = self.instance.directory().to_path_buf();

        info!(
            working_directory = %cwd.display(),
            "Runner started with working directory"
        );

        // Subscribe to permission requests from the bus and forward to TUI
        let mut permission_rx = self.bus.subscribe::<BusPermissionRequest>().await;
        let permission_update_tx = update_tx.clone();
        tokio::spawn(async move {
            info!("Permission request forwarding task started");
            while let Ok(req) = permission_rx.recv().await {
                info!(
                    request_id = %req.id,
                    tool = %req.tool,
                    action = %req.action,
                    "Runner received permission request from bus, forwarding to TUI"
                );
                if let Err(e) = permission_update_tx.send(AppUpdate::PermissionRequest(
                    PermissionRequestUpdate {
                        id: req.id,
                        tool: req.tool,
                        action: req.action,
                        description: req.description,
                        path: req.path,
                    },
                )) {
                    warn!("Failed to forward permission request to TUI: {}", e);
                }
            }
            info!("Permission request forwarding task ended");
        });

        // Subscribe to permission responses from the bus and forward to TUI
        // This allows the TUI to dismiss permission dialogs when resolved by other means
        // (e.g., timeout, allow-all, or another connected client)
        let mut permission_response_rx = self.bus.subscribe::<BusPermissionResponse>().await;
        let permission_resolved_tx = update_tx.clone();
        tokio::spawn(async move {
            info!("Permission response forwarding task started");
            while let Ok(resp) = permission_response_rx.recv().await {
                debug!(
                    request_id = %resp.id,
                    allowed = resp.allowed,
                    "Runner received permission response from bus, forwarding to TUI"
                );
                if let Err(e) = permission_resolved_tx.send(AppUpdate::PermissionResolved {
                    request_id: resp.id,
                    allowed: resp.allowed,
                }) {
                    warn!("Failed to forward permission response to TUI: {}", e);
                }
            }
            info!("Permission response forwarding task ended");
        });

        // Send initial model info
        {
            let provider = self.provider.read().await;
            let model_info = provider.model_info();
            send_update(
                &update_tx,
                AppUpdate::ModelInfo {
                    context_limit: model_info.limit.context,
                },
            );
        }

        // Send initial token usage based on loaded history
        // This ensures the UI shows token counts even before the first prompt
        {
            let context_state = self.get_context_state().await;
            let history = self.history.read().await;

            // If we have history, send token usage estimate
            if !history.is_empty() {
                let estimated_tokens = context_state.estimated_tokens;
                let context_limit = context_state.context_limit;
                let usage_percent = context_state.usage_percent();

                // Send ContextStatus for accurate context usage display
                send_update(
                    &update_tx,
                    AppUpdate::ContextStatus {
                        estimated_tokens,
                        context_limit,
                        usage_percent,
                        needs_compaction: context_state.needs_compaction(),
                    },
                );

                debug!(
                    estimated_tokens = estimated_tokens,
                    context_limit = context_limit,
                    usage_percent = usage_percent,
                    messages = history.len(),
                    "Sent initial context status based on loaded history"
                );
            }
        }

        // Send initial Observational Memory state if observations were loaded
        // This ensures the frontend gets the persisted observations on startup
        if self.om_config.enabled {
            if let Some(ref memory_state) = self.memory_state {
                let state = memory_state.read().await;
                
                if !state.observations.is_empty() || state.loaded_from_previous_session {
                    // Get threshold info from token state machine if available
                    let (observation_tokens, message_tokens, reflector_threshold, observer_threshold) = 
                        if let Some(ref tsm) = self.token_state_machine {
                            let sm = tsm.read().await;
                            let thresholds = sm.thresholds();
                            (
                                sm.observation_tokens(),
                                sm.message_tokens(),
                                thresholds.reflector_threshold,
                                thresholds.observer_threshold,
                            )
                        } else {
                            (state.observation_tokens, state.unobserved_message_tokens, 20_000, 10_000)
                        };
                    
                    let stats = &state.stats;
                    let avg_compression = if stats.tokens_observed > 0 {
                        stats.tokens_observed as f32 / stats.tokens_after_compression.max(1) as f32
                    } else {
                        1.0
                    };
                    
                    let loaded_session_date = if state.loaded_from_previous_session && !state.observations.is_empty() {
                        state.observations.first()
                            .map(|o| o.observation_date.format("%B %d").to_string())
                    } else {
                        None
                    };
                    
                    // Convert observations to snapshots
                    let observations: Vec<wonopcode_agent_loop::ObservationSnapshot> = state.observations.iter()
                        .map(observation_to_snapshot)
                        .collect();
                    
                    info!(
                        observations = observations.len(),
                        observation_tokens = observation_tokens,
                        loaded_from_previous = state.loaded_from_previous_session,
                        "Sending initial OM state to frontend"
                    );
                    
                    // Create snapshot and convert to TUI format
                    let snapshot = wonopcode_agent_loop::ObservationalMemoryStateSnapshot {
                        enabled: true,
                        observations,
                        observation_tokens,
                        reflector_threshold,
                        message_tokens,
                        observer_threshold,
                        system_tokens: 0,
                        total_observations: stats.total_observations,
                        reflections_count: stats.reflections_run,
                        avg_compression,
                        cache_savings: 0.0,
                        loaded_from_previous_session: state.loaded_from_previous_session,
                        loaded_session_date,
                    };
                    
                    send_update(
                        &update_tx,
                        AppUpdate::ObservationalMemoryUpdate(
                            wonopcode_tui::ObservationalMemoryStateUpdate {
                                enabled: snapshot.enabled,
                                observations: snapshot.observations.into_iter()
                                    .map(convert_observation_snapshot)
                                    .collect(),
                                observation_tokens: snapshot.observation_tokens,
                                reflector_threshold: snapshot.reflector_threshold,
                                message_tokens: snapshot.message_tokens,
                                observer_threshold: snapshot.observer_threshold,
                                system_tokens: snapshot.system_tokens,
                                total_observations: snapshot.total_observations,
                                reflections_count: snapshot.reflections_count,
                                avg_compression: snapshot.avg_compression,
                                cache_savings: snapshot.cache_savings,
                                loaded_from_previous_session: snapshot.loaded_from_previous_session,
                                loaded_session_date: snapshot.loaded_session_date,
                            },
                        ),
                    );
                }
            }
        }

        // Send initial MCP status (including unsupported servers)
        {
            let mcp_updates = self.build_mcp_status().await;
            if !mcp_updates.is_empty() {
                send_update(&update_tx, AppUpdate::McpUpdated(mcp_updates));
            }
        }

        // LSP servers start on-demand when files are accessed via the LSP tool.
        // We don't send any initial status - the sidebar will show "No active servers"
        // until an LSP server is actually used and reports its status.

        // Send initial sandbox status
        {
            let (status, system_msg) = if let Some(ref manager) = self.sandbox_manager {
                let runtime_type = manager.runtime_type_display();
                let runtime_lower = runtime_type.to_lowercase();
                if manager.is_ready().await {
                    // Sandbox is already running, update permission manager state
                    self.permission_manager.set_sandbox_running(true);

                    // Get container ID from runtime info
                    let container_id = if let Ok(runtime) = manager.runtime().await {
                        let wrapper: Arc<dyn std::any::Any + Send + Sync> =
                            Arc::new(SandboxRuntimeWrapper(runtime.clone()));
                        self.permission_manager
                            .set_sandbox_runtime_any(Some(wrapper))
                            .await;
                        runtime.info().await.container_id
                    } else {
                        None
                    };

                    (
                        wonopcode_tui::SandboxStatusUpdate {
                            state: "running".to_string(),
                            runtime_type: Some(runtime_type),
                            error: None,
                            container_id,
                        },
                        Some(format!(
                            "⬡ Sandbox active ({runtime_lower}) - commands execute in isolated container"
                        )),
                    )
                } else {
                    (
                        wonopcode_tui::SandboxStatusUpdate {
                            state: "stopped".to_string(),
                            runtime_type: Some(runtime_type),
                            error: None,
                            container_id: None,
                        },
                        Some(format!(
                            "⬡ Sandbox available ({runtime_lower}) - use /sandbox start to enable isolation"
                        )),
                    )
                }
            } else {
                (
                    wonopcode_tui::SandboxStatusUpdate {
                        state: "disabled".to_string(),
                        runtime_type: None,
                        error: None,
                        container_id: None,
                    },
                    None, // Don't show message when sandbox is completely disabled
                )
            };
            send_update(&update_tx, AppUpdate::SandboxUpdated(status));
            if let Some(msg) = system_msg {
                send_update(&update_tx, AppUpdate::SystemMessage(msg));
            }
        }

        while let Some(action) = action_rx.recv().await {
            match action {
                AppAction::SendPrompt(text) => {
                    debug!(prompt_text = %text, "Received SendPrompt action");

                    // Handle slash commands
                    if let Some(response) = self.handle_slash_command(&text).await {
                        send_update(&update_tx, AppUpdate::SystemMessage(response));
                        continue;
                    }

                    // Reset cancellation token for new prompt
                    self.reset_cancel_token().await;

                    // NOTE: Started event is sent inside run_prompt_via_agent_loop
                    // AFTER pre-prompt compaction completes, so the compaction message
                    // appears before the streaming message in the UI.

                    // Run the prompt with concurrent cancellation handling
                    debug!(prompt_len = text.len(), "Running prompt");

                    // Get a clone of the cancel token for checking
                    let cancel_token = self.get_cancel_token().await;

                    // Use a loop to process Cancel actions while the prompt runs
                    // Add timeout for prompt operation to prevent indefinite "thinking" state
                    let prompt_timeout = tokio::time::Duration::from_secs(86400); // 24 hours - effectively no timeout
                    let prompt_future = tokio::time::timeout(
                        prompt_timeout,
                        self.run_prompt_via_agent_loop(&text, &cwd, &update_tx),
                    );
                    tokio::pin!(prompt_future);

                    let result = loop {
                        tokio::select! {
                            biased;

                            // Check for incoming actions (especially Cancel)
                            Some(inner_action) = action_rx.recv() => {
                                match inner_action {
                                    AppAction::Cancel => {
                                        debug!("Cancelling current operation");
                                        cancel_token.cancel();
                                        // Don't break - let the prompt handle the cancellation
                                    }
                                    AppAction::Quit => {
                                        debug!("Quit requested during prompt");
                                        cancel_token.cancel();
                                        // Return after prompt finishes
                                    }
                                    AppAction::PermissionResponse {
                                        request_id,
                                        allow,
                                        remember,
                                    } => {
                                        // Permission responses must be handled even during prompt execution
                                        // because MCP tools wait for them
                                        debug!(
                                            request_id = %request_id,
                                            allow = allow,
                                            remember = remember,
                                            "Received permission response during prompt execution"
                                        );
                                        self.permission_manager
                                            .respond(&request_id, allow, remember)
                                            .await;
                                    }
                                    AppAction::SetAllowAll { enabled } => {
                                        // Allow-all mode changes must be handled during prompt execution
                                        // so users can toggle it while the agent is working
                                        info!(
                                            enabled = enabled,
                                            "Setting allow-all mode during prompt execution"
                                        );
                                        self.permission_manager.set_allow_all(enabled);
                                        send_update(
                                            &update_tx,
                                            AppUpdate::AllowAllChanged { enabled },
                                        );
                                    }
                                    _ => {
                                        // Ignore other actions during prompt execution
                                        debug!("Ignoring action during prompt execution: {:?}", inner_action);
                                    }
                                }
                            }

                            // Wait for prompt to complete
                            res = &mut prompt_future => {
                                let inner_result = match res {
                                    Ok(inner_result) => inner_result,
                                    Err(_timeout_error) => {
                                        warn!("Prompt operation timed out after 5 minutes");
                                        // Send error to prevent TUI from being stuck in thinking
                                        Err("Operation timed out after 5 minutes".into())
                                    }
                                };
                                break inner_result;
                            }
                        }
                    };

                    match result {
                        Ok(result_text) => {
                            debug!(
                                result_len = result_text.len(),
                                "Prompt completed successfully"
                            );

                            // Send completion signal to TUI - this is critical to exit "thinking" state
                            debug!("Sending AppUpdate::Completed to TUI to exit thinking state");
                            send_update(&update_tx, AppUpdate::Completed { text: result_text });

                            // Sync todos to TUI
                            self.sync_todos_to_tui(&cwd, &update_tx);
                        }
                        Err(e) => {
                            let err_str = e.to_string();
                            if err_str.contains("Cancelled") {
                                debug!("Prompt was cancelled");
                                send_update(&update_tx, AppUpdate::Error("Cancelled".to_string()));
                            } else if err_str.contains("timed out") {
                                error!("Prompt operation timed out - this prevents UI from getting stuck");
                                send_update(&update_tx, AppUpdate::Error("Operation timed out. Please try a shorter prompt or check your connection.".to_string()));
                            } else {
                                error!("Prompt error: {}", e);
                                send_update(&update_tx, AppUpdate::Error(err_str));
                            }
                        }
                    }
                }
                AppAction::SendPromptWithImages { prompt, images } => {
                    debug!(
                        prompt_text = %prompt,
                        image_count = images.len(),
                        "Received SendPromptWithImages action"
                    );

                    // Handle slash commands (images are ignored for slash commands)
                    if let Some(response) = self.handle_slash_command(&prompt).await {
                        send_update(&update_tx, AppUpdate::SystemMessage(response));
                        continue;
                    }

                    // Convert protocol ImageData to agent-loop PromptImage
                    let prompt_images: Vec<wonopcode_agent_loop::PromptImage> = images
                        .into_iter()
                        .map(|img| wonopcode_agent_loop::PromptImage {
                            id: img.id,
                            data: img.data,
                            media_type: img.media_type,
                        })
                        .collect();

                    // Reset cancellation token for new prompt
                    self.reset_cancel_token().await;

                    // NOTE: Started event is sent inside run_prompt_via_agent_loop_with_images
                    // AFTER pre-prompt compaction completes, so the compaction message
                    // appears before the streaming message in the UI.

                    // Run the prompt with images
                    debug!(
                        prompt_len = prompt.len(),
                        image_count = prompt_images.len(),
                        "Running prompt with images"
                    );

                    // Get a clone of the cancel token for checking
                    let cancel_token = self.get_cancel_token().await;

                    let prompt_timeout = tokio::time::Duration::from_secs(86400);
                    let prompt_future = tokio::time::timeout(
                        prompt_timeout,
                        self.run_prompt_via_agent_loop_with_images(
                            &prompt,
                            &cwd,
                            &update_tx,
                            prompt_images,
                        ),
                    );
                    tokio::pin!(prompt_future);

                    let result = loop {
                        tokio::select! {
                            biased;

                            Some(inner_action) = action_rx.recv() => {
                                match inner_action {
                                    AppAction::Cancel => {
                                        debug!("Cancelling current operation");
                                        cancel_token.cancel();
                                    }
                                    AppAction::Quit => {
                                        debug!("Quit requested during prompt");
                                        cancel_token.cancel();
                                    }
                                    AppAction::PermissionResponse {
                                        request_id,
                                        allow,
                                        remember,
                                    } => {
                                        debug!(
                                            request_id = %request_id,
                                            allow = allow,
                                            remember = remember,
                                            "Received permission response during prompt execution"
                                        );
                                        self.permission_manager
                                            .respond(&request_id, allow, remember)
                                            .await;
                                    }
                                    AppAction::SetAllowAll { enabled } => {
                                        info!(
                                            enabled = enabled,
                                            "Setting allow-all mode during prompt execution"
                                        );
                                        self.permission_manager.set_allow_all(enabled);
                                        send_update(
                                            &update_tx,
                                            AppUpdate::AllowAllChanged { enabled },
                                        );
                                    }
                                    _ => {
                                        debug!("Ignoring action during prompt execution: {:?}", inner_action);
                                    }
                                }
                            }

                            res = &mut prompt_future => {
                                let inner_result = match res {
                                    Ok(inner_result) => inner_result,
                                    Err(_timeout_error) => {
                                        warn!("Prompt operation timed out");
                                        Err("Operation timed out".into())
                                    }
                                };
                                break inner_result;
                            }
                        }
                    };

                    match result {
                        Ok(result_text) => {
                            debug!(
                                result_len = result_text.len(),
                                "Prompt with images completed successfully"
                            );
                            send_update(&update_tx, AppUpdate::Completed { text: result_text });
                            self.sync_todos_to_tui(&cwd, &update_tx);
                        }
                        Err(e) => {
                            let err_str = e.to_string();
                            if err_str.contains("Cancelled") {
                                debug!("Prompt was cancelled");
                                send_update(&update_tx, AppUpdate::Error("Cancelled".to_string()));
                            } else {
                                error!("Prompt error: {}", e);
                                send_update(&update_tx, AppUpdate::Error(err_str));
                            }
                        }
                    }
                }
                AppAction::Cancel => {
                    // Cancel received outside of prompt execution - just log it
                    debug!("Cancel received but no operation in progress");
                }
                AppAction::Quit => {
                    info!("Runner shutting down");
                    break;
                }
                AppAction::SwitchSession(session_id) => {
                    debug!(session_id = %session_id, "Switching session");
                    // Clear history for session switch
                    {
                        let mut history = self.history.write().await;
                        history.clear();
                    }
                }
                AppAction::ChangeModel(model_spec) => {
                    info!(model = %model_spec, "Changing model");
                    match self.change_model(&model_spec).await {
                        Ok(()) => {
                            send_update(
                                &update_tx,
                                AppUpdate::Status(format!("Model changed to {model_spec}")),
                            );
                        }
                        Err(e) => {
                            error!("Failed to change model: {}", e);
                            send_update(
                                &update_tx,
                                AppUpdate::Error(format!("Failed to change model: {e}")),
                            );
                        }
                    }
                }
                AppAction::ChangeAgent(agent_name) => {
                    debug!(agent = %agent_name, "Changing agent");
                    // Agent change is mostly a TUI concern for now
                    // Future: could change tool permissions, system prompt, etc.
                    let _ =
                        update_tx.send(AppUpdate::Status(format!("Agent changed to {agent_name}")));
                }
                AppAction::NewSession => {
                    debug!("Creating new session");
                    // Clear history for new session
                    {
                        let mut history = self.history.write().await;
                        history.clear();
                    }
                    // Also clear CLI session if using Claude CLI provider
                    // The CLI maintains its own session history, so we need to
                    // reset it to start fresh.
                    {
                        let provider = self.provider.read().await;
                        if provider.is_stateful() {
                            provider.reset_session().await;
                        }
                    }
                    // Clear observational memory state for new session
                    if self.om_config.enabled {
                        if let Some(ref memory_state) = self.memory_state {
                            let mut state = memory_state.write().await;
                            state.clear();
                            info!("Cleared observational memory for new session");
                        }
                        // Also clear persisted observations from disk
                        if let Some(ref project_dir) = self.om_config.project_dir {
                            let persistence = ObservationPersistence::new(project_dir);
                            if let Err(e) = persistence.clear() {
                                warn!(error = %e, "Failed to clear persisted observations");
                            } else {
                                info!("Cleared persisted observations for new session");
                            }
                        }
                    }
                }
                AppAction::OpenEditor { .. } => {
                    // Editor is handled synchronously in the TUI, nothing to do here
                }
                AppAction::Undo => {
                    debug!("Undo requested");
                    // For now, history sync is handled by the TUI
                    // Future: could sync with runner's history
                }
                AppAction::Redo => {
                    debug!("Redo requested");
                    // For now, history sync is handled by the TUI
                    // Future: could sync with runner's history
                }
                AppAction::Revert { message_id } => {
                    debug!(message_id = %message_id, "Revert requested");
                    send_update(
                        &update_tx,
                        AppUpdate::Status(format!("Reverting to message {message_id}...")),
                    );

                    // Create SessionRevert and perform revert
                    let project_id = self.instance.project_id().await;
                    let session_repo = Arc::new(self.instance.session_repo());
                    let session_revert =
                        wonopcode_core::SessionRevert::new(session_repo, self.bus.clone());

                    // Get current session ID
                    // NOTE: Session ID tracking is managed by the Instance/SessionRepository.
                    // The Runner operates on the "default" session for single-session CLI mode.
                    // Multi-session support is available via the server API.
                    let session_id = "default".to_string();

                    let input = wonopcode_core::RevertInput {
                        session_id: session_id.clone(),
                        message_id: message_id.clone(),
                        part_id: None,
                    };

                    match session_revert.revert(&project_id, input).await {
                        Ok(_session) => {
                            // Also clear runner's in-memory history after the revert point
                            let mut history = self.history.write().await;
                            // Find and truncate history at the revert point
                            // For now, clear all - proper implementation would track message IDs
                            history.clear();
                            let _ =
                                update_tx.send(AppUpdate::Status("Revert complete".to_string()));
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to revert session");
                            let _ =
                                update_tx.send(AppUpdate::Status(format!("Revert failed: {e}")));
                        }
                    }
                }
                AppAction::Unrevert => {
                    debug!("Unrevert requested");

                    // Create SessionRevert and perform unrevert
                    let project_id = self.instance.project_id().await;
                    let session_repo = Arc::new(self.instance.session_repo());
                    let session_revert =
                        wonopcode_core::SessionRevert::new(session_repo, self.bus.clone());

                    // Get current session ID
                    let session_id = "default".to_string();

                    match session_revert.unrevert(&project_id, &session_id).await {
                        Ok(_session) => {
                            let _ =
                                update_tx.send(AppUpdate::Status("Unrevert complete".to_string()));
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to unrevert session");
                            let _ =
                                update_tx.send(AppUpdate::Status(format!("Unrevert failed: {e}")));
                        }
                    }
                }
                AppAction::Compact => {
                    let _ =
                        update_tx.send(AppUpdate::Status("Compacting conversation...".to_string()));

                    // Get current messages
                    let mut messages: Vec<ProviderMessage> = {
                        let history = self.history.read().await;
                        history.clone()
                    };

                    if messages.len() < 4 {
                        send_update(
                            &update_tx,
                            AppUpdate::CompactionNotNeeded,
                        );
                        continue;
                    }

                    // Send CompactionStarted event so UI can show running indicator
                    let messages_before = messages.len();
                    send_update(
                        &update_tx,
                        AppUpdate::CompactionStarted {
                            compaction_type: wonopcode_tui::CompactionType::Manual,
                            messages_before,
                        },
                    );

                    // Get context limit and estimate token usage
                    let (context_limit, estimated_tokens) = {
                        let provider = self.provider.read().await;
                        let limit = provider.model_info().limit.context;
                        let tokens = compaction::estimate_token_usage(&messages);
                        (limit, tokens)
                    };

                    // Perform full compaction: prune first, then summarize if still needed
                    let provider = self.provider.read().await;
                    let progress_callback = create_progress_callback(update_tx.clone());
                    match compaction::compact(
                        &mut messages,
                        &provider,
                        &self.compaction_config,
                        &estimated_tokens,
                        context_limit,
                        false, // Don't add auto-continue for manual compaction
                        Some(progress_callback),
                    )
                    .await
                    {
                        CompactionResult::Compacted {
                            messages: new_messages,
                            summary,
                            messages_summarized,
                        } => {
                            let action = if messages_summarized > 0 {
                                "summarized"
                            } else {
                                "pruned tool outputs from"
                            };
                            let messages_before = messages.len();
                            let tokens_before = estimated_tokens.total();
                            let tokens_after = compaction::estimate_messages_tokens(&new_messages);
                            debug!(
                                action = action,
                                messages_summarized = messages_summarized,
                                new_count = new_messages.len(),
                                "Manual compaction successful"
                            );

                            // Update history
                            {
                                let mut history = self.history.write().await;
                                *history = new_messages.clone();
                            }

                            // Send manual compaction event to UI
                            send_update(
                                &update_tx,
                                AppUpdate::CompactionPerformed {
                                    compaction_type: wonopcode_tui::CompactionType::Manual,
                                    messages_before,
                                    messages_after: new_messages.len(),
                                    tokens_before,
                                    tokens_after,
                                    summary: if !summary.is_empty() {
                                        Some(summary.clone())
                                    } else {
                                        None
                                    },
                                },
                            );

                            // Persist compacted messages to session for history reload
                            // This replaces all existing messages with the new compacted history
                            if let Some(ref svc) = self.session_service {
                                match svc.replace_with_compacted_messages(
                                    &new_messages,
                                    "manual",
                                    messages_before,
                                    tokens_before,
                                    tokens_after,
                                    if !summary.is_empty() { Some(summary.clone()) } else { None },
                                ).await {
                                    Ok(_saved_count) => {}
                                    Err(e) => {
                                        warn!(error = %e, "Failed to persist compacted messages to session (manual)");
                                    }
                                }
                            }
                        }
                        CompactionResult::NotNeeded | CompactionResult::InsufficientMessages => {
                            // Send dedicated event so UI can update the in-progress indicator
                            send_update(
                                &update_tx,
                                AppUpdate::CompactionNotNeeded,
                            );
                        }
                        CompactionResult::Failed(err) => {
                            warn!(error = %err, "Compaction failed");
                            // Send CompactionNotNeeded to reset the UI state
                            // (this will unblock input and clear the in-progress indicator)
                            send_update(
                                &update_tx,
                                AppUpdate::CompactionNotNeeded,
                            );
                            send_update(
                                &update_tx,
                                AppUpdate::Error(format!("Compaction failed: {err}")),
                            );
                        }
                    }
                }
                AppAction::EmulateHistory { message_pairs } => {
                    info!(
                        message_pairs = message_pairs,
                        "EMULATE_HISTORY: Received EmulateHistory action - starting injection"
                    );
                    warn!(
                        "EMULATE_HISTORY_DEBUG: Starting test message injection with {} pairs",
                        message_pairs
                    );
                    
                    // Generate test messages and inject them into history
                    let lorem_user = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam.";
                    let lorem_agent = "Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident.";
                    
                    let mut new_messages = Vec::new();
                    for i in 0..message_pairs {
                        // Add user message
                        let user_content = format!(
                            "Test message #{} from user. {} Here's some code:\n```rust\nfn test_{}() {{\n    println!(\"Hello from message {}\");\n}}\n```",
                            i + 1, lorem_user, i, i + 1
                        );
                        new_messages.push(ProviderMessage::user(&user_content));
                        
                        // Add assistant message
                        let assistant_content = format!(
                            "Test response #{} from agent. This is a simulated response to test context compaction. {} Here's some code:\n```rust\nfn example_{}() {{\n    println!(\"Hello from message {}\");\n}}\n```",
                            i + 1, lorem_agent, i, i + 1
                        );
                        new_messages.push(ProviderMessage::assistant(&assistant_content));
                    }
                    
                    // Inject into history
                    let messages_added = new_messages.len();
                    let final_count = {
                        let mut history = self.history.write().await;
                        history.extend(new_messages);
                        history.len()
                    };
                    
                    info!(
                        messages_added = messages_added,
                        total_messages = final_count,
                        "EMULATE_HISTORY: Injected test messages into runner history"
                    );
                    
                    let status_msg = format!(
                        "Emulated {} message pairs. Runner now has {} messages. Run /compact to test compaction.",
                        message_pairs, final_count
                    );
                    warn!(
                        "EMULATE_HISTORY_DEBUG: Sending status update: {}",
                        status_msg
                    );
                    send_update(
                        &update_tx,
                        AppUpdate::Status(status_msg),
                    );
                    warn!("EMULATE_HISTORY_DEBUG: Status update sent successfully");
                }
                AppAction::RenameSession { title } => {
                    debug!(title = %title, "Rename session requested");
                    // Session rename is persisted via the Instance/SessionRepository
                    // The title is already updated in the TUI state
                    let project_id = self.instance.project_id().await;
                    let session_repo = self.instance.session_repo();
                    if let Err(e) = session_repo
                        .update(&project_id, "default", |session| {
                            session.title = title.clone();
                        })
                        .await
                    {
                        warn!(error = %e, "Failed to persist session rename");
                    }
                    let _ =
                        update_tx.send(AppUpdate::Status(format!("Session renamed to: {title}")));
                }
                AppAction::McpToggle { name } => {
                    debug!(server = %name, "MCP toggle requested");
                    if let Some(ref mcp_client) = self.mcp_client {
                        match mcp_client.toggle_server(&name).await {
                            Ok(enabled) => {
                                let status = if enabled { "enabled" } else { "disabled" };
                                send_update(
                                    &update_tx,
                                    AppUpdate::Status(format!("MCP server '{name}' {status}")),
                                );

                                // Send updated MCP status (including unsupported servers)
                                let mcp_updates = self.build_mcp_status().await;
                                send_update(&update_tx, AppUpdate::McpUpdated(mcp_updates));
                            }
                            Err(e) => {
                                warn!(server = %name, error = %e, "Failed to toggle MCP server");
                                send_update(
                                    &update_tx,
                                    AppUpdate::Error(format!("Failed to toggle '{name}': {e}")),
                                );
                            }
                        }
                    } else {
                        send_update(
                            &update_tx,
                            AppUpdate::Error("No MCP client configured".to_string()),
                        );
                    }
                }
                AppAction::McpReconnect { name } => {
                    debug!(server = %name, "MCP reconnect requested");
                    if let Some(ref mcp_client) = self.mcp_client {
                        send_update(
                            &update_tx,
                            AppUpdate::Status(format!("Reconnecting to '{name}'...")),
                        );

                        match mcp_client.reconnect_server(&name).await {
                            Ok(()) => {
                                send_update(
                                    &update_tx,
                                    AppUpdate::Status(format!("Reconnected to '{name}'")),
                                );

                                // Send updated MCP status (including unsupported servers)
                                let mcp_updates = self.build_mcp_status().await;
                                send_update(&update_tx, AppUpdate::McpUpdated(mcp_updates));
                            }
                            Err(e) => {
                                warn!(server = %name, error = %e, "Failed to reconnect MCP server");
                                send_update(
                                    &update_tx,
                                    AppUpdate::Error(format!("Failed to reconnect '{name}': {e}")),
                                );
                            }
                        }
                    } else {
                        send_update(
                            &update_tx,
                            AppUpdate::Error("No MCP client configured".to_string()),
                        );
                    }
                }
                AppAction::ForkSession { message_id } => {
                    debug!(message_id = ?message_id, "Fork session requested");
                    let project_id = self.instance.project_id().await;
                    let session_repo = self.instance.session_repo();

                    // Fork the current session
                    match session_repo
                        .fork(&project_id, "default", message_id.as_deref())
                        .await
                    {
                        Ok(forked) => {
                            debug!(forked_id = %forked.id, "Session forked successfully");
                            // Clear runner's history for the new session
                            {
                                let mut history = self.history.write().await;
                                history.clear();
                            }
                            send_update(
                                &update_tx,
                                AppUpdate::Status(format!(
                                    "Forked to new session: {}",
                                    forked.title
                                )),
                            );
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to fork session");
                            send_update(&update_tx, AppUpdate::Error(format!("Fork failed: {e}")));
                        }
                    }
                }
                AppAction::ShareSession => {
                    debug!("Share session requested");
                    let project_id = self.instance.project_id().await;

                    // Use the share module to create a share
                    let session_repo = self.instance.session_repo();
                    match wonopcode_core::share::share_session(
                        &session_repo,
                        &project_id,
                        "default",
                        None,
                    )
                    .await
                    {
                        Ok(share_info) => {
                            debug!(url = %share_info.url, "Session shared successfully");
                            send_update(
                                &update_tx,
                                AppUpdate::Status(format!("Shared at: {}", share_info.url)),
                            );
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to share session");
                            send_update(&update_tx, AppUpdate::Error(format!("Share failed: {e}")));
                        }
                    }
                }
                AppAction::UnshareSession => {
                    debug!("Unshare session requested");
                    // Unsharing requires the share secret which we don't store in the TUI
                    // This would need to be retrieved from session metadata
                    send_update(
                        &update_tx,
                        AppUpdate::Status(
                            "Unshare requires share secret - use CLI: wonopcode unshare"
                                .to_string(),
                        ),
                    );
                }
                AppAction::GotoMessage { message_id } => {
                    debug!(message_id = %message_id, "Go to message requested");
                    // This is handled in the TUI (scroll to message)
                    send_update(
                        &update_tx,
                        AppUpdate::Status(format!("Navigated to message {message_id}")),
                    );
                }
                AppAction::SandboxStart => {
                    debug!("Sandbox start requested");
                    self.handle_sandbox_start(&update_tx).await;
                }
                AppAction::SandboxStop => {
                    debug!("Sandbox stop requested");
                    self.handle_sandbox_stop(&update_tx).await;
                }
                AppAction::SandboxRestart => {
                    debug!("Sandbox restart requested");
                    self.handle_sandbox_stop(&update_tx).await;
                    self.handle_sandbox_start(&update_tx).await;
                }
                AppAction::SetAllowAll { enabled } => {
                    info!(enabled = enabled, "Setting allow-all mode");
                    self.permission_manager.set_allow_all(enabled);
                    send_update(&update_tx, AppUpdate::AllowAllChanged { enabled });
                }
                AppAction::SaveSettings { scope, config } => {
                    debug!("Saving settings to {:?}", scope);
                    let project_dir = match scope {
                        SaveScope::Project => Some(self.instance.directory()),
                        SaveScope::Global => None,
                    };
                    match config.save_partial(project_dir).await {
                        Ok(()) => {
                            let location = match scope {
                                SaveScope::Project => "project config",
                                SaveScope::Global => "global config",
                            };
                            send_update(
                                &update_tx,
                                AppUpdate::SystemMessage(format!("Settings saved to {location}")),
                            );

                            // Reload permission rules if permission config changed
                            if let Some(perm_config) = &config.permission {
                                self.permission_manager
                                    .reload_from_config(perm_config)
                                    .await;
                                info!("Permission rules reloaded after settings change");
                            }
                        }
                        Err(e) => {
                            error!("Failed to save settings: {}", e);
                            send_update(
                                &update_tx,
                                AppUpdate::Error(format!("Failed to save settings: {e}")),
                            );
                        }
                    }
                }
                AppAction::UpdateTestProviderSettings {
                    emulate_thinking,
                    emulate_tool_calls,
                    emulate_tool_observed,
                    emulate_streaming,
                } => {
                    info!("Updating test provider settings");
                    let mut config = self.config.write().await;
                    config.test_provider_settings =
                        Some(wonopcode_provider::test::TestProviderSettings {
                            emulate_thinking,
                            emulate_tool_calls,
                            emulate_tool_observed,
                            emulate_streaming,
                        });
                }
                AppAction::PermissionResponse {
                    request_id,
                    allow,
                    remember,
                } => {
                    info!(
                        request_id = %request_id,
                        allow = allow,
                        remember = remember,
                        "Received permission response from TUI"
                    );
                    self.permission_manager
                        .respond(&request_id, allow, remember)
                        .await;
                }
                AppAction::EnableAllowAll { request_id } => {
                    info!(
                        request_id = %request_id,
                        "Enabling 'allow all' mode - all tool executions will be auto-approved"
                    );
                    // Apply sandbox rules to allow all operations
                    self.permission_manager.apply_sandbox_rules().await;
                    // Also respond to the current pending request
                    self.permission_manager
                        .respond(&request_id, true, false)
                        .await;
                }
                AppAction::GitStatus => {
                    self.handle_git_status(&update_tx).await;
                }
                AppAction::GitStage { paths } => {
                    self.handle_git_stage(&update_tx, paths).await;
                }
                AppAction::GitUnstage { paths } => {
                    self.handle_git_unstage(&update_tx, paths).await;
                }
                AppAction::GitCheckout { paths } => {
                    self.handle_git_checkout(&update_tx, paths).await;
                }
                AppAction::GitCommit { message } => {
                    self.handle_git_commit(&update_tx, message).await;
                }
                AppAction::GitHistory => {
                    self.handle_git_history(&update_tx).await;
                }
                AppAction::GitPush => {
                    self.handle_git_push(&update_tx).await;
                }
                AppAction::GitPull => {
                    self.handle_git_pull(&update_tx).await;
                }
            }
        }

        // Save Observational Memory state to disk before shutdown
        if self.om_config.enabled {
            if let Some(ref project_dir) = self.om_config.project_dir {
                if let Some(ref memory_state) = self.memory_state {
                    let state = memory_state.read().await;
                    if !state.observations.is_empty() {
                        let persistence = ObservationPersistence::new(project_dir);
                        match persistence.save(&state) {
                            Ok(()) => {
                                info!(
                                    observations = state.observations.len(),
                                    "Saved observations for cross-session memory"
                                );
                            }
                            Err(e) => {
                                warn!(error = %e, "Failed to save observations to disk");
                            }
                        }
                    }
                }
            }
        }

        // Cleanup: stop sandbox container on exit
        self.cleanup_sandbox().await;

        // Cleanup: remove temp todo file
        self.todo_store.cleanup();
    }

    /// Sync todos from store to TUI.
    /// This is called after each prompt completes to pick up any changes.
    fn sync_todos_to_tui(&self, cwd: &Path, update_tx: &mpsc::UnboundedSender<AppUpdate>) {
        let phased_todos = todo::get_phased_todos(self.todo_store.as_ref(), cwd);
        if !phased_todos.is_empty() {
            let (phases, todos) = convert_phased_todos_to_updates(&phased_todos);
            send_update(&update_tx, AppUpdate::TodosUpdated { phases, todos });
        }
    }

    /// Cleanup sandbox container on exit.
    /// Always tries to stop the container, even if we don't think it's running,
    /// because the MCP server subprocess might have started it.
    async fn cleanup_sandbox(&self) {
        if let Some(ref manager) = self.sandbox_manager {
            info!("Cleaning up sandbox container on exit");
            // Always try to stop - the MCP server might have started a container
            // that we don't know about (since it has its own SandboxManager)
            if let Err(e) = manager.stop().await {
                // This is expected if no container was running
                debug!(error = %e, "Failed to stop sandbox on exit (may not have been running)");
            } else {
                info!("Sandbox container stopped");
            }
        }
    }

    /// Handle sandbox start action.
    async fn handle_sandbox_start(&self, update_tx: &mpsc::UnboundedSender<AppUpdate>) {
        info!(
            sandbox_manager_present = self.sandbox_manager.is_some(),
            "Handling sandbox start"
        );
        if let Some(ref manager) = self.sandbox_manager {
            // Send starting status
            send_update(
                &update_tx,
                AppUpdate::SandboxUpdated(wonopcode_tui::SandboxStatusUpdate {
                    state: "starting".to_string(),
                    runtime_type: Some(manager.runtime_type_display()),
                    error: None,
                    container_id: None,
                }),
            );

            match manager.start().await {
                Ok(()) => {
                    info!("Sandbox started successfully");

                    // Update shared sandbox state in permission manager
                    // This allows MCP tools to know sandbox is running
                    self.permission_manager.set_sandbox_running(true);

                    // Share sandbox runtime with permission manager for MCP tools
                    if let Ok(runtime) = manager.runtime().await {
                        let wrapper: Arc<dyn std::any::Any + Send + Sync> =
                            Arc::new(SandboxRuntimeWrapper(runtime));
                        self.permission_manager
                            .set_sandbox_runtime_any(Some(wrapper))
                            .await;
                    }

                    // Get container ID from runtime info
                    let container_id = if let Ok(runtime) = manager.runtime().await {
                        runtime.info().await.container_id
                    } else {
                        None
                    };

                    self.bus
                        .publish(SandboxStatusChanged {
                            state: SandboxState::Running,
                            runtime_type: Some(manager.runtime_type_display()),
                            error: None,
                        })
                        .await;
                    send_update(
                        &update_tx,
                        AppUpdate::SandboxUpdated(wonopcode_tui::SandboxStatusUpdate {
                            state: "running".to_string(),
                            runtime_type: Some(manager.runtime_type_display()),
                            error: None,
                            container_id,
                        }),
                    );

                    // Send system message about sandbox starting
                    let runtime = manager.runtime_type_display().to_lowercase();
                    send_update(&update_tx, AppUpdate::SystemMessage(format!(
                        "⬡ Sandbox started ({runtime}) - commands will execute in isolated container"
                    )));

                    // Recreate provider with sandbox enabled so MCP server uses sandbox
                    self.recreate_provider_with_sandbox(true).await;
                }
                Err(e) => {
                    warn!(error = %e, "Failed to start sandbox");
                    self.bus
                        .publish(SandboxStatusChanged {
                            state: SandboxState::Error,
                            runtime_type: Some(manager.runtime_type_display()),
                            error: Some(e.to_string()),
                        })
                        .await;
                    send_update(
                        &update_tx,
                        AppUpdate::SandboxUpdated(wonopcode_tui::SandboxStatusUpdate {
                            state: "error".to_string(),
                            runtime_type: Some(manager.runtime_type_display()),
                            error: Some(e.to_string()),
                            container_id: None,
                        }),
                    );
                }
            }
        } else {
            send_update(
                &update_tx,
                AppUpdate::SandboxUpdated(wonopcode_tui::SandboxStatusUpdate {
                    state: "disabled".to_string(),
                    runtime_type: None,
                    error: Some("Sandbox not configured".to_string()),
                    container_id: None,
                }),
            );
        }
    }

    /// Recreate the provider with updated sandbox state.
    /// This is needed when sandbox is started/stopped dynamically.
    async fn recreate_provider_with_sandbox(&self, sandbox_enabled: bool) {
        let config = self.config.read().await;
        if config.provider == "anthropic" && config.api_key.is_empty() {
            // Determine allow_all based on config override, sandbox state and permission config
            let allow_all_for_mcp = if config.allow_all {
                true // Explicit allow_all override (headless mode)
            } else if sandbox_enabled {
                let core_config = self.instance.config().await;
                core_config
                    .permission
                    .as_ref()
                    .and_then(|p| p.allow_all_in_sandbox)
                    .unwrap_or(true)
            } else {
                false
            };

            // Get the current CLI session ID before recreating the provider
            // This preserves Claude CLI session persistence across provider recreations
            let cli_session_id = {
                let provider = self.provider.read().await;
                provider.get_cli_session_id().await
            };

            // Using Claude CLI - recreate provider with new sandbox state
            debug!(
                sandbox_enabled = sandbox_enabled,
                allow_all_for_mcp = allow_all_for_mcp,
                cli_session_id = ?cli_session_id,
                "Recreating Claude CLI provider after sandbox state change"
            );

            match create_provider(&config, Some(sandbox_enabled), allow_all_for_mcp) {
                Ok(new_provider) => {
                    // Restore the CLI session ID to the new provider
                    if cli_session_id.is_some() {
                        new_provider
                            .set_cli_session_id(cli_session_id.clone())
                            .await;
                        debug!(cli_session_id = ?cli_session_id, "Restored CLI session ID to new provider");
                    }

                    drop(config); // Release read lock before acquiring write lock
                    let mut provider = self.provider.write().await;
                    *provider = new_provider;
                    info!(
                        sandbox_enabled = sandbox_enabled,
                        allow_all_for_mcp = allow_all_for_mcp,
                        cli_session_id = ?cli_session_id,
                        "Recreated Claude CLI provider after sandbox state change"
                    );
                }
                Err(e) => {
                    warn!(error = %e, "Failed to recreate provider after sandbox state change");
                }
            }
        }
    }

    /// Handle sandbox stop action.
    async fn handle_sandbox_stop(&self, update_tx: &mpsc::UnboundedSender<AppUpdate>) {
        if let Some(ref manager) = self.sandbox_manager {
            match manager.stop().await {
                Ok(()) => {
                    info!("Sandbox stopped successfully");

                    // Update shared sandbox state in permission manager
                    self.permission_manager.set_sandbox_running(false);
                    self.permission_manager.set_sandbox_runtime_any(None).await;

                    self.bus
                        .publish(SandboxStatusChanged {
                            state: SandboxState::Stopped,
                            runtime_type: Some(manager.runtime_type_display()),
                            error: None,
                        })
                        .await;
                    send_update(
                        &update_tx,
                        AppUpdate::SandboxUpdated(wonopcode_tui::SandboxStatusUpdate {
                            state: "stopped".to_string(),
                            runtime_type: Some(manager.runtime_type_display()),
                            error: None,
                            container_id: None,
                        }),
                    );

                    // Send system message about sandbox stopping
                    send_update(
                        &update_tx,
                        AppUpdate::SystemMessage(
                            "◇ Sandbox stopped - commands will execute directly on host"
                                .to_string(),
                        ),
                    );

                    // Recreate provider with sandbox disabled so MCP server doesn't use sandbox
                    self.recreate_provider_with_sandbox(false).await;
                }
                Err(e) => {
                    warn!(error = %e, "Failed to stop sandbox");
                    send_update(
                        &update_tx,
                        AppUpdate::SandboxUpdated(wonopcode_tui::SandboxStatusUpdate {
                            state: "error".to_string(),
                            runtime_type: Some(manager.runtime_type_display()),
                            error: Some(e.to_string()),
                            container_id: None,
                        }),
                    );
                }
            }
        } else {
            send_update(
                &update_tx,
                AppUpdate::SandboxUpdated(wonopcode_tui::SandboxStatusUpdate {
                    state: "disabled".to_string(),
                    runtime_type: None,
                    error: Some("Sandbox not configured".to_string()),
                    container_id: None,
                }),
            );
        }
    }

    /// Handle git status action.
    async fn handle_git_status(&self, update_tx: &mpsc::UnboundedSender<AppUpdate>) {
        let cwd = self.instance.directory();
        let git = GitOperations::new(cwd);

        match git.status() {
            Ok(status) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitStatusUpdated(GitStatusUpdate {
                        branch: status.branch,
                        ahead: status.ahead,
                        behind: status.behind,
                        files: status
                            .files
                            .into_iter()
                            .map(|f| GitFileUpdate {
                                path: f.path,
                                status: match f.status {
                                    wonopcode_server::GitFileState::Modified => {
                                        "modified".to_string()
                                    }
                                    wonopcode_server::GitFileState::Added => "added".to_string(),
                                    wonopcode_server::GitFileState::Deleted => {
                                        "deleted".to_string()
                                    }
                                    wonopcode_server::GitFileState::Renamed => {
                                        "renamed".to_string()
                                    }
                                    wonopcode_server::GitFileState::Untracked => {
                                        "untracked".to_string()
                                    }
                                    wonopcode_server::GitFileState::Conflicted => {
                                        "conflicted".to_string()
                                    }
                                },
                                staged: f.staged,
                            })
                            .collect(),
                    }),
                );
            }
            Err(e) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: false,
                        message: e.to_string(),
                    },
                );
            }
        }
    }

    /// Handle git stage action.
    async fn handle_git_stage(
        &self,
        update_tx: &mpsc::UnboundedSender<AppUpdate>,
        paths: Vec<String>,
    ) {
        let cwd = self.instance.directory();
        let git = GitOperations::new(cwd);

        match git.stage(&paths) {
            Ok(()) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: true,
                        message: format!("Staged {} file(s)", paths.len()),
                    },
                );
                // Refresh status
                self.handle_git_status(update_tx).await;
            }
            Err(e) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: false,
                        message: e.to_string(),
                    },
                );
            }
        }
    }

    /// Handle git unstage action.
    async fn handle_git_unstage(
        &self,
        update_tx: &mpsc::UnboundedSender<AppUpdate>,
        paths: Vec<String>,
    ) {
        let cwd = self.instance.directory();
        let git = GitOperations::new(cwd);

        match git.unstage(&paths) {
            Ok(()) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: true,
                        message: format!("Unstaged {} file(s)", paths.len()),
                    },
                );
                // Refresh status
                self.handle_git_status(update_tx).await;
            }
            Err(e) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: false,
                        message: e.to_string(),
                    },
                );
            }
        }
    }

    /// Handle git checkout action.
    async fn handle_git_checkout(
        &self,
        update_tx: &mpsc::UnboundedSender<AppUpdate>,
        paths: Vec<String>,
    ) {
        let cwd = self.instance.directory();
        let git = GitOperations::new(cwd);

        match git.checkout(&paths) {
            Ok(()) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: true,
                        message: format!("Discarded changes to {} file(s)", paths.len()),
                    },
                );
                // Refresh status
                self.handle_git_status(update_tx).await;
            }
            Err(e) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: false,
                        message: e.to_string(),
                    },
                );
            }
        }
    }

    /// Handle git commit action.
    async fn handle_git_commit(
        &self,
        update_tx: &mpsc::UnboundedSender<AppUpdate>,
        message: String,
    ) {
        let cwd = self.instance.directory();
        let git = GitOperations::new(cwd);

        match git.commit(&message) {
            Ok(commit_info) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: true,
                        message: format!("Committed: {}", commit_info.id),
                    },
                );
                // Refresh status
                self.handle_git_status(update_tx).await;
            }
            Err(e) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: false,
                        message: e.to_string(),
                    },
                );
            }
        }
    }

    /// Handle git history action.
    async fn handle_git_history(&self, update_tx: &mpsc::UnboundedSender<AppUpdate>) {
        let cwd = self.instance.directory();
        let git = GitOperations::new(cwd);

        match git.history(50) {
            Ok(commits) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitHistoryUpdated(
                        commits
                            .into_iter()
                            .map(|c| GitCommitUpdate {
                                id: c.id,
                                message: c.message,
                                author: c.author,
                                date: c.timestamp,
                            })
                            .collect(),
                    ),
                );
            }
            Err(e) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: false,
                        message: e.to_string(),
                    },
                );
            }
        }
    }

    /// Handle git push action.
    async fn handle_git_push(&self, update_tx: &mpsc::UnboundedSender<AppUpdate>) {
        let cwd = self.instance.directory();
        let git = GitOperations::new(cwd);

        match git.push(None, None) {
            Ok(()) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: true,
                        message: "Push successful".to_string(),
                    },
                );
                // Refresh status
                self.handle_git_status(update_tx).await;
            }
            Err(e) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: false,
                        message: e.to_string(),
                    },
                );
            }
        }
    }

    /// Handle git pull action.
    async fn handle_git_pull(&self, update_tx: &mpsc::UnboundedSender<AppUpdate>) {
        let cwd = self.instance.directory();
        let git = GitOperations::new(cwd);

        match git.pull(None, None) {
            Ok(()) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: true,
                        message: "Pull successful".to_string(),
                    },
                );
                // Refresh status
                self.handle_git_status(update_tx).await;
            }
            Err(e) => {
                send_update(
                    &update_tx,
                    AppUpdate::GitOperationResult {
                        success: false,
                        message: e.to_string(),
                    },
                );
            }
        }
    }

    /// Build the full MCP status including external and unsupported servers.
    async fn build_mcp_status(&self) -> Vec<McpStatusUpdate> {
        let mut mcp_updates: Vec<McpStatusUpdate> = Vec::new();

        // Add external servers (local/stdio passed to Claude CLI) as connected
        for name in &self.external_mcp_server_names {
            mcp_updates.push(McpStatusUpdate {
                name: name.clone(),
                connected: true, // These work through Claude CLI
                error: None,
            });
        }

        // Add unsupported/disabled servers
        for (name, reason) in &self.unsupported_mcp_servers {
            mcp_updates.push(McpStatusUpdate {
                name: name.clone(),
                connected: false,
                error: Some(reason.clone()),
            });
        }

        // Add connected servers from MCP client (remote HTTP/SSE servers)
        if let Some(ref mcp_client) = self.mcp_client {
            let client_servers: Vec<McpStatusUpdate> = mcp_client
                .list_servers()
                .await
                .into_iter()
                .map(|(name, connected, error)| McpStatusUpdate {
                    name,
                    connected,
                    error,
                })
                .collect();
            mcp_updates.extend(client_servers);
        }

        mcp_updates
    }

    /// Handle slash commands like /tools and /help
    async fn handle_slash_command(&self, input: &str) -> Option<String> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return None;
        }

        let parts: Vec<&str> = trimmed[1..].split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }

        match parts[0] {
            "tools" => Some(self.format_tools_list().await),
            "help" => Some(self.format_help()),
            _ => Some(format!(
                "Unknown command: /{}\nType /help for available commands.",
                parts[0]
            )),
        }
    }

    /// Format list of all available tools for debugging
    async fn format_tools_list(&self) -> String {
        let all_tools = self.tools.all();
        let mut tools_vec: Vec<_> = all_tools.collect();
        tools_vec.sort_by_key(|tool| tool.id());

        let mut output = String::new();
        output.push_str("Available Tools (sorted alphabetically):\n");
        output.push_str("=".repeat(50).as_str());
        output.push('\n');

        // Group tools by prefix to identify duplicates
        let mut tool_groups: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        for tool in &tools_vec {
            let id = tool.id().to_string();
            let base_name = if let Some(pos) = id.rfind('_') {
                id[..pos].to_string()
            } else if let Some(stripped) = id.strip_prefix("mcp_") {
                stripped.to_string()
            } else {
                id.clone()
            };

            tool_groups.entry(base_name).or_default().push(id);
        }

        // Show duplicates first if any
        let mut has_duplicates = false;
        for (base_name, ids) in &tool_groups {
            if ids.len() > 1 {
                if !has_duplicates {
                    output.push_str("\n🚨 DUPLICATE TOOLS DETECTED:\n");
                    output.push_str("-".repeat(30).as_str());
                    output.push('\n');
                    has_duplicates = true;
                }
                output.push_str(&format!("Base name: {}\n", base_name));
                for id in ids {
                    output.push_str(&format!("  - {}\n", id));
                }
                output.push('\n');
            }
        }

        if has_duplicates {
            output.push_str("=".repeat(50).as_str());
            output.push('\n');
        }

        // Show all tools
        output.push_str(&format!("\nAll {} Tools:\n", tools_vec.len()));
        output.push_str("-".repeat(30).as_str());
        output.push('\n');

        for tool in tools_vec {
            let id = tool.id();
            let description = tool.description();

            // Truncate long descriptions
            let short_desc = if description.len() > 80 {
                format!("{}...", wonopcode_util::truncate_to_char_boundary(&description, 77))
            } else {
                description.to_string()
            };

            output.push_str(&format!("{}\n  {}\n\n", id, short_desc));
        }

        output.push_str("=".repeat(50).as_str());
        output.push('\n');

        if has_duplicates {
            output.push_str("\n⚠️  Tool name conflicts detected! This may cause API errors.\n");
        } else {
            output.push_str("\n✅ No duplicate tool names detected.\n");
        }

        output
    }

    /// Format help text for available slash commands
    fn format_help(&self) -> String {
        let mut help = String::new();
        help.push_str("Available Commands:\n");
        help.push_str("=".repeat(30).as_str());
        help.push('\n');
        help.push_str("/tools  - Show all available tools with IDs and descriptions\n");
        help.push_str("/help   - Show this help message\n");
        help.push_str("\nType any command starting with '/' to use it.\n");
        help
    }

    /// Load workstream context from the current directory.
    ///
    /// Returns the ticket ID and default tracker ID for the current workstream.
    /// If no workstream state exists, returns (None, None).
    fn load_workstream_context(cwd: &Path) -> (Option<String>, Option<String>) {
        match WorkstreamState::load(cwd) {
            Ok(Some(state)) => {
                let ticket_id = if state.ticket_id.is_empty() {
                    None
                } else {
                    Some(state.ticket_id.clone())
                };
                let tracker_id = state.tracker_id().map(String::from);

                debug!(
                    ticket_id = ?ticket_id,
                    tracker_id = ?tracker_id,
                    "Loaded workstream context"
                );

                (ticket_id, tracker_id)
            }
            Ok(None) => {
                trace!("No workstream state found");
                (None, None)
            }
            Err(e) => {
                warn!(error = %e, "Failed to load workstream state");
                (None, None)
            }
        }
    }
}

/// Create a provider from configuration.
///
/// # Arguments
/// * `config` - Runner configuration
/// * `_sandbox_enabled` - Whether sandbox is enabled (currently unused, HTTP MCP handles this server-side)
/// * `_allow_all` - Whether to allow all tool executions (currently unused, HTTP MCP handles this server-side)
fn create_provider(
    config: &RunnerConfig,
    _sandbox_enabled: Option<bool>,
    _allow_all: bool,
) -> Result<BoxedLanguageModel, Box<dyn std::error::Error + Send + Sync>> {
    let model_info = get_model_info(&config.model_id, &config.provider);

    match config.provider.as_str() {
        "anthropic" => {
            // Anthropic API provider - requires API key
            if !config.api_key.is_empty() {
                info!("Using Anthropic API with provided key");
                let provider = AnthropicProvider::new(&config.api_key, model_info)?;
                Ok(Arc::new(provider))
            } else {
                Err("No Anthropic API key provided. Set ANTHROPIC_API_KEY or use 'anthropic-cli' provider for subscription access.".into())
            }
        }
        "anthropic-cli" => {
            // Anthropic CLI provider - requires Claude CLI to be installed and authenticated
            let cli_available = ClaudeCliProvider::is_available();
            debug!(
                cli_available = cli_available,
                "Checking Claude CLI availability"
            );

            if !cli_available {
                return Err("Claude CLI not found. Install with: npm install -g @anthropic-ai/claude-code".into());
            }

            let cli_authenticated = ClaudeCliProvider::is_authenticated();
            debug!(
                cli_authenticated = cli_authenticated,
                "Checking Claude CLI authentication"
            );

            if !cli_authenticated {
                return Err("Claude CLI not authenticated. Run 'claude login' to authenticate.".into());
            }

            info!("Using Claude CLI for subscription-based access with custom tools");
            // MCP requires HTTP transport - mcp_url must be provided
            if let Some(ref mcp_url) = config.mcp_url {
                info!(
                    mcp_url = %mcp_url,
                    has_secret = config.mcp_secret.is_some(),
                    external_servers = config.external_mcp_servers.len(),
                    "Using MCP HTTP transport"
                );

                // Build MCP config with external servers
                let mut mcp_config = if let Some(ref secret) = config.mcp_secret {
                    wonopcode_provider::claude_cli::McpCliConfig::with_secret(
                        mcp_url.clone(),
                        secret.clone(),
                    )
                } else {
                    wonopcode_provider::claude_cli::McpCliConfig::new(mcp_url.clone())
                };

                // Add external MCP servers (local/stdio servers from .mcp.json)
                for (name, (command_args, env)) in &config.external_mcp_servers {
                    if let Some((command, args)) = command_args.split_first() {
                        let external_server =
                            wonopcode_provider::claude_cli::ExternalMcpServer {
                                command: command.clone(),
                                args: args.to_vec(),
                                env: env.clone(),
                            };
                        mcp_config =
                            mcp_config.with_external_server(name, external_server);
                        info!(server = %name, "Added external MCP server");
                    }
                }

                let mut provider =
                    wonopcode_provider::claude_cli::ClaudeCliProvider::with_mcp_config(
                        model_info, mcp_config,
                    )?;
                // Set working directory if configured
                if let Some(ref workdir) = config.working_directory {
                    provider.set_working_directory(workdir.clone());
                }
                Ok(Arc::new(provider))
            } else {
                // No MCP URL provided - use Claude CLI without custom tools
                info!("No MCP URL provided, using Claude CLI without custom tools");
                let mut provider =
                    wonopcode_provider::claude_cli::ClaudeCliProvider::new(model_info)?;
                // Set working directory if configured
                if let Some(ref workdir) = config.working_directory {
                    provider.set_working_directory(workdir.clone());
                }
                Ok(Arc::new(provider))
            }
        }
        "openai" => {
            let provider = OpenAIProvider::new(&config.api_key, model_info)?;
            Ok(Arc::new(provider))
        }
        "openai-codex" => {
            // OpenAI Codex using the Responses API
            // Supports both API key and ChatGPT subscription authentication
            use wonopcode_provider::codex::CodexProvider;
            let provider = if config.api_key.is_empty() {
                // No API key - try to use existing credentials or ChatGPT subscription
                CodexProvider::new(model_info)?
            } else {
                // Use provided API key
                CodexProvider::with_api_key(&config.api_key, model_info)?
            };
            Ok(Arc::new(provider))
        }
        "test" => {
            // Test provider for UI/UX testing - no API key required
            let provider = wonopcode_provider::test::TestProvider::new(model_info);
            Ok(Arc::new(provider))
        }
        "compoundcoders" => {
            // Compound Coders API provider
            use wonopcode_provider::compoundcoders::CompoundCodersProvider;
            if config.api_key.is_empty() {
                Err("No Compound Coders API key provided. Set COMPOUNDCODERS_API_KEY.".into())
            } else {
                let provider = CompoundCodersProvider::new(&config.api_key, model_info)?;
                Ok(Arc::new(provider))
            }
        }
        _ => Err(format!("Unknown provider: {}", config.provider).into()),
    }
}

/// Get model info for a model ID.
/// Uses the centralized registry for all model lookups.
fn get_model_info(model_id: &str, provider: &str) -> ModelInfo {
    wonopcode_provider::registry::get_model_info(model_id, provider)
}

/// Infer the provider from a model name.
/// Uses the centralized registry for provider inference.
fn infer_provider_from_model(model: &str) -> Option<&'static str> {
    Some(wonopcode_provider::registry::infer_provider(model))
}

/// Build system prompt with environment context.
fn build_system_prompt_for_session(provider: &str, model: &str, cwd: &Path) -> String {
    let renderer = match system_prompt::SystemPromptRenderer::new() {
        Ok(r) => r,
        Err(e) => {
            warn!("Failed to create system prompt renderer: {}", e);
            return String::new();
        }
    };
    let mut vars = system_prompt::SystemPromptVars::from_env(cwd, model, provider);
    vars.agent_md = system_prompt::load_custom_instructions(cwd);
    renderer.render(&vars).unwrap_or_default()
}

/// Load API key from environment or credentials file.
///
/// Checks in order:
/// 1. Environment variables (highest priority for CI/Docker)
/// 2. CredentialsManager (unified config with legacy support)
pub fn load_api_key(provider: &str) -> Option<String> {
    // Use CredentialsManager which handles both env vars and file-based credentials
    if let Some(creds_manager) = wonopcode_core::CredentialsManager::new() {
        return creds_manager.get_api_key(provider);
    }

    // Fallback to direct env var check if CredentialsManager fails
    // Note: anthropic-cli doesn't use an API key, it uses Claude CLI auth
    let env_var = match provider {
        "anthropic" => "ANTHROPIC_API_KEY",
        "anthropic-cli" => return None, // CLI provider doesn't use API key
        "openai" => "OPENAI_API_KEY",
        "openai-codex" => "OPENAI_API_KEY", // Codex uses the same API key as OpenAI
        _ => return None,
    };

    std::env::var(env_var).ok().filter(|k| !k.is_empty())
}

/// Get the authentication method configured for a provider.
///
/// Returns the configured auth method if credentials are set,
/// otherwise None.
pub fn get_auth_method(provider: &str) -> Option<wonopcode_core::AuthMethod> {
    wonopcode_core::CredentialsManager::new()?.get_auth_method(provider)
}

/// Check if a provider has valid credentials configured.
pub fn has_credentials(provider: &str) -> bool {
    wonopcode_core::CredentialsManager::new()
        .map(|cm| cm.has_credentials(provider))
        .unwrap_or(false)
}

/// Get the server configuration for a repository path.
///
/// This returns the per-server configuration including provider, model,
/// and auth method preferences.
pub fn get_server_config(repo_path: &std::path::Path) -> wonopcode_core::ServerInstanceConfig {
    wonopcode_core::ServerConfigManager::new()
        .map(|sm| sm.get_config(repo_path))
        .unwrap_or_default()
}

/// Get provider status information.
///
/// Returns status for a provider including availability and configured auth method.
pub fn get_provider_status(provider: &str) -> wonopcode_core::ProviderStatus {
    use wonopcode_provider::claude_cli::ClaudeCliProvider;
    use wonopcode_provider::codex::CodexProvider;

    let creds_manager = wonopcode_core::CredentialsManager::new();
    let has_api_key = creds_manager
        .as_ref()
        .map(|cm| cm.get_api_key(provider).is_some())
        .unwrap_or(false);
    let auth_method = creds_manager
        .as_ref()
        .and_then(|cm| cm.get_auth_method(provider));

    // Check CLI availability for anthropic-cli and openai-codex
    let (cli_available, cli_authenticated) = match provider {
        "anthropic-cli" => (
            ClaudeCliProvider::is_available(),
            ClaudeCliProvider::is_authenticated(),
        ),
        "openai-codex" => (
            CodexProvider::is_available(),
            CodexProvider::has_credentials(),
        ),
        _ => (false, false),
    };

    // Provider is available based on its auth mechanism
    let available = match provider {
        "anthropic" => has_api_key, // API provider requires API key
        "anthropic-cli" => cli_available && cli_authenticated, // CLI provider requires CLI auth
        "openai" => has_api_key, // API provider requires API key
        "openai-codex" => has_api_key || cli_authenticated, // Codex can use either
        _ => has_api_key,
    };

    wonopcode_core::ProviderStatus {
        id: provider.to_string(),
        name: provider_display_name(provider),
        available,
        auth_method,
        cli_available,
        cli_authenticated,
        model_count: 0, // TODO: Add model counting
    }
}

/// Get display name for a provider.
fn provider_display_name(provider: &str) -> String {
    match provider {
        "anthropic" => "Anthropic API".to_string(),
        "anthropic-cli" => "Claude CLI (Subscription)".to_string(),
        "openai" => "OpenAI API".to_string(),
        "openai-codex" => "OpenAI Codex".to_string(),
        _ => provider.to_string(),
    }
}

/// Convert wonopcode McpRemoteConfig to wonopcode_mcp ServerConfig.
fn convert_mcp_remote_config(name: &str, config: &McpRemoteConfig) -> McpServerConfig {
    let mut server_config = McpServerConfig::sse(name, &config.url);

    // Add headers if specified
    if let Some(headers) = &config.headers {
        for (key, value) in headers {
            server_config = server_config.with_header(key, value);
        }
    }

    server_config
}

/// Convert core SandboxConfig to wonopcode-sandbox SandboxConfig.
fn convert_sandbox_config(core_config: &CoreSandboxConfig) -> SandboxConfig {
    use wonopcode_sandbox::{MountConfig, NetworkPolicy, ResourceLimits};

    // Parse runtime type
    let runtime = match core_config.runtime.as_deref() {
        Some("docker") => SandboxRuntimeType::Docker,
        Some("podman") => SandboxRuntimeType::Podman,
        Some("lima") => SandboxRuntimeType::Lima,
        Some("none") => SandboxRuntimeType::None,
        _ => SandboxRuntimeType::Auto,
    };

    // Parse network policy
    let network = match core_config.network.as_deref() {
        Some("full") => NetworkPolicy::Full,
        Some("none") => NetworkPolicy::None,
        _ => NetworkPolicy::Limited,
    };

    // Build resource limits
    let resources = if let Some(res) = &core_config.resources {
        ResourceLimits {
            memory: res.memory.clone().unwrap_or_else(|| "2G".to_string()),
            cpus: res.cpus.unwrap_or(2.0),
            disk: None,
            pids: res.pids.unwrap_or(256),
            readonly_rootfs: false,
        }
    } else {
        ResourceLimits::default()
    };

    // Build mount config
    let mounts = if let Some(m) = &core_config.mounts {
        MountConfig {
            workspace_writable: m.workspace_writable.unwrap_or(true),
            readonly: std::collections::HashMap::new(),
            persist_caches: m.persist_caches.unwrap_or(true),
            workspace_path: m
                .workspace_path
                .clone()
                .unwrap_or_else(|| "/workspace".to_string()),
        }
    } else {
        MountConfig::default()
    };

    SandboxConfig {
        enabled: core_config.enabled.unwrap_or(false),
        runtime,
        image: core_config.image.clone(),
        resources,
        network,
        mounts,
        bypass_tools: core_config.bypass_tools.clone().unwrap_or_default(),
        keep_alive: core_config.keep_alive.unwrap_or(true),
        startup_timeout_secs: 60,
    }
}

/// Get a workstream ID based on the current directory.
///
/// This uses the git branch name if available, otherwise falls back to
/// a sanitized version of the directory name.
pub fn get_workstream_id(cwd: &Path) -> String {
    // Try to get the git branch name
    if let Ok(output) = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .current_dir(cwd)
        .output()
    {
        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !branch.is_empty() && branch != "HEAD" {
                // Sanitize branch name for use as directory name
                return branch.replace('/', "-").replace('\\', "-");
            }
        }
    }

    // Fall back to directory name
    cwd.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_string())
        .unwrap_or_else(|| "default".to_string())
}
