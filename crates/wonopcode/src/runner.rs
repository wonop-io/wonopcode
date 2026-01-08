//! Runner module - connects the TUI to the AI prompt loop.

use futures::future::join_all;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use wonopcode_core::bus::{Bus, PermissionRequest as BusPermissionRequest, SandboxState, SandboxStatusChanged};
use wonopcode_core::config::{McpConfig, McpLocalConfig, SandboxConfig as CoreSandboxConfig};
use wonopcode_core::permission::{Decision, PermissionCheck, PermissionManager};
use wonopcode_core::system_prompt;
use wonopcode_core::Instance;
use wonopcode_mcp::{McpClient, ServerConfig as McpServerConfig};
use wonopcode_provider::{
    anthropic::AnthropicProvider,
    claude_cli::ClaudeCliProvider,
    google::GoogleProvider,
    model::ModelInfo,
    openai::OpenAIProvider,
    openrouter::OpenRouterProvider,
    stream::{FinishReason, StreamChunk},
    BoxedLanguageModel, GenerateOptions, Message as ProviderMessage, ToolDefinition,
};
use wonopcode_sandbox::{SandboxConfig, SandboxManager, SandboxRuntime, SandboxRuntimeType};
use wonopcode_snapshot::{SnapshotConfig, SnapshotStore};
use wonopcode_tools::{mcp::McpToolsBuilder, task, todo, ToolRegistry};
use wonopcode_tui::{
    AppAction, AppUpdate, LspStatusUpdate, McpStatusUpdate, ModifiedFileUpdate,
    PermissionRequestUpdate, SaveScope, TodoUpdate,
};
use wonopcode_util::perf;
use wonopcode_util::FileTimeState;

use crate::compaction::{self, CompactionConfig, CompactionResult};

/// Doom loop detection threshold - number of consecutive identical tool calls to trigger detection.
const DOOM_LOOP_THRESHOLD: usize = 3;

/// Maximum tool calls to keep in doom loop detector before pruning old ones.
const DOOM_LOOP_MAX_RECORDS: usize = 100;

/// Maximum messages before triggering automatic compaction (regardless of token count).
const AUTO_COMPACT_MESSAGE_THRESHOLD: usize = 100;

/// Target message count after automatic compaction.
const AUTO_COMPACT_TARGET_MESSAGES: usize = 50;

/// Represents a tool call for doom loop tracking.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolCallRecord {
    /// Tool name.
    name: String,
    /// JSON-serialized arguments (for comparison).
    args_json: String,
}

impl ToolCallRecord {
    fn new(name: &str, args: &serde_json::Value) -> Self {
        Self {
            name: name.to_string(),
            args_json: serde_json::to_string(args).unwrap_or_default(),
        }
    }
}

/// Doom loop detector tracks recent tool calls and detects repetitive patterns.
#[derive(Debug, Default)]
struct DoomLoopDetector {
    /// Recent tool calls within the current prompt run.
    recent_calls: Vec<ToolCallRecord>,
}

impl DoomLoopDetector {
    /// Create a new doom loop detector.
    fn new() -> Self {
        Self {
            recent_calls: Vec::new(),
        }
    }

    /// Reset the detector (e.g., at the start of a new prompt).
    fn reset(&mut self) {
        self.recent_calls.clear();
    }

    /// Record a tool call and check if it triggers doom loop detection.
    /// Returns true if a doom loop is detected.
    fn record_and_check(&mut self, name: &str, args: &serde_json::Value) -> bool {
        let record = ToolCallRecord::new(name, args);
        self.recent_calls.push(record.clone());

        // Prune old records to prevent unbounded growth
        if self.recent_calls.len() > DOOM_LOOP_MAX_RECORDS {
            let drain_count = self.recent_calls.len() - DOOM_LOOP_MAX_RECORDS / 2;
            self.recent_calls.drain(0..drain_count);
        }

        // Check if the last N calls are identical
        if self.recent_calls.len() >= DOOM_LOOP_THRESHOLD {
            let last_n = &self.recent_calls[self.recent_calls.len() - DOOM_LOOP_THRESHOLD..];
            if last_n.iter().all(|r| *r == record) {
                return true;
            }
        }

        false
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
    /// MCP HTTP URL for headless mode.
    /// When set, the Claude CLI provider will connect to this URL instead of
    /// spawning a child process for MCP tools.
    pub mcp_url: Option<String>,
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
            mcp_url: None,
        }
    }
}

/// The runner connects the TUI to the AI.
pub struct Runner {
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
    /// Doom loop detector for preventing infinite tool call loops.
    doom_loop_detector: RwLock<DoomLoopDetector>,
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
}

impl Runner {
    /// Create a new runner.
    pub fn new(
        config: RunnerConfig,
        instance: Instance,
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
        tools.register(Arc::new(todo::TodoWriteTool::new(todo_store.clone())));
        tools.register(Arc::new(todo::TodoReadTool::new(todo_store.clone())));
        tools.register(Arc::new(wonopcode_tools::lsp::LspTool::with_client(
            lsp_client.clone(),
        )));
        tools.register(Arc::new(wonopcode_tools::task::TaskTool::new()));
        tools.register(Arc::new(
            wonopcode_tools::plan_mode::EnterPlanModeTool::new(),
        ));
        tools.register(Arc::new(wonopcode_tools::plan_mode::ExitPlanModeTool::new()));
        // Note: skill and batch tools require async initialization, done in new_with_features

        // Create event bus and permission manager
        let bus = Bus::new();
        let permission_manager = Arc::new(PermissionManager::new(bus.clone()));

        // Create file time tracker
        let file_time = Arc::new(FileTimeState::new());

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            instance,
            provider: Arc::new(RwLock::new(provider)),
            tools: Arc::new(tools),
            cancel: Arc::new(RwLock::new(CancellationToken::new())),
            history: RwLock::new(Vec::new()),
            compaction_config: CompactionConfig::default(),
            snapshot_store: None, // Will be initialized async in new_with_features
            mcp_client: None,     // Will be initialized async if configured
            doom_loop_detector: RwLock::new(DoomLoopDetector::new()),
            permission_manager,
            bus,
            file_time,
            sandbox_manager: None, // Will be initialized async in new_with_features
            todo_store,
            lsp_client,
        })
    }

    /// Create a new runner with snapshot support.
    pub async fn new_with_snapshots(
        config: RunnerConfig,
        instance: Instance,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_features(config, instance, None).await
    }

    /// Create a new runner with full feature support (snapshots, MCP, skills).
    pub async fn new_with_features(
        config: RunnerConfig,
        instance: Instance,
        mcp_configs: Option<HashMap<String, McpConfig>>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut runner = Self::new(config, instance)?;

        // Initialize default permission rules
        for rule in PermissionManager::default_rules() {
            runner.permission_manager.add_rule(rule).await;
        }
        info!("Permission manager initialized with default rules");

        // Initialize snapshot store with proper directory
        let cwd = runner.instance.directory();
        let snapshot_dir = cwd.join(".wonopcode").join("snapshots");

        match SnapshotStore::new(snapshot_dir, cwd.to_path_buf(), SnapshotConfig::default()).await {
            Ok(store) => {
                info!("Snapshot store initialized");
                runner.snapshot_store = Some(Arc::new(store));
            }
            Err(e) => {
                warn!("Failed to initialize snapshot store: {}", e);
            }
        }

        // Initialize sandbox manager if configured (using lazy detection for faster startup)
        let core_config = runner.instance.config().await;
        info!(sandbox_config = ?core_config.sandbox, "Checking sandbox configuration");
        if let Some(sandbox_cfg) = &core_config.sandbox {
            info!(enabled = ?sandbox_cfg.enabled, runtime = ?sandbox_cfg.runtime, "Sandbox config found");
            if sandbox_cfg.enabled.unwrap_or(false) {
                let sandbox_config = convert_sandbox_config(sandbox_cfg);
                // Use lazy initialization to avoid blocking startup with runtime detection
                let manager = SandboxManager::new_lazy(sandbox_config, cwd.to_path_buf());

                // Check availability asynchronously in the background
                let runtime_type = manager.runtime_type();
                if let Some(rt) = runtime_type {
                    // Runtime was explicitly configured (not Auto)
                    if !matches!(rt, SandboxRuntimeType::None) {
                        info!(runtime = ?rt, "Sandbox manager initialized");
                        runner
                            .bus
                            .publish(SandboxStatusChanged {
                                state: SandboxState::Stopped,
                                runtime_type: Some(format!("{:?}", rt)),
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
                    info!("Sandbox manager initialized with lazy runtime detection");
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

        // Apply sandbox permission rules if sandbox is enabled and allow_all_in_sandbox is true
        if runner.sandbox_manager.is_some() {
            let allow_all = core_config
                .permission
                .as_ref()
                .and_then(|p| p.allow_all_in_sandbox)
                .unwrap_or(true); // Default to true for sandbox

            if allow_all {
                runner.permission_manager.apply_sandbox_rules().await;
                info!("Applied sandbox permission rules (allow_all_in_sandbox=true)");
            } else {
                info!("Sandbox enabled but allow_all_in_sandbox=false, write operations will prompt");
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
                        info!(
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

        Ok(runner)
    }

    /// Initialize MCP client and connect to configured servers.
    async fn initialize_mcp(&mut self, configs: HashMap<String, McpConfig>) {
        let mcp_client = Arc::new(McpClient::new());

        // Collect enabled server configs for parallel connection
        let mut server_configs: Vec<(String, McpServerConfig)> = Vec::new();

        for (name, config) in configs {
            match &config {
                McpConfig::Local(local_config) => {
                    // Check if enabled (default true)
                    if local_config.enabled == Some(false) {
                        debug!(server = %name, "MCP server disabled, skipping");
                        continue;
                    }
                    // Convert to McpServerConfig
                    let server_config = convert_mcp_config(&name, local_config);
                    server_configs.push((name, server_config));
                }
                McpConfig::Remote(remote_config) => {
                    // Check if enabled
                    if remote_config.enabled == Some(false) {
                        debug!(server = %name, "MCP server disabled, skipping");
                        continue;
                    }
                    // Remote (SSE) not yet supported
                    warn!(server = %name, "Remote MCP servers not yet supported");
                }
            }
        }

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
                    info!(server = %name, "MCP server connected");
                    connected_servers += 1;
                }
                Err(e) => {
                    warn!(server = %name, error = %e, "Failed to connect MCP server");
                }
            }
        }

        if connected_servers > 0 {
            // Register MCP tools
            let builder = McpToolsBuilder::new(mcp_client.clone());
            let mcp_tools = builder.build_all().await;

            info!(
                servers = connected_servers,
                tools = mcp_tools.len(),
                "MCP initialized"
            );

            // We need a mutable tools registry - create a new one with MCP tools
            let mut new_tools = ToolRegistry::with_builtins();
            new_tools.register(Arc::new(wonopcode_tools::bash::BashTool));
            new_tools.register(Arc::new(wonopcode_tools::webfetch::WebFetchTool));
            new_tools.register(Arc::new(todo::TodoWriteTool::new(self.todo_store.clone())));
            new_tools.register(Arc::new(todo::TodoReadTool::new(self.todo_store.clone())));
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

    /// Send LSP status updates to the UI.
    async fn send_lsp_status(&self, update_tx: &mpsc::UnboundedSender<AppUpdate>) {
        let servers = self.lsp_client.status().await;
        if !servers.is_empty() {
            let lsp_updates: Vec<LspStatusUpdate> = servers
                .iter()
                .map(|s| LspStatusUpdate {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    root: s.root.clone(),
                    connected: s.status == wonopcode_lsp::LspServerStatus::Connected,
                })
                .collect();
            let _ = update_tx.send(AppUpdate::LspUpdated(lsp_updates));
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
                return Err(format!("No API key found for provider '{}'. Set the environment variable or run 'wonopcode auth login {}'", provider_name, provider_name).into());
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
                mcp_url: old_config.mcp_url.clone(),
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

        let new_provider = create_provider(&new_config, sandbox_enabled, allow_all_for_mcp)?;

        // Update config and provider
        {
            let mut config = self.config.write().await;
            *config = new_config;
        }
        {
            let mut provider = self.provider.write().await;
            *provider = new_provider;
        }

        info!(provider = %provider_name, model = %model_id, "Model changed successfully");
        Ok(())
    }

    /// Run the action handler loop.
    pub async fn run(
        self,
        mut action_rx: mpsc::UnboundedReceiver<AppAction>,
        update_tx: mpsc::UnboundedSender<AppUpdate>,
    ) {
        let cwd = self.instance.directory().to_path_buf();

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
                        description: format!("{:?}", req.details),
                        path: None, // Could extract from details if needed
                    },
                ));
            }
        });

        // Send initial model info
        {
            let provider = self.provider.read().await;
            let model_info = provider.model_info();
            let _ = update_tx.send(AppUpdate::ModelInfo {
                context_limit: model_info.limit.context,
            });
        }

        // Send initial MCP status
        if let Some(ref mcp_client) = self.mcp_client {
            let mcp_updates: Vec<McpStatusUpdate> = mcp_client
                .list_servers()
                .await
                .into_iter()
                .map(|(name, connected, error)| McpStatusUpdate {
                    name,
                    connected,
                    error,
                })
                .collect();

            if !mcp_updates.is_empty() {
                let _ = update_tx.send(AppUpdate::McpUpdated(mcp_updates));
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
                    (
                        wonopcode_tui::SandboxStatusUpdate {
                            state: "running".to_string(),
                            runtime_type: Some(runtime_type),
                            error: None,
                        },
                        Some(format!(
                            "⬡ Sandbox active ({}) - commands execute in isolated container",
                            runtime_lower
                        )),
                    )
                } else {
                    (
                        wonopcode_tui::SandboxStatusUpdate {
                            state: "stopped".to_string(),
                            runtime_type: Some(runtime_type),
                            error: None,
                        },
                        Some(format!(
                            "⬡ Sandbox available ({}) - use /sandbox start to enable isolation",
                            runtime_lower
                        )),
                    )
                }
            } else {
                (
                    wonopcode_tui::SandboxStatusUpdate {
                        state: "disabled".to_string(),
                        runtime_type: None,
                        error: None,
                    },
                    None, // Don't show message when sandbox is completely disabled
                )
            };
            let _ = update_tx.send(AppUpdate::SandboxUpdated(status));
            if let Some(msg) = system_msg {
                let _ = update_tx.send(AppUpdate::SystemMessage(msg));
            }
        }

        while let Some(action) = action_rx.recv().await {
            match action {
                AppAction::SendPrompt(text) => {
                    debug!(prompt_text = %text, "Received SendPrompt action");
                    // Reset cancellation token for new prompt
                    self.reset_cancel_token().await;

                    // Send started update
                    let _ = update_tx.send(AppUpdate::Started);

                    // Run the prompt with concurrent cancellation handling
                    info!(prompt_len = text.len(), "Running prompt");

                    // Get a clone of the cancel token for checking
                    let cancel_token = self.get_cancel_token().await;

                    // Use a loop to process Cancel actions while the prompt runs
                    let prompt_future = self.run_prompt(&text, &cwd, &update_tx);
                    tokio::pin!(prompt_future);

                    let result = loop {
                        tokio::select! {
                            biased;

                            // Check for incoming actions (especially Cancel)
                            Some(inner_action) = action_rx.recv() => {
                                match inner_action {
                                    AppAction::Cancel => {
                                        info!("Cancelling current operation");
                                        cancel_token.cancel();
                                        // Don't break - let the prompt handle the cancellation
                                    }
                                    AppAction::Quit => {
                                        info!("Quit requested during prompt");
                                        cancel_token.cancel();
                                        // Return after prompt finishes
                                    }
                                    _ => {
                                        // Ignore other actions during prompt execution
                                        debug!("Ignoring action during prompt execution: {:?}", inner_action);
                                    }
                                }
                            }

                            // Wait for prompt to complete
                            res = &mut prompt_future => {
                                break res;
                            }
                        }
                    };

                    match result {
                        Ok(result_text) => {
                            info!(
                                result_len = result_text.len(),
                                "Prompt completed successfully"
                            );
                            let _ = update_tx.send(AppUpdate::Completed { text: result_text });

                            // Sync todos to TUI
                            self.sync_todos_to_tui(&cwd, &update_tx);
                        }
                        Err(e) => {
                            let err_str = e.to_string();
                            if err_str.contains("Cancelled") {
                                info!("Prompt was cancelled");
                                let _ = update_tx.send(AppUpdate::Error("Cancelled".to_string()));
                            } else {
                                error!("Prompt error: {}", e);
                                let _ = update_tx.send(AppUpdate::Error(err_str));
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
                    info!(session_id = %session_id, "Switching session");
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
                            let _ = update_tx.send(AppUpdate::Status(format!(
                                "Model changed to {}",
                                model_spec
                            )));
                        }
                        Err(e) => {
                            error!("Failed to change model: {}", e);
                            let _ = update_tx
                                .send(AppUpdate::Error(format!("Failed to change model: {}", e)));
                        }
                    }
                }
                AppAction::ChangeAgent(agent_name) => {
                    info!(agent = %agent_name, "Changing agent");
                    // Agent change is mostly a TUI concern for now
                    // Future: could change tool permissions, system prompt, etc.
                    let _ = update_tx.send(AppUpdate::Status(format!(
                        "Agent changed to {}",
                        agent_name
                    )));
                }
                AppAction::NewSession => {
                    info!("Creating new session");
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
                    info!("Undo requested");
                    // For now, history sync is handled by the TUI
                    // Future: could sync with runner's history
                }
                AppAction::Redo => {
                    info!("Redo requested");
                    // For now, history sync is handled by the TUI
                    // Future: could sync with runner's history
                }
                AppAction::Revert { message_id } => {
                    info!(message_id = %message_id, "Revert requested");
                    let _ = update_tx.send(AppUpdate::Status(format!(
                        "Reverting to message {}...",
                        message_id
                    )));

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
                                update_tx.send(AppUpdate::Status(format!("Revert failed: {}", e)));
                        }
                    }
                }
                AppAction::Unrevert => {
                    info!("Unrevert requested");

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
                            let _ = update_tx
                                .send(AppUpdate::Status(format!("Unrevert failed: {}", e)));
                        }
                    }
                }
                AppAction::Compact => {
                    info!("Compact requested");
                    let _ =
                        update_tx.send(AppUpdate::Status("Compacting conversation...".to_string()));

                    // Get current messages
                    let mut messages: Vec<ProviderMessage> = {
                        let history = self.history.read().await;
                        history.clone()
                    };

                    if messages.len() < 4 {
                        let _ = update_tx.send(AppUpdate::Status(
                            "Not enough messages to compact".to_string(),
                        ));
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
                            info!(
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
                                format!("Compacted {} messages", messages_summarized)
                            } else {
                                "Pruned old tool outputs".to_string()
                            };
                            let _ = update_tx.send(AppUpdate::Status(status));
                        }
                        CompactionResult::NotNeeded => {
                            let _ = update_tx
                                .send(AppUpdate::Status("Compaction not needed".to_string()));
                        }
                        CompactionResult::InsufficientMessages => {
                            let _ = update_tx.send(AppUpdate::Status(
                                "Not enough messages to compact".to_string(),
                            ));
                        }
                        CompactionResult::Failed(err) => {
                            warn!(error = %err, "Compaction failed");
                            let _ = update_tx
                                .send(AppUpdate::Error(format!("Compaction failed: {}", err)));
                        }
                    }
                }
                AppAction::RenameSession { title } => {
                    info!(title = %title, "Rename session requested");
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
                        update_tx.send(AppUpdate::Status(format!("Session renamed to: {}", title)));
                }
                AppAction::McpToggle { name } => {
                    info!(server = %name, "MCP toggle requested");
                    if let Some(ref mcp_client) = self.mcp_client {
                        match mcp_client.toggle_server(&name).await {
                            Ok(enabled) => {
                                let status = if enabled { "enabled" } else { "disabled" };
                                let _ = update_tx.send(AppUpdate::Status(format!(
                                    "MCP server '{}' {}",
                                    name, status
                                )));

                                // Send updated MCP status
                                let mcp_updates: Vec<McpStatusUpdate> = mcp_client
                                    .list_servers()
                                    .await
                                    .into_iter()
                                    .map(|(name, connected, error)| McpStatusUpdate {
                                        name,
                                        connected,
                                        error,
                                    })
                                    .collect();
                                let _ = update_tx.send(AppUpdate::McpUpdated(mcp_updates));
                            }
                            Err(e) => {
                                warn!(server = %name, error = %e, "Failed to toggle MCP server");
                                let _ = update_tx.send(AppUpdate::Error(format!(
                                    "Failed to toggle '{}': {}",
                                    name, e
                                )));
                            }
                        }
                    } else {
                        let _ = update_tx
                            .send(AppUpdate::Error("No MCP client configured".to_string()));
                    }
                }
                AppAction::McpReconnect { name } => {
                    info!(server = %name, "MCP reconnect requested");
                    if let Some(ref mcp_client) = self.mcp_client {
                        let _ = update_tx
                            .send(AppUpdate::Status(format!("Reconnecting to '{}'...", name)));

                        match mcp_client.reconnect_server(&name).await {
                            Ok(()) => {
                                let _ = update_tx
                                    .send(AppUpdate::Status(format!("Reconnected to '{}'", name)));

                                // Send updated MCP status
                                let mcp_updates: Vec<McpStatusUpdate> = mcp_client
                                    .list_servers()
                                    .await
                                    .into_iter()
                                    .map(|(name, connected, error)| McpStatusUpdate {
                                        name,
                                        connected,
                                        error,
                                    })
                                    .collect();
                                let _ = update_tx.send(AppUpdate::McpUpdated(mcp_updates));
                            }
                            Err(e) => {
                                warn!(server = %name, error = %e, "Failed to reconnect MCP server");
                                let _ = update_tx.send(AppUpdate::Error(format!(
                                    "Failed to reconnect '{}': {}",
                                    name, e
                                )));
                            }
                        }
                    } else {
                        let _ = update_tx
                            .send(AppUpdate::Error("No MCP client configured".to_string()));
                    }
                }
                AppAction::ForkSession { message_id } => {
                    info!(message_id = ?message_id, "Fork session requested");
                    let project_id = self.instance.project_id().await;
                    let session_repo = self.instance.session_repo();

                    // Fork the current session
                    match session_repo
                        .fork(&project_id, "default", message_id.as_deref())
                        .await
                    {
                        Ok(forked) => {
                            info!(forked_id = %forked.id, "Session forked successfully");
                            // Clear runner's history for the new session
                            {
                                let mut history = self.history.write().await;
                                history.clear();
                            }
                            let _ = update_tx.send(AppUpdate::Status(format!(
                                "Forked to new session: {}",
                                forked.title
                            )));
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to fork session");
                            let _ = update_tx.send(AppUpdate::Error(format!("Fork failed: {}", e)));
                        }
                    }
                }
                AppAction::ShareSession => {
                    info!("Share session requested");
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
                            info!(url = %share_info.url, "Session shared successfully");
                            let _ = update_tx
                                .send(AppUpdate::Status(format!("Shared at: {}", share_info.url)));
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to share session");
                            let _ =
                                update_tx.send(AppUpdate::Error(format!("Share failed: {}", e)));
                        }
                    }
                }
                AppAction::UnshareSession => {
                    info!("Unshare session requested");
                    // Unsharing requires the share secret which we don't store in the TUI
                    // This would need to be retrieved from session metadata
                    let _ = update_tx.send(AppUpdate::Status(
                        "Unshare requires share secret - use CLI: wonopcode unshare".to_string(),
                    ));
                }
                AppAction::GotoMessage { message_id } => {
                    info!(message_id = %message_id, "Go to message requested");
                    // This is handled in the TUI (scroll to message)
                    let _ = update_tx.send(AppUpdate::Status(format!(
                        "Navigated to message {}",
                        message_id
                    )));
                }
                AppAction::SandboxStart => {
                    info!("Sandbox start requested");
                    self.handle_sandbox_start(&update_tx).await;
                }
                AppAction::SandboxStop => {
                    info!("Sandbox stop requested");
                    self.handle_sandbox_stop(&update_tx).await;
                }
                AppAction::SandboxRestart => {
                    info!("Sandbox restart requested");
                    self.handle_sandbox_stop(&update_tx).await;
                    self.handle_sandbox_start(&update_tx).await;
                }
                AppAction::SaveSettings { scope, config } => {
                    info!("Saving settings to {:?}", scope);
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
                            let _ = update_tx.send(AppUpdate::SystemMessage(format!(
                                "Settings saved to {}",
                                location
                            )));
                        }
                        Err(e) => {
                            error!("Failed to save settings: {}", e);
                            let _ = update_tx
                                .send(AppUpdate::Error(format!("Failed to save settings: {}", e)));
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
        let todos = todo::get_todos(self.todo_store.as_ref(), cwd);
        if !todos.is_empty() {
            let todo_updates: Vec<TodoUpdate> = todos
                .into_iter()
                .map(|t| TodoUpdate {
                    id: t.id,
                    content: t.content,
                    status: match t.status {
                        todo::TodoStatus::Pending => "pending".to_string(),
                        todo::TodoStatus::InProgress => "in_progress".to_string(),
                        todo::TodoStatus::Completed => "completed".to_string(),
                        todo::TodoStatus::Cancelled => "cancelled".to_string(),
                    },
                    priority: match t.priority {
                        todo::TodoPriority::High => "high".to_string(),
                        todo::TodoPriority::Medium => "medium".to_string(),
                        todo::TodoPriority::Low => "low".to_string(),
                    },
                })
                .collect();
            let _ = update_tx.send(AppUpdate::TodosUpdated(todo_updates));
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
        info!(sandbox_manager_present = self.sandbox_manager.is_some(), "Handling sandbox start");
        if let Some(ref manager) = self.sandbox_manager {
            // Send starting status
            let _ = update_tx.send(AppUpdate::SandboxUpdated(
                wonopcode_tui::SandboxStatusUpdate {
                    state: "starting".to_string(),
                    runtime_type: Some(manager.runtime_type_display()),
                    error: None,
                },
            ));

            match manager.start().await {
                Ok(()) => {
                    info!("Sandbox started successfully");
                    self.bus
                        .publish(SandboxStatusChanged {
                            state: SandboxState::Running,
                            runtime_type: Some(manager.runtime_type_display()),
                            error: None,
                        })
                        .await;
                    let _ = update_tx.send(AppUpdate::SandboxUpdated(
                        wonopcode_tui::SandboxStatusUpdate {
                            state: "running".to_string(),
                            runtime_type: Some(manager.runtime_type_display()),
                            error: None,
                        },
                    ));

                    // Send system message about sandbox starting
                    let runtime = manager.runtime_type_display().to_lowercase();
                    let _ = update_tx.send(AppUpdate::SystemMessage(format!(
                        "⬡ Sandbox started ({}) - commands will execute in isolated container",
                        runtime
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
                    let _ = update_tx.send(AppUpdate::SandboxUpdated(
                        wonopcode_tui::SandboxStatusUpdate {
                            state: "error".to_string(),
                            runtime_type: Some(manager.runtime_type_display()),
                            error: Some(e.to_string()),
                        },
                    ));
                }
            }
        } else {
            let _ = update_tx.send(AppUpdate::SandboxUpdated(
                wonopcode_tui::SandboxStatusUpdate {
                    state: "disabled".to_string(),
                    runtime_type: None,
                    error: Some("Sandbox not configured".to_string()),
                },
            ));
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

            // Using Claude CLI - recreate provider with new sandbox state
            debug!(
                sandbox_enabled = sandbox_enabled,
                allow_all_for_mcp = allow_all_for_mcp,
                "Recreating Claude CLI provider after sandbox state change"
            );

            match create_provider(&config, Some(sandbox_enabled), allow_all_for_mcp) {
                Ok(new_provider) => {
                    drop(config); // Release read lock before acquiring write lock
                    let mut provider = self.provider.write().await;
                    *provider = new_provider;
                    info!(
                        sandbox_enabled = sandbox_enabled,
                        allow_all_for_mcp = allow_all_for_mcp,
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
                    self.bus
                        .publish(SandboxStatusChanged {
                            state: SandboxState::Stopped,
                            runtime_type: Some(manager.runtime_type_display()),
                            error: None,
                        })
                        .await;
                    let _ = update_tx.send(AppUpdate::SandboxUpdated(
                        wonopcode_tui::SandboxStatusUpdate {
                            state: "stopped".to_string(),
                            runtime_type: Some(manager.runtime_type_display()),
                            error: None,
                        },
                    ));

                    // Send system message about sandbox stopping
                    let _ = update_tx.send(AppUpdate::SystemMessage(
                        "◇ Sandbox stopped - commands will execute directly on host".to_string(),
                    ));

                    // Recreate provider with sandbox disabled so MCP server doesn't use sandbox
                    self.recreate_provider_with_sandbox(false).await;
                }
                Err(e) => {
                    warn!(error = %e, "Failed to stop sandbox");
                    let _ = update_tx.send(AppUpdate::SandboxUpdated(
                        wonopcode_tui::SandboxStatusUpdate {
                            state: "error".to_string(),
                            runtime_type: Some(manager.runtime_type_display()),
                            error: Some(e.to_string()),
                        },
                    ));
                }
            }
        } else {
            let _ = update_tx.send(AppUpdate::SandboxUpdated(
                wonopcode_tui::SandboxStatusUpdate {
                    state: "disabled".to_string(),
                    runtime_type: None,
                    error: Some("Sandbox not configured".to_string()),
                },
            ));
        }
    }

    /// Run a single prompt and return the response.
    async fn run_prompt(
        &self,
        user_input: &str,
        cwd: &Path,
        update_tx: &mpsc::UnboundedSender<AppUpdate>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        use futures::StreamExt;

        // Get the cancellation token for this prompt
        let cancel = self.get_cancel_token().await;

        // Reset doom loop detector for this prompt
        {
            let mut detector = self.doom_loop_detector.write().await;
            detector.reset();
        }

        // Get existing history and check context limit
        let mut messages: Vec<ProviderMessage> = {
            let history = self.history.read().await;
            history.clone()
        };

        // Log message history size for performance monitoring
        let history_size: usize = messages
            .iter()
            .map(|m| {
                m.content
                    .iter()
                    .map(|p| match p {
                        wonopcode_provider::ContentPart::Text { text } => text.len(),
                        wonopcode_provider::ContentPart::ToolUse { input, .. } => {
                            input.to_string().len()
                        }
                        wonopcode_provider::ContentPart::ToolResult { content, .. } => {
                            content.len()
                        }
                        wonopcode_provider::ContentPart::Thinking { text } => text.len(),
                        wonopcode_provider::ContentPart::Image { .. } => 1000,
                    })
                    .sum::<usize>()
            })
            .sum();
        perf::log_message_history("prompt_start", messages.len(), history_size);

        // Get context limit from model
        let context_limit = {
            let provider = self.provider.read().await;
            provider.model_info().limit.context
        };

        // Check if message-count-based compaction is needed (regardless of token count)
        if messages.len() > AUTO_COMPACT_MESSAGE_THRESHOLD {
            info!(
                messages = messages.len(),
                threshold = AUTO_COMPACT_MESSAGE_THRESHOLD,
                "Message count exceeds threshold, triggering automatic compaction"
            );
            let _ = update_tx.send(AppUpdate::Status(format!(
                "Auto-compacting {} messages...",
                messages.len()
            )));

            let compact_start = Instant::now();
            let messages_before = messages.len();

            // Estimate token usage for compaction
            let estimated_tokens = compaction::estimate_token_usage(&messages);

            // Perform compaction
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
                    perf::log_compaction(messages_before, new_messages.len(), duration);

                    messages = new_messages;

                    // Update history
                    {
                        let mut history = self.history.write().await;
                        *history = messages.clone();
                    }

                    let _ = update_tx.send(AppUpdate::Status(format!(
                        "Auto-compacted {} → {} messages",
                        messages_before,
                        messages.len()
                    )));
                }
                CompactionResult::NotNeeded | CompactionResult::InsufficientMessages => {
                    // If AI compaction not possible, do simple truncation
                    if messages.len() > AUTO_COMPACT_TARGET_MESSAGES {
                        let keep_first = 1;
                        let keep_recent = AUTO_COMPACT_TARGET_MESSAGES - keep_first - 1;
                        let first = messages[0].clone();
                        let recent: Vec<_> = messages
                            .drain(messages.len().saturating_sub(keep_recent)..)
                            .collect();
                        let dropped_count = messages.len() - 1;
                        messages.clear();
                        messages.push(first);
                        messages.push(ProviderMessage::assistant(format!(
                            "[Context auto-compacted: {} earlier messages truncated to prevent memory growth]",
                            dropped_count
                        )));
                        messages.extend(recent);

                        perf::log_compaction(
                            messages_before,
                            messages.len(),
                            compact_start.elapsed(),
                        );

                        {
                            let mut history = self.history.write().await;
                            *history = messages.clone();
                        }

                        let _ = update_tx.send(AppUpdate::Status(format!(
                            "Truncated {} → {} messages",
                            messages_before,
                            messages.len()
                        )));
                    }
                }
                CompactionResult::Failed(err) => {
                    warn!(error = %err, "Auto-compaction failed");
                }
            }
        }

        // Check if compaction is needed
        if compaction::needs_compaction(&messages, context_limit, &self.compaction_config) {
            info!(
                messages = messages.len(),
                context_limit = context_limit,
                "Context approaching limit, attempting smart compaction"
            );
            let _ = update_tx.send(AppUpdate::Status("Compacting conversation...".to_string()));

            // Estimate token usage for compaction decision
            let estimated_tokens = compaction::estimate_token_usage(&messages);

            // Perform full compaction: prune first, then summarize if still needed
            let provider = self.provider.read().await;
            match compaction::compact(
                &mut messages,
                &provider,
                &self.compaction_config,
                &estimated_tokens,
                context_limit,
                false, // Don't add auto-continue for pre-prompt compaction
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
                    info!(
                        action = action,
                        messages_summarized = messages_summarized,
                        new_count = new_messages.len(),
                        "Compaction successful"
                    );
                    messages = new_messages;

                    // Update history
                    {
                        let mut history = self.history.write().await;
                        *history = messages.clone();
                    }

                    let status = if messages_summarized > 0 {
                        format!(
                            "Summarized {} messages to save context",
                            messages_summarized
                        )
                    } else {
                        "Pruned old tool outputs to save context".to_string()
                    };
                    let _ = update_tx.send(AppUpdate::Status(status));
                }
                CompactionResult::NotNeeded | CompactionResult::InsufficientMessages => {
                    debug!("Compaction not needed or insufficient messages");
                }
                CompactionResult::Failed(err) => {
                    warn!(
                        "Smart compaction failed: {}, falling back to simple truncation",
                        err
                    );
                    // Fall back to simple truncation
                    let keep_recent = self.compaction_config.preserve_turns * 2; // 2 messages per turn
                    if messages.len() > keep_recent + 1 {
                        let first = messages.remove(0);
                        let recent: Vec<_> = messages
                            .drain(messages.len().saturating_sub(keep_recent)..)
                            .collect();
                        let dropped_count = messages.len();
                        messages.clear();
                        messages.push(first);
                        messages.push(ProviderMessage::assistant(format!(
                            "[Context compacted: {} earlier messages truncated due to context limits]",
                            dropped_count
                        )));
                        messages.extend(recent);

                        {
                            let mut history = self.history.write().await;
                            *history = messages.clone();
                        }
                    }
                }
            }
        }

        let mut final_text = String::new();
        let mut steps = 0;
        const MAX_STEPS: usize = 50;

        // Track total token usage across steps
        let mut total_input: u32 = 0;
        let mut total_output: u32 = 0;

        // Add user message
        let user_msg = ProviderMessage::user(user_input);
        messages.push(user_msg.clone());

        // Store user message in history
        {
            let mut history = self.history.write().await;
            history.push(user_msg);
        }

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

        // Main loop
        loop {
            if cancel.is_cancelled() {
                return Err("Cancelled".into());
            }

            if steps >= MAX_STEPS {
                warn!("Max steps reached");
                break;
            }

            steps += 1;
            debug!(
                step = steps,
                messages_count = messages.len(),
                "Running prompt step"
            );

            // Build options
            let options = {
                let config = self.config.read().await;
                GenerateOptions {
                    temperature: config.temperature,
                    max_tokens: config.max_tokens,
                    system: config.system_prompt.clone().or_else(|| {
                        Some(build_system_prompt_for_session(
                            &config.provider,
                            &config.model_id,
                            cwd,
                        ))
                    }),
                    tools: tool_defs.clone(),
                    abort: Some(cancel.clone()),
                    // Pass test provider settings if available
                    provider_options: config
                        .test_provider_settings
                        .as_ref()
                        .and_then(|s| serde_json::to_value(s).ok()),
                    ..Default::default()
                }
            };

            // Call provider
            debug!(
                step = steps,
                message_count = messages.len(),
                tool_count = tool_defs.len(),
                "Calling provider"
            );

            // Log message summary for debugging
            for (i, msg) in messages.iter().enumerate() {
                debug!(
                    index = i,
                    role = ?msg.role,
                    content_parts = msg.content.len(),
                    "Message in request"
                );
            }

            info!(
                step = steps,
                message_count = messages.len(),
                tool_count = tool_defs.len(),
                "Calling provider.generate()"
            );

            let stream = {
                let provider = self.provider.read().await;
                provider.generate(messages.clone(), options).await?
            };

            info!(
                step = steps,
                "Provider returned stream, starting to process"
            );
            tokio::pin!(stream);

            let mut current_text = String::new();
            let mut tool_calls: Vec<(String, String, String)> = Vec::new(); // (id, name, args)
            let mut finish_reason = FinishReason::EndTurn;
            let mut step_usage = wonopcode_provider::stream::Usage::default();
            // Track observed tools (from Claude CLI) to extract file modification info
            let mut observed_tools: std::collections::HashMap<String, (String, String)> =
                std::collections::HashMap::new(); // id -> (name, input)

            // Process stream
            let mut chunk_count = 0u32;
            while let Some(chunk_result) = stream.next().await {
                if cancel.is_cancelled() {
                    return Err("Cancelled".into());
                }

                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Stream error (will be skipped): {}", e);
                        continue;
                    }
                };

                chunk_count += 1;
                if chunk_count == 1 {
                    info!(step = steps, "Received first chunk from provider");
                }

                // Log all chunks for debugging
                debug!(chunk = ?chunk, "Received stream chunk");

                match chunk {
                    StreamChunk::TextStart => {}
                    StreamChunk::TextDelta(delta) => {
                        current_text.push_str(&delta);
                        let _ = update_tx.send(AppUpdate::TextDelta(delta));
                    }
                    StreamChunk::TextEnd => {}
                    StreamChunk::ToolCallStart { id, name } => {
                        debug!(id = %id, name = %name, "Tool call started");
                        tool_calls.push((id.clone(), name.clone(), String::new()));
                        // Don't send ToolStarted yet - wait until we have the input
                    }
                    StreamChunk::ToolCallDelta { delta, .. } => {
                        if let Some(call) = tool_calls.last_mut() {
                            call.2.push_str(&delta);
                        }
                    }
                    StreamChunk::ToolCall {
                        id,
                        name,
                        arguments,
                    } => {
                        debug!(id = %id, name = %name, "Tool call complete");
                        if let Some(call) = tool_calls.iter_mut().find(|c| c.0 == id) {
                            call.2 = arguments;
                        } else {
                            tool_calls.push((id, name, arguments));
                        }
                    }
                    StreamChunk::ReasoningStart => {}
                    StreamChunk::ReasoningDelta(_) => {}
                    StreamChunk::ReasoningEnd => {}
                    StreamChunk::ToolObserved { id, name, input } => {
                        // Tool was observed being executed externally (e.g., by Claude CLI)
                        // Store for later processing when result arrives
                        debug!(id = %id, name = %name, "Tool observed (external execution)");
                        observed_tools.insert(id.clone(), (name.clone(), input.clone()));
                        // Notify the TUI
                        let _ = update_tx.send(AppUpdate::ToolStarted { name, id, input });
                    }
                    StreamChunk::ToolResultObserved {
                        id,
                        success,
                        output,
                    } => {
                        // Tool result was observed (external execution completed)
                        debug!(id = %id, success = %success, "Tool result observed");

                        // Look up the observed tool info
                        let tool_info = observed_tools.remove(&id);

                        // Check if this was a file-modifying tool and send ModifiedFilesUpdated
                        if success {
                            if let Some((ref tool_name, ref input)) = tool_info {
                                let base_tool_name =
                                    tool_name.rsplit("__").next().unwrap_or(tool_name);

                                // Send LSP status update if LSP tool was used (by MCP server)
                                if base_tool_name == "lsp" {
                                    // Parse the input to extract file path and send LSP status
                                    if let Ok(input_json) =
                                        serde_json::from_str::<serde_json::Value>(input)
                                    {
                                        if let Some(file_path) =
                                            input_json.get("file").and_then(|v| v.as_str())
                                        {
                                            // Determine language server name from file extension
                                            let server_name = if file_path.ends_with(".rs") {
                                                "rust-analyzer"
                                            } else if file_path.ends_with(".ts")
                                                || file_path.ends_with(".tsx")
                                                || file_path.ends_with(".js")
                                                || file_path.ends_with(".jsx")
                                            {
                                                "typescript-language-server"
                                            } else if file_path.ends_with(".py") {
                                                "pyright"
                                            } else if file_path.ends_with(".go") {
                                                "gopls"
                                            } else {
                                                "lsp"
                                            };

                                            // Get the project root from the file path
                                            let root = std::path::Path::new(file_path)
                                                .parent()
                                                .and_then(|p| p.to_str())
                                                .unwrap_or(".")
                                                .to_string();

                                            let lsp_update = LspStatusUpdate {
                                                id: server_name.to_string(),
                                                name: server_name.to_string(),
                                                root,
                                                connected: true,
                                            };
                                            let _ = update_tx
                                                .send(AppUpdate::LspUpdated(vec![lsp_update]));
                                        }
                                    }
                                }

                                // Sync todos if todowrite was executed (by MCP server)
                                if base_tool_name == "todowrite" {
                                    // Read todos from the shared file store
                                    let todos = todo::get_todos(self.todo_store.as_ref(), cwd);
                                    let todo_updates: Vec<TodoUpdate> = todos
                                        .into_iter()
                                        .map(|t| TodoUpdate {
                                            id: t.id,
                                            content: t.content,
                                            status: match t.status {
                                                todo::TodoStatus::Pending => "pending".to_string(),
                                                todo::TodoStatus::InProgress => {
                                                    "in_progress".to_string()
                                                }
                                                todo::TodoStatus::Completed => {
                                                    "completed".to_string()
                                                }
                                                todo::TodoStatus::Cancelled => {
                                                    "cancelled".to_string()
                                                }
                                            },
                                            priority: match t.priority {
                                                todo::TodoPriority::High => "high".to_string(),
                                                todo::TodoPriority::Medium => "medium".to_string(),
                                                todo::TodoPriority::Low => "low".to_string(),
                                            },
                                        })
                                        .collect();
                                    if !todo_updates.is_empty() {
                                        let _ =
                                            update_tx.send(AppUpdate::TodosUpdated(todo_updates));
                                    }
                                }
                            }
                        }

                        if success {
                            if let Some((ref tool_name, ref input)) = tool_info {
                                if let Some(update) =
                                    extract_modified_file_from_observed_tool(tool_name, input, &output)
                                {
                                    debug!(path = %update.path, added = update.added, removed = update.removed, "Sending ModifiedFilesUpdated for observed tool");
                                    let _ = update_tx
                                        .send(AppUpdate::ModifiedFilesUpdated(vec![update]));
                                }
                            }
                        }

                        // Extract metadata from output for tools that provide structured info
                        let metadata = tool_info
                            .as_ref()
                            .and_then(|(name, _)| {
                                extract_metadata_from_observed_tool(name, &output)
                            })
                            .map(serde_json::Value::Object);

                        let _ = update_tx.send(AppUpdate::ToolCompleted {
                            id,
                            success,
                            output,
                            metadata,
                        });
                    }
                    StreamChunk::FinishStep {
                        usage,
                        finish_reason: reason,
                    } => {
                        step_usage.merge(&usage);
                        finish_reason = reason;

                        // Calculate cost and get context limit
                        let (cost, context_limit) = {
                            let provider = self.provider.read().await;
                            let model_info = provider.model_info();
                            let cost = model_info.cost.calculate(
                                total_input + step_usage.input_tokens,
                                total_output + step_usage.output_tokens,
                            );
                            (cost, model_info.limit.context)
                        };

                        // Send token usage update
                        let _ = update_tx.send(AppUpdate::TokenUsage {
                            input: total_input + step_usage.input_tokens,
                            output: total_output + step_usage.output_tokens,
                            cost,
                            context_limit,
                        });
                    }
                    StreamChunk::Error(e) => {
                        warn!("Stream error: {}", e);
                    }
                }
            }

            // Accumulate usage for this step
            total_input += step_usage.input_tokens;
            total_output += step_usage.output_tokens;

            info!(
                step = steps,
                chunks = chunk_count,
                text_len = current_text.len(),
                tool_calls = tool_calls.len(),
                finish_reason = ?finish_reason,
                "Step completed"
            );

            // Update final text
            final_text = current_text.clone();

            // Add assistant message to history
            if !current_text.is_empty() || !tool_calls.is_empty() {
                let mut content = vec![];
                if !current_text.is_empty() {
                    content.push(wonopcode_provider::ContentPart::text(&current_text));
                }
                for (id, name, args) in &tool_calls {
                    let input: serde_json::Value =
                        serde_json::from_str(args).unwrap_or(serde_json::Value::Null);
                    content.push(wonopcode_provider::ContentPart::tool_use(id, name, input));
                }

                messages.push(ProviderMessage {
                    role: wonopcode_provider::Role::Assistant,
                    content,
                });
            }

            // Execute tool calls - ALL tools run in parallel
            if !tool_calls.is_empty() {
                info!("Executing {} tool calls in parallel", tool_calls.len());

                // Check for doom loop on each tool call before executing
                let doom_loop_permission = {
                    let config = self.config.read().await;
                    config.doom_loop
                };

                // Track which tools are blocked by doom loop or permissions
                let mut doom_loop_blocked: Vec<(String, String, String)> = Vec::new();
                let mut permission_blocked: Vec<(String, String, String)> = Vec::new();
                let mut allowed_calls: Vec<(String, String, String)> = Vec::new();

                for (call_id, tool_name, args_str) in tool_calls {
                    let input: serde_json::Value =
                        serde_json::from_str(&args_str).unwrap_or(serde_json::Value::Null);

                    // Check doom loop detector first
                    let is_doom_loop = {
                        let mut detector = self.doom_loop_detector.write().await;
                        detector.record_and_check(&tool_name, &input)
                    };

                    if is_doom_loop {
                        warn!(
                            tool = %tool_name,
                            "Doom loop detected: {} consecutive identical calls",
                            DOOM_LOOP_THRESHOLD
                        );

                        match doom_loop_permission {
                            Decision::Allow => {
                                // Allow the tool to run despite doom loop
                            }
                            Decision::Deny => {
                                // Block the tool and return error to the model
                                doom_loop_blocked.push((call_id, tool_name, args_str));
                                continue;
                            }
                            Decision::Ask => {
                                // For now, treat Ask as Deny with a warning
                                let _ = update_tx.send(AppUpdate::Status(format!(
                                    "Doom loop detected: '{}' called {} times with identical args",
                                    tool_name, DOOM_LOOP_THRESHOLD
                                )));
                                doom_loop_blocked.push((call_id, tool_name, args_str));
                                continue;
                            }
                        }
                    }

                    // Check tool permissions
                    // Normalize tool name - MCP tools have prefix like "mcp__wonopcode-tools__read"
                    let normalized_tool_name =
                        tool_name.rsplit("__").next().unwrap_or(&tool_name);
                    let path = extract_path_from_input(&input);
                    let action = determine_tool_action(normalized_tool_name, &input);
                    let description = format_tool_description(normalized_tool_name, &input);

                    let check = PermissionCheck {
                        id: call_id.clone(),
                        tool: normalized_tool_name.to_string(),
                        action: action.clone(),
                        description,
                        path: path.clone(),
                        details: input.clone(),
                    };

                    let allowed = self.permission_manager.check("default", check).await;

                    if allowed {
                        allowed_calls.push((call_id, tool_name, args_str));
                    } else {
                        warn!(tool = %tool_name, action = %action, "Tool execution denied by permission manager");
                        permission_blocked.push((call_id, tool_name, args_str));
                    }
                }

                // Handle doom loop blocked tools - add error responses to messages
                for (call_id, tool_name, _args_str) in &doom_loop_blocked {
                    let error_msg = format!(
                        "Tool execution blocked: doom loop detected. \
                        You have called '{}' {} times in a row with identical arguments. \
                        Please try a different approach or use different arguments.",
                        tool_name, DOOM_LOOP_THRESHOLD
                    );

                    let _ = update_tx.send(AppUpdate::ToolStarted {
                        name: tool_name.clone(),
                        id: call_id.clone(),
                        input: "{}".to_string(),
                    });
                    let _ = update_tx.send(AppUpdate::ToolCompleted {
                        id: call_id.clone(),
                        success: false,
                        output: error_msg.clone(),
                        metadata: None,
                    });

                    messages.push(ProviderMessage::tool_result(call_id, &error_msg));
                }

                // Handle permission blocked tools - add error responses to messages
                for (call_id, tool_name, _args_str) in &permission_blocked {
                    let error_msg = format!(
                        "Tool execution denied: permission not granted for '{}'. \
                        The user has declined to allow this tool execution.",
                        tool_name
                    );

                    let _ = update_tx.send(AppUpdate::ToolStarted {
                        name: tool_name.clone(),
                        id: call_id.clone(),
                        input: "{}".to_string(),
                    });
                    let _ = update_tx.send(AppUpdate::ToolCompleted {
                        id: call_id.clone(),
                        success: false,
                        output: error_msg.clone(),
                        metadata: None,
                    });

                    messages.push(ProviderMessage::tool_result(call_id, &error_msg));
                }

                // Replace tool_calls with allowed_calls
                let tool_calls = allowed_calls;

                if tool_calls.is_empty() {
                    // All tools were blocked, continue to get model response
                    debug!("All tool calls blocked by doom loop or permission checks");
                    continue;
                }

                // Send ToolStarted for allowed tools and log invocations
                for (call_id, tool_name, args_str) in &tool_calls {
                    // Log tool invocation for all providers
                    info!(
                        tool = %tool_name,
                        call_id = %call_id,
                        args_preview = %args_str.chars().take(200).collect::<String>(),
                        "Tool invoked"
                    );

                    let _ = update_tx.send(AppUpdate::ToolStarted {
                        name: tool_name.clone(),
                        id: call_id.clone(),
                        input: args_str.clone(),
                    });
                }

                if tool_calls.len() > 1 {
                    let _ = update_tx.send(AppUpdate::Status(format!(
                        "Running {} tools in parallel",
                        tool_calls.len()
                    )));
                }

                // Spawn all tools concurrently
                let tool_futures: Vec<_> = tool_calls
                    .into_iter()
                    .map(|(call_id, tool_name, args_str)| {
                        let update_tx = update_tx.clone();
                        let cwd = cwd.to_path_buf();
                        let provider = self.provider.clone();
                        let config = self.config.clone();
                        let tools = self.tools.clone();
                        let cancel = cancel.clone();
                        let snapshot_store = self.snapshot_store.clone();
                        let file_time = self.file_time.clone();
                        let sandbox_manager = self.sandbox_manager.clone();
                        let todo_store = self.todo_store.clone();

                        async move {
                            let tool_start = Instant::now();
                            let input: serde_json::Value =
                                serde_json::from_str(&args_str).unwrap_or(serde_json::Value::Null);

                            // Get sandbox runtime for this tool (if enabled and not bypassed)
                            let sandbox = get_sandbox_for_tool(&tool_name, &sandbox_manager).await;

                            // Special handling for task tool - run subagent
                            let result = if tool_name == "task" {
                                match serde_json::from_value::<task::TaskArgs>(input.clone()) {
                                    Ok(args) => {
                                        let _ = update_tx.send(AppUpdate::Status(format!(
                                            "Running {} subagent: {}",
                                            args.subagent_type, args.description
                                        )));

                                        match run_subagent_standalone(
                                            &args.subagent_type,
                                            &args.prompt,
                                            &cwd,
                                            provider,
                                            config,
                                            tools.clone(),
                                            cancel,
                                            snapshot_store.clone(),
                                            file_time.clone(),
                                            sandbox.clone(),
                                        )
                                        .await
                                        {
                                            Ok(response) => Ok(wonopcode_tools::ToolOutput::new(
                                                format!("Task completed: {}", args.description),
                                                response,
                                            )),
                                            Err(e) => {
                                                error!(
                                                    tool = "task",
                                                    subagent_type = %args.subagent_type,
                                                    description = %args.description,
                                                    error = %e,
                                                    "Subagent execution failed"
                                                );
                                                Err(wonopcode_tools::ToolError::execution_failed(
                                                    format!("Subagent failed: {}", e),
                                                ))
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        // Truncate prompt for logging
                                        let prompt_truncated = if args_str.len() > 500 {
                                            format!("{}... [truncated]", &args_str[..500])
                                        } else {
                                            args_str.clone()
                                        };
                                        error!(
                                            tool = "task",
                                            error = %e,
                                            arguments = %prompt_truncated,
                                            "Invalid task arguments"
                                        );
                                        Err(wonopcode_tools::ToolError::validation(format!(
                                            "Invalid task arguments: {}",
                                            e
                                        )))
                                    }
                                }
                            } else {
                                // Execute regular tool
                                execute_tool_standalone(
                                    &tool_name,
                                    input,
                                    &cwd,
                                    tools,
                                    cancel,
                                    snapshot_store,
                                    file_time,
                                    sandbox,
                                )
                                .await
                            };

                            let (output, success, metadata) = match result {
                                Ok(out) => {
                                    let output = if out.output.len() > 50000 {
                                        format!(
                                            "{}\n\n... [Output truncated: {} chars total, showing first 50000]",
                                            &out.output[..50000],
                                            out.output.len()
                                        )
                                    } else {
                                        out.output
                                    };
                                    (output, true, out.metadata)
                                }
                                Err(e) => (format!("Error: {}", e), false, serde_json::Value::Null),
                            };

                            // Log tool completion with performance metrics
                            let tool_duration = tool_start.elapsed();
                            info!(
                                tool = %tool_name,
                                call_id = %call_id,
                                success = success,
                                output_len = output.len(),
                                duration_ms = tool_duration.as_millis(),
                                "Tool completed"
                            );
                            perf::log_tool(&tool_name, tool_duration, success);

                            let _ = update_tx.send(AppUpdate::ToolCompleted {
                                id: call_id.clone(),
                                success,
                                output: output.clone(),
                                metadata: Some(metadata.clone()),
                            });

                            // Check for agent change in metadata (from plan mode tools)
                            if success {
                                if let Some(agent) =
                                    metadata.get("agent_change").and_then(|v| v.as_str())
                                {
                                    info!(agent = %agent, "Agent changed via tool");
                                    let _ = update_tx.send(AppUpdate::AgentChanged(agent.to_string()));
                                }
                            }

                            // Send incremental modified file update for file-modifying tools
                            if success
                                && (tool_name == "write"
                                    || tool_name == "edit"
                                    || tool_name == "multiedit")
                            {
                                if let Some(obj) = metadata.as_object() {
                                    let mut updates = Vec::new();

                                    // For write tool: path and bytes
                                    if let Some(path) = obj.get("path").and_then(|v| v.as_str()) {
                                        let added = obj
                                            .get("bytes")
                                            .and_then(|v| v.as_u64())
                                            .map(|b| (b / 40) as u32)
                                            .unwrap_or(1);
                                        updates.push(ModifiedFileUpdate {
                                            path: path.to_string(),
                                            added,
                                            removed: 0,
                                        });
                                    }
                                    // For edit tool: file, additions, deletions
                                    if let Some(file) = obj.get("file").and_then(|v| v.as_str()) {
                                        let added = obj
                                            .get("additions")
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(0)
                                            as u32;
                                        let removed = obj
                                            .get("deletions")
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(0)
                                            as u32;
                                        updates.push(ModifiedFileUpdate {
                                            path: file.to_string(),
                                            added,
                                            removed,
                                        });
                                    }
                                    // For multiedit tool: files and edits count
                                    if let Some(files) = obj.get("files").and_then(|v| v.as_u64()) {
                                        let edits = obj
                                            .get("edits")
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(0)
                                            as u32;
                                        if files > 0 {
                                            updates.push(ModifiedFileUpdate {
                                                path: format!("{} files", files),
                                                added: edits,
                                                removed: 0,
                                            });
                                        }
                                    }

                                    if !updates.is_empty() {
                                        let _ =
                                            update_tx.send(AppUpdate::ModifiedFilesUpdated(updates));
                                    }
                                }
                            }

                            // Sync todos immediately if todowrite was executed
                            // Normalize tool name - MCP tools have prefix like "mcp__wonopcode-tools__todowrite"
                            let base_tool_name =
                                tool_name.rsplit("__").next().unwrap_or(&tool_name);
                            if base_tool_name == "todowrite" && success {
                                // Read todos from the file-based store (shared with MCP server)
                                let todos = todo::get_todos(todo_store.as_ref(), &cwd);
                                let todo_updates: Vec<TodoUpdate> = todos
                                    .into_iter()
                                    .map(|t| TodoUpdate {
                                        id: t.id,
                                        content: t.content,
                                        status: match t.status {
                                            todo::TodoStatus::Pending => "pending".to_string(),
                                            todo::TodoStatus::InProgress => "in_progress".to_string(),
                                            todo::TodoStatus::Completed => "completed".to_string(),
                                            todo::TodoStatus::Cancelled => "cancelled".to_string(),
                                        },
                                        priority: match t.priority {
                                            todo::TodoPriority::High => "high".to_string(),
                                            todo::TodoPriority::Medium => "medium".to_string(),
                                            todo::TodoPriority::Low => "low".to_string(),
                                        },
                                    })
                                    .collect();
                                if !todo_updates.is_empty() {
                                    let _ = update_tx.send(AppUpdate::TodosUpdated(todo_updates));
                                }
                            }

                            (call_id, tool_name, output, success, metadata)
                        }
                    })
                    .collect();

                // Wait for all tools to complete
                let tool_results = futures::future::join_all(tool_futures).await;

                // Post-process results - add tool results to messages
                // Note: Modified file and todo updates are now sent incrementally after each tool completes
                let mut has_lsp_tool = false;
                for (call_id, tool_name, output, _success, _metadata) in &tool_results {
                    // Add tool result to messages
                    messages.push(ProviderMessage::tool_result(call_id, output));
                    // Normalize tool name - MCP tools have prefix like "mcp__wonopcode-tools__lsp"
                    let base_tool_name = tool_name.rsplit("__").next().unwrap_or(tool_name);
                    if base_tool_name == "lsp" {
                        has_lsp_tool = true;
                    }
                }

                // Send LSP status update if LSP tool was used
                if has_lsp_tool {
                    self.send_lsp_status(update_tx).await;
                }

                // Always continue after executing tool calls to get the model's response
                // (Some models like o1 may not set finish_reason to ToolUse)
                info!(
                    tool_results_count = tool_results.len(),
                    messages_count = messages.len(),
                    "Tool calls executed, continuing loop to get model response"
                );
                continue;
            }

            debug!(
                final_text_len = final_text.len(),
                finish_reason = ?finish_reason,
                "Prompt loop ending"
            );

            // Done - store final assistant message in history
            if !final_text.is_empty() {
                let mut history = self.history.write().await;
                history.push(ProviderMessage::assistant(&final_text));
            }
            break;
        }

        Ok(final_text)
    }
}

/// Run a subagent standalone (without self reference).
/// This allows running multiple subagents in parallel from async closures.
#[allow(clippy::too_many_arguments)]
async fn run_subagent_standalone(
    agent_type: &str,
    prompt: &str,
    cwd: &Path,
    provider: Arc<RwLock<BoxedLanguageModel>>,
    config: Arc<RwLock<RunnerConfig>>,
    tools: Arc<ToolRegistry>,
    cancel: CancellationToken,
    snapshot_store: Option<Arc<SnapshotStore>>,
    file_time: Arc<FileTimeState>,
    sandbox: Option<Arc<dyn SandboxRuntime>>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use futures::StreamExt;
    use wonopcode_provider::message::ContentPart;

    debug!(agent = agent_type, "Running subagent (standalone)");

    // Get agent-specific system prompt
    let system_prompt = task::get_subagent_prompt(agent_type);

    // Get agent-specific tool configuration
    let tool_config = task::get_subagent_tools(agent_type);

    // Build tool definitions for allowed tools only
    let tool_defs: Vec<ToolDefinition> = tools
        .all()
        .filter(|t| {
            tool_config
                .iter()
                .find(|(name, _)| *name == t.id())
                .map(|(_, enabled)| *enabled)
                .unwrap_or(false)
        })
        .map(|t| ToolDefinition {
            name: t.id().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters_schema(),
        })
        .collect();

    // Build messages - just the user prompt
    let mut messages = vec![ProviderMessage::user(prompt)];

    let mut final_text = String::new();
    let mut steps = 0;
    const MAX_STEPS: usize = 20; // Limit subagent steps

    // Subagent loop
    loop {
        if cancel.is_cancelled() {
            return Err("Cancelled".into());
        }

        if steps >= MAX_STEPS {
            warn!(agent = agent_type, "Subagent max steps reached");
            break;
        }

        steps += 1;
        debug!(agent = agent_type, step = steps, "Subagent step");

        // Build options with agent-specific system prompt
        let options = {
            let cfg = config.read().await;
            GenerateOptions {
                temperature: cfg.temperature,
                max_tokens: cfg.max_tokens,
                system: Some(system_prompt.to_string()),
                tools: tool_defs.clone(),
                abort: Some(cancel.clone()),
                ..Default::default()
            }
        };

        // Call provider
        let stream = {
            let provider = provider.read().await;
            provider.generate(messages.clone(), options).await?
        };
        tokio::pin!(stream);

        let mut current_text = String::new();
        let mut tool_calls: Vec<(String, String, String)> = Vec::new();
        let mut finish_reason = FinishReason::EndTurn;

        // Process stream
        while let Some(chunk_result) = stream.next().await {
            if cancel.is_cancelled() {
                return Err("Cancelled".into());
            }

            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    warn!("Subagent stream error: {}", e);
                    continue;
                }
            };

            match chunk {
                StreamChunk::TextDelta(delta) => {
                    current_text.push_str(&delta);
                }
                StreamChunk::ToolCallStart { id, name } => {
                    tool_calls.push((id, name, String::new()));
                }
                StreamChunk::ToolCallDelta { id: _, delta } => {
                    if let Some((_, _, args)) = tool_calls.last_mut() {
                        args.push_str(&delta);
                    }
                }
                StreamChunk::FinishStep {
                    finish_reason: reason,
                    ..
                } => {
                    finish_reason = reason;
                }
                _ => {}
            }
        }

        // Accumulate text output from all steps
        if !current_text.is_empty() {
            if !final_text.is_empty() {
                final_text.push_str("\n\n");
            }
            final_text.push_str(&current_text);
        }

        // If no tool calls, we're done
        if tool_calls.is_empty() {
            break;
        }

        // Check if we should continue (tool calls)
        if finish_reason != FinishReason::ToolUse {
            break;
        }

        // Add assistant message with tool calls
        let mut assistant_msg = if current_text.is_empty() {
            ProviderMessage {
                role: wonopcode_provider::message::Role::Assistant,
                content: vec![],
            }
        } else {
            ProviderMessage::assistant(&current_text)
        };

        // Add tool use parts
        for (id, name, args_str) in &tool_calls {
            let input: serde_json::Value = serde_json::from_str(args_str)
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
            assistant_msg
                .content
                .push(ContentPart::tool_use(id, name, input));
        }
        messages.push(assistant_msg);

        // Execute tool calls (only allowed tools)
        for (id, name, args_str) in &tool_calls {
            // Check if tool is allowed for this agent
            let is_allowed = tool_config
                .iter()
                .find(|(n, _)| *n == name.as_str())
                .map(|(_, enabled)| *enabled)
                .unwrap_or(false);

            let output = if !is_allowed {
                format!("Tool '{}' is not available for {} agent", name, agent_type)
            } else {
                let args: serde_json::Value = serde_json::from_str(args_str)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                // Execute tool
                let tool = match tools.get(name) {
                    Some(t) => t,
                    None => {
                        messages.push(ProviderMessage::tool_result(
                            id,
                            format!("Unknown tool: {}", name),
                        ));
                        continue;
                    }
                };

                let ctx = wonopcode_tools::ToolContext {
                    session_id: "subagent".to_string(),
                    message_id: "subagent".to_string(),
                    agent: agent_type.to_string(),
                    abort: cancel.clone(),
                    root_dir: cwd.to_path_buf(),
                    cwd: cwd.to_path_buf(),
                    snapshot: snapshot_store.clone(),
                    file_time: Some(file_time.clone()),
                    sandbox: sandbox.clone(),
                };

                match tool.execute(args, &ctx).await {
                    Ok(out) => out.output,
                    Err(e) => format!("Error: {}", e),
                }
            };

            // Add tool result
            messages.push(ProviderMessage::tool_result(id, &output));
        }
    }

    debug!(agent = agent_type, steps = steps, "Subagent completed");

    // If no text output was generated, provide a fallback message
    if final_text.is_empty() {
        if steps == 0 {
            Ok(
                "Subagent did not produce any output. The model may have failed to respond."
                    .to_string(),
            )
        } else {
            Ok(format!(
                "Subagent completed after {} steps but did not produce a text summary.",
                steps
            ))
        }
    } else {
        Ok(final_text)
    }
}

/// Execute a tool standalone (without self reference).
/// This allows running multiple tools in parallel from async closures.
#[allow(clippy::too_many_arguments)]
async fn execute_tool_standalone(
    tool_name: &str,
    input: serde_json::Value,
    cwd: &Path,
    tools: Arc<ToolRegistry>,
    cancel: CancellationToken,
    snapshot_store: Option<Arc<SnapshotStore>>,
    file_time: Arc<FileTimeState>,
    sandbox: Option<Arc<dyn SandboxRuntime>>,
) -> Result<wonopcode_tools::ToolOutput, wonopcode_tools::ToolError> {
    let tool = tools.get(tool_name).ok_or_else(|| {
        wonopcode_tools::ToolError::validation(format!("Unknown tool: {}", tool_name))
    })?;

    let ctx = wonopcode_tools::ToolContext {
        session_id: "default".to_string(),
        message_id: "default".to_string(),
        agent: "default".to_string(),
        abort: cancel,
        root_dir: cwd.to_path_buf(),
        cwd: cwd.to_path_buf(),
        snapshot: snapshot_store,
        file_time: Some(file_time),
        sandbox,
    };

    info!(tool = tool_name, "Executing tool");
    let result = tool.execute(input.clone(), &ctx).await;

    match &result {
        Ok(_) => {
            info!(tool = tool_name, "Tool execution completed successfully");
        }
        Err(e) => {
            // Truncate arguments for logging to avoid huge log entries
            let args_str =
                serde_json::to_string(&input).unwrap_or_else(|_| "<invalid>".to_string());
            let args_truncated = if args_str.len() > 2000 {
                format!(
                    "{}... [truncated, {} total chars]",
                    &args_str[..2000],
                    args_str.len()
                )
            } else {
                args_str
            };

            error!(
                tool = tool_name,
                error = %e,
                arguments = %args_truncated,
                "Tool execution failed"
            );
        }
    }

    result
}

/// Get sandbox runtime for a tool from the sandbox manager.
///
/// Returns None if the sandbox manager is not configured, the tool bypasses sandbox,
/// the sandbox was explicitly stopped, or the sandbox failed to start.
async fn get_sandbox_for_tool(
    tool_name: &str,
    sandbox_manager: &Option<Arc<SandboxManager>>,
) -> Option<Arc<dyn SandboxRuntime>> {
    let manager = match sandbox_manager.as_ref() {
        Some(m) => m,
        None => {
            info!(tool = tool_name, "No sandbox manager configured for tool");
            return None;
        }
    };

    // Check if tool should bypass sandbox
    if manager.should_bypass_tool(tool_name) {
        info!(tool = tool_name, "Tool bypasses sandbox");
        return None;
    }

    // Check if sandbox was explicitly stopped by user
    if manager.is_explicitly_stopped().await {
        info!(
            tool = tool_name,
            "Sandbox explicitly stopped, not using sandbox"
        );
        return None;
    }

    // Get the runtime, starting it if needed
    match manager.runtime().await {
        Ok(runtime) => {
            // Check if sandbox is ready
            let is_ready = runtime.is_ready().await;
            info!(
                tool = tool_name,
                is_ready = is_ready,
                "Got sandbox runtime for tool"
            );
            if !is_ready {
                // Auto-start sandbox for tool execution
                info!(tool = tool_name, "Starting sandbox for tool");
                if let Err(e) = runtime.start().await {
                    warn!(tool = tool_name, error = %e, "Failed to start sandbox for tool");
                    return None;
                }
                info!(tool = tool_name, "Sandbox started for tool");
            }
            Some(runtime as Arc<dyn SandboxRuntime>)
        }
        Err(e) => {
            warn!(tool = tool_name, error = %e, "Failed to get sandbox runtime for tool");
            None
        }
    }
}

/// Create a provider from configuration.
///
/// # Arguments
/// * `config` - Runner configuration
/// * `sandbox_enabled` - Whether sandbox is enabled (for Claude CLI provider)
/// * `allow_all` - Whether to allow all tool executions without permission checks
fn create_provider(
    config: &RunnerConfig,
    sandbox_enabled: Option<bool>,
    allow_all: bool,
) -> Result<BoxedLanguageModel, Box<dyn std::error::Error + Send + Sync>> {
    use wonopcode_provider::{deepinfra, groq, mistral, together, xai};

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
                        // Check if we should use HTTP transport (headless mode)
                        if let Some(ref mcp_url) = config.mcp_url {
                            info!(mcp_url = %mcp_url, "Using MCP HTTP transport");
                            let provider = wonopcode_provider::claude_cli::with_custom_tools_http(
                                model_info,
                                mcp_url.clone(),
                            )?;
                            Ok(Arc::new(provider))
                        } else {
                            // Use stdio transport (spawns child process)
                            let cwd = std::env::current_dir().ok();
                            let provider = wonopcode_provider::claude_cli::with_custom_tools(
                                model_info,
                                cwd,
                                None,            // Session ID will be auto-generated
                                sandbox_enabled, // Pass sandbox enabled state to MCP server
                                allow_all,       // Pass permission mode to MCP server
                            )?;
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
    use wonopcode_provider::{deepinfra, groq, mistral, together, xai};

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
                entries.push(format!("{}...", prefix));
                break;
            }

            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);

            if is_dir {
                entries.push(format!("{}{}/", prefix, name_str));
                *count += 1;
                collect_entries(
                    &entry.path(),
                    &format!("{}  ", prefix),
                    depth + 1,
                    max_depth,
                    entries,
                    count,
                    max_files,
                );
            } else {
                entries.push(format!("{}{}", prefix, name_str));
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
pub fn load_api_key(provider: &str) -> Option<String> {
    // Validate provider first
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

    // Try environment variables first
    if let Ok(key) = std::env::var(env_var) {
        if !key.is_empty() {
            debug!(provider = %provider, source = "env", key_len = key.len(), "Found API key");
            return Some(key);
        }
    }

    // Try credentials file
    if let Some(key) = load_api_key_from_file(provider) {
        debug!(provider = %provider, source = "file", key_len = key.len(), "Found API key");
        return Some(key);
    }

    debug!(provider = %provider, "No API key found");
    None
}

/// Load API key from the credentials file.
fn load_api_key_from_file(provider: &str) -> Option<String> {
    let credentials_path =
        wonopcode_core::config::Config::global_config_dir()?.join("credentials.json");

    if !credentials_path.exists() {
        debug!(path = %credentials_path.display(), "Credentials file not found");
        return None;
    }

    let content = std::fs::read_to_string(&credentials_path).ok()?;

    // Try parsing as simple HashMap<String, String> first (legacy format)
    if let Ok(credentials) = serde_json::from_str::<HashMap<String, String>>(&content) {
        debug!(provider = %provider, format = "legacy", "Parsed credentials file");
        return credentials.get(provider).cloned();
    }

    // Try parsing as HashMap<String, Value> for new nested format
    if let Ok(credentials) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&content) {
        debug!(provider = %provider, format = "nested", "Parsed credentials file");
        if let Some(provider_config) = credentials.get(provider) {
            // For API key auth, look for "key" field
            if let Some(key) = provider_config.get("key").and_then(|v| v.as_str()) {
                return Some(key.to_string());
            }
            // For OAuth auth (anthropic subscription), return None - we use CLI instead
            if provider_config.get("type").and_then(|v| v.as_str()) == Some("oauth") {
                debug!(provider = %provider, "Found OAuth credentials, will use CLI");
                return None;
            }
        }
    }

    debug!(provider = %provider, "Could not parse credentials file");
    None
}

/// Convert wonopcode McpLocalConfig to wonopcode_mcp ServerConfig.
fn convert_mcp_config(name: &str, config: &McpLocalConfig) -> McpServerConfig {
    // Parse command - first element is the command, rest are args
    let (command, args) = if config.command.is_empty() {
        (String::new(), Vec::new())
    } else {
        (config.command[0].clone(), config.command[1..].to_vec())
    };

    let mut server_config = McpServerConfig::stdio(name, &command, args);

    // Add environment variables if specified
    if let Some(env) = &config.environment {
        for (key, value) in env {
            server_config = server_config.with_env(key, value);
        }
    }

    server_config
}

/// Extract path from tool input for permission checking.
fn extract_path_from_input(input: &serde_json::Value) -> Option<String> {
    // Different tools use different field names for paths
    let path_fields = ["filePath", "path", "file", "directory", "workdir"];

    for field in &path_fields {
        if let Some(path) = input.get(field).and_then(|v| v.as_str()) {
            return Some(path.to_string());
        }
    }

    // For bash tool, try to extract path from command
    if let Some(command) = input.get("command").and_then(|v| v.as_str()) {
        // Extract first path-like argument from command
        let parts: Vec<&str> = command.split_whitespace().collect();
        for part in parts.iter().skip(1) {
            if part.starts_with('/') || part.starts_with("./") || part.starts_with("../") {
                return Some(part.to_string());
            }
        }
    }

    None
}

/// Extract modified file info from an observed tool (Claude CLI MCP tools).
/// Returns a ModifiedFileUpdate if the tool modifies files.
fn extract_modified_file_from_observed_tool(
    tool_name: &str,
    input_json: &str,
    output: &str,
) -> Option<ModifiedFileUpdate> {
    // Normalize tool name - MCP tools have prefix like "mcp__wonopcode-tools__edit"
    let base_name = tool_name.rsplit("__").next().unwrap_or(tool_name);

    // Only process file-modifying tools
    match base_name {
        "edit" | "write" | "multiedit" | "patch" => {}
        _ => return None,
    }

    // Try to extract metadata from output first (contains real line counts)
    let metadata = extract_tool_metadata_from_output(output);

    // Parse the input JSON
    let input: serde_json::Value = serde_json::from_str(input_json).ok()?;

    match base_name {
        "edit" => {
            // Single file tool - extract filePath from input, line counts from metadata
            let path = metadata
                .as_ref()
                .and_then(|m| m.get("file").and_then(|v| v.as_str()))
                .or_else(|| input.get("filePath").and_then(|v| v.as_str()))?;
            let added = metadata
                .as_ref()
                .and_then(|m| m.get("additions").and_then(|v| v.as_u64()))
                .unwrap_or(0) as u32;
            let removed = metadata
                .as_ref()
                .and_then(|m| m.get("deletions").and_then(|v| v.as_u64()))
                .unwrap_or(0) as u32;
            Some(ModifiedFileUpdate {
                path: path.to_string(),
                added,
                removed,
            })
        }
        "write" => {
            // Write tool - extract path from metadata or input
            let path = metadata
                .as_ref()
                .and_then(|m| m.get("path").and_then(|v| v.as_str()))
                .or_else(|| input.get("filePath").and_then(|v| v.as_str()))?;
            // Write tool reports bytes, estimate lines (avg ~40 chars/line)
            let added = metadata
                .as_ref()
                .and_then(|m| m.get("bytes").and_then(|v| v.as_u64()))
                .map(|b| ((b / 40) as u32).max(1))
                .unwrap_or(1);
            Some(ModifiedFileUpdate {
                path: path.to_string(),
                added,
                removed: 0,
            })
        }
        "multiedit" => {
            // Multi-edit tool - extract from metadata or input
            let (paths, added, removed) = if let Some(ref meta) = metadata {
                let paths: Vec<String> = meta
                    .get("paths")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let added = meta
                    .get("additions")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let removed = meta
                    .get("deletions")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                (paths, added, removed)
            } else {
                // Fallback to parsing input
                let edits = input.get("edits").and_then(|v| v.as_array())?;
                let mut paths_set: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                for edit in edits {
                    if let Some(path) = edit.get("filePath").and_then(|v| v.as_str()) {
                        paths_set.insert(path.to_string());
                    }
                }
                (paths_set.into_iter().collect(), 0, 0)
            };

            if paths.is_empty() {
                return None;
            }

            if paths.len() == 1 {
                Some(ModifiedFileUpdate {
                    path: paths.into_iter().next()?,
                    added,
                    removed,
                })
            } else {
                Some(ModifiedFileUpdate {
                    path: format!("{} files", paths.len()),
                    added,
                    removed,
                })
            }
        }
        "patch" => {
            // Patch tool - extract from metadata
            let (files_count, added, removed) = if let Some(ref meta) = metadata {
                let files_modified = meta
                    .get("files_modified")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let files_added = meta
                    .get("files_added")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let total_files = files_modified + files_added;
                let added = meta
                    .get("additions")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let removed = meta
                    .get("deletions")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                (total_files as usize, added, removed)
            } else {
                // Fallback: try to parse patch_text from input
                let patch_text = input.get("patch_text").and_then(|v| v.as_str())?;
                // Count files by looking for "*** " patterns
                let count = patch_text
                    .lines()
                    .filter(|l| {
                        l.starts_with("*** Add File:")
                            || l.starts_with("*** Update File:")
                            || l.starts_with("*** Delete File:")
                    })
                    .count();
                (count.max(1), 0, 0)
            };

            if files_count == 0 {
                return None;
            }

            if files_count == 1 {
                // For single file, try to get the path
                let path = input
                    .get("patch_text")
                    .and_then(|v| v.as_str())
                    .and_then(|text| {
                        text.lines().find_map(|l| {
                            l.strip_prefix("*** Add File: ")
                                .or_else(|| l.strip_prefix("*** Update File: "))
                                .or_else(|| l.strip_prefix("*** Delete File: "))
                        })
                    })
                    .unwrap_or("1 file");
                Some(ModifiedFileUpdate {
                    path: path.to_string(),
                    added,
                    removed,
                })
            } else {
                Some(ModifiedFileUpdate {
                    path: format!("{} files", files_count),
                    added,
                    removed,
                })
            }
        }
        _ => None,
    }
}

/// Extract tool metadata from output string.
/// Looks for the <!-- TOOL_METADATA: {...} --> marker appended by MCP executor.
fn extract_tool_metadata_from_output(output: &str) -> Option<serde_json::Value> {
    const MARKER: &str = "<!-- TOOL_METADATA: ";
    let start = output.rfind(MARKER)?;
    let json_start = start + MARKER.len();
    let end = output[json_start..].find(" -->")?;
    let json_str = &output[json_start..json_start + end];
    serde_json::from_str(json_str).ok()
}

/// Extract metadata from observed tool output.
/// Some tools (like todowrite/todoread) include structured info in their output
/// that we can parse to provide metadata for the TUI.
fn extract_metadata_from_observed_tool(
    tool_name: &str,
    output: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    // Normalize tool name - MCP tools have prefix like "mcp__wonopcode-tools__todowrite"
    let base_name = tool_name.rsplit("__").next().unwrap_or(tool_name);

    match base_name {
        "todowrite" | "todoread" => {
            // Parse output like "Todo list updated: 2 pending, 1 in progress, 3 completed"
            // or "5 todos: 2 pending, 1 in progress, 2 completed"
            let mut metadata = serde_json::Map::new();

            // Extract counts using simple pattern matching
            if let Some(pending) = extract_count_before(output, " pending") {
                metadata.insert("pending".to_string(), serde_json::json!(pending));
            }
            if let Some(in_progress) = extract_count_before(output, " in progress") {
                metadata.insert("in_progress".to_string(), serde_json::json!(in_progress));
            }
            if let Some(completed) = extract_count_before(output, " completed") {
                metadata.insert("completed".to_string(), serde_json::json!(completed));
            }

            // Calculate total
            let total = metadata.values().filter_map(|v| v.as_u64()).sum::<u64>();
            if total > 0 {
                metadata.insert("total".to_string(), serde_json::json!(total));
                Some(metadata)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Helper to extract a number that appears before a given suffix in text.
/// E.g., extract_count_before("2 pending, 1 done", " pending") -> Some(2)
fn extract_count_before(text: &str, suffix: &str) -> Option<u64> {
    let idx = text.find(suffix)?;
    let before = &text[..idx];
    // Find the last number in the text before the suffix
    let num_str: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    num_str.parse().ok()
}

/// Determine the action type for a tool call.
fn determine_tool_action(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "read" => "read".to_string(),
        "write" => "write".to_string(),
        "edit" | "multiedit" => "edit".to_string(),
        "glob" | "grep" => "search".to_string(),
        "bash" => {
            // Determine if it's a read or write operation
            if let Some(command) = input.get("command").and_then(|v| v.as_str()) {
                let read_commands = [
                    "cat",
                    "head",
                    "tail",
                    "less",
                    "more",
                    "ls",
                    "pwd",
                    "find",
                    "grep",
                    "rg",
                    "tree",
                    "git status",
                    "git log",
                    "git diff",
                ];
                let write_commands = [
                    "rm",
                    "mv",
                    "cp",
                    "mkdir",
                    "rmdir",
                    "touch",
                    "chmod",
                    "chown",
                    "git add",
                    "git commit",
                    "git push",
                ];

                if read_commands.iter().any(|c| command.starts_with(c)) {
                    "execute_read".to_string()
                } else if write_commands.iter().any(|c| command.starts_with(c)) {
                    "execute_write".to_string()
                } else {
                    "execute".to_string()
                }
            } else {
                "execute".to_string()
            }
        }
        "webfetch" => "fetch".to_string(),
        "task" => "spawn_agent".to_string(),
        "todowrite" | "todoread" => "manage_todos".to_string(),
        "lsp" => "lsp_query".to_string(),
        "skill" => "load_skill".to_string(),
        _ => "execute".to_string(),
    }
}

/// Format a human-readable description of the tool call for permission prompts.
fn format_tool_description(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "read" => {
            if let Some(path) = input.get("filePath").and_then(|v| v.as_str()) {
                format!("Read file: {}", path)
            } else {
                "Read a file".to_string()
            }
        }
        "write" => {
            if let Some(path) = input.get("filePath").and_then(|v| v.as_str()) {
                format!("Write to file: {}", path)
            } else {
                "Write to a file".to_string()
            }
        }
        "edit" => {
            if let Some(path) = input.get("filePath").and_then(|v| v.as_str()) {
                format!("Edit file: {}", path)
            } else {
                "Edit a file".to_string()
            }
        }
        "bash" => {
            if let Some(command) = input.get("command").and_then(|v| v.as_str()) {
                let truncated = if command.len() > 60 {
                    format!("{}...", &command[..60])
                } else {
                    command.to_string()
                };
                format!("Execute: {}", truncated)
            } else {
                "Execute a bash command".to_string()
            }
        }
        "glob" => {
            if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
                format!("Search files: {}", pattern)
            } else {
                "Search for files".to_string()
            }
        }
        "grep" => {
            if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
                format!("Search content: {}", pattern)
            } else {
                "Search file contents".to_string()
            }
        }
        "webfetch" => {
            if let Some(url) = input.get("url").and_then(|v| v.as_str()) {
                format!("Fetch URL: {}", url)
            } else {
                "Fetch a web page".to_string()
            }
        }
        "task" => {
            if let Some(desc) = input.get("description").and_then(|v| v.as_str()) {
                format!("Run task: {}", desc)
            } else {
                "Run a sub-task".to_string()
            }
        }
        _ => format!("Execute tool: {}", tool_name),
    }
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

#[cfg(test)]
mod observed_tool_tests {
    use super::*;

    #[test]
    fn test_extract_count_before() {
        assert_eq!(
            extract_count_before("2 pending, 1 done", " pending"),
            Some(2)
        );
        assert_eq!(
            extract_count_before("10 in progress", " in progress"),
            Some(10)
        );
        assert_eq!(extract_count_before("no numbers here", " pending"), None);
        assert_eq!(
            extract_count_before("5 todos: 3 pending", " pending"),
            Some(3)
        );
    }

    #[test]
    fn test_extract_metadata_from_todowrite() {
        let output = "Todo list updated: 2 pending, 1 in progress, 3 completed";
        let metadata =
            extract_metadata_from_observed_tool("mcp__wonopcode-tools__todowrite", output);
        assert!(metadata.is_some());
        let m = metadata.unwrap();
        assert_eq!(m.get("pending").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(m.get("in_progress").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(m.get("completed").and_then(|v| v.as_u64()), Some(3));
        assert_eq!(m.get("total").and_then(|v| v.as_u64()), Some(6));
    }

    #[test]
    fn test_extract_metadata_from_todoread() {
        let output = "5 todos: 2 pending, 1 in progress, 2 completed";
        let metadata = extract_metadata_from_observed_tool("todoread", output);
        assert!(metadata.is_some());
        let m = metadata.unwrap();
        assert_eq!(m.get("pending").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(m.get("in_progress").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(m.get("completed").and_then(|v| v.as_u64()), Some(2));
    }
}
