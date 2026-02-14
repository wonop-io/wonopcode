//! Runner module - connects the TUI to the AI prompt loop.
// @ace:implements COMP-T90R9Q-8J4

use futures::future::join_all;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use wonopcode_agent_loop::{
    BoxedAgentLoop, CompactionConfig as LoopCompactionConfig, LoopConfig, LoopContext, LoopUpdate,
};
use wonopcode_core::bus::{
    Bus, PermissionRequest as BusPermissionRequest, SandboxState, SandboxStatusChanged,
};
use wonopcode_core::config::{McpConfig, McpRemoteConfig, SandboxConfig as CoreSandboxConfig};
use wonopcode_core::permission::{Decision, PermissionManager};
use wonopcode_core::system_prompt;
use wonopcode_core::Instance;
use wonopcode_core::SessionService;
use wonopcode_mcp::{McpClient, ServerConfig as McpServerConfig};
use wonopcode_provider::{
    anthropic::AnthropicProvider, claude_cli::ClaudeCliProvider, google::GoogleProvider,
    model::ModelInfo, openai::OpenAIProvider, openrouter::OpenRouterProvider, BoxedLanguageModel,
    Message as ProviderMessage, ToolDefinition,
};
use wonopcode_sandbox::{SandboxConfig, SandboxManager, SandboxRuntime, SandboxRuntimeType};
use wonopcode_server::GitOperations;
use wonopcode_snapshot::{SnapshotConfig, SnapshotStore};
use wonopcode_tools::{mcp::McpToolsBuilder, mcp_todo_adapter, todo, ToolEvent, ToolRegistry};
use wonopcode_tui::{
    AppAction, AppUpdate, GitCommitUpdate, GitFileUpdate, GitStatusUpdate, McpStatusUpdate,
    PermissionRequestUpdate, PhaseUpdate, SaveScope, TodoUpdate,
};
use wonopcode_util::FileTimeState;

use crate::compaction;
use crate::compaction::{CompactionConfig, CompactionResult};

/// Helper to send updates to the TUI with proper error logging.
/// This replaces `let _ = update_tx.send(...)` to avoid silent failures.
fn send_update(update_tx: &mpsc::UnboundedSender<AppUpdate>, update: AppUpdate) {
    if let Err(e) = update_tx.send(update) {
        warn!("Failed to send update to TUI (channel closed): {}", e);
    }
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
                let hash = rest.len() as u64 * 31 + rest.as_bytes().iter().map(|&b| b as u64).sum::<u64>();
                (rest.to_string(), format!("todo_{:x}", hash))
            }
        } else {
            let hash = rest.len() as u64 * 31 + rest.as_bytes().iter().map(|&b| b as u64).sum::<u64>();
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

        // Create tool registry with all tools
        let mut tools = ToolRegistry::with_builtins();
        tools.register(Arc::new(wonopcode_tools::bash::BashTool));
        tools.register(Arc::new(wonopcode_tools::webfetch::WebFetchTool));

        // ACE tools for structured workflow (replaces legacy todowrite/todoread)
        tools.register(Arc::new(wonopcode_tools::AceTodoReadTool));
        tools.register(Arc::new(wonopcode_tools::AceTodoWriteTool));
        tools.register(Arc::new(wonopcode_tools::AceTodoUpdateTool));
        tools.register(Arc::new(wonopcode_tools::AceCreateArtifactTool));
        tools.register(Arc::new(wonopcode_tools::AceReadArtifactTool));
        tools.register(Arc::new(wonopcode_tools::AceWhatNowTool));
        tools.register(Arc::new(wonopcode_tools::AceSubmitCheckpointTool));

        tools.register(Arc::new(wonopcode_tools::lsp::LspTool::with_client(
            lsp_client.clone(),
        )));
        tools.register(Arc::new(wonopcode_tools::task::TaskTool::new()));
        tools.register(Arc::new(
            wonopcode_tools::plan_mode::EnterPlanModeTool::new(),
        ));
        tools.register(Arc::new(wonopcode_tools::plan_mode::ExitPlanModeTool::new()));
        // Note: skill and batch tools require async initialization, done in new_with_features

        // Use shared bus/permission_manager or create new ones
        let bus = shared_bus.unwrap_or_default();
        let permission_manager = shared_permission_manager
            .unwrap_or_else(|| Arc::new(PermissionManager::new(bus.clone())));

        // Create file time tracker
        let file_time = Arc::new(FileTimeState::new());

        // Use provided agent loop or create default StandardLoop
        let agent_loop: BoxedAgentLoop =
            agent_loop.unwrap_or_else(|| Box::new(wonopcode_agent_loop::StandardLoop::new()));

        debug!(
            loop_name = agent_loop.name(),
            "Runner created with agent loop"
        );

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
        Self::new_with_shared(config, instance, mcp_configs, None, None, None, None).await
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
    pub async fn new_with_shared(
        mut config: RunnerConfig,
        instance: Instance,
        mcp_configs: Option<HashMap<String, McpConfig>>,
        shared_bus: Option<Bus>,
        shared_permission_manager: Option<Arc<PermissionManager>>,
        session_service: Option<Arc<SessionService>>,
        agent_loop: Option<BoxedAgentLoop>,
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
        let cwd = runner.instance.directory();
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

        // Initialize skill tool (discovers skills from project directories)
        let skill_dirs = vec![cwd.to_path_buf()];
        let skill_tool = wonopcode_tools::skill::SkillTool::discover(&skill_dirs).await;

        // Re-register tools with skill support
        // This should always succeed since we just created the runner and haven't shared the Arc yet
        if let Some(tools) = Arc::get_mut(&mut runner.tools) {
            tools.register(Arc::new(skill_tool));
        } else {
            // This should never happen during initialization, but log if it does
            warn!("Could not register skill tool: tools registry already shared");
        }

        // Initialize MCP client if configured
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
            let mut new_tools = ToolRegistry::with_builtins();
            new_tools.register(Arc::new(wonopcode_tools::bash::BashTool));
            new_tools.register(Arc::new(wonopcode_tools::webfetch::WebFetchTool));

            // ACE tools for structured workflow (replaces legacy todowrite/todoread)
            new_tools.register(Arc::new(wonopcode_tools::AceTodoReadTool));
            new_tools.register(Arc::new(wonopcode_tools::AceTodoWriteTool));
            new_tools.register(Arc::new(wonopcode_tools::AceTodoUpdateTool));
            new_tools.register(Arc::new(wonopcode_tools::AceCreateArtifactTool));
            new_tools.register(Arc::new(wonopcode_tools::AceReadArtifactTool));
            new_tools.register(Arc::new(wonopcode_tools::AceWhatNowTool));
            new_tools.register(Arc::new(wonopcode_tools::AceSubmitCheckpointTool));

            new_tools.register(Arc::new(wonopcode_tools::lsp::LspTool::with_client(
                self.lsp_client.clone(),
            )));
            new_tools.register(Arc::new(wonopcode_tools::task::TaskTool::new()));
            new_tools.register(Arc::new(
                wonopcode_tools::plan_mode::EnterPlanModeTool::new(),
            ));
            new_tools.register(Arc::new(wonopcode_tools::plan_mode::ExitPlanModeTool::new()));

            // Re-discover skills for the new registry
            let cwd = self.instance.directory();
            let skill_dirs = vec![cwd.to_path_buf()];
            let skill_tool = wonopcode_tools::skill::SkillTool::discover(&skill_dirs).await;
            new_tools.register(Arc::new(skill_tool));

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
        use std::time::Instant;

        // Get the cancellation token for this prompt
        let cancel = self.get_cancel_token().await;

        // Get mutable access to conversation history
        let mut messages = {
            let history = self.history.read().await;
            history.clone()
        };

        // === PRE-PROMPT COMPACTION ===
        // Check if compaction is needed before starting (message count or context limit)
        let context_limit = {
            let provider = self.provider.read().await;
            provider.model_info().limit.context
        };

        const AUTO_COMPACT_MESSAGE_THRESHOLD: usize = 100;

        if messages.len() > AUTO_COMPACT_MESSAGE_THRESHOLD {
            info!(
                messages = messages.len(),
                threshold = AUTO_COMPACT_MESSAGE_THRESHOLD,
                "Message count exceeds threshold, triggering automatic compaction"
            );
            send_update(
                update_tx,
                AppUpdate::Status(format!("Auto-compacting {} messages...", messages.len())),
            );

            let compact_start = Instant::now();
            let messages_before = messages.len();
            let estimated_tokens = compaction::estimate_token_usage(&messages);

            let provider = self.provider.read().await;
            match compaction::compact(
                &mut messages,
                &provider,
                &self.compaction_config,
                &estimated_tokens,
                context_limit,
                false,
            )
            .await
            {
                CompactionResult::Compacted {
                    messages: new_messages,
                    summary: _,
                    messages_summarized,
                } => {
                    let duration = compact_start.elapsed();
                    info!(
                        messages_before = messages_before,
                        messages_after = new_messages.len(),
                        messages_summarized = messages_summarized,
                        duration_ms = duration.as_millis(),
                        "Auto-compaction successful"
                    );
                    messages = new_messages;

                    // Update history with compacted messages
                    {
                        let mut history = self.history.write().await;
                        *history = messages.clone();
                    }

                    let status = if messages_summarized > 0 {
                        format!("Summarized {messages_summarized} messages to save context")
                    } else {
                        "Pruned old tool outputs to save context".to_string()
                    };
                    send_update(update_tx, AppUpdate::Status(status));
                }
                CompactionResult::NotNeeded | CompactionResult::InsufficientMessages => {
                    debug!("Compaction not needed or insufficient messages");
                }
                CompactionResult::Failed(err) => {
                    warn!(
                        "Auto-compaction failed: {}, continuing without compaction",
                        err
                    );
                }
            }
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
                        let _ = update_tx_for_tool_events.send(AppUpdate::TodosUpdated { phases, todos });
                    }
                    // Other events are logged but not forwarded yet
                    ToolEvent::ArtifactCreated { id, artifact_type } => {
                        debug!("Tool event: ArtifactCreated {} ({})", id, artifact_type);
                    }
                    ToolEvent::ArtifactUpdated { id } => {
                        debug!("Tool event: ArtifactUpdated {}", id);
                    }
                    ToolEvent::TaskStatusChanged { id, old_status, new_status } => {
                        debug!("Tool event: TaskStatusChanged {} ({} -> {})", id, old_status, new_status);
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
                        // Note: ACE tools (ace_todo_write, ace_todo_update) emit events directly
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
                                let (phases, todos) = convert_phased_todos_to_updates(&phased_todos);
                                let _ = update_tx_clone.send(AppUpdate::TodosUpdated { phases, todos });
                            }
                        }
                        AppUpdate::ToolCompleted {
                            id,
                            success,
                            output,
                            metadata,
                        }
                    },
                    LoopUpdate::ResponseComplete { text } => AppUpdate::Completed { text },
                    LoopUpdate::TokenUsage {
                        input,
                        output,
                        cost,
                        context_limit,
                    } => AppUpdate::TokenUsage {
                        input,
                        output,
                        cost,
                        context_limit,
                    },
                    LoopUpdate::Status(status) => AppUpdate::Status(status),
                    LoopUpdate::Error(error) => AppUpdate::Error(error),
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
                max_iterations: 50,
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

        // Build LoopContext
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
        };

        // Run the agent loop
        let result = {
            let agent_loop = self.agent_loop.lock().await;
            agent_loop.run_prompt(&mut ctx, user_input).await
        };

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
                    // Only process messages that were added during this turn
                    // (skip the ones that existed before the loop ran)
                    let new_messages: Vec<_> = messages.iter().skip(messages_count_before_loop).collect();
                    
                    info!(
                        messages_before = messages_count_before_loop,
                        messages_after = messages.len(),
                        new_message_count = new_messages.len(),
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
                                let tools_in_msg = msg.content.iter().filter(|c| {
                                    matches!(c, wonopcode_provider::ContentPart::ToolUse { .. })
                                }).count();
                                tool_count += tools_in_msg;
                                
                                info!(
                                    content_parts = msg.content.len(),
                                    tool_use_parts = tools_in_msg,
                                    "SESSION PERSISTENCE: Saving assistant message"
                                );
                                
                                // This message may contain ContentPart::ToolUse parts
                                // which will be converted to MessagePart::Tool by save_assistant_message
                                match svc.save_assistant_message(msg, &last_saved_id).await {
                                    Ok(msg_id) => {
                                        info!(
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
                                        if let wonopcode_provider::ContentPart::ToolResult { tool_use_id, content: result_content, is_error } = content {
                                            let output = result_content.clone();
                                            let success = !is_error.unwrap_or(false);
                                            
                                            debug!(
                                                assistant_msg_id = %assistant_msg_id,
                                                tool_use_id = %tool_use_id,
                                                success = %success,
                                                output_len = output.len(),
                                                "SESSION PERSISTENCE: Updating tool result"
                                            );
                                            
                                            if let Err(e) = svc.update_tool_result(
                                                assistant_msg_id,
                                                tool_use_id,
                                                output,
                                                success,
                                                None,
                                            ).await {
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
                    
                    info!(
                        assistant_messages_saved = assistant_count,
                        total_tool_parts = tool_count,
                        tool_results_updated = tool_results_updated,
                        "SESSION PERSISTENCE: Completed saving messages"
                    );
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
            // For Anthropic, allow CLI-based subscription auth
            if provider_name == "anthropic"
                && ClaudeCliProvider::is_available()
                && ClaudeCliProvider::is_authenticated()
            {
                info!("Using Claude CLI subscription for model change");
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
            (provider.provider_id().to_string(), provider.model_info().id.clone())
        };
        let cli_session_id = if old_provider_id == "anthropic-cli" 
            && provider_name == "anthropic"
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
            while let Ok(req) = permission_rx.recv().await {
                let _ = permission_update_tx.send(AppUpdate::PermissionRequest(
                    PermissionRequestUpdate {
                        id: req.id,
                        tool: req.tool,
                        action: req.action,
                        description: req.description,
                        path: req.path,
                    },
                ));
            }
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

                    // Send started update
                    send_update(&update_tx, AppUpdate::Started);

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
                    debug!("Compact requested");
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
                            AppUpdate::Status("Not enough messages to compact".to_string()),
                        );
                        continue;
                    }

                    // Get context limit and estimate token usage
                    let (context_limit, estimated_tokens) = {
                        let provider = self.provider.read().await;
                        let limit = provider.model_info().limit.context;
                        let tokens = compaction::estimate_token_usage(&messages);
                        (limit, tokens)
                    };

                    // Perform full compaction: prune first, then summarize if still needed
                    let provider = self.provider.read().await;
                    match compaction::compact(
                        &mut messages,
                        &provider,
                        &self.compaction_config,
                        &estimated_tokens,
                        context_limit,
                        false, // Don't add auto-continue for manual compaction
                    )
                    .await
                    {
                        CompactionResult::Compacted {
                            messages: new_messages,
                            summary: _,
                            messages_summarized,
                        } => {
                            let action = if messages_summarized > 0 {
                                "summarized"
                            } else {
                                "pruned tool outputs from"
                            };
                            debug!(
                                action = action,
                                messages_summarized = messages_summarized,
                                new_count = new_messages.len(),
                                "Manual compaction successful"
                            );

                            // Update history
                            {
                                let mut history = self.history.write().await;
                                *history = new_messages;
                            }

                            let status = if messages_summarized > 0 {
                                format!("Compacted {messages_summarized} messages")
                            } else {
                                "Pruned old tool outputs".to_string()
                            };
                            send_update(&update_tx, AppUpdate::Status(status));
                        }
                        CompactionResult::NotNeeded => {
                            send_update(
                                &update_tx,
                                AppUpdate::Status("Compaction not needed".to_string()),
                            );
                        }
                        CompactionResult::InsufficientMessages => {
                            send_update(
                                &update_tx,
                                AppUpdate::Status("Not enough messages to compact".to_string()),
                            );
                        }
                        CompactionResult::Failed(err) => {
                            warn!(error = %err, "Compaction failed");
                            send_update(
                                &update_tx,
                                AppUpdate::Error(format!("Compaction failed: {err}")),
                            );
                        }
                    }
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
                    send_update(
                        &update_tx,
                        AppUpdate::AllowAllChanged { enabled },
                    );
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
                format!("{}...", &description[..77])
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
    use wonopcode_provider::{compoundcoder, deepinfra, groq, mistral, together, xai};

    let model_info = get_model_info(&config.model_id, &config.provider);

    match config.provider.as_str() {
        "anthropic" => {
            // Priority:
            // 1. If API key is provided, use direct API
            // 2. If Claude CLI is available and authenticated, use subscription with custom tools
            // 3. Return error
            if !config.api_key.is_empty() {
                info!("Using Anthropic API key");
                let provider = AnthropicProvider::new(&config.api_key, model_info)?;
                Ok(Arc::new(provider))
            } else {
                let cli_available = ClaudeCliProvider::is_available();
                debug!(
                    cli_available = cli_available,
                    "Checking Claude CLI availability"
                );

                if cli_available {
                    let cli_authenticated = ClaudeCliProvider::is_authenticated();
                    debug!(
                        cli_authenticated = cli_authenticated,
                        "Checking Claude CLI authentication"
                    );

                    if cli_authenticated {
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
                    } else {
                        Err("Claude CLI found but not authenticated. Run 'wonopcode auth login anthropic' to authenticate.".into())
                    }
                } else {
                    Err("No Anthropic API key provided. Set ANTHROPIC_API_KEY or install Claude CLI for subscription access.".into())
                }
            }
        }
        "openai" => {
            let provider = OpenAIProvider::new(&config.api_key, model_info)?;
            Ok(Arc::new(provider))
        }
        "openrouter" => {
            let provider = OpenRouterProvider::new(&config.api_key, model_info)?;
            Ok(Arc::new(provider))
        }
        "google" => {
            let provider = GoogleProvider::new(&config.api_key, model_info)?;
            Ok(Arc::new(provider))
        }
        "xai" => {
            let provider = xai::XaiProvider::new(&config.api_key, model_info)?;
            Ok(Arc::new(provider))
        }
        "mistral" => {
            let provider = mistral::MistralProvider::new(&config.api_key, model_info)?;
            Ok(Arc::new(provider))
        }
        "groq" => {
            let provider = groq::GroqProvider::new(&config.api_key, model_info)?;
            Ok(Arc::new(provider))
        }
        "deepinfra" => {
            let provider = deepinfra::DeepInfraProvider::new(&config.api_key, model_info)?;
            Ok(Arc::new(provider))
        }
        "together" => {
            let provider = together::TogetherProvider::new(&config.api_key, model_info)?;
            Ok(Arc::new(provider))
        }
        "compoundcoder" => {
            let provider = compoundcoder::CompoundCoderProvider::new(&config.api_key, model_info)?;
            Ok(Arc::new(provider))
        }
        "test" => {
            // Test provider for UI/UX testing - no API key required
            let provider = wonopcode_provider::test::TestProvider::new(model_info);
            Ok(Arc::new(provider))
        }
        _ => Err(format!("Unknown provider: {}", config.provider).into()),
    }
}

/// Get model info for a model ID.
fn get_model_info(model_id: &str, provider: &str) -> ModelInfo {
    use wonopcode_provider::{compoundcoder, deepinfra, groq, mistral, together, xai};

    // Check built-in models
    match model_id {
        // Anthropic - Latest (Claude 4.5)
        "claude-sonnet-4-5-20250929" | "claude-sonnet-4-5" => {
            wonopcode_provider::model::anthropic::claude_sonnet_4_5()
        }
        "claude-haiku-4-5-20251001" | "claude-haiku-4-5" => {
            wonopcode_provider::model::anthropic::claude_haiku_4_5()
        }
        "claude-opus-4-5-20251101" | "claude-opus-4-5" => {
            wonopcode_provider::model::anthropic::claude_opus_4_5()
        }
        // Anthropic - Legacy (Claude 4.x)
        "claude-sonnet-4-20250514" | "claude-sonnet-4-0" | "claude-sonnet-4" => {
            wonopcode_provider::model::anthropic::claude_sonnet_4()
        }
        "claude-opus-4-1-20250805" | "claude-opus-4-1" => {
            wonopcode_provider::model::anthropic::claude_opus_4_1()
        }
        "claude-opus-4-20250514" | "claude-opus-4-0" | "claude-opus-4" => {
            wonopcode_provider::model::anthropic::claude_opus_4()
        }
        // Anthropic - Legacy (Claude 3.x)
        "claude-3-7-sonnet-20250219" | "claude-3-7-sonnet" | "claude-3-7-sonnet-latest" => {
            wonopcode_provider::model::anthropic::claude_sonnet_3_7()
        }
        "claude-3-haiku-20240307" | "claude-3-haiku" => {
            wonopcode_provider::model::anthropic::claude_haiku_3()
        }
        // OpenAI - GPT-5 Series
        "gpt-5.2" => wonopcode_provider::model::openai::gpt_5_2(),
        "gpt-5.1" => wonopcode_provider::model::openai::gpt_5_1(),
        "gpt-5" => wonopcode_provider::model::openai::gpt_5(),
        "gpt-5-mini" => wonopcode_provider::model::openai::gpt_5_mini(),
        "gpt-5-nano" => wonopcode_provider::model::openai::gpt_5_nano(),
        // OpenAI - GPT-4.1 Series
        "gpt-4.1" => wonopcode_provider::model::openai::gpt_4_1(),
        "gpt-4.1-mini" => wonopcode_provider::model::openai::gpt_4_1_mini(),
        "gpt-4.1-nano" => wonopcode_provider::model::openai::gpt_4_1_nano(),
        // OpenAI - O-Series
        "o3" => wonopcode_provider::model::openai::o3(),
        "o3-mini" => wonopcode_provider::model::openai::o3_mini(),
        "o4-mini" => wonopcode_provider::model::openai::o4_mini(),
        // OpenAI - Legacy
        "gpt-4o" => wonopcode_provider::model::openai::gpt_4o(),
        "gpt-4o-mini" => wonopcode_provider::model::openai::gpt_4o_mini(),
        "o1" => wonopcode_provider::model::openai::o1(),
        // Google
        "gemini-2.0-flash" | "gemini-2.0-flash-exp" => {
            wonopcode_provider::model::google::gemini_2_flash()
        }
        "gemini-1.5-pro" | "gemini-1.5-pro-latest" => {
            wonopcode_provider::model::google::gemini_1_5_pro()
        }
        "gemini-1.5-flash" | "gemini-1.5-flash-latest" => {
            wonopcode_provider::model::google::gemini_1_5_flash()
        }
        // xAI (Grok)
        "grok-3" => xai::models::grok_3(),
        "grok-3-mini" => xai::models::grok_3_mini(),
        "grok-2" | "grok-2-1212" => xai::models::grok_2(),
        // Mistral
        "mistral-large" | "mistral-large-latest" => mistral::models::mistral_large(),
        "mistral-small" | "mistral-small-latest" => mistral::models::mistral_small(),
        "codestral" | "codestral-latest" => mistral::models::codestral(),
        "pixtral-large" | "pixtral-large-latest" => mistral::models::pixtral_large(),
        // Groq
        "llama-3.3-70b-versatile" => groq::models::llama_3_3_70b(),
        "llama-3.1-8b-instant" => groq::models::llama_3_1_8b(),
        "mixtral-8x7b-32768" => groq::models::mixtral_8x7b(),
        "gemma2-9b-it" => groq::models::gemma_2_9b(),
        "deepseek-r1-distill-llama-70b" => groq::models::deepseek_r1_distill(),
        // DeepInfra
        "deepseek-ai/DeepSeek-V3" if provider == "deepinfra" => deepinfra::models::deepseek_v3(),
        "deepseek-ai/DeepSeek-R1" if provider == "deepinfra" => deepinfra::models::deepseek_r1(),
        "Qwen/Qwen2.5-72B-Instruct" => deepinfra::models::qwen_2_5_72b(),
        "meta-llama/Meta-Llama-3.1-405B-Instruct" => deepinfra::models::llama_3_1_405b(),
        // Together
        "deepseek-ai/DeepSeek-V3" if provider == "together" => together::models::deepseek_v3(),
        "deepseek-ai/DeepSeek-R1" if provider == "together" => together::models::deepseek_r1(),
        "meta-llama/Llama-3.3-70B-Instruct-Turbo" => together::models::llama_3_3_70b(),
        "Qwen/Qwen2.5-72B-Instruct-Turbo" => together::models::qwen_2_5_72b(),
        "Qwen/Qwen2.5-Coder-32B-Instruct" => together::models::qwen_2_5_coder(),
        // CompoundCoder
        "wonop/gpt" => compoundcoder::models::wonop_gpt(),
        "wonop/qwen" => compoundcoder::models::wonop_qwen(),
        "wonop/devstral2" => compoundcoder::models::wonop_devstral2(),
        // Test provider
        "test-128b" => wonopcode_provider::test::TestProvider::test_128b(),
        _ => ModelInfo::new(model_id, provider).with_name(model_id),
    }
}

/// Infer the provider from a model name.
fn infer_provider_from_model(model: &str) -> Option<&'static str> {
    let model_lower = model.to_lowercase();

    // OpenAI models
    if model_lower.starts_with("gpt-")
        || model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("chatgpt")
    {
        return Some("openai");
    }

    // Anthropic models
    if model_lower.starts_with("claude") {
        return Some("anthropic");
    }

    // Google models
    if model_lower.starts_with("gemini") {
        return Some("google");
    }

    // xAI (Grok) models
    if model_lower.starts_with("grok") {
        return Some("xai");
    }

    // Mistral models
    if model_lower.starts_with("mistral")
        || model_lower.starts_with("codestral")
        || model_lower.starts_with("pixtral")
    {
        return Some("mistral");
    }

    // Groq-hosted models (Llama, Mixtral on Groq)
    if model_lower.contains("groq") {
        return Some("groq");
    }

    // Test provider
    if model_lower.starts_with("test-") {
        return Some("test");
    }

    None
}

/// Build system prompt with environment context.
fn build_system_prompt_for_session(provider: &str, model: &str, cwd: &Path) -> String {
    // Detect if git repo
    let is_git_repo = cwd.join(".git").exists();

    // Get platform
    let platform = std::env::consts::OS;

    // Generate file tree (limited to top-level for now)
    let file_tree = generate_file_tree(cwd, 2, 20);

    // Load custom instructions from AGENTS.md, CLAUDE.md, etc.
    let custom_instructions = load_custom_instructions(cwd);

    // Generate environment context
    let environment =
        system_prompt::environment_context(cwd, is_git_repo, platform, file_tree.as_deref());

    // Build full prompt
    system_prompt::build_system_prompt(
        provider,
        model,
        None, // agent_prompt - will be added for subagents
        custom_instructions.as_deref(),
        &environment,
    )
}

/// Generate a simple file tree for the environment context.
fn generate_file_tree(dir: &Path, max_depth: usize, max_files: usize) -> Option<String> {
    let mut entries = Vec::new();
    let mut count = 0;

    fn collect_entries(
        dir: &Path,
        prefix: &str,
        depth: usize,
        max_depth: usize,
        entries: &mut Vec<String>,
        count: &mut usize,
        max_files: usize,
    ) {
        if depth > max_depth || *count >= max_files {
            return;
        }

        let Ok(read_dir) = std::fs::read_dir(dir) else {
            return;
        };

        let mut items: Vec<_> = read_dir
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name();
                let name_str = name.to_string_lossy();
                // Skip hidden files and common noise directories
                !name_str.starts_with('.')
                    && name_str != "node_modules"
                    && name_str != "target"
                    && name_str != "__pycache__"
                    && name_str != "venv"
                    && name_str != ".git"
            })
            .collect();

        items.sort_by_key(|e| e.file_name());

        for entry in items {
            if *count >= max_files {
                entries.push(format!("{prefix}..."));
                break;
            }

            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);

            if is_dir {
                entries.push(format!("{prefix}{name_str}/"));
                *count += 1;
                collect_entries(
                    &entry.path(),
                    &format!("{prefix}  "),
                    depth + 1,
                    max_depth,
                    entries,
                    count,
                    max_files,
                );
            } else {
                entries.push(format!("{prefix}{name_str}"));
                *count += 1;
            }
        }
    }

    collect_entries(dir, "", 0, max_depth, &mut entries, &mut count, max_files);

    if entries.is_empty() {
        None
    } else {
        Some(entries.join("\n"))
    }
}

/// Load custom instructions from common instruction files.
fn load_custom_instructions(cwd: &Path) -> Option<String> {
    // Look for custom instruction files in order of priority
    let instruction_files = [
        ".wonopcode/AGENTS.md",
        "AGENTS.md",
        ".claude/CLAUDE.md",
        "CLAUDE.md",
        ".wonopcode/instructions.md",
        ".cursor/rules",
    ];

    let mut instructions = Vec::new();

    for file in &instruction_files {
        let path = cwd.join(file);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if !content.trim().is_empty() {
                    instructions.push(format!(
                        "# Instructions from {}\n\n{}",
                        file,
                        content.trim()
                    ));
                }
            }
        }
    }

    if instructions.is_empty() {
        None
    } else {
        Some(instructions.join("\n\n"))
    }
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
    let env_var = match provider {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "google" => "GOOGLE_API_KEY",
        "xai" => "XAI_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        "groq" => "GROQ_API_KEY",
        "deepinfra" => "DEEPINFRA_API_KEY",
        "together" => "TOGETHER_API_KEY",
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

    let creds_manager = wonopcode_core::CredentialsManager::new();
    let has_api_key = creds_manager
        .as_ref()
        .map(|cm| cm.get_api_key(provider).is_some())
        .unwrap_or(false);
    let auth_method = creds_manager.as_ref().and_then(|cm| cm.get_auth_method(provider));

    // Check CLI availability for anthropic
    let (cli_available, cli_authenticated) = if provider == "anthropic" {
        (
            ClaudeCliProvider::is_available(),
            ClaudeCliProvider::is_authenticated(),
        )
    } else {
        (false, false)
    };

    // Provider is available if it has an API key OR (for anthropic) has CLI auth
    let available = has_api_key
        || (provider == "anthropic" && cli_authenticated)
        || auth_method == Some(wonopcode_core::AuthMethod::ClaudeCli);

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
        "anthropic" => "Anthropic".to_string(),
        "openai" => "OpenAI".to_string(),
        "openrouter" => "OpenRouter".to_string(),
        "google" => "Google".to_string(),
        "xai" => "xAI".to_string(),
        "mistral" => "Mistral".to_string(),
        "groq" => "Groq".to_string(),
        "deepinfra" => "DeepInfra".to_string(),
        "together" => "Together".to_string(),
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
