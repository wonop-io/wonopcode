//! Claude CLI provider for subscription-based access.
//!
//! This provider uses the Claude Code CLI to access Claude models using a
//! Claude Max/Pro subscription instead of API credits.
//!
//! # Prerequisites
//!
//! 1. Install Claude Code CLI: `npm install -g @anthropic-ai/claude-code`
//! 2. Authenticate: `claude setup-token` or run `claude` interactively
//!
//! # Architecture
//!
//! ## Without Custom Tools (Default)
//!
//! ```text
//! ┌─────────────────┐     JSON/stdin/stdout     ┌──────────────┐
//! │ ClaudeCliProvider│ ◄──────────────────────► │  Claude Code │
//! │                  │                          │     CLI      │
//! └─────────────────┘                           └──────────────┘
//!                                                      │
//!                                                      │ OAuth (internal)
//!                                                      ▼
//!                                               ┌──────────────┐
//!                                               │  claude.ai   │
//!                                               └──────────────┘
//! ```
//!
//! ## With Custom Tools (MCP Mode)
//!
//! ```text
//! ┌─────────────────┐                           ┌──────────────┐
//! │ ClaudeCliProvider│ ───────────────────────► │  Claude Code │
//! │                  │   --mcp-config           │     CLI      │
//! └─────────────────┘                           └──────────────┘
//!                                                      │
//!                                          MCP │      │ OAuth
//!                                              ▼      ▼
//!                                       ┌──────────────────┐
//!                                       │ wonopcode        │
//!                                       │ mcp-serve        │
//!                                       │ (custom tools)   │
//!                                       └──────────────────┘
//! ```

use crate::{
    error::ProviderError,
    model::{ModelCost, ModelInfo},
    stream::StreamChunk,
    GenerateOptions, LanguageModel, Message, ProviderResult,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tracing::{debug, info, warn};

/// MCP transport configuration.
///
/// MCP servers are connected via HTTP/SSE.
#[derive(Debug, Clone, PartialEq)]
pub struct McpTransport {
    /// URL for the MCP SSE endpoint (e.g., "http://localhost:3000/mcp/sse").
    pub url: String,
    /// Optional headers to include in requests (e.g., for authentication).
    pub headers: HashMap<String, String>,
}

/// Configuration for MCP (Model Context Protocol) integration.
#[derive(Debug, Clone)]
pub struct McpCliConfig {
    /// Whether to use custom tools via MCP.
    pub use_custom_tools: bool,
    /// MCP transport configuration (HTTP/SSE URL).
    pub transport: McpTransport,
    /// External MCP servers to pass through.
    pub external_servers: HashMap<String, ExternalMcpServer>,
}

impl McpCliConfig {
    /// Create a new MCP config with HTTP transport.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            use_custom_tools: true,
            transport: McpTransport {
                url: url.into(),
                headers: HashMap::new(),
            },
            external_servers: HashMap::new(),
        }
    }

    /// Create a new MCP config with HTTP transport and authentication.
    pub fn with_secret(url: impl Into<String>, secret: impl Into<String>) -> Self {
        let mut headers = HashMap::new();
        headers.insert("X-API-Key".to_string(), secret.into());
        Self {
            use_custom_tools: true,
            transport: McpTransport {
                url: url.into(),
                headers,
            },
            external_servers: HashMap::new(),
        }
    }

    /// Add an external MCP server.
    pub fn with_external_server(
        mut self,
        name: impl Into<String>,
        server: ExternalMcpServer,
    ) -> Self {
        self.external_servers.insert(name.into(), server);
        self
    }
}

/// Configuration for an external MCP server.
#[derive(Debug, Clone)]
pub struct ExternalMcpServer {
    /// Command to run the server.
    pub command: String,
    /// Arguments for the command.
    pub args: Vec<String>,
    /// Environment variables.
    pub env: HashMap<String, String>,
}

/// Provider that uses Claude Code CLI for subscription-based access.
///
/// This provider spawns the Claude Code CLI as a subprocess and communicates
/// with it via JSON over stdin/stdout. The CLI handles OAuth authentication
/// with claude.ai using the user's subscription.
///
/// When `mcp_config` is set, the provider generates an MCP configuration
/// that points to wonopcode's MCP server, enabling custom tool execution.
///
/// Session resumption: The provider captures the session_id from Claude CLI
/// output and can reuse it for subsequent calls via `--resume`.
pub struct ClaudeCliProvider {
    model: ModelInfo,
    /// MCP configuration, if using custom tools.
    mcp_config: Option<McpCliConfig>,
    /// Captured session ID from the CLI for resumption.
    /// Protected by RwLock for interior mutability.
    session_id: std::sync::Arc<tokio::sync::RwLock<Option<String>>>,
}

impl ClaudeCliProvider {
    /// Create a new CLI-based provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the Claude CLI is not installed or not in PATH.
    pub fn new(model: ModelInfo) -> ProviderResult<Self> {
        // Verify CLI is available
        Self::check_cli_available()?;

        info!(model = %model.id, "Created Claude CLI provider (no custom tools)");

        Ok(Self {
            model,
            mcp_config: None,
            session_id: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        })
    }

    /// Create a new CLI provider with MCP configuration for custom tools.
    pub fn with_mcp_config(model: ModelInfo, mcp_config: McpCliConfig) -> ProviderResult<Self> {
        Self::check_cli_available()?;

        info!(
            model = %model.id,
            use_custom_tools = mcp_config.use_custom_tools,
            "Created Claude CLI provider with MCP config"
        );

        Ok(Self {
            model,
            mcp_config: Some(mcp_config),
            session_id: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        })
    }

    /// Get the current CLI session ID if one has been established.
    pub async fn get_session_id(&self) -> Option<String> {
        self.session_id.read().await.clone()
    }

    /// Set an initial session ID for resumption.
    ///
    /// This is useful when recreating a provider (e.g., when sandbox state changes)
    /// and you want to preserve the existing CLI session.
    pub async fn set_session_id(&self, session_id: Option<String>) {
        *self.session_id.write().await = session_id;
    }

    /// Clear the session ID, forcing a new session on the next call.
    pub async fn clear_session(&self) {
        *self.session_id.write().await = None;
    }

    /// Check if Claude CLI is installed and accessible.
    pub fn check_cli_available() -> ProviderResult<()> {
        let output = Command::new("claude").arg("--version").output();

        match output {
            Ok(o) if o.status.success() => {
                let version = String::from_utf8_lossy(&o.stdout);
                debug!(version = %version.trim(), "Claude CLI found");
                Ok(())
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                Err(ProviderError::internal(format!(
                    "Claude CLI returned error: {stderr}"
                )))
            }
            Err(_) => Err(ProviderError::internal(
                "Claude Code CLI not found. Install with: npm install -g @anthropic-ai/claude-code"
                    .to_string(),
            )),
        }
    }

    /// Check if CLI is authenticated with a subscription (sync, fast check).
    ///
    /// This performs a quick heuristic check by looking for the Claude CLI
    /// directory, which indicates the CLI has been set up and used.
    /// For a definitive check, use `check_auth_async()`.
    ///
    /// Returns `true` if Claude CLI appears to be set up.
    pub fn is_authenticated() -> bool {
        // Quick check: look for ~/.claude directory which indicates CLI setup
        // The actual OAuth auth is handled by the CLI itself
        if let Some(home) = dirs::home_dir() {
            let claude_dir = home.join(".claude");
            if claude_dir.exists() && claude_dir.is_dir() {
                // Check for settings.json or any session data as indicator
                let has_settings = claude_dir.join("settings.json").exists();
                let has_history = claude_dir.join("history.jsonl").exists();

                if has_settings || has_history {
                    debug!(dir = %claude_dir.display(), "Claude CLI directory found with config");
                    return true;
                }
            }
        }

        debug!("Claude CLI not configured");
        false
    }

    /// Perform a full authentication check by running a test query.
    /// This is slower but definitive. Use sparingly.
    pub async fn check_auth_async() -> bool {
        use tokio::sync::OnceCell;

        // Cache the result to avoid repeated expensive checks
        static AUTH_CHECK: OnceCell<bool> = OnceCell::const_new();

        *AUTH_CHECK
            .get_or_init(|| async { Self::check_auth_uncached_async().await })
            .await
    }

    /// Perform the actual authentication check (uncached, async).
    async fn check_auth_uncached_async() -> bool {
        let output = TokioCommand::new("claude")
            .args(["-p", "hi", "--output-format", "json"])
            .output()
            .await;

        match output {
            Ok(o) => {
                if o.status.success() {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    // Check if we got a valid JSON response (not an error)
                    if stdout.contains("\"type\":\"result\"") {
                        debug!("Claude CLI is authenticated");
                        return true;
                    }
                }
                let stderr = String::from_utf8_lossy(&o.stderr);
                debug!(stderr = %stderr.trim(), "Claude CLI not authenticated");
                false
            }
            Err(e) => {
                debug!(error = %e, "Failed to run Claude CLI auth check");
                false
            }
        }
    }

    /// Force re-check authentication status (clears cache).
    pub fn clear_auth_cache() {
        warn!("Auth cache cannot be cleared at runtime - restart the process for a fresh check");
    }

    /// Check if CLI is available (cached for performance).
    pub fn is_available() -> bool {
        use std::sync::OnceLock;

        static AVAILABLE: OnceLock<bool> = OnceLock::new();

        *AVAILABLE.get_or_init(|| Self::check_cli_available().is_ok())
    }

    /// Format messages into a prompt string for the CLI.
    fn format_messages(&self, messages: &[Message]) -> String {
        let mut parts = Vec::new();

        for msg in messages {
            let role = match msg.role {
                crate::message::Role::System => "System",
                crate::message::Role::User => "Human",
                crate::message::Role::Assistant => "Assistant",
                crate::message::Role::Tool => "Tool Result",
            };

            let content = msg.text();
            if !content.is_empty() {
                parts.push(format!("{role}: {content}"));
            }
        }

        parts.join("\n\n")
    }

    /// Extract only the last user message for session resumption.
    ///
    /// When resuming a Claude CLI session, the conversation history is already
    /// stored by the CLI. We only need to send the new user message.
    fn extract_last_user_message(&self, messages: &[Message]) -> String {
        // Find the last user message
        for msg in messages.iter().rev() {
            if matches!(msg.role, crate::message::Role::User) {
                let content = msg.text();
                if !content.is_empty() {
                    return content;
                }
            }
        }
        // Fallback: if no user message found, return empty string
        String::new()
    }

    /// Generate MCP configuration file for custom tools.
    ///
    /// Returns the path to the generated config file.
    /// Panics if mcp_config is None (should only be called when use_custom_tools is true).
    fn generate_mcp_config(&self) -> Result<PathBuf, ProviderError> {
        let mcp_config = self.mcp_config.as_ref().ok_or_else(|| {
            ProviderError::internal("MCP config required but not set".to_string())
        })?;

        // Build MCP servers configuration
        let mut mcp_servers = serde_json::Map::new();

        // Add our tools server via HTTP/SSE
        let mut server_config = serde_json::json!({
            "type": "sse",
            "url": mcp_config.transport.url
        });

        // Add headers if configured (e.g., for authentication)
        if !mcp_config.transport.headers.is_empty() {
            let headers: serde_json::Map<String, serde_json::Value> = mcp_config
                .transport
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            server_config["headers"] = serde_json::Value::Object(headers);
        }

        mcp_servers.insert("wonopcode-tools".to_string(), server_config);

        info!(
            url = %mcp_config.transport.url,
            has_auth = !mcp_config.transport.headers.is_empty(),
            "Generated MCP HTTP config"
        );

        // Add external MCP servers
        for (name, server) in &mcp_config.external_servers {
            let mut server_env = serde_json::Map::new();
            for (k, v) in &server.env {
                server_env.insert(k.clone(), serde_json::Value::String(v.clone()));
            }

            mcp_servers.insert(
                name.clone(),
                serde_json::json!({
                    "type": "sse",
                    "url": server.command,
                    "headers": server_env
                }),
            );
        }

        let config = serde_json::json!({
            "mcpServers": mcp_servers
        });

        // Write to temp file
        let config_path = std::env::temp_dir().join(format!(
            "wonopcode-mcp-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));

        std::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap())
            .map_err(|e| ProviderError::internal(format!("Failed to write MCP config: {e}")))?;

        debug!(path = %config_path.display(), "Generated MCP config");

        Ok(config_path)
    }

    /// Get the pattern for allowed tools.
    fn get_allowed_tools_pattern(&self) -> String {
        let mut patterns = vec!["mcp__wonopcode-tools__*".to_string()];

        // Add patterns for external servers
        if let Some(mcp_config) = &self.mcp_config {
            for name in mcp_config.external_servers.keys() {
                patterns.push(format!("mcp__{name}__*"));
            }
        }

        patterns.join(",")
    }

    /// Get the list of Claude CLI's built-in tools to disable.
    fn builtin_tools_to_disable() -> &'static str {
        // Disable all built-in tools when using our custom tools
        // Note: Using "Bash" not "Bash(*)" - the pattern syntax only restricts
        // specific subpatterns, it doesn't disable the entire tool.
        // AskUserQuestion is disabled because it requires interactive stdin which
        // doesn't work when Claude CLI is spawned programmatically.
        // EnterPlanMode/ExitPlanMode are disabled because we provide our own
        // implementation via MCP that properly switches the agent mode.
        "Bash,Read,Write,Edit,MultiEdit,Glob,Grep,WebSearch,WebFetch,Task,TodoRead,TodoWrite,AskUserQuestion,EnterPlanMode,ExitPlanMode"
    }
}

/// Claude CLI streaming JSON message types
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum CliMessage {
    #[serde(rename = "system")]
    System { session_id: Option<String> },
    #[serde(rename = "assistant")]
    Assistant {
        message: AssistantMessage,
        session_id: Option<String>,
    },
    #[serde(rename = "user")]
    User {
        message: UserMessage,
        /// Tool use result can be either:
        /// - An object with stdout/stderr (built-in tools)
        /// - An array of content blocks (MCP tools)
        tool_use_result: Option<serde_json::Value>,
        #[serde(default)]
        _session_id: Option<String>,
    },
    #[serde(rename = "result")]
    Result {
        result: String,
        #[serde(default)]
        is_error: bool,
        usage: Option<CliUsage>,
        session_id: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct UserMessage {
    #[serde(default)]
    _role: String,
    content: Vec<ToolResultContent>,
}

#[derive(Debug, Deserialize)]
struct ToolResultContent {
    tool_use_id: String,
    #[serde(rename = "type", default)]
    _content_type: String,
    /// Content can be either a string or an array of content blocks
    /// We use serde_json::Value to handle both cases and convert in code
    #[serde(default)]
    content: serde_json::Value,
    #[serde(default)]
    is_error: bool,
}

impl ToolResultContent {
    /// Extract the text content from the content field.
    /// Handles both string content and array of content blocks.
    fn text_content(&self) -> String {
        match &self.content {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(arr) => {
                // Content is an array like [{"type": "text", "text": "..."}]
                let mut result = String::new();
                for block in arr {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        result.push_str(text);
                    }
                }
                result
            }
            serde_json::Value::Null => String::new(),
            // For other types, convert to string representation
            other => other.to_string(),
        }
    }
}

/// Extract output from tool_use_result which can be either:
/// - An object with stdout/stderr (built-in tools like Bash)
/// - An array of content blocks (MCP tools)
fn extract_tool_result_output(result: &serde_json::Value) -> Option<String> {
    // Try object format first (built-in tools)
    if let Some(obj) = result.as_object() {
        if let Some(stdout) = obj.get("stdout").and_then(|v| v.as_str()) {
            if !stdout.is_empty() {
                return Some(stdout.to_string());
            }
        }
        if let Some(stderr) = obj.get("stderr").and_then(|v| v.as_str()) {
            if !stderr.is_empty() {
                return Some(stderr.to_string());
            }
        }
    }

    // Try array format (MCP tools) - array of content blocks like [{"type":"text","text":"..."}]
    if let Some(arr) = result.as_array() {
        let mut output = String::new();
        for block in arr {
            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                output.push_str(text);
            }
        }
        if !output.is_empty() {
            return Some(output);
        }
    }

    None
}

#[derive(Debug, Deserialize)]
struct AssistantMessage {
    content: Vec<ContentBlock>,
    usage: Option<MessageUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct MessageUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct CliUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[async_trait]
impl LanguageModel for ClaudeCliProvider {
    async fn get_cli_session_id(&self) -> Option<String> {
        self.get_session_id().await
    }

    async fn set_cli_session_id(&self, session_id: Option<String>) {
        self.set_session_id(session_id).await;
    }

    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        // Check for existing session to resume
        let existing_session = self.session_id.read().await.clone();

        // When resuming a session, Claude CLI already has the conversation history.
        // We only need to send the new user message, not the full history.
        let prompt = if existing_session.is_some() {
            // Extract only the last user message for the resumed session
            self.extract_last_user_message(&messages)
        } else {
            // New session: format the full conversation history
            self.format_messages(&messages)
        };

        // Check if we should use custom tools
        // Always use custom tools if configured, regardless of whether tools were passed
        // This ensures Claude CLI uses our MCP tools instead of its built-in tools
        let use_custom_tools = self
            .mcp_config
            .as_ref()
            .map(|c| c.use_custom_tools)
            .unwrap_or(false);

        debug!(
            mcp_use_custom_tools = use_custom_tools,
            tools_count = options.tools.len(),
            use_custom_tools = use_custom_tools,
            "Checking custom tools config"
        );

        // Generate MCP config if using custom tools
        let mcp_config_path = if use_custom_tools {
            Some(self.generate_mcp_config()?)
        } else {
            None
        };

        info!(
            model = %self.model.id,
            prompt_len = prompt.len(),
            use_custom_tools = use_custom_tools,
            resume_session = ?existing_session,
            "Sending query to Claude CLI"
        );

        // Build CLI arguments
        let mut args = vec![
            "-p".to_string(),
            prompt,
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
            "--model".to_string(),
            self.model.id.clone(),
        ];

        // Add session resumption if we have a previous session
        if let Some(ref session) = existing_session {
            args.push("--resume".to_string());
            args.push(session.clone());
            debug!(session_id = %session, "Resuming previous CLI session");
        } else {
            // Only set system prompt on new sessions (not when resuming)
            // When resuming, Claude CLI already has the system prompt from the original session
            if let Some(ref system) = options.system {
                if !system.is_empty() {
                    args.push("--system-prompt".to_string());
                    args.push(system.clone());
                    debug!(system_len = system.len(), "Setting system prompt");
                }
            }
        }

        // Add MCP config if using custom tools
        if let Some(ref config_path) = mcp_config_path {
            args.push("--mcp-config".to_string());
            args.push(config_path.to_string_lossy().to_string());

            // Allow our MCP tools
            args.push("--allowedTools".to_string());
            args.push(self.get_allowed_tools_pattern());

            // Disallow Claude's built-in tools
            args.push("--disallowedTools".to_string());
            args.push(Self::builtin_tools_to_disable().to_string());

            // Use acceptEdits permission mode to auto-accept MCP tool calls
            // This is needed because --allowedTools only controls visibility,
            // not whether Claude CLI prompts for permission on each tool use.
            // Since we're running our own tools via MCP, we want to accept them.
            args.push("--permission-mode".to_string());
            args.push("acceptEdits".to_string());

            debug!(
                mcp_config = %config_path.display(),
                allowed_tools = %self.get_allowed_tools_pattern(),
                disallowed_tools = %Self::builtin_tools_to_disable(),
                "Using MCP config for custom tools"
            );
        }

        // Log the full command for debugging
        info!(
            command = "claude",
            args = ?args,
            "Spawning Claude CLI"
        );

        // Spawn the Claude CLI process with streaming JSON output
        let mut child = TokioCommand::new("claude")
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ProviderError::internal(format!("Failed to spawn Claude CLI: {e}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::internal("Failed to capture stdout".to_string()))?;

        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        // Clone the config path for cleanup in the stream
        let config_path_for_cleanup = mcp_config_path;

        // Clone session ID handle for updating in the stream
        let session_id_handle = self.session_id.clone();

        // Clone abort token for cancellation
        let abort = options.abort.clone();

        // Create the output stream that parses CLI JSON output
        let output_stream = try_stream! {
            let mut total_text = String::new();
            let mut input_tokens: u32 = 0;
            let mut output_tokens: u32 = 0;
            let mut text_started = false;
            let mut captured_session_id: Option<String> = None;

            // Track tool calls so we can set finish_reason correctly
            let mut tool_calls: Vec<(String, String, String)> = Vec::new(); // (id, name, args)

            while let Ok(Some(line)) = lines.next_line().await {
                // Check for cancellation
                if let Some(ref token) = abort {
                    if token.is_cancelled() {
                        // Kill the child process if we can
                        let _ = child.kill().await;
                        Err(ProviderError::Cancelled)?;
                    }
                }

                if line.trim().is_empty() {
                    continue;
                }

                debug!(line_len = line.len(), "Received line from Claude CLI");

                // Try to parse as our message type
                match serde_json::from_str::<CliMessage>(&line) {
                    Ok(CliMessage::Assistant { message, session_id }) => {
                        // Capture session ID if we haven't already
                        if captured_session_id.is_none() {
                            if let Some(sid) = session_id {
                                debug!(session_id = %sid, "Captured CLI session ID");
                                captured_session_id = Some(sid);
                            }
                        }
                        // Extract content from content blocks
                        for block in message.content {
                            match block {
                                ContentBlock::Text { text } => {
                                    if !text.is_empty() {
                                        if !text_started {
                                            yield StreamChunk::TextStart;
                                            text_started = true;
                                        }
                                        debug!(text_len = text.len(), "Got text from assistant");
                                        total_text.push_str(&text);
                                        yield StreamChunk::TextDelta(text);
                                    }
                                }
                                ContentBlock::ToolUse { id, name, input } => {
                                    // Tool use - emit as observed since CLI executes tools
                                    debug!(id = %id, name = %name, "Emitting ToolObserved for CLI tool use");

                                    // End text block if it was started
                                    if text_started {
                                        yield StreamChunk::TextEnd;
                                        text_started = false;
                                    }

                                    // Convert input to JSON string
                                    let input_str = serde_json::to_string(&input)
                                        .unwrap_or_else(|_| "{}".to_string());

                                    // Track the tool call for matching with results
                                    tool_calls.push((id.clone(), name.clone(), input_str.clone()));

                                    // Emit observed tool (not ToolCall, so runner won't execute)
                                    // Note: With MCP, the CLI will call our MCP server which
                                    // actually executes the tool. We still emit ToolObserved
                                    // for TUI display purposes.
                                    yield StreamChunk::ToolObserved {
                                        id,
                                        name,
                                        input: input_str,
                                    };
                                }
                                ContentBlock::Other => {
                                    debug!("Got unknown content block type");
                                }
                            }
                        }
                        // Update token counts if available
                        if let Some(usage) = message.usage {
                            if let Some(i) = usage.input_tokens {
                                input_tokens = i as u32;
                            }
                            if let Some(o) = usage.output_tokens {
                                output_tokens = o as u32;
                            }
                        }
                    }
                    Ok(CliMessage::User { message, tool_use_result, .. }) => {
                        // Tool result from CLI's tool execution (via MCP or built-in)
                        debug!(content_count = message.content.len(), has_result = tool_use_result.is_some(), "Received tool result from CLI");

                        // Extract tool_use_id from the message content
                        if let Some(content) = message.content.first() {
                            let tool_id = content.tool_use_id.clone();

                            // Get the output - handle both built-in tools (object with stdout/stderr)
                            // and MCP tools (array of content blocks)
                            let output = if let Some(ref result) = tool_use_result {
                                extract_tool_result_output(result)
                                    .unwrap_or_else(|| content.text_content())
                            } else {
                                content.text_content()
                            };

                            let success = !content.is_error;

                            yield StreamChunk::ToolResultObserved {
                                id: tool_id,
                                success,
                                output,
                            };
                        }
                    }
                    Ok(CliMessage::Result { result, is_error, usage, session_id }) => {
                        // Capture session ID if we haven't already
                        if captured_session_id.is_none() {
                            if let Some(sid) = session_id {
                                debug!(session_id = %sid, "Captured CLI session ID from result");
                                captured_session_id = Some(sid);
                            }
                        }
                        if is_error {
                            warn!(error = %result, "Claude CLI returned error");
                            if text_started {
                                yield StreamChunk::TextEnd;
                            }
                            Err(ProviderError::internal(&result))?;
                        }

                        // If we haven't streamed any text yet, use the result
                        if total_text.is_empty() && !result.is_empty() {
                            if !text_started {
                                yield StreamChunk::TextStart;
                                text_started = true;
                            }
                            info!(result_len = result.len(), "Using result as text (no streaming)");
                            yield StreamChunk::TextDelta(result);
                        }

                        // Update final token counts
                        if let Some(u) = usage {
                            if let Some(i) = u.input_tokens {
                                input_tokens = i as u32;
                            }
                            if let Some(o) = u.output_tokens {
                                output_tokens = o as u32;
                            }
                        }

                        if text_started {
                            yield StreamChunk::TextEnd;
                        }

                        // Always use EndTurn since Claude CLI executes tools internally
                        // (whether via built-in tools or MCP, we don't want the runner
                        // to try to execute them again)
                        let finish_reason = crate::stream::FinishReason::EndTurn;

                        yield StreamChunk::FinishStep {
                            usage: crate::stream::Usage {
                                input_tokens,
                                output_tokens,
                                ..Default::default()
                            },
                            finish_reason,
                        };

                        info!(input_tokens, output_tokens, "Stream completed");
                        break;
                    }
                    Ok(CliMessage::System { session_id }) => {
                        // Capture session ID from system message (usually first message)
                        if captured_session_id.is_none() {
                            if let Some(sid) = session_id {
                                debug!(session_id = %sid, "Captured CLI session ID from system message");
                                captured_session_id = Some(sid);
                            }
                        }
                        debug!("Received system message");
                    }
                    Err(e) => {
                        // Not all lines are valid JSON messages (could be debug output)
                        // Log at debug level to avoid noise
                        debug!(error = %e, line_preview = %line.chars().take(100).collect::<String>(), "Failed to parse line");
                    }
                }
            }

            // Wait for the process to complete
            let status = child.wait().await
                .map_err(|e| ProviderError::internal(format!("Failed to wait for Claude CLI: {e}")))?;

            if !status.success() {
                warn!(code = ?status.code(), "Claude CLI exited with error");
            }

            // Store captured session ID for future resumption
            if let Some(sid) = captured_session_id {
                let mut session_lock = session_id_handle.write().await;
                if session_lock.is_none() {
                    info!(session_id = %sid, "Stored CLI session ID for resumption");
                    *session_lock = Some(sid);
                }
            }

            // Clean up MCP config file
            if let Some(path) = config_path_for_cleanup {
                if let Err(e) = std::fs::remove_file(&path) {
                    debug!(path = %path.display(), error = %e, "Failed to clean up MCP config");
                }
            }
        };

        Ok(Box::pin(output_stream))
    }

    fn model_info(&self) -> &ModelInfo {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "anthropic-cli"
    }
}

impl std::fmt::Debug for ClaudeCliProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let use_custom_tools = self
            .mcp_config
            .as_ref()
            .map(|c| c.use_custom_tools)
            .unwrap_or(false);
        f.debug_struct("ClaudeCliProvider")
            .field("model", &self.model.id)
            .field("use_custom_tools", &use_custom_tools)
            .finish()
    }
}

/// Create a ClaudeCliProvider with zero-cost model info.
///
/// This is a convenience function that creates the provider and updates
/// the model cost to reflect that subscription usage is free.
pub fn with_subscription_pricing(model: ModelInfo) -> ProviderResult<ClaudeCliProvider> {
    let mut model = model;
    // Zero out all costs since subscription covers usage
    model.cost = ModelCost {
        input: 0.0,
        output: 0.0,
        cache_read: 0.0,
        cache_write: 0.0,
    };
    ClaudeCliProvider::new(model)
}

/// Create a ClaudeCliProvider with custom tools enabled via MCP.
///
/// This enables Claude CLI to use wonopcode's custom tools instead of
/// its built-in tools. The tools are executed by our MCP server.
/// Create a ClaudeCliProvider with custom tools over HTTP transport.
///
/// This enables Claude CLI to connect to a running MCP HTTP server.
/// The tools are executed by our MCP server via HTTP/SSE.
///
/// # Arguments
/// * `model` - Model information
/// * `mcp_url` - URL for the MCP SSE endpoint (e.g., "http://localhost:3000/mcp/sse")
/// * `secret` - Optional secret for authentication
pub fn with_custom_tools(
    model: ModelInfo,
    mcp_url: String,
    secret: Option<String>,
) -> ProviderResult<ClaudeCliProvider> {
    let mut model = model;
    // Zero out all costs since subscription covers usage
    model.cost = ModelCost {
        input: 0.0,
        output: 0.0,
        cache_read: 0.0,
        cache_write: 0.0,
    };

    let mcp_config = if let Some(secret) = secret {
        McpCliConfig::with_secret(mcp_url, secret)
    } else {
        McpCliConfig::new(mcp_url)
    };

    ClaudeCliProvider::with_mcp_config(model, mcp_config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_available() {
        let _ = ClaudeCliProvider::is_available();
    }

    #[test]
    fn test_is_authenticated() {
        let _ = ClaudeCliProvider::is_authenticated();
    }

    #[test]
    fn test_builtin_tools_to_disable() {
        let tools = ClaudeCliProvider::builtin_tools_to_disable();
        assert!(tools.contains("Bash"));
        assert!(tools.contains("Read"));
        assert!(tools.contains("Write"));
    }

    #[test]
    fn test_mcp_config_new() {
        let config = McpCliConfig::new("http://localhost:3000/mcp/sse");
        assert!(config.use_custom_tools);
        assert_eq!(config.transport.url, "http://localhost:3000/mcp/sse");
    }
}
