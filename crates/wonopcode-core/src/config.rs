//! Configuration management for wonopcode.
//!
//! Configuration is loaded from multiple sources and merged:
//! 1. Global config: `~/.config/wonopcode/config.json`
//! 2. Environment variable: `WONOPCODE_CONFIG_CONTENT`
//! 3. Project config: `wonopcode.json` or `wonopcode.jsonc` in project directory
//! 4. MCP server config: `.mcp.json` in project directory (standard MCP format)
//! 5. Environment overrides: `WONOPCODE_*` variables
//!
//! Supports JSONC (JSON with comments) and variable substitution:
//! - `{env:VAR_NAME}` - Substitute environment variable
//! - `{file:path}` - Substitute file contents
//!
//! ## MCP Server Configuration
//!
//! MCP servers can be configured in two ways:
//!
//! 1. **In `wonopcode.json`** (wonopcode format):
//! ```json
//! {
//!   "mcp": {
//!     "my-server": {
//!       "type": "local",
//!       "command": ["npx", "-y", "@example/mcp-server"]
//!     },
//!     "remote-server": {
//!       "type": "remote",
//!       "url": "https://api.example.com/mcp"
//!     }
//!   }
//! }
//! ```
//!
//! 2. **In `.mcp.json`** (standard MCP format, compatible with Claude Desktop/VS Code):
//! ```json
//! {
//!   "mcpServers": {
//!     "my-server": {
//!       "command": "npx",
//!       "args": ["-y", "@example/mcp-server"],
//!       "env": { "API_KEY": "..." }
//!     }
//!   }
//! }
//! ```

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

/// Strip JSON comments (// and /* */).
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

// ============================================================================
// Standard MCP JSON format (compatible with Claude Desktop, VS Code, etc.)
// ============================================================================

/// Standard MCP JSON configuration file format.
///
/// This is the format used by Claude Desktop, VS Code, and other MCP clients.
/// File: `.mcp.json` or `.vscode/mcp.json`
///
/// Supports both formats:
/// - Claude Desktop: `mcpServers` key
/// - VS Code: `servers` key
///
/// Example (Claude Desktop format):
/// ```json
/// {
///   "mcpServers": {
///     "my-server": {
///       "command": "npx",
///       "args": ["-y", "@example/server"],
///       "env": { "API_KEY": "..." }
///     }
///   }
/// }
/// ```
///
/// Example (VS Code format):
/// ```json
/// {
///   "servers": {
///     "my-server": {
///       "command": "npx",
///       "args": ["-y", "@example/server"]
///     }
///   }
/// }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct McpJsonFile {
    /// MCP server configurations (Claude Desktop format).
    #[serde(rename = "mcpServers", skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<HashMap<String, McpJsonServer>>,

    /// MCP server configurations (VS Code format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servers: Option<HashMap<String, McpJsonServer>>,

    /// Input variables for sensitive data (VS Code format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Vec<McpJsonInput>>,
}

/// Standard MCP server configuration in `.mcp.json`.
///
/// Supports both stdio (local) and HTTP/SSE (remote) servers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpJsonServer {
    /// Standard stdio server (command + args).
    Stdio(McpJsonStdioServer),
    /// HTTP/SSE remote server.
    Remote(McpJsonRemoteServer),
}

/// Stdio (local) MCP server in standard format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpJsonStdioServer {
    /// Command to run (e.g., "npx", "python", "node").
    pub command: String,

    /// Arguments for the command.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,

    /// Environment variables.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,

    /// Working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    /// Server type hint (usually "stdio" or omitted).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub server_type: Option<String>,
}

/// HTTP/SSE remote MCP server in standard format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpJsonRemoteServer {
    /// Server type: "http", "sse", or "streamable-http".
    #[serde(rename = "type")]
    pub server_type: String,

    /// Server URL.
    pub url: String,

    /// HTTP headers for authentication.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
}

/// Input variable definition for `.mcp.json` (VS Code format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpJsonInput {
    /// Input type (e.g., "promptString").
    #[serde(rename = "type")]
    pub input_type: String,

    /// Unique identifier.
    pub id: String,

    /// User-friendly description.
    pub description: String,

    /// Whether to mask input (for passwords/API keys).
    #[serde(default)]
    pub password: bool,
}

impl McpJsonFile {
    /// Load and parse a `.mcp.json` file.
    pub async fn load(path: &Path) -> CoreResult<Self> {
        let content = tokio::fs::read_to_string(path).await?;
        let content = substitute_variables(&content, path)?;
        let content = strip_comments(&content);

        serde_json::from_str(&content).map_err(|e| {
            ConfigError::InvalidJson {
                path: path.display().to_string(),
                message: e.to_string(),
            }
            .into()
        })
    }

    /// Convert to wonopcode MCP config format.
    ///
    /// Merges servers from both `mcpServers` (Claude Desktop format) and
    /// `servers` (VS Code format) keys.
    pub fn to_mcp_configs(&self) -> HashMap<String, McpConfig> {
        let mut configs = HashMap::new();

        // Helper to convert a single server
        let convert_server = |server: &McpJsonServer| -> McpConfig {
            match server {
                McpJsonServer::Stdio(stdio) => {
                    // Convert stdio server to local config
                    let mut command = vec![stdio.command.clone()];
                    command.extend(stdio.args.clone());

                    McpConfig::Local(McpLocalConfig {
                        command,
                        environment: if stdio.env.is_empty() {
                            None
                        } else {
                            Some(stdio.env.clone())
                        },
                        enabled: Some(true),
                        timeout: None,
                    })
                }
                McpJsonServer::Remote(remote) => {
                    // Convert remote server to remote config
                    McpConfig::Remote(McpRemoteConfig {
                        url: remote.url.clone(),
                        enabled: Some(true),
                        headers: if remote.headers.is_empty() {
                            None
                        } else {
                            Some(remote.headers.clone())
                        },
                        oauth: None,
                        timeout: None,
                    })
                }
            }
        };

        // Load from mcpServers (Claude Desktop format)
        if let Some(servers) = &self.mcp_servers {
            for (name, server) in servers {
                configs.insert(name.clone(), convert_server(server));
            }
        }

        // Load from servers (VS Code format) - these override mcpServers if same name
        if let Some(servers) = &self.servers {
            for (name, server) in servers {
                configs.insert(name.clone(), convert_server(server));
            }
        }

        configs
    }
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
    /// 2. Global `.mcp.json` from `~/.config/wonopcode/`
    /// 3. `WONOPCODE_CONFIG_CONTENT` environment variable
    /// 4. Project config from working directory
    /// 5. Project `.mcp.json` from working directory
    /// 6. `.vscode/mcp.json` from working directory (VS Code format)
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

            // 2. Load global .mcp.json if it exists
            let global_mcp_path = global_dir.join(".mcp.json");
            if global_mcp_path.exists() {
                match McpJsonFile::load(&global_mcp_path).await {
                    Ok(mcp_json) => {
                        let mcp_configs = mcp_json.to_mcp_configs();
                        if !mcp_configs.is_empty() {
                            tracing::info!(
                                path = %global_mcp_path.display(),
                                servers = mcp_configs.len(),
                                "Loaded global MCP servers from .mcp.json"
                            );
                            config.mcp = Some(merge_hashmap(config.mcp, Some(mcp_configs)).unwrap_or_default());
                            sources.push(global_mcp_path);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %global_mcp_path.display(),
                            error = %e,
                            "Failed to load global .mcp.json"
                        );
                    }
                }
            }
        }

        // 3. Load from environment variable
        if let Ok(content) = std::env::var("WONOPCODE_CONFIG_CONTENT") {
            let loaded = Self::parse_jsonc(&content, "<env>")?;
            config = config.merge(loaded);
        }

        // 4. Load project config (walk up from project_dir)
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

            // 5. Load project .mcp.json if it exists
            let project_mcp_path = dir.join(".mcp.json");
            if project_mcp_path.exists() {
                match McpJsonFile::load(&project_mcp_path).await {
                    Ok(mcp_json) => {
                        let mcp_configs = mcp_json.to_mcp_configs();
                        if !mcp_configs.is_empty() {
                            tracing::info!(
                                path = %project_mcp_path.display(),
                                servers = mcp_configs.len(),
                                "Loaded project MCP servers from .mcp.json"
                            );
                            config.mcp = Some(merge_hashmap(config.mcp, Some(mcp_configs)).unwrap_or_default());
                            sources.push(project_mcp_path);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %project_mcp_path.display(),
                            error = %e,
                            "Failed to load project .mcp.json"
                        );
                    }
                }
            }

            // 6. Load VS Code .vscode/mcp.json if it exists
            let vscode_mcp_path = dir.join(".vscode").join("mcp.json");
            if vscode_mcp_path.exists() {
                match McpJsonFile::load(&vscode_mcp_path).await {
                    Ok(mcp_json) => {
                        let mcp_configs = mcp_json.to_mcp_configs();
                        if !mcp_configs.is_empty() {
                            tracing::info!(
                                path = %vscode_mcp_path.display(),
                                servers = mcp_configs.len(),
                                "Loaded VS Code MCP servers from .vscode/mcp.json"
                            );
                            config.mcp = Some(merge_hashmap(config.mcp, Some(mcp_configs)).unwrap_or_default());
                            sources.push(vscode_mcp_path);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %vscode_mcp_path.display(),
                            error = %e,
                            "Failed to load VS Code .vscode/mcp.json"
                        );
                    }
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
        let content = substitute_variables(&content, path)?;
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
        let stripped = strip_comments(content);

        serde_json::from_str(&stripped).map_err(|e| {
            ConfigError::InvalidJson {
                path: source.to_string(),
                message: e.to_string(),
            }
            .into()
        })
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

    // =========================================================================
    // UX-Critical: Variable Substitution Tests
    // If these fail, users' API keys and secrets won't be loaded correctly
    // =========================================================================

    #[test]
    fn user_env_variables_are_substituted_in_config() {
        // UX: Users commonly store API keys in environment variables
        // If this fails, their provider configurations won't work
        std::env::set_var("TEST_API_KEY_12345", "secret-key-value");

        let content = r#"{"provider": {"openai": {"options": {"api_key": "{env:TEST_API_KEY_12345}"}}}}"#;
        let result = substitute_variables(content, Path::new("/tmp/config.json")).unwrap();

        assert!(
            result.contains("secret-key-value"),
            "Environment variable should be substituted"
        );
        assert!(
            !result.contains("{env:"),
            "Substitution placeholder should be removed"
        );

        std::env::remove_var("TEST_API_KEY_12345");
    }

    #[test]
    fn missing_env_variable_returns_helpful_error() {
        // UX: When users forget to set an env var, they should get a clear error
        // not a cryptic parsing failure
        let content = r#"{"api_key": "{env:NONEXISTENT_VAR_THAT_DOES_NOT_EXIST}"}"#;
        let result = substitute_variables(content, Path::new("/tmp/config.json"));

        assert!(result.is_err(), "Missing env var should return error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("NONEXISTENT_VAR_THAT_DOES_NOT_EXIST"),
            "Error should mention the missing variable name: {}",
            err
        );
    }

    #[tokio::test]
    async fn user_file_references_are_substituted() {
        // UX: Users can store secrets in files (e.g., for Docker secrets)
        let dir = tempfile::tempdir().unwrap();
        let secret_path = dir.path().join("api_key.txt");
        tokio::fs::write(&secret_path, "my-secret-from-file\n")
            .await
            .unwrap();

        let content = r#"{"api_key": "{file:api_key.txt}"}"#;
        let config_path = dir.path().join("config.json");
        let result = substitute_variables(content, &config_path).unwrap();

        assert!(
            result.contains("my-secret-from-file"),
            "File content should be substituted"
        );
        assert!(
            !result.contains("{file:"),
            "Substitution placeholder should be removed"
        );
    }

    #[test]
    fn missing_file_reference_returns_helpful_error() {
        // UX: When users reference a non-existent file, the error should be clear
        let content = r#"{"api_key": "{file:nonexistent_secret.txt}"}"#;
        let result = substitute_variables(content, Path::new("/tmp/config.json"));

        assert!(result.is_err(), "Missing file should return error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("nonexistent_secret.txt"),
            "Error should mention the missing file: {}",
            err
        );
    }

    // =========================================================================
    // UX-Critical: JSONC Comment Stripping Tests
    // If these fail, users' commented configs will fail to parse
    // =========================================================================

    #[test]
    fn test_strip_comments() {
        let input = r#"{
            // Line comment
            "key": "value", // trailing comment
            /* block comment */
            "key2": "val/*not a comment*/ue"
        }"#;

        let result = strip_comments(input);
        assert!(!result.contains("Line comment"));
        assert!(!result.contains("trailing comment"));
        assert!(!result.contains("block comment"));
        assert!(result.contains("val/*not a comment*/ue"));
    }

    #[test]
    fn test_strip_comments_escaped_quotes() {
        // Test that escaped quotes in strings don't break comment detection
        let input = r#"{"key": "value with \"escaped\" quote"}"#;
        let result = strip_comments(input);
        assert_eq!(result, input); // No change expected
    }

    #[test]
    fn test_strip_comments_multiline_block() {
        // Test multi-line block comment with newlines preserved
        let input = "{\n/* comment\nspanning\nlines */\n\"key\": \"value\"\n}";
        let result = strip_comments(input);
        assert!(result.contains("\"key\""));
        assert!(!result.contains("comment"));
        assert!(!result.contains("spanning"));
        // Newlines should be preserved
        assert!(result.contains('\n'));
    }

    #[test]
    fn test_strip_comments_no_comments() {
        // Test input without any comments
        let input = r#"{"key": "value"}"#;
        let result = strip_comments(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_strip_comments_line_ending() {
        // Test line comment at end of input (no newline)
        let input = r#"{"key": "value"} // end comment"#;
        let result = strip_comments(input);
        assert!(result.contains("\"key\""));
        assert!(!result.contains("end comment"));
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

    #[tokio::test]
    async fn test_mcp_json_stdio_server() {
        let dir = tempfile::tempdir().unwrap();
        let mcp_path = dir.path().join(".mcp.json");

        // Create .mcp.json with stdio server (Claude Desktop format)
        let mcp_json = r#"{
            "mcpServers": {
                "my-server": {
                    "command": "npx",
                    "args": ["-y", "@example/mcp-server"],
                    "env": {
                        "API_KEY": "test-key"
                    }
                }
            }
        }"#;
        tokio::fs::write(&mcp_path, mcp_json).await.unwrap();

        // Load and verify
        let mcp_file = McpJsonFile::load(&mcp_path).await.unwrap();
        let configs = mcp_file.to_mcp_configs();

        assert_eq!(configs.len(), 1);
        let config = configs.get("my-server").unwrap();
        match config {
            McpConfig::Local(local) => {
                assert_eq!(local.command, vec!["npx", "-y", "@example/mcp-server"]);
                let env = local.environment.as_ref().unwrap();
                assert_eq!(env.get("API_KEY"), Some(&"test-key".to_string()));
            }
            McpConfig::Remote(_) => panic!("Expected local config"),
        }
    }

    #[tokio::test]
    async fn test_mcp_json_remote_server() {
        let dir = tempfile::tempdir().unwrap();
        let mcp_path = dir.path().join(".mcp.json");

        // Create .mcp.json with remote server
        let mcp_json = r#"{
            "mcpServers": {
                "remote-server": {
                    "type": "sse",
                    "url": "https://api.example.com/mcp",
                    "headers": {
                        "Authorization": "Bearer token123"
                    }
                }
            }
        }"#;
        tokio::fs::write(&mcp_path, mcp_json).await.unwrap();

        // Load and verify
        let mcp_file = McpJsonFile::load(&mcp_path).await.unwrap();
        let configs = mcp_file.to_mcp_configs();

        assert_eq!(configs.len(), 1);
        let config = configs.get("remote-server").unwrap();
        match config {
            McpConfig::Remote(remote) => {
                assert_eq!(remote.url, "https://api.example.com/mcp");
                let headers = remote.headers.as_ref().unwrap();
                assert_eq!(
                    headers.get("Authorization"),
                    Some(&"Bearer token123".to_string())
                );
            }
            McpConfig::Local(_) => panic!("Expected remote config"),
        }
    }

    #[tokio::test]
    async fn test_mcp_json_loaded_with_config() {
        let dir = tempfile::tempdir().unwrap();

        // Create .mcp.json with a server
        let mcp_json = r#"{
            "mcpServers": {
                "test-server": {
                    "command": "node",
                    "args": ["server.js"]
                }
            }
        }"#;
        tokio::fs::write(dir.path().join(".mcp.json"), mcp_json)
            .await
            .unwrap();

        // Load config from directory
        let (config, sources) = Config::load(Some(dir.path())).await.unwrap();

        // Verify .mcp.json was loaded
        assert!(sources
            .iter()
            .any(|s| s.file_name() == Some(std::ffi::OsStr::new(".mcp.json"))));

        // Verify MCP servers were loaded
        let mcp = config.mcp.expect("MCP config should be present");
        assert!(mcp.contains_key("test-server"));
    }

    #[tokio::test]
    async fn test_mcp_json_merged_with_wonopcode_config() {
        let dir = tempfile::tempdir().unwrap();

        // Create wonopcode.json with one MCP server
        let wonop_json = r#"{
            "mcp": {
                "wonop-server": {
                    "type": "remote",
                    "url": "https://wonop.example.com/mcp"
                }
            }
        }"#;
        tokio::fs::write(dir.path().join("wonopcode.json"), wonop_json)
            .await
            .unwrap();

        // Create .mcp.json with another server
        let mcp_json = r#"{
            "mcpServers": {
                "mcp-server": {
                    "command": "npx",
                    "args": ["server"]
                }
            }
        }"#;
        tokio::fs::write(dir.path().join(".mcp.json"), mcp_json)
            .await
            .unwrap();

        // Load config from directory
        let (config, _) = Config::load(Some(dir.path())).await.unwrap();

        // Verify both servers were loaded
        let mcp = config.mcp.expect("MCP config should be present");
        assert_eq!(mcp.len(), 2);
        assert!(mcp.contains_key("wonop-server"));
        assert!(mcp.contains_key("mcp-server"));
    }

    #[tokio::test]
    async fn test_vscode_mcp_format() {
        let dir = tempfile::tempdir().unwrap();
        let mcp_path = dir.path().join(".mcp.json");

        // Create .mcp.json with VS Code format (servers key)
        let mcp_json = r#"{
            "servers": {
                "vscode-server": {
                    "command": "npx",
                    "args": ["-y", "@vscode/mcp-server"]
                }
            }
        }"#;
        tokio::fs::write(&mcp_path, mcp_json).await.unwrap();

        // Load and verify
        let mcp_file = McpJsonFile::load(&mcp_path).await.unwrap();
        let configs = mcp_file.to_mcp_configs();

        assert_eq!(configs.len(), 1);
        assert!(configs.contains_key("vscode-server"));
    }

    #[tokio::test]
    async fn test_vscode_mcp_json_in_dotdir() {
        let dir = tempfile::tempdir().unwrap();

        // Create .vscode directory and mcp.json
        let vscode_dir = dir.path().join(".vscode");
        tokio::fs::create_dir_all(&vscode_dir).await.unwrap();

        let mcp_json = r#"{
            "servers": {
                "vscode-server": {
                    "command": "node",
                    "args": ["server.js"]
                }
            }
        }"#;
        tokio::fs::write(vscode_dir.join("mcp.json"), mcp_json)
            .await
            .unwrap();

        // Load config from directory
        let (config, sources) = Config::load(Some(dir.path())).await.unwrap();

        // Verify .vscode/mcp.json was loaded
        assert!(sources.iter().any(|s| s
            .to_string_lossy()
            .contains(".vscode/mcp.json")));

        // Verify MCP servers were loaded
        let mcp = config.mcp.expect("MCP config should be present");
        assert!(mcp.contains_key("vscode-server"));
    }

    // =========================================================================
    // UX-Critical: Config Loading Priority Tests
    // If these fail, users' project-specific settings won't override global ones
    // =========================================================================

    #[tokio::test]
    async fn project_config_overrides_global_settings() {
        // UX: Users expect project config to override their global config
        // This is critical for per-project model selection, permissions, etc.
        let project_dir = tempfile::tempdir().unwrap();

        // Create project config with specific settings
        let project_config = r#"{
            "model": "anthropic/claude-3-5-sonnet",
            "theme": "tokyo-night"
        }"#;
        tokio::fs::write(project_dir.path().join("wonopcode.json"), project_config)
            .await
            .unwrap();

        // Load config
        let (config, _) = Config::load(Some(project_dir.path())).await.unwrap();

        // Verify project settings are applied
        assert_eq!(
            config.model,
            Some("anthropic/claude-3-5-sonnet".to_string())
        );
        assert_eq!(config.theme, Some("tokyo-night".to_string()));
    }

    #[tokio::test]
    async fn config_loads_without_any_config_files() {
        // UX: App should work out of the box without requiring config files
        let empty_dir = tempfile::tempdir().unwrap();

        let (config, sources) = Config::load(Some(empty_dir.path())).await.unwrap();

        // Should return default config with no sources
        assert!(sources.is_empty() || sources.iter().all(|s| !s.starts_with(empty_dir.path())));
        // Default values should be usable
        assert!(config.theme.is_none()); // Will use built-in default
        assert!(config.model.is_none()); // Will use built-in default
    }

    #[tokio::test]
    async fn invalid_json_shows_file_path_in_error() {
        // UX: When config parsing fails, users need to know WHICH file is broken
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("wonopcode.json");

        // Create invalid JSON
        tokio::fs::write(&config_path, r#"{ "theme": "dark", invalid }"#)
            .await
            .unwrap();

        let result = Config::load_file(&config_path).await;

        assert!(result.is_err(), "Invalid JSON should return error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("wonopcode.json") || err.contains("invalid"),
            "Error should mention the file or parse error: {}",
            err
        );
    }

    // =========================================================================
    // UX-Critical: Permission Configuration Tests
    // If these fail, security-critical permission settings won't work
    // =========================================================================

    #[test]
    fn permission_config_parses_simple_values() {
        // UX: Users configure permissions to control what the AI can do
        let json = r#"{
            "permission": {
                "edit": "allow",
                "webfetch": "ask",
                "external_directory": "deny"
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        let perm = config.permission.unwrap();

        assert_eq!(perm.edit, Some(Permission::Allow));
        assert_eq!(perm.webfetch, Some(Permission::Ask));
        assert_eq!(perm.external_directory, Some(Permission::Deny));
    }

    #[test]
    fn permission_config_parses_pattern_maps() {
        // UX: Users can set different permissions for different command patterns
        let json = r#"{
            "permission": {
                "bash": {
                    "git *": "allow",
                    "rm -rf *": "deny",
                    "*": "ask"
                }
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        let perm = config.permission.unwrap();

        match perm.bash {
            Some(PermissionOrMap::Map(map)) => {
                assert_eq!(map.get("git *"), Some(&Permission::Allow));
                assert_eq!(map.get("rm -rf *"), Some(&Permission::Deny));
                assert_eq!(map.get("*"), Some(&Permission::Ask));
            }
            _ => panic!("Expected permission map for bash"),
        }
    }

    // =========================================================================
    // UX-Critical: Sandbox Configuration Tests
    // If these fail, isolated execution won't work correctly
    // =========================================================================

    #[test]
    fn sandbox_config_parses_all_options() {
        // UX: Users configure sandbox for secure code execution
        let json = r#"{
            "sandbox": {
                "enabled": true,
                "runtime": "docker",
                "image": "node:20",
                "network": "limited",
                "resources": {
                    "memory": "2G",
                    "cpus": 2.0,
                    "pids": 100
                },
                "mounts": {
                    "workspace_writable": true,
                    "persist_caches": true
                },
                "bypass_tools": ["read", "glob"]
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        let sandbox = config.sandbox.unwrap();

        assert_eq!(sandbox.enabled, Some(true));
        assert_eq!(sandbox.runtime, Some("docker".to_string()));
        assert_eq!(sandbox.image, Some("node:20".to_string()));
        assert_eq!(sandbox.network, Some("limited".to_string()));

        let resources = sandbox.resources.unwrap();
        assert_eq!(resources.memory, Some("2G".to_string()));
        assert_eq!(resources.cpus, Some(2.0));
        assert_eq!(resources.pids, Some(100));

        let mounts = sandbox.mounts.unwrap();
        assert_eq!(mounts.workspace_writable, Some(true));
        assert_eq!(mounts.persist_caches, Some(true));

        let bypass = sandbox.bypass_tools.unwrap();
        assert!(bypass.contains(&"read".to_string()));
        assert!(bypass.contains(&"glob".to_string()));
    }

    // =========================================================================
    // UX-Critical: Agent Configuration Tests
    // If these fail, custom agents won't work correctly
    // =========================================================================

    #[test]
    fn agent_config_with_all_options() {
        // UX: Users create custom agents with specific behaviors
        let json = r##"{
            "agent": {
                "code-review": {
                    "model": "anthropic/claude-3-5-sonnet",
                    "temperature": 0.3,
                    "prompt": "You are a code reviewer. Be thorough and constructive.",
                    "tools": {
                        "bash": false,
                        "read": true,
                        "write": false
                    },
                    "mode": "subagent",
                    "max_steps": 10,
                    "color": "#FF5733"
                }
            }
        }"##;

        let config: Config = serde_json::from_str(json).unwrap();
        let agents = config.agent.unwrap();
        let reviewer = agents.get("code-review").unwrap();

        assert_eq!(
            reviewer.model,
            Some("anthropic/claude-3-5-sonnet".to_string())
        );
        assert_eq!(reviewer.temperature, Some(0.3));
        assert!(reviewer.prompt.as_ref().unwrap().contains("code reviewer"));
        assert_eq!(reviewer.mode, Some(AgentMode::Subagent));
        assert_eq!(reviewer.max_steps, Some(10));
        assert_eq!(reviewer.color, Some("#FF5733".to_string()));

        let tools = reviewer.tools.as_ref().unwrap();
        assert_eq!(tools.get("bash"), Some(&false));
        assert_eq!(tools.get("read"), Some(&true));
        assert_eq!(tools.get("write"), Some(&false));
    }

    // =========================================================================
    // UX-Critical: Model Parsing Tests
    // If these fail, users can't switch between AI providers
    // =========================================================================

    #[test]
    fn model_string_parses_provider_and_name() {
        // UX: Users specify models as "provider/model-name"
        assert_eq!(
            Config::parse_model("anthropic/claude-3-5-sonnet"),
            Some(("anthropic", "claude-3-5-sonnet"))
        );
        assert_eq!(
            Config::parse_model("openai/gpt-4o"),
            Some(("openai", "gpt-4o"))
        );
        assert_eq!(
            Config::parse_model("google/gemini-2.0-flash"),
            Some(("google", "gemini-2.0-flash"))
        );
    }

    #[test]
    fn invalid_model_strings_return_none() {
        // UX: Invalid model strings should be handled gracefully
        assert_eq!(Config::parse_model("invalid"), None);
        assert_eq!(Config::parse_model(""), None);
        assert_eq!(Config::parse_model("no-slash-here"), None);
    }

    #[test]
    fn model_with_multiple_slashes_parses_correctly() {
        // UX: Some model names might contain slashes (e.g., org/repo/model)
        let result = Config::parse_model("openrouter/anthropic/claude-3");
        // Should split on first slash
        assert_eq!(result, Some(("openrouter", "anthropic/claude-3")));
    }

    // =========================================================================
    // UX-Critical: TUI Configuration Tests
    // If these fail, the user interface won't behave as configured
    // =========================================================================

    #[test]
    fn tui_config_merge_preserves_unset_fields() {
        // UX: When updating TUI settings, only changed fields should be affected
        let base = TuiConfig {
            mouse: Some(true),
            markdown: Some(true),
            syntax_highlighting: Some(true),
            ..Default::default()
        };

        let update = TuiConfig {
            markdown: Some(false), // Only changing this
            ..Default::default()
        };

        let merged = base.merge(update);

        assert_eq!(merged.mouse, Some(true)); // Preserved
        assert_eq!(merged.markdown, Some(false)); // Updated
        assert_eq!(merged.syntax_highlighting, Some(true)); // Preserved
    }

    #[test]
    fn tui_config_merge_all_fields() {
        // Test that all TuiConfig fields can be merged
        let base = TuiConfig {
            disabled: Some(false),
            mouse: Some(true),
            paste: Some(PasteMode::Bracketed),
            markdown: Some(true),
            syntax_highlighting: Some(true),
            code_backgrounds: Some(false),
            tables: Some(true),
            streaming_fps: Some(30),
            max_messages: Some(100),
            low_memory_mode: Some(false),
            enable_test_commands: Some(false),
            test_model_enabled: Some(false),
            test_emulate_thinking: Some(false),
            test_emulate_tool_calls: Some(false),
            test_emulate_tool_observed: Some(false),
            test_emulate_streaming: Some(false),
        };

        // Update with new values for all fields
        let update = TuiConfig {
            disabled: Some(true),
            mouse: Some(false),
            paste: Some(PasteMode::Direct),
            markdown: Some(false),
            syntax_highlighting: Some(false),
            code_backgrounds: Some(true),
            tables: Some(false),
            streaming_fps: Some(60),
            max_messages: Some(200),
            low_memory_mode: Some(true),
            enable_test_commands: Some(true),
            test_model_enabled: Some(true),
            test_emulate_thinking: Some(true),
            test_emulate_tool_calls: Some(true),
            test_emulate_tool_observed: Some(true),
            test_emulate_streaming: Some(true),
        };

        let merged = base.merge(update);

        // All fields should be updated
        assert_eq!(merged.disabled, Some(true));
        assert_eq!(merged.mouse, Some(false));
        assert_eq!(merged.paste, Some(PasteMode::Direct));
        assert_eq!(merged.markdown, Some(false));
        assert_eq!(merged.syntax_highlighting, Some(false));
        assert_eq!(merged.code_backgrounds, Some(true));
        assert_eq!(merged.tables, Some(false));
        assert_eq!(merged.streaming_fps, Some(60));
        assert_eq!(merged.max_messages, Some(200));
        assert_eq!(merged.low_memory_mode, Some(true));
        assert_eq!(merged.enable_test_commands, Some(true));
        assert_eq!(merged.test_model_enabled, Some(true));
        assert_eq!(merged.test_emulate_thinking, Some(true));
        assert_eq!(merged.test_emulate_tool_calls, Some(true));
        assert_eq!(merged.test_emulate_tool_observed, Some(true));
        assert_eq!(merged.test_emulate_streaming, Some(true));
    }

    #[test]
    fn tui_config_default() {
        let config = TuiConfig::default();
        assert!(config.disabled.is_none());
        assert!(config.mouse.is_none());
        assert!(config.paste.is_none());
        assert!(config.markdown.is_none());
        assert!(config.syntax_highlighting.is_none());
        assert!(config.code_backgrounds.is_none());
        assert!(config.tables.is_none());
        assert!(config.streaming_fps.is_none());
        assert!(config.max_messages.is_none());
        assert!(config.low_memory_mode.is_none());
        assert!(config.enable_test_commands.is_none());
        assert!(config.test_model_enabled.is_none());
        assert!(config.test_emulate_thinking.is_none());
        assert!(config.test_emulate_tool_calls.is_none());
        assert!(config.test_emulate_tool_observed.is_none());
        assert!(config.test_emulate_streaming.is_none());
    }

    #[test]
    fn paste_mode_serialization() {
        let bracketed = PasteMode::Bracketed;
        let json = serde_json::to_string(&bracketed).unwrap();
        assert_eq!(json, r#""bracketed""#);

        let direct = PasteMode::Direct;
        let json = serde_json::to_string(&direct).unwrap();
        assert_eq!(json, r#""direct""#);

        let parsed: PasteMode = serde_json::from_str(r#""bracketed""#).unwrap();
        assert_eq!(parsed, PasteMode::Bracketed);
    }

    #[test]
    fn log_level_serialization() {
        let debug = LogLevel::Debug;
        let json = serde_json::to_string(&debug).unwrap();
        assert_eq!(json, r#""debug""#);

        let info = LogLevel::Info;
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(json, r#""info""#);

        let warn = LogLevel::Warn;
        let json = serde_json::to_string(&warn).unwrap();
        assert_eq!(json, r#""warn""#);

        let error = LogLevel::Error;
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, r#""error""#);

        let parsed: LogLevel = serde_json::from_str(r#""debug""#).unwrap();
        assert_eq!(parsed, LogLevel::Debug);
    }

    #[test]
    fn share_mode_serialization() {
        let manual = ShareMode::Manual;
        let json = serde_json::to_string(&manual).unwrap();
        assert_eq!(json, r#""manual""#);

        let auto = ShareMode::Auto;
        let json = serde_json::to_string(&auto).unwrap();
        assert_eq!(json, r#""auto""#);

        let disabled = ShareMode::Disabled;
        let json = serde_json::to_string(&disabled).unwrap();
        assert_eq!(json, r#""disabled""#);

        let parsed: ShareMode = serde_json::from_str(r#""manual""#).unwrap();
        assert_eq!(parsed, ShareMode::Manual);
    }

    #[test]
    fn auto_update_serialization() {
        let enabled = AutoUpdate::Bool(true);
        let json = serde_json::to_string(&enabled).unwrap();
        assert_eq!(json, "true");

        let disabled = AutoUpdate::Bool(false);
        let json = serde_json::to_string(&disabled).unwrap();
        assert_eq!(json, "false");

        let parsed: AutoUpdate = serde_json::from_str("true").unwrap();
        assert_eq!(parsed, AutoUpdate::Bool(true));
    }

    #[test]
    fn agent_mode_serialization() {
        let subagent = AgentMode::Subagent;
        let json = serde_json::to_string(&subagent).unwrap();
        assert_eq!(json, r#""subagent""#);

        let primary = AgentMode::Primary;
        let json = serde_json::to_string(&primary).unwrap();
        assert_eq!(json, r#""primary""#);

        let all = AgentMode::All;
        let json = serde_json::to_string(&all).unwrap();
        assert_eq!(json, r#""all""#);

        let parsed: AgentMode = serde_json::from_str(r#""subagent""#).unwrap();
        assert_eq!(parsed, AgentMode::Subagent);
    }

    #[test]
    fn timeout_config_serialization() {
        let disabled = TimeoutConfig::Disabled(false);
        let json = serde_json::to_string(&disabled).unwrap();
        assert_eq!(json, "false");

        let ms = TimeoutConfig::Milliseconds(5000);
        let json = serde_json::to_string(&ms).unwrap();
        assert_eq!(json, "5000");

        let parsed: TimeoutConfig = serde_json::from_str("false").unwrap();
        match parsed {
            TimeoutConfig::Disabled(v) => assert!(!v),
            _ => panic!("Expected Disabled"),
        }

        let parsed: TimeoutConfig = serde_json::from_str("5000").unwrap();
        match parsed {
            TimeoutConfig::Milliseconds(v) => assert_eq!(v, 5000),
            _ => panic!("Expected Milliseconds"),
        }
    }

    #[test]
    fn auto_update_mode_serialization() {
        let auto = AutoUpdateMode::Auto;
        let json = serde_json::to_string(&auto).unwrap();
        assert_eq!(json, r#""auto""#);

        let notify = AutoUpdateMode::Notify;
        let json = serde_json::to_string(&notify).unwrap();
        assert_eq!(json, r#""notify""#);

        let disabled = AutoUpdateMode::Disabled;
        let json = serde_json::to_string(&disabled).unwrap();
        assert_eq!(json, r#""disabled""#);

        let parsed: AutoUpdateMode = serde_json::from_str(r#""auto""#).unwrap();
        assert_eq!(parsed, AutoUpdateMode::Auto);

        // Test default
        let default = AutoUpdateMode::default();
        assert_eq!(default, AutoUpdateMode::Notify);
    }

    #[test]
    fn config_default() {
        let config = Config::default();
        assert!(config.schema.is_none());
        assert!(config.theme.is_none());
        assert!(config.log_level.is_none());
        assert!(config.model.is_none());
        assert!(config.small_model.is_none());
        assert!(config.default_agent.is_none());
        assert!(config.username.is_none());
        assert!(config.snapshot.is_none());
        assert!(config.share.is_none());
        assert!(config.autoupdate.is_none());
    }

    #[test]
    fn mcp_local_config_serialization() {
        let config = McpLocalConfig {
            command: vec!["npx".to_string(), "server".to_string()],
            environment: Some([("KEY".to_string(), "value".to_string())].into()),
            enabled: Some(true),
            timeout: Some(5000),
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("npx"));
        assert!(json.contains("KEY"));

        let parsed: McpLocalConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.command, vec!["npx", "server"]);
        assert_eq!(parsed.enabled, Some(true));
        assert_eq!(parsed.timeout, Some(5000));
    }

    #[test]
    fn mcp_remote_config_serialization() {
        let config = McpRemoteConfig {
            url: "https://example.com/mcp".to_string(),
            enabled: Some(true),
            headers: Some([("Auth".to_string(), "token".to_string())].into()),
            oauth: None,
            timeout: Some(10000),
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("example.com"));

        let parsed: McpRemoteConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.url, "https://example.com/mcp");
        assert_eq!(parsed.timeout, Some(10000));
    }

    #[test]
    fn mcp_oauth_config_default() {
        let oauth = McpOAuthConfig::default();
        assert!(oauth.client_id.is_none());
        assert!(oauth.client_secret.is_none());
        assert!(oauth.scope.is_none());
    }

    #[test]
    fn keybinds_config_default() {
        let config = KeybindsConfig::default();
        assert!(config.leader.is_none());
        assert!(config.app_exit.is_none());
        assert!(config.editor_open.is_none());
        assert!(config.theme_list.is_none());
        assert!(config.sidebar_toggle.is_none());
        assert!(config.session_new.is_none());
        assert!(config.session_list.is_none());
        assert!(config.extra.is_empty());
    }

    #[test]
    fn server_config_default() {
        let config = ServerConfig::default();
        assert!(config.disabled.is_none());
        assert!(config.port.is_none());
        assert!(config.api_key.is_none());
    }

    #[test]
    fn agent_config_default() {
        let config = AgentConfig::default();
        assert!(config.model.is_none());
        assert!(config.temperature.is_none());
        assert!(config.top_p.is_none());
        assert!(config.prompt.is_none());
        assert!(config.tools.is_none());
        assert!(config.disable.is_none());
        assert!(config.description.is_none());
        assert!(config.mode.is_none());
        assert!(config.color.is_none());
        assert!(config.max_steps.is_none());
        assert!(config.permission.is_none());
        assert!(config.sandbox.is_none());
    }

    #[test]
    fn provider_config_default() {
        let config = ProviderConfig::default();
        assert!(config.api.is_none());
        assert!(config.name.is_none());
        assert!(config.env.is_none());
        assert!(config.id.is_none());
        assert!(config.whitelist.is_none());
        assert!(config.blacklist.is_none());
        assert!(config.models.is_none());
        assert!(config.options.is_none());
    }

    #[test]
    fn provider_options_default() {
        let options = ProviderOptions::default();
        assert!(options.api_key.is_none());
        assert!(options.base_url.is_none());
        assert!(options.timeout.is_none());
        assert!(options.extra.is_empty());
    }

    #[test]
    fn model_override_default() {
        let override_ = ModelOverride::default();
        assert!(override_.name.is_none());
        assert!(override_.context_length.is_none());
        assert!(override_.max_tokens.is_none());
    }

    #[test]
    fn permission_config_default() {
        let config = PermissionConfig::default();
        assert!(config.edit.is_none());
        assert!(config.bash.is_none());
        assert!(config.webfetch.is_none());
        assert!(config.external_directory.is_none());
        assert!(config.allow_all_in_sandbox.is_none());
    }

    #[test]
    fn compaction_config_default() {
        let config = CompactionConfig::default();
        assert!(config.auto.is_none());
        assert!(config.prune.is_none());
    }

    #[test]
    fn enterprise_config_default() {
        let config = EnterpriseConfig::default();
        assert!(config.url.is_none());
    }

    #[test]
    fn experimental_config_default() {
        let config = ExperimentalConfig::default();
        assert!(config.flags.is_empty());
    }

    #[test]
    fn sandbox_config_default() {
        let config = SandboxConfig::default();
        assert!(config.enabled.is_none());
        assert!(config.runtime.is_none());
        assert!(config.image.is_none());
        assert!(config.resources.is_none());
        assert!(config.network.is_none());
        assert!(config.mounts.is_none());
        assert!(config.bypass_tools.is_none());
        assert!(config.keep_alive.is_none());
    }

    #[test]
    fn sandbox_resources_config_default() {
        let config = SandboxResourcesConfig::default();
        assert!(config.memory.is_none());
        assert!(config.cpus.is_none());
        assert!(config.pids.is_none());
    }

    #[test]
    fn sandbox_mounts_config_default() {
        let config = SandboxMountsConfig::default();
        assert!(config.workspace_writable.is_none());
        assert!(config.persist_caches.is_none());
        assert!(config.workspace_path.is_none());
    }

    #[test]
    fn update_config_default() {
        let config = UpdateConfig::default();
        assert!(config.auto.is_none());
        assert!(config.channel.is_none());
        assert!(config.check_interval.is_none());
    }

    #[test]
    fn agent_sandbox_config_default() {
        let config = AgentSandboxConfig::default();
        assert!(config.enabled.is_none());
        assert!(config.workspace_writable.is_none());
        assert!(config.network.is_none());
        assert!(config.bypass_tools.is_none());
        assert!(config.resources.is_none());
    }

    #[test]
    fn agent_permission_config_default() {
        let config = AgentPermissionConfig::default();
        assert!(config.edit.is_none());
        assert!(config.bash.is_none());
        assert!(config.skill.is_none());
        assert!(config.webfetch.is_none());
        assert!(config.doom_loop.is_none());
        assert!(config.external_directory.is_none());
    }

    #[test]
    fn mcp_json_file_default() {
        let file = McpJsonFile::default();
        assert!(file.mcp_servers.is_none());
        assert!(file.servers.is_none());
        assert!(file.inputs.is_none());
    }

    #[test]
    fn test_global_config_dir() {
        // Just verify it doesn't panic
        let dir = Config::global_config_dir();
        if let Some(d) = dir {
            assert!(!d.as_os_str().is_empty());
        }
    }

    #[test]
    fn test_all_global_config_dirs() {
        let dirs = Config::all_global_config_dirs();
        // Should return at least one directory
        assert!(!dirs.is_empty());
    }

    #[test]
    fn test_data_dir() {
        // Just verify it doesn't panic
        let dir = Config::data_dir();
        if let Some(d) = dir {
            assert!(!d.as_os_str().is_empty());
        }
    }

    // =========================================================================
    // UX-Critical: Custom Command Configuration Tests
    // If these fail, users' custom slash commands won't work
    // =========================================================================

    #[test]
    fn custom_command_config_parses() {
        // UX: Users create custom /commands for repetitive tasks
        let json = r#"{
            "command": {
                "review": {
                    "template": "Review this code for bugs and security issues: $ARGUMENTS",
                    "description": "Review code for issues",
                    "agent": "code-review",
                    "model": "anthropic/claude-3-5-sonnet"
                },
                "test": {
                    "template": "Write tests for: $ARGUMENTS",
                    "subtask": true
                }
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        let commands = config.command.unwrap();

        let review = commands.get("review").unwrap();
        assert!(review.template.contains("$ARGUMENTS"));
        assert_eq!(review.description, Some("Review code for issues".to_string()));
        assert_eq!(review.agent, Some("code-review".to_string()));

        let test = commands.get("test").unwrap();
        assert_eq!(test.subtask, Some(true));
    }

    // =========================================================================
    // UX-Critical: Provider Configuration Tests  
    // If these fail, AI providers won't be configured correctly
    // =========================================================================

    #[test]
    fn provider_config_with_custom_base_url() {
        // UX: Users may use self-hosted or enterprise API endpoints
        let json = r#"{
            "provider": {
                "custom-openai": {
                    "api": "openai",
                    "name": "My Custom OpenAI",
                    "options": {
                        "baseURL": "https://my-proxy.example.com/v1",
                        "api_key": "my-key"
                    }
                }
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        let providers = config.provider.unwrap();
        let custom = providers.get("custom-openai").unwrap();

        assert_eq!(custom.api, Some("openai".to_string()));
        assert_eq!(custom.name, Some("My Custom OpenAI".to_string()));

        let options = custom.options.as_ref().unwrap();
        assert_eq!(
            options.base_url,
            Some("https://my-proxy.example.com/v1".to_string())
        );
    }

    #[test]
    fn provider_config_with_model_whitelist() {
        // UX: Users can restrict which models are available
        let json = r#"{
            "provider": {
                "openai": {
                    "whitelist": ["gpt-4o", "gpt-4o-mini"],
                    "blacklist": ["gpt-3.5-turbo"]
                }
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        let providers = config.provider.unwrap();
        let openai = providers.get("openai").unwrap();

        let whitelist = openai.whitelist.as_ref().unwrap();
        assert!(whitelist.contains(&"gpt-4o".to_string()));
        assert!(whitelist.contains(&"gpt-4o-mini".to_string()));

        let blacklist = openai.blacklist.as_ref().unwrap();
        assert!(blacklist.contains(&"gpt-3.5-turbo".to_string()));
    }
}
