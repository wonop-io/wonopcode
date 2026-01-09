//! Configuration management for wonopcode.
//!
//! Configuration is loaded from multiple sources and merged:
//! 1. Global config: `~/.config/wonopcode/config.json`
//! 2. Environment variable: `WONOPCODE_CONFIG_CONTENT`
//! 3. Project config: `wonopcode.json` or `wonopcode.jsonc` in project directory
//! 4. Environment overrides: `WONOPCODE_*` variables
//!
//! Supports JSONC (JSON with comments) and variable substitution:
//! - `{env:VAR_NAME}` - Substitute environment variable
//! - `{file:path}` - Substitute file contents

use crate::error::{ConfigError, CoreResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Static regex for variable substitution, compiled once.
static VAR_REGEX: OnceLock<regex::Regex> = OnceLock::new();

/// Get the variable substitution regex, compiling it once on first use.
fn var_regex() -> &'static regex::Regex {
    VAR_REGEX.get_or_init(|| {
        regex::Regex::new(r"\{(env|file):([^}]+)\}")
            .expect("Invalid regex pattern - this is a compile-time constant")
    })
}

/// Main configuration structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// JSON Schema reference.
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// Theme name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,

    /// Log level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<LogLevel>,

    /// Primary model in "provider/model" format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Small/fast model for quick tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_model: Option<String>,

    /// Default agent name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_agent: Option<String>,

    /// Username for display.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Enable snapshot tracking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<bool>,

    /// Share mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share: Option<ShareMode>,

    /// Auto-update setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autoupdate: Option<AutoUpdate>,

    /// Disabled provider IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_providers: Option<Vec<String>>,

    /// Enabled provider IDs (whitelist mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled_providers: Option<Vec<String>>,

    /// TUI settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tui: Option<TuiConfig>,

    /// Server settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<ServerConfig>,

    /// Keybind overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keybinds: Option<KeybindsConfig>,

    /// Custom commands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<HashMap<String, CommandConfig>>,

    /// Agent configurations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<HashMap<String, AgentConfig>>,

    /// Provider configurations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<HashMap<String, ProviderConfig>>,

    /// MCP server configurations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp: Option<HashMap<String, McpConfig>>,

    /// Permission settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<PermissionConfig>,

    /// Tool enable/disable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<HashMap<String, bool>>,

    /// Additional instructions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Vec<String>>,

    /// Compaction settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction: Option<CompactionConfig>,

    /// Enterprise settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enterprise: Option<EnterpriseConfig>,

    /// Experimental features.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<ExperimentalConfig>,

    /// Sandbox configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxConfig>,

    /// Update configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update: Option<UpdateConfig>,
}

/// Log levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Share mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShareMode {
    Manual,
    Auto,
    Disabled,
}

/// Auto-update setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AutoUpdate {
    Bool(bool),
    Notify,
}

/// TUI configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TuiConfig {
    /// Disable TUI and use basic mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,

    /// Enable mouse support.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mouse: Option<bool>,

    /// Paste mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paste: Option<PasteMode>,

    /// Enable markdown rendering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<bool>,

    /// Enable syntax highlighting for code blocks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub syntax_highlighting: Option<bool>,

    /// Show background color for code blocks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_backgrounds: Option<bool>,

    /// Render markdown tables with borders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tables: Option<bool>,

    /// Max frames per second during streaming.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming_fps: Option<u32>,

    /// Maximum messages to keep in memory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_messages: Option<usize>,

    /// Aggressive memory optimization mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub low_memory_mode: Option<bool>,

    /// Enable test/debug commands like /add_test_messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_test_commands: Option<bool>,

    // Test provider settings
    /// Enable the test model in the model selector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_model_enabled: Option<bool>,

    /// Test provider: simulate thinking/reasoning blocks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_emulate_thinking: Option<bool>,

    /// Test provider: simulate tool calls (standard execution).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_emulate_tool_calls: Option<bool>,

    /// Test provider: simulate observed tools (CLI-style external execution).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_emulate_tool_observed: Option<bool>,

    /// Test provider: simulate streaming delays.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_emulate_streaming: Option<bool>,
}

impl TuiConfig {
    /// Merge with another TuiConfig, preferring values from other if present.
    pub fn merge(mut self, other: Self) -> Self {
        if other.disabled.is_some() {
            self.disabled = other.disabled;
        }
        if other.mouse.is_some() {
            self.mouse = other.mouse;
        }
        if other.paste.is_some() {
            self.paste = other.paste;
        }
        if other.markdown.is_some() {
            self.markdown = other.markdown;
        }
        if other.syntax_highlighting.is_some() {
            self.syntax_highlighting = other.syntax_highlighting;
        }
        if other.code_backgrounds.is_some() {
            self.code_backgrounds = other.code_backgrounds;
        }
        if other.tables.is_some() {
            self.tables = other.tables;
        }
        if other.streaming_fps.is_some() {
            self.streaming_fps = other.streaming_fps;
        }
        if other.max_messages.is_some() {
            self.max_messages = other.max_messages;
        }
        if other.low_memory_mode.is_some() {
            self.low_memory_mode = other.low_memory_mode;
        }
        if other.enable_test_commands.is_some() {
            self.enable_test_commands = other.enable_test_commands;
        }
        if other.test_model_enabled.is_some() {
            self.test_model_enabled = other.test_model_enabled;
        }
        if other.test_emulate_thinking.is_some() {
            self.test_emulate_thinking = other.test_emulate_thinking;
        }
        if other.test_emulate_tool_calls.is_some() {
            self.test_emulate_tool_calls = other.test_emulate_tool_calls;
        }
        if other.test_emulate_tool_observed.is_some() {
            self.test_emulate_tool_observed = other.test_emulate_tool_observed;
        }
        if other.test_emulate_streaming.is_some() {
            self.test_emulate_streaming = other.test_emulate_streaming;
        }
        self
    }
}

/// Paste mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PasteMode {
    Bracketed,
    Direct,
}

/// Server configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Disable server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,

    /// Server port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// API key for MCP endpoint authentication.
    ///
    /// If set, clients must provide this key in the `X-API-Key` header
    /// or `Authorization: Bearer <key>` header when connecting to MCP endpoints.
    ///
    /// Supports variable substitution: `{env:WONOPCODE_API_KEY}`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// Keybind configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindsConfig {
    /// Leader key prefix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leader: Option<String>,

    /// App exit keybind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_exit: Option<String>,

    /// Open editor keybind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor_open: Option<String>,

    /// Theme list keybind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_list: Option<String>,

    /// Sidebar toggle keybind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar_toggle: Option<String>,

    /// New session keybind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_new: Option<String>,

    /// Session list keybind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_list: Option<String>,

    // Additional keybinds can be added as needed
    /// Additional keybinds as a map.
    #[serde(flatten)]
    pub extra: HashMap<String, String>,
}

/// Custom command configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandConfig {
    /// Command template.
    pub template: String,

    /// Command description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Agent to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,

    /// Model to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Run as subtask.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask: Option<bool>,
}

/// Agent configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    /// Model override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Top-p sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// Custom prompt/instructions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,

    /// Tool enable/disable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<HashMap<String, bool>>,

    /// Disable this agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable: Option<bool>,

    /// Agent description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Agent mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<AgentMode>,

    /// Display color (hex).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    /// Maximum steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_steps: Option<u32>,

    /// Permission overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<AgentPermissionConfig>,

    /// Per-agent sandbox configuration overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<AgentSandboxConfig>,
}

/// Per-agent sandbox configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSandboxConfig {
    /// Override sandbox enabled state for this agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Mount workspace as writable (default: true).
    /// Set to false for read-only exploration agents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_writable: Option<bool>,

    /// Override network policy for this agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// Additional tools that bypass sandbox for this agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bypass_tools: Option<Vec<String>>,

    /// Override resource limits for this agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<SandboxResourcesConfig>,
}

/// Agent mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    Subagent,
    Primary,
    All,
}

/// Agent permission configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentPermissionConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<Permission>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bash: Option<PermissionOrMap>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<PermissionOrMap>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub webfetch: Option<Permission>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub doom_loop: Option<Permission>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_directory: Option<Permission>,
}

/// Permission value or pattern map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PermissionOrMap {
    Single(Permission),
    Map(HashMap<String, Permission>),
}

/// Permission level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    Ask,
    Allow,
    Deny,
}

/// Provider configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    /// API type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,

    /// Display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Environment variables to check for API key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<String>>,

    /// Provider ID override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Model whitelist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whitelist: Option<Vec<String>>,

    /// Model blacklist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blacklist: Option<Vec<String>>,

    /// Model-specific overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<HashMap<String, ModelOverride>>,

    /// Provider options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<ProviderOptions>,
}

/// Model override configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// Provider options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    #[serde(rename = "baseURL", skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<TimeoutConfig>,

    /// Additional options.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Timeout configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TimeoutConfig {
    Disabled(bool),
    Milliseconds(u64),
}

/// MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpConfig {
    Local(McpLocalConfig),
    Remote(McpRemoteConfig),
}

/// Local MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpLocalConfig {
    /// Command and arguments.
    pub command: Vec<String>,

    /// Environment variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<HashMap<String, String>>,

    /// Enable/disable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Timeout in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// Remote MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRemoteConfig {
    /// Server URL.
    pub url: String,

    /// Enable/disable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// HTTP headers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,

    /// OAuth configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth: Option<McpOAuthConfig>,

    /// Timeout in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// MCP OAuth configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct McpOAuthConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Global permission configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<Permission>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bash: Option<PermissionOrMap>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub webfetch: Option<Permission>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_directory: Option<Permission>,

    /// When true, allow all tool executions without prompting when running
    /// inside a sandbox. This is safe because the sandbox isolates the
    /// execution environment. Default: true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_all_in_sandbox: Option<bool>,
}

/// Compaction configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CompactionConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub prune: Option<bool>,
}

/// Enterprise configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EnterpriseConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Experimental features.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ExperimentalConfig {
    /// Additional experimental flags.
    #[serde(flatten)]
    pub flags: HashMap<String, serde_json::Value>,
}

/// Sandbox configuration for isolated tool execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxConfig {
    /// Enable sandboxing (default: false).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Sandbox runtime type: "auto", "docker", "podman", "lima", "none".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,

    /// Container image to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Resource limits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<SandboxResourcesConfig>,

    /// Network policy: "limited", "full", "none".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// Mount configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mounts: Option<SandboxMountsConfig>,

    /// Tools that bypass sandbox (run on host).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bypass_tools: Option<Vec<String>>,

    /// Keep sandbox running between commands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<bool>,
}

/// Sandbox resource limits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxResourcesConfig {
    /// Memory limit (e.g., "2G", "512M").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,

    /// CPU limit (e.g., 2.0 = 2 CPUs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpus: Option<f32>,

    /// Process (PID) limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pids: Option<u32>,
}

/// Sandbox mount configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxMountsConfig {
    /// Mount workspace as writable (default: true).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_writable: Option<bool>,

    /// Persist package caches across sessions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persist_caches: Option<bool>,

    /// Custom workspace path in container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
}

/// Update configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// Auto-update mode.
    /// - `"auto"`: Automatically install updates on startup
    /// - `"notify"`: Only notify about updates (default)
    /// - `"disabled"`: Disable update checks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto: Option<AutoUpdateMode>,

    /// Release channel: "stable", "beta", or "nightly".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<crate::version::ReleaseChannel>,

    /// Check interval in hours (default: 24).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_interval: Option<u32>,
}

/// Auto-update mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutoUpdateMode {
    /// Automatically install updates.
    Auto,
    /// Only notify about updates (default).
    #[default]
    Notify,
    /// Disable update checks.
    Disabled,
}

impl Config {
    /// Load configuration from all sources.
    ///
    /// Loading order (later sources override earlier):
    /// 1. Global config from `~/.config/wonopcode/`
    /// 2. `WONOPCODE_CONFIG_CONTENT` environment variable
    /// 3. Project config from working directory
    pub async fn load(project_dir: Option<&Path>) -> CoreResult<(Self, Vec<PathBuf>)> {
        let mut config = Config::default();
        let mut sources = Vec::new();

        // 1. Load global config
        if let Some(global_dir) = Self::global_config_dir() {
            for name in &["config.json", "wonopcode.json", "wonopcode.jsonc"] {
                let path = global_dir.join(name);
                if path.exists() {
                    let loaded = Self::load_file(&path).await?;
                    config = config.merge(loaded);
                    sources.push(path);
                    break;
                }
            }
        }

        // 2. Load from environment variable
        if let Ok(content) = std::env::var("WONOPCODE_CONFIG_CONTENT") {
            let loaded = Self::parse_jsonc(&content, "<env>")?;
            config = config.merge(loaded);
        }

        // 3. Load project config (walk up from project_dir)
        if let Some(dir) = project_dir {
            for name in &["wonopcode.jsonc", "wonopcode.json"] {
                let path = dir.join(name);
                if path.exists() {
                    let loaded = Self::load_file(&path).await?;
                    config = config.merge(loaded);
                    sources.push(path);
                    break;
                }
            }
        }

        Ok((config, sources))
    }

    /// Get the global config directory.
    ///
    /// On Unix systems, prefers `~/.config/wonopcode` (XDG standard) over
    /// the platform-specific directory for better compatibility with other CLI tools.
    pub fn global_config_dir() -> Option<PathBuf> {
        // On Unix, prefer ~/.config/wonopcode (common for CLI tools)
        #[cfg(unix)]
        {
            if let Some(home) = dirs::home_dir() {
                let xdg_config = home.join(".config").join("wonopcode");
                if xdg_config.exists() {
                    return Some(xdg_config);
                }
            }
        }

        // Fall back to platform-specific config directory
        dirs::config_dir().map(|d| d.join("wonopcode"))
    }

    /// Get all possible global config directories (for documentation/debugging).
    pub fn all_global_config_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        #[cfg(unix)]
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".config").join("wonopcode"));
        }

        if let Some(platform_dir) = dirs::config_dir() {
            dirs.push(platform_dir.join("wonopcode"));
        }

        dirs
    }

    /// Get the data directory.
    pub fn data_dir() -> Option<PathBuf> {
        dirs::data_local_dir().map(|d| d.join("wonopcode"))
    }

    /// Load configuration from a file.
    pub async fn load_file(path: &Path) -> CoreResult<Self> {
        let content = tokio::fs::read_to_string(path).await?;
        let content = Self::substitute_variables(&content, path)?;
        Self::parse_jsonc(&content, &path.display().to_string())
    }

    /// Save configuration to the project config file.
    /// If project_dir is Some, saves to `{project_dir}/wonopcode.json`.
    /// Otherwise saves to the global config directory.
    pub async fn save(&self, project_dir: Option<&Path>) -> CoreResult<()> {
        let path = if let Some(dir) = project_dir {
            dir.join("wonopcode.json")
        } else {
            let global_dir = Self::global_config_dir().ok_or_else(|| {
                ConfigError::InvalidPath("Could not determine config directory".to_string())
            })?;

            // Ensure directory exists
            tokio::fs::create_dir_all(&global_dir).await?;
            global_dir.join("config.json")
        };

        // Serialize to pretty JSON
        let content = serde_json::to_string_pretty(self).map_err(|e| ConfigError::InvalidJson {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;

        tokio::fs::write(&path, content).await?;
        tracing::info!("Saved configuration to {}", path.display());
        Ok(())
    }

    /// Save only specific fields to the project config (for partial updates).
    /// This loads existing config, merges with changes, and saves.
    pub async fn save_partial(&self, project_dir: Option<&Path>) -> CoreResult<()> {
        let path = if let Some(dir) = project_dir {
            dir.join("wonopcode.json")
        } else {
            let global_dir = Self::global_config_dir().ok_or_else(|| {
                ConfigError::InvalidPath("Could not determine config directory".to_string())
            })?;
            tokio::fs::create_dir_all(&global_dir).await?;
            global_dir.join("config.json")
        };

        // Load existing config if it exists
        let existing = if path.exists() {
            Self::load_file(&path).await.unwrap_or_default()
        } else {
            Config::default()
        };

        // Merge: existing config + our changes
        let merged = existing.merge(self.clone());

        // Serialize and save
        let content =
            serde_json::to_string_pretty(&merged).map_err(|e| ConfigError::InvalidJson {
                path: path.display().to_string(),
                message: e.to_string(),
            })?;

        tokio::fs::write(&path, content).await?;
        tracing::info!("Saved configuration to {}", path.display());
        Ok(())
    }

    /// Parse JSONC (JSON with comments).
    fn parse_jsonc(content: &str, source: &str) -> CoreResult<Self> {
        // Strip comments (// and /* */)
        let stripped = Self::strip_comments(content);

        serde_json::from_str(&stripped).map_err(|e| {
            ConfigError::InvalidJson {
                path: source.to_string(),
                message: e.to_string(),
            }
            .into()
        })
    }

    /// Strip JSON comments.
    fn strip_comments(input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let mut chars = input.chars().peekable();
        let mut in_string = false;
        let mut escape_next = false;

        while let Some(c) = chars.next() {
            if escape_next {
                result.push(c);
                escape_next = false;
                continue;
            }

            if c == '\\' && in_string {
                result.push(c);
                escape_next = true;
                continue;
            }

            if c == '"' {
                in_string = !in_string;
                result.push(c);
                continue;
            }

            if in_string {
                result.push(c);
                continue;
            }

            // Check for comments
            if c == '/' {
                if let Some(&next) = chars.peek() {
                    if next == '/' {
                        // Line comment - skip to end of line
                        chars.next();
                        for c in chars.by_ref() {
                            if c == '\n' {
                                result.push('\n');
                                break;
                            }
                        }
                        continue;
                    } else if next == '*' {
                        // Block comment - skip to */
                        chars.next();
                        let mut prev = ' ';
                        for c in chars.by_ref() {
                            if prev == '*' && c == '/' {
                                break;
                            }
                            // Preserve newlines for error reporting
                            if c == '\n' {
                                result.push('\n');
                            }
                            prev = c;
                        }
                        continue;
                    }
                }
            }

            result.push(c);
        }

        result
    }

    /// Substitute variables in config content.
    ///
    /// Supports:
    /// - `{env:VAR_NAME}` - Environment variable
    /// - `{file:path}` - File contents (relative to config file)
    fn substitute_variables(content: &str, config_path: &Path) -> CoreResult<String> {
        let re = var_regex();
        let config_dir = config_path.parent().unwrap_or(Path::new("."));

        let mut result = content.to_string();
        let mut last_error: Option<ConfigError> = None;

        // Process all substitutions
        for cap in re.captures_iter(content) {
            // These are guaranteed by the regex pattern to exist
            let Some(full_match) = cap.get(0).map(|m| m.as_str()) else {
                continue;
            };
            let Some(kind) = cap.get(1).map(|m| m.as_str()) else {
                continue;
            };
            let Some(value) = cap.get(2).map(|m| m.as_str()) else {
                continue;
            };

            let replacement = match kind {
                "env" => match std::env::var(value) {
                    Ok(v) => v,
                    Err(_) => {
                        last_error = Some(ConfigError::EnvVarNotFound {
                            name: value.to_string(),
                        });
                        continue;
                    }
                },
                "file" => {
                    let file_path = config_dir.join(value);
                    match std::fs::read_to_string(&file_path) {
                        Ok(v) => v.trim().to_string(),
                        Err(_) => {
                            last_error = Some(ConfigError::FileRefNotFound {
                                path: file_path.display().to_string(),
                            });
                            continue;
                        }
                    }
                }
                _ => continue,
            };

            result = result.replace(full_match, &replacement);
        }

        // Return error if any substitution failed
        if let Some(e) = last_error {
            return Err(e.into());
        }

        Ok(result)
    }

    /// Merge another config into this one (other takes precedence).
    pub fn merge(mut self, other: Self) -> Self {
        // Simple fields - other overwrites if Some
        if other.schema.is_some() {
            self.schema = other.schema;
        }
        if other.theme.is_some() {
            self.theme = other.theme;
        }
        if other.log_level.is_some() {
            self.log_level = other.log_level;
        }
        if other.model.is_some() {
            self.model = other.model;
        }
        if other.small_model.is_some() {
            self.small_model = other.small_model;
        }
        if other.default_agent.is_some() {
            self.default_agent = other.default_agent;
        }
        if other.username.is_some() {
            self.username = other.username;
        }
        if other.snapshot.is_some() {
            self.snapshot = other.snapshot;
        }
        if other.share.is_some() {
            self.share = other.share;
        }
        if other.autoupdate.is_some() {
            self.autoupdate = other.autoupdate;
        }
        if other.disabled_providers.is_some() {
            self.disabled_providers = other.disabled_providers;
        }
        if other.enabled_providers.is_some() {
            self.enabled_providers = other.enabled_providers;
        }
        if other.instructions.is_some() {
            self.instructions = other.instructions;
        }

        // Nested structs - merge field by field
        self.tui = match (self.tui, other.tui) {
            (Some(base), Some(other)) => Some(base.merge(other)),
            (base, None) => base,
            (None, other) => other,
        };
        self.server = merge_option(self.server, other.server);
        self.keybinds = merge_option(self.keybinds, other.keybinds);
        self.permission = merge_option(self.permission, other.permission);
        self.compaction = merge_option(self.compaction, other.compaction);
        self.enterprise = merge_option(self.enterprise, other.enterprise);
        self.experimental = merge_option(self.experimental, other.experimental);
        self.sandbox = merge_option(self.sandbox, other.sandbox);
        self.update = merge_option(self.update, other.update);

        // HashMaps - merge entries
        self.command = merge_hashmap(self.command, other.command);
        self.agent = merge_hashmap(self.agent, other.agent);
        self.provider = merge_hashmap(self.provider, other.provider);
        self.mcp = merge_hashmap(self.mcp, other.mcp);
        self.tools = merge_hashmap(self.tools, other.tools);

        self
    }

    /// Get the model ID parts (provider, model).
    pub fn parse_model(model: &str) -> Option<(&str, &str)> {
        model.split_once('/')
    }
}

/// Merge two Option values.
fn merge_option<T>(base: Option<T>, other: Option<T>) -> Option<T> {
    match (base, other) {
        (_, Some(o)) => Some(o),
        (b, None) => b,
    }
}

/// Merge two HashMaps.
fn merge_hashmap<K: std::hash::Hash + Eq, V>(
    base: Option<HashMap<K, V>>,
    other: Option<HashMap<K, V>>,
) -> Option<HashMap<K, V>> {
    match (base, other) {
        (Some(mut b), Some(o)) => {
            b.extend(o);
            Some(b)
        }
        (b, None) => b,
        (None, o) => o,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_comments() {
        let input = r#"{
            // Line comment
            "key": "value", // trailing comment
            /* block comment */
            "key2": "val/*not a comment*/ue"
        }"#;

        let result = Config::strip_comments(input);
        assert!(!result.contains("Line comment"));
        assert!(!result.contains("trailing comment"));
        assert!(!result.contains("block comment"));
        assert!(result.contains("val/*not a comment*/ue"));
    }

    #[test]
    fn test_parse_jsonc() {
        let input = r#"{
            // This is a comment
            "theme": "dark",
            "log_level": "debug"
        }"#;

        let config = Config::parse_jsonc(input, "test").unwrap();
        assert_eq!(config.theme, Some("dark".to_string()));
        assert_eq!(config.log_level, Some(LogLevel::Debug));
    }

    #[test]
    fn test_merge_config() {
        let base = Config {
            theme: Some("light".to_string()),
            model: Some("anthropic/claude-3-5-sonnet".to_string()),
            ..Default::default()
        };

        let other = Config {
            theme: Some("dark".to_string()),
            username: Some("alice".to_string()),
            ..Default::default()
        };

        let merged = base.merge(other);
        assert_eq!(merged.theme, Some("dark".to_string())); // overwritten
        assert_eq!(
            merged.model,
            Some("anthropic/claude-3-5-sonnet".to_string())
        ); // preserved
        assert_eq!(merged.username, Some("alice".to_string())); // added
    }

    #[test]
    fn test_parse_model() {
        assert_eq!(
            Config::parse_model("anthropic/claude-3-5-sonnet"),
            Some(("anthropic", "claude-3-5-sonnet"))
        );
        assert_eq!(Config::parse_model("invalid"), None);
    }

    #[tokio::test]
    async fn test_config_save_and_load() {
        let dir = tempfile::tempdir().unwrap();

        // Create a config with some values
        let config = Config {
            theme: Some("tokyo-night".to_string()),
            model: Some("anthropic/claude-3-5-sonnet".to_string()),
            username: Some("test-user".to_string()),
            ..Default::default()
        };

        // Save to the temp directory
        config.save(Some(dir.path())).await.unwrap();

        // Verify the file was created
        let config_path = dir.path().join("wonopcode.json");
        assert!(config_path.exists());

        // Load and verify
        let loaded = Config::load_file(&config_path).await.unwrap();
        assert_eq!(loaded.theme, Some("tokyo-night".to_string()));
        assert_eq!(
            loaded.model,
            Some("anthropic/claude-3-5-sonnet".to_string())
        );
        assert_eq!(loaded.username, Some("test-user".to_string()));
    }

    #[tokio::test]
    async fn test_config_save_partial() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("wonopcode.json");

        // Create initial config
        let initial = Config {
            theme: Some("dark".to_string()),
            model: Some("openai/gpt-4".to_string()),
            ..Default::default()
        };
        initial.save(Some(dir.path())).await.unwrap();

        // Create partial update
        let update = Config {
            theme: Some("light".to_string()),       // Change theme
            username: Some("new-user".to_string()), // Add username
            ..Default::default()
        };
        update.save_partial(Some(dir.path())).await.unwrap();

        // Load and verify merge
        let loaded = Config::load_file(&config_path).await.unwrap();
        assert_eq!(loaded.theme, Some("light".to_string())); // Updated
        assert_eq!(loaded.model, Some("openai/gpt-4".to_string())); // Preserved
        assert_eq!(loaded.username, Some("new-user".to_string())); // Added
    }

    #[tokio::test]
    async fn test_tui_render_settings_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("wonopcode.json");

        // Create config with TUI render settings
        let tui = TuiConfig {
            markdown: Some(false),
            syntax_highlighting: Some(false),
            code_backgrounds: Some(true),
            tables: Some(false),
            streaming_fps: Some(10),
            max_messages: Some(50),
            low_memory_mode: Some(true),
            ..Default::default()
        };

        let config = Config {
            tui: Some(tui),
            ..Default::default()
        };

        // Save
        config.save(Some(dir.path())).await.unwrap();

        // Load and verify
        let loaded = Config::load_file(&config_path).await.unwrap();
        let tui = loaded.tui.expect("TUI config should be present");

        assert_eq!(tui.markdown, Some(false));
        assert_eq!(tui.syntax_highlighting, Some(false));
        assert_eq!(tui.code_backgrounds, Some(true));
        assert_eq!(tui.tables, Some(false));
        assert_eq!(tui.streaming_fps, Some(10));
        assert_eq!(tui.max_messages, Some(50));
        assert_eq!(tui.low_memory_mode, Some(true));
    }

    #[tokio::test]
    async fn test_tui_config_partial_merge() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("wonopcode.json");

        // Create initial config with mouse and markdown settings
        let initial_tui = TuiConfig {
            mouse: Some(true),
            markdown: Some(true),
            syntax_highlighting: Some(true),
            ..Default::default()
        };

        let initial = Config {
            tui: Some(initial_tui),
            theme: Some("dark".to_string()),
            ..Default::default()
        };
        initial.save(Some(dir.path())).await.unwrap();

        // Create partial update that only changes markdown
        let update_tui = TuiConfig {
            markdown: Some(false), // Only this field is set
            ..Default::default()
        };

        let update = Config {
            tui: Some(update_tui),
            ..Default::default()
        };
        update.save_partial(Some(dir.path())).await.unwrap();

        // Load and verify - mouse should be preserved, markdown updated
        let loaded = Config::load_file(&config_path).await.unwrap();
        let tui = loaded.tui.expect("TUI config should be present");

        assert_eq!(tui.mouse, Some(true)); // Preserved from original
        assert_eq!(tui.markdown, Some(false)); // Updated
        assert_eq!(tui.syntax_highlighting, Some(true)); // Preserved
        assert_eq!(loaded.theme, Some("dark".to_string())); // Preserved
    }
}
