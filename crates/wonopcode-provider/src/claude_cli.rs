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
    message::{ContentPart, ImageSource},
    model::{ModelCost, ModelInfo},
    stream::StreamChunk,
    GenerateOptions, LanguageModel, Message, ProviderResult,
};
use async_stream::try_stream;
use async_trait::async_trait;
use base64::Engine;
use futures::stream::BoxStream;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tracing::{debug, info, trace, warn};

/// Cache for the Claude CLI binary path.
/// This is cached because finding the binary can be slow if it's not in PATH.
static CLAUDE_CLI_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Clear the Claude CLI path cache.
/// Call this after changing the custom Claude CLI path in settings.
pub fn clear_claude_cli_cache() {
    // Note: OnceLock doesn't have a clear method, so we can't actually clear it.
    // The cache will persist until the process restarts.
    warn!(
        "Claude CLI path cache cannot be cleared at runtime - restart the process for new settings"
    );
}

/// Build an enhanced PATH that includes common Node.js installation locations.
/// This is necessary for packaged apps that don't inherit the user's full PATH.
/// The Claude CLI uses #!/usr/bin/env node, so node must be in PATH.
pub fn build_enhanced_path() -> String {
    let mut paths: Vec<String> = Vec::new();

    // Add common Node.js/npm locations
    paths.push("/opt/homebrew/bin".to_string()); // Homebrew on Apple Silicon
    paths.push("/usr/local/bin".to_string()); // Homebrew on Intel / system

    // Add user's local bin
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/bin").to_string_lossy().to_string());
        paths.push(home.join(".npm-global/bin").to_string_lossy().to_string());

        // nvm - check for current version symlink first
        let nvm_current = home.join(".nvm/current/bin");
        if nvm_current.exists() {
            paths.push(nvm_current.to_string_lossy().to_string());
        }
    }

    // Add existing PATH
    if let Ok(existing_path) = std::env::var("PATH") {
        paths.push(existing_path);
    }

    #[cfg(unix)]
    let separator = ":";
    #[cfg(windows)]
    let separator = ";";

    paths.join(separator)
}

/// Find the Claude CLI binary, searching common installation locations.
///
/// This is important for packaged applications (like Tauri desktop apps) that may
/// not inherit the user's full PATH environment variable.
///
/// Search order:
/// 0. Custom path from WONOPCODE_CLAUDE_CLI_PATH environment variable
/// 1. Standard PATH lookup
/// 2. Homebrew on macOS: /opt/homebrew/bin/claude, /usr/local/bin/claude
/// 3. npm global: ~/.npm-global/bin/claude
/// 4. User local: ~/.local/bin/claude
fn find_claude_cli() -> Option<PathBuf> {
    CLAUDE_CLI_PATH
        .get_or_init(|| {
            debug!("Searching for Claude CLI...");

            // Build enhanced PATH to ensure Node.js is available
            // Claude CLI uses #!/usr/bin/env node, so node must be in PATH
            let enhanced_path = build_enhanced_path();

            // 0. Check custom path from environment variable first
            // This is set by the desktop app from user settings at startup
            if let Ok(custom_path) = std::env::var("WONOPCODE_CLAUDE_CLI_PATH") {
                if !custom_path.is_empty() {
                    let path = PathBuf::from(&custom_path);
                    debug!(path = %path.display(), "Checking custom Claude CLI path from WONOPCODE_CLAUDE_CLI_PATH");

                    if path.exists() {
                        let mut cmd = Command::new(&path);
                        cmd.arg("--version");
                        cmd.env("PATH", &enhanced_path);

                        match cmd.output() {
                            Ok(output) if output.status.success() => {
                                debug!(path = %path.display(), "Using custom Claude CLI path");
                                return Some(path);
                            }
                            Ok(output) => {
                                warn!(
                                    path = %path.display(),
                                    status = ?output.status,
                                    stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                                    "Custom Claude CLI path exists but failed to run"
                                );
                            }
                            Err(e) => {
                                warn!(path = %path.display(), error = %e, "Failed to execute custom Claude CLI path");
                            }
                        }
                    } else {
                        warn!(path = %path.display(), "Custom Claude CLI path does not exist");
                    }
                }
            }

            // 1. Try standard PATH lookup first (with enhanced PATH)
            let mut cmd = Command::new("claude");
            cmd.arg("--version");
            cmd.env("PATH", &enhanced_path);

            match cmd.output() {
                Ok(output) if output.status.success() => {
                    debug!("Found claude in PATH");
                    return Some(PathBuf::from("claude"));
                }
                Ok(output) => {
                    debug!(
                        status = ?output.status,
                        stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                        "claude in PATH failed"
                    );
                }
                Err(e) => {
                    debug!(error = %e, "claude not found in PATH");
                }
            }

            // 2. Check common installation locations
            let common_paths = [
                // Homebrew on Apple Silicon
                "/opt/homebrew/bin/claude",
                // Homebrew on Intel Mac / Linux
                "/usr/local/bin/claude",
                // User local bin
                "~/.local/bin/claude",
                // npm global (common location)
                "~/.npm-global/bin/claude",
            ];

            let home = dirs::home_dir();
            debug!(home_dir = ?home, "Checking common installation locations");

            for path_str in common_paths {
                let path = if let Some(stripped) = path_str.strip_prefix("~/") {
                    if let Some(ref h) = home {
                        h.join(stripped)
                    } else {
                        continue;
                    }
                } else {
                    PathBuf::from(path_str)
                };

                debug!(path = %path.display(), exists = path.exists(), "Checking location");

                if path.exists() {
                    // Verify it actually works (with enhanced PATH for Node.js)
                    let mut cmd = Command::new(&path);
                    cmd.arg("--version");
                    cmd.env("PATH", &enhanced_path);

                    match cmd.output() {
                        Ok(output) if output.status.success() => {
                            debug!(path = %path.display(), "Found Claude CLI at non-PATH location");
                            return Some(path);
                        }
                        Ok(output) => {
                            debug!(
                                path = %path.display(),
                                status = ?output.status,
                                stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                                "Claude CLI exists but failed to run"
                            );
                        }
                        Err(e) => {
                            debug!(path = %path.display(), error = %e, "Failed to execute Claude CLI");
                        }
                    }
                }
            }

            // 3. Try to find via npm root
            #[cfg(unix)]
            {
                debug!("Trying npm root lookup...");
                if let Ok(output) = Command::new("npm").args(["root", "-g"]).output() {
                    if output.status.success() {
                        let npm_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        debug!(npm_root = %npm_root, "Found npm global root");
                        let npm_claude = Path::new(&npm_root)
                            .parent() // Go up from node_modules
                            .map(|p| p.join("bin").join("claude"));
                        if let Some(p) = npm_claude {
                            debug!(path = %p.display(), exists = p.exists(), "Checking npm global bin");
                            if p.exists() {
                                if let Ok(output) = Command::new(&p).arg("--version").output() {
                                    if output.status.success() {
                                        debug!(path = %p.display(), "Found Claude CLI via npm root");
                                        return Some(p);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    debug!("npm command not available");
                }
            }

            warn!("Claude CLI not found in PATH or common locations");
            None
        })
        .clone()
}

/// Get an async Command for running the Claude CLI.
/// Uses the cached path if claude is not in PATH.
fn claude_async_command() -> Option<TokioCommand> {
    find_claude_cli().map(TokioCommand::new)
}

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

/// Accumulated token usage across the session.
///
/// This tracks the total tokens used across all turns in the session.
/// The Claude CLI reports the current context size (not deltas), so we need
/// to track the previous values to compute deltas for each turn.
#[derive(Debug, Clone, Default)]
pub struct AccumulatedUsage {
    /// Total input tokens accumulated across all turns.
    pub total_input_tokens: u64,
    /// Total output tokens accumulated across all turns.
    pub total_output_tokens: u64,
    /// Total cost accumulated (if tracked).
    pub total_cost: f64,
    /// Number of completed turns.
    pub num_turns: u32,
    /// Last reported context size (input tokens from CLI, used to compute deltas).
    /// This is the full context size reported by Claude CLI, not a delta.
    last_context_input: u64,
    /// Last reported output tokens (used to compute deltas).
    last_output: u64,
}

impl AccumulatedUsage {
    /// Reset the accumulated usage (e.g., when starting a new session).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Update with new turn data from Claude CLI.
    ///
    /// Takes the **delta** tokens (new tokens this turn, excluding cache reads)
    /// and accumulates them.
    ///
    /// # Arguments
    /// - `input_delta`: New input tokens this turn (input_tokens + cache_creation, NOT cache_read)
    /// - `output_tokens`: Output tokens this turn (each response is fresh)
    /// - `context_input_tokens`: Full context size for last_request tracking
    /// - `cost`: Cumulative cost from Claude CLI (it reports total, not delta)
    ///
    /// Returns (input_delta, output_delta) for this turn.
    pub fn update_from_cli_turn(
        &mut self,
        input_delta: u64,
        output_tokens: u64,
        context_input_tokens: u64,
        cost: Option<f64>,
    ) -> (u64, u64) {
        // Update accumulated totals
        self.total_input_tokens += input_delta;
        self.total_output_tokens += output_tokens;
        if let Some(c) = cost {
            self.total_cost = c; // CLI reports cumulative cost, so use it directly
        }
        self.num_turns += 1;

        // Store current context size for context tracking (not for delta calculation)
        self.last_context_input = context_input_tokens;
        self.last_output = output_tokens;

        (input_delta, output_tokens)
    }
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
///
/// Token tracking: The provider maintains accumulated token usage across the
/// session, computing deltas from the Claude CLI's context size reports.
pub struct ClaudeCliProvider {
    model: ModelInfo,
    /// MCP configuration, if using custom tools.
    mcp_config: Option<McpCliConfig>,
    /// Captured session ID from the CLI for resumption.
    /// Protected by RwLock for interior mutability.
    session_id: std::sync::Arc<tokio::sync::RwLock<Option<String>>>,
    /// Working directory for the CLI process.
    /// If not set, inherits from the parent process.
    working_directory: Option<PathBuf>,
    /// Accumulated token usage for the session.
    /// Protected by RwLock for interior mutability.
    accumulated_usage: std::sync::Arc<tokio::sync::RwLock<AccumulatedUsage>>,
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

        debug!(model = %model.id, "Created Claude CLI provider (no custom tools)");

        Ok(Self {
            model,
            mcp_config: None,
            session_id: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
            working_directory: None,
            accumulated_usage: std::sync::Arc::new(tokio::sync::RwLock::new(
                AccumulatedUsage::default(),
            )),
        })
    }

    /// Create a new CLI provider with MCP configuration for custom tools.
    pub fn with_mcp_config(model: ModelInfo, mcp_config: McpCliConfig) -> ProviderResult<Self> {
        Self::check_cli_available()?;

        debug!(
            model = %model.id,
            use_custom_tools = mcp_config.use_custom_tools,
            "Created Claude CLI provider with MCP config"
        );

        Ok(Self {
            model,
            mcp_config: Some(mcp_config),
            session_id: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
            working_directory: None,
            accumulated_usage: std::sync::Arc::new(tokio::sync::RwLock::new(
                AccumulatedUsage::default(),
            )),
        })
    }

    /// Set the working directory for the CLI process.
    ///
    /// This sets the current working directory when spawning the Claude CLI,
    /// which affects where the CLI and any tools it invokes execute.
    pub fn set_working_directory(&mut self, dir: PathBuf) {
        debug!(working_directory = %dir.display(), "Set working directory for Claude CLI");
        self.working_directory = Some(dir);
    }

    /// Create a new CLI provider with a specific working directory.
    pub fn with_working_directory(
        model: ModelInfo,
        working_directory: PathBuf,
    ) -> ProviderResult<Self> {
        Self::check_cli_available()?;

        debug!(
            model = %model.id,
            working_directory = %working_directory.display(),
            "Created Claude CLI provider with working directory"
        );

        Ok(Self {
            model,
            mcp_config: None,
            session_id: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
            working_directory: Some(working_directory),
            accumulated_usage: std::sync::Arc::new(tokio::sync::RwLock::new(
                AccumulatedUsage::default(),
            )),
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
    /// Also resets the accumulated token usage.
    pub async fn clear_session(&self) {
        let mut guard = self.session_id.write().await;
        *guard = None;
        drop(guard);
        self.accumulated_usage.write().await.reset();
    }

    /// Get the accumulated token usage for the session.
    pub async fn get_accumulated_usage(&self) -> AccumulatedUsage {
        self.accumulated_usage.read().await.clone()
    }

    /// Reset the accumulated token usage without clearing the session.
    pub async fn reset_accumulated_usage(&self) {
        self.accumulated_usage.write().await.reset();
    }

    /// Check if Claude CLI is installed and accessible.
    ///
    /// This searches in common installation locations beyond just PATH,
    /// which is important for packaged desktop apps that don't inherit
    /// the user's full environment.
    pub fn check_cli_available() -> ProviderResult<()> {
        // Use the cached find_claude_cli which searches common locations
        if find_claude_cli().is_some() {
            debug!("Claude CLI found");
            Ok(())
        } else {
            Err(ProviderError::internal(
                "Claude Code CLI not found. Install with: npm install -g @anthropic-ai/claude-code"
                    .to_string(),
            ))
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
    #[allow(clippy::cognitive_complexity)]
    async fn check_auth_uncached_async() -> bool {
        let Some(mut cmd) = claude_async_command() else {
            debug!("Claude CLI not found for auth check");
            return false;
        };

        let output = cmd
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

    /// Save an image to a file and return the path.
    ///
    /// Claude CLI can reference images by file path, so we save base64 images
    /// to files within the working directory (so Claude CLI has permission to read them).
    /// Images are stored in `.wonopcode/images/` within the working directory.
    fn save_image_to_file(
        &self,
        media_type: &str,
        data: &str,
        index: usize,
    ) -> Result<PathBuf, ProviderError> {
        // Determine file extension from media type
        let extension = match media_type {
            "image/png" => "png",
            "image/jpeg" | "image/jpg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => "png", // Default to png for unknown types
        };

        // Create a unique filename with timestamp for uniqueness
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let filename = format!("image_{}_{}.{}", timestamp, index, extension);

        // Determine base directory - use working directory if set, otherwise temp dir
        let base_dir = if let Some(ref workdir) = self.working_directory {
            // Save in .wonopcode/images/ within the working directory
            // This ensures Claude CLI has permission to read them
            workdir.join(".wonopcode").join("images")
        } else {
            // Fallback to system temp dir (may not work without explicit permissions)
            std::env::temp_dir().join("wonopcode_images")
        };

        // Ensure the directory exists
        std::fs::create_dir_all(&base_dir).map_err(|e| {
            ProviderError::internal(format!("Failed to create image directory: {e}"))
        })?;

        let path = base_dir.join(&filename);

        // Decode base64 and write to file
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| ProviderError::internal(format!("Failed to decode base64 image: {e}")))?;

        std::fs::write(&path, decoded)
            .map_err(|e| ProviderError::internal(format!("Failed to write image to file: {e}")))?;

        debug!(path = %path.display(), "Saved image for Claude CLI");
        Ok(path)
    }

    /// Extract images from a message and save them to temp files.
    ///
    /// Returns a vector of file paths to the saved images.
    fn extract_and_save_images(&self, msg: &Message) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let mut image_index = 0;

        for part in &msg.content {
            if let ContentPart::Image { source } = part {
                match source {
                    ImageSource::Base64 { media_type, data } => {
                        match self.save_image_to_file(media_type, data, image_index) {
                            Ok(path) => {
                                paths.push(path);
                                image_index += 1;
                            }
                            Err(e) => {
                                warn!(error = %e, "Failed to save image to file");
                            }
                        }
                    }
                    ImageSource::Url { url } => {
                        // For URL images, we'd need to download them first
                        // For now, just log a warning
                        warn!(url = %url, "URL images not yet supported in Claude CLI provider");
                    }
                }
            }
        }

        paths
    }

    /// Format messages into a prompt string for the CLI.
    ///
    /// This now handles images by saving them to temp files and referencing
    /// the file paths in the prompt.
    fn format_messages(&self, messages: &[Message]) -> String {
        let mut parts = Vec::new();

        for msg in messages {
            let role = match msg.role {
                crate::message::Role::System => "System",
                crate::message::Role::User => "Human",
                crate::message::Role::Assistant => "Assistant",
                crate::message::Role::Tool => "Tool Result",
            };

            // Extract and save any images
            let image_paths = self.extract_and_save_images(msg);

            // Build content with text and image references
            let mut content_parts = Vec::new();

            // Add text content
            let text = msg.text();
            if !text.is_empty() {
                content_parts.push(text);
            }

            // Add image file references
            for path in &image_paths {
                content_parts.push(format!("[Image: {}]", path.display()));
            }

            if !content_parts.is_empty() {
                let content = content_parts.join("\n");
                parts.push(format!("{role}: {content}"));
            }
        }

        parts.join("\n\n")
    }

    /// Extract only the last user message for session resumption.
    ///
    /// When resuming a Claude CLI session, the conversation history is already
    /// stored by the CLI. We only need to send the new user message.
    ///
    /// This now handles images by saving them to temp files and referencing
    /// the file paths in the prompt.
    fn extract_last_user_message(&self, messages: &[Message]) -> String {
        // Find the last user message
        for msg in messages.iter().rev() {
            if matches!(msg.role, crate::message::Role::User) {
                // Extract and save any images
                let image_paths = self.extract_and_save_images(msg);

                // Build content with text and image references
                let mut content_parts = Vec::new();

                // Add text content
                let text = msg.text();
                if !text.is_empty() {
                    content_parts.push(text);
                }

                // Add image file references
                for path in &image_paths {
                    content_parts.push(format!("[Image: {}]", path.display()));
                }

                if !content_parts.is_empty() {
                    return content_parts.join("\n");
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

        debug!(
            url = %mcp_config.transport.url,
            has_auth = !mcp_config.transport.headers.is_empty(),
            "Generated MCP HTTP config"
        );

        // Add external MCP servers (stdio-based, command-line programs)
        for (name, server) in &mcp_config.external_servers {
            let mut server_env = serde_json::Map::new();
            for (k, v) in &server.env {
                server_env.insert(k.clone(), serde_json::Value::String(v.clone()));
            }

            mcp_servers.insert(
                name.clone(),
                serde_json::json!({
                    "command": server.command,
                    "args": server.args,
                    "env": server_env
                }),
            );

            debug!(
                server_name = %name,
                command = %server.command,
                args = ?server.args,
                "Added external stdio MCP server"
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

        let config_json = serde_json::to_string_pretty(&config)
            .map_err(|e| ProviderError::internal(format!("Failed to serialize MCP config: {e}")))?;
        std::fs::write(&config_path, config_json)
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
        // EnterWorktree is disabled because we manage worktrees/workstreams ourselves.
        // TaskOutput, NotebookEdit, KillShell, Skill are disabled because we don't
        // support these features or provide our own implementations.
        "Bash,Read,Write,Edit,MultiEdit,Glob,Grep,WebSearch,WebFetch,Task,TodoRead,TodoWrite,AskUserQuestion,EnterPlanMode,ExitPlanMode,EnterWorktree,TaskOutput,NotebookEdit,KillShell,Skill"
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
        /// Total cost for the entire session in USD (cumulative)
        #[serde(default)]
        total_cost_usd: Option<f64>,
        /// Number of turns in the session (cumulative)
        #[serde(default)]
        num_turns: Option<u32>,
    },
    /// Real-time streaming events for token-by-token output
    #[serde(rename = "stream_event")]
    StreamEvent { event: StreamEventData },
}

/// Stream event data from Claude CLI's real-time streaming
#[derive(Debug, Deserialize)]
struct StreamEventData {
    /// Event type: "content_block_start", "content_block_delta", "content_block_stop", etc.
    #[serde(rename = "type")]
    event_type: String,
    /// Index of the content block (for multi-block responses)
    #[serde(default)]
    index: u32,
    /// Content block for "content_block_start" events
    #[serde(default)]
    content_block: Option<StreamContentBlock>,
    /// Delta for "content_block_delta" events
    #[serde(default)]
    delta: Option<StreamDelta>,
}

/// Content block information for stream_event content_block_start
#[derive(Debug, Deserialize)]
struct StreamContentBlock {
    /// Block type: "text", "tool_use"
    #[serde(rename = "type")]
    block_type: String,
    /// Tool use ID (only for tool_use blocks)
    #[serde(default)]
    id: Option<String>,
    /// Tool name (only for tool_use blocks)
    #[serde(default)]
    name: Option<String>,
}

/// Delta for stream_event content_block_delta
#[derive(Debug, Deserialize)]
struct StreamDelta {
    /// Delta type: "text_delta", "input_json_delta"
    #[serde(rename = "type")]
    delta_type: String,
    /// Text content (for text_delta)
    #[serde(default)]
    text: Option<String>,
    /// Partial JSON input (for input_json_delta - tool input streaming)
    #[serde(default)]
    partial_json: Option<String>,
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
    /// Tokens used to create new cache entries
    cache_creation_input_tokens: Option<u64>,
    /// Tokens read from existing cache entries  
    cache_read_input_tokens: Option<u64>,
}

impl MessageUsage {
    /// Get total input tokens including cached tokens.
    /// This represents the full context size being processed.
    fn total_input_tokens(&self) -> u64 {
        let base = self.input_tokens.unwrap_or(0);
        let cache_creation = self.cache_creation_input_tokens.unwrap_or(0);
        let cache_read = self.cache_read_input_tokens.unwrap_or(0);
        base + cache_creation + cache_read
    }

    /// Get input tokens that are NEW this turn (not cache reads).
    /// This is the actual delta that should be accumulated across turns.
    /// = input_tokens + cache_creation_input_tokens (excludes cache_read)
    fn delta_input_tokens(&self) -> u64 {
        let base = self.input_tokens.unwrap_or(0);
        let cache_creation = self.cache_creation_input_tokens.unwrap_or(0);
        // Deliberately exclude cache_read_input_tokens as those were counted in previous turns
        base + cache_creation
    }

    /// Get cache read tokens for this turn.
    fn cache_read_tokens(&self) -> u64 {
        self.cache_read_input_tokens.unwrap_or(0)
    }
}

#[derive(Debug, Deserialize)]
struct CliUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    /// Tokens used to create new cache entries
    cache_creation_input_tokens: Option<u64>,
    /// Tokens read from existing cache entries  
    cache_read_input_tokens: Option<u64>,
}

impl CliUsage {
    /// Get total input tokens including cached tokens.
    /// This represents the full context size being processed.
    fn total_input_tokens(&self) -> u64 {
        let base = self.input_tokens.unwrap_or(0);
        let cache_creation = self.cache_creation_input_tokens.unwrap_or(0);
        let cache_read = self.cache_read_input_tokens.unwrap_or(0);
        base + cache_creation + cache_read
    }

    /// Get input tokens that are NEW this turn (not cache reads).
    /// This is the actual delta that should be accumulated across turns.
    /// = input_tokens + cache_creation_input_tokens (excludes cache_read)
    fn delta_input_tokens(&self) -> u64 {
        let base = self.input_tokens.unwrap_or(0);
        let cache_creation = self.cache_creation_input_tokens.unwrap_or(0);
        // Deliberately exclude cache_read_input_tokens as those were counted in previous turns
        base + cache_creation
    }

    /// Get cache read tokens for this turn.
    fn cache_read_tokens(&self) -> u64 {
        self.cache_read_input_tokens.unwrap_or(0)
    }
}

#[async_trait]
impl LanguageModel for ClaudeCliProvider {
    /// Claude CLI is stateful - it maintains conversation history via --resume.
    /// After compaction, we must reset the session to use the new compacted history.
    fn is_stateful(&self) -> bool {
        true
    }

    async fn reset_session(&self) {
        self.clear_session().await;
    }

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

        debug!(
            model = %self.model.id,
            prompt_len = prompt.len(),
            use_custom_tools = use_custom_tools,
            resume_session = ?existing_session,
            "Sending query to Claude CLI"
        );

        // Build CLI arguments
        // Note: --include-partial-messages is REQUIRED for token-by-token streaming
        // Without it, Claude CLI only emits full "assistant" messages, not stream_event deltas
        let mut args = vec![
            "-p".to_string(),
            prompt,
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
            "--include-partial-messages".to_string(),
            "--model".to_string(),
            self.model.id.clone(),
        ];

        // Add session resumption if we have a previous session
        if let Some(ref session) = existing_session {
            args.push("--resume".to_string());
            args.push(session.clone());
            debug!(session_id = %session, "Resuming previous CLI session");
        }
        // NOTE: We intentionally do NOT pass --system-prompt to Claude CLI.
        // 
        // Claude CLI automatically reads CLAUDE.md from the working directory and
        // incorporates it into its system prompt. Our HMS (Hierarchical Memory System)
        // writes CLAUDE.md to the project root before starting each session, so Claude
        // CLI will pick up the rendered memory context automatically.
        //
        // Using --system-prompt would REPLACE Claude CLI's entire system prompt
        // (except tool definitions), which would:
        // 1. Override Claude Code's built-in coding guidelines and safety instructions
        // 2. Create confusion if CLAUDE.md also exists (double loading)
        //
        // By not passing --system-prompt, we get the best of both worlds:
        // - Claude Code's full default system prompt
        // - Our custom instructions via CLAUDE.md
        //
        // For non-CLI providers (anthropic API, openai, etc.), the system prompt
        // is passed directly via the API, which is handled in the standard agent loop.

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
        // Note: We intentionally do NOT set --permission-mode here.
        // Claude CLI will use its default permission behavior, which prompts for
        // dangerous operations. The TUI should display these prompts to the user.

        // Log the full command for debugging
        trace!(
            command = "claude",
            args = ?args,
            working_directory = ?self.working_directory,
            "Spawning Claude CLI"
        );

        // Spawn the Claude CLI process with streaming JSON output
        // Use find_claude_cli to handle packaged apps that don't have claude in PATH
        let cli_path = find_claude_cli().ok_or_else(|| {
            ProviderError::internal(
                "Claude Code CLI not found. Install with: npm install -g @anthropic-ai/claude-code"
                    .to_string(),
            )
        })?;
        let mut cmd = TokioCommand::new(cli_path);
        cmd.args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Set enhanced PATH to ensure Node.js is available
        // Claude CLI uses #!/usr/bin/env node, so node must be in PATH
        cmd.env("PATH", build_enhanced_path());

        // Set working directory if configured
        if let Some(ref workdir) = self.working_directory {
            cmd.current_dir(workdir);
        }

        let mut child = cmd
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

        // Clone accumulated usage handle for token tracking in the stream
        let accumulated_usage_handle = self.accumulated_usage.clone();

        // Clone abort token for cancellation
        let abort = options.abort.clone();

        // Create the output stream that parses CLI JSON output
        let output_stream = try_stream! {
            let mut total_text = String::new();
            // These store the CLI-reported context sizes (full context, NOT deltas)
            let mut cli_context_input_tokens: u64 = 0;
            // This stores the DELTA input tokens (new tokens this turn, excluding cache reads)
            let mut cli_delta_input_tokens: u64 = 0;
            // This stores the cache read tokens for this turn
            let mut cli_cache_read_tokens: u64 = 0;
            let mut cli_output_tokens: u64 = 0;
            #[allow(unused_assignments)]
            let mut cli_cost: Option<f64> = None;
            let mut text_started = false;
            let mut captured_session_id: Option<String> = None;

            // Track tool calls so we can set finish_reason correctly
            let mut tool_calls: Vec<(String, String, String)> = Vec::new(); // (id, name, args)

            // Track streaming tool input JSON accumulation
            // Maps content block index -> (tool_id, tool_name, accumulated_json)
            let mut streaming_tool_inputs: std::collections::HashMap<u32, (String, String, String)> = std::collections::HashMap::new();

            // Track which content blocks we've already emitted via stream_event
            // so we don't duplicate when the full "assistant" message arrives
            let mut streamed_text_indices: std::collections::HashSet<u32> = std::collections::HashSet::new();
            let mut streamed_tool_indices: std::collections::HashSet<u32> = std::collections::HashSet::new();

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

                tracing::trace!(line_len = line.len(), line_preview = %line.chars().take(100).collect::<String>(), "Received line from CLI");

                // Check for result message specifically to log raw JSON
                if line.contains("\"type\":\"result\"") {
                    info!(
                        line_preview = %line.chars().take(500).collect::<String>(),
                        "Received result message (raw)"
                    );
                }

                // Try to parse as our message type
                match serde_json::from_str::<CliMessage>(&line) {
                    Ok(CliMessage::Assistant { message, session_id }) => {
                        tracing::trace!(content_blocks = message.content.len(), session_id = ?session_id, "Parsed assistant message");
                        // Capture session ID if we haven't already
                        if captured_session_id.is_none() {
                            if let Some(sid) = session_id {
                                debug!(session_id = %sid, "Captured CLI session ID");
                                captured_session_id = Some(sid);
                            }
                        }
                        // Extract content from content blocks
                        for (block_index, block) in message.content.into_iter().enumerate() {
                            match block {
                                ContentBlock::Text { text } => {
                                    // Skip text blocks that were already streamed via stream_event
                                    // This prevents duplicate text when the full "assistant" message arrives
                                    let block_idx = block_index as u32;
                                    if streamed_text_indices.contains(&block_idx) {
                                        tracing::debug!(
                                            block_index = block_idx,
                                            text_len = text.len(),
                                            streamed_indices = ?streamed_text_indices,
                                            "Skipping already-streamed text block (dedup)"
                                        );
                                        continue;
                                    }

                                    // DEDUP WARNING: If we reach here with non-empty text while streaming was active,
                                    // this could indicate a bug where the index wasn't properly tracked
                                    if !text.is_empty() && !total_text.is_empty() {
                                        tracing::warn!(
                                            block_index = block_idx,
                                            text_len = text.len(),
                                            total_text_len = total_text.len(),
                                            streamed_indices = ?streamed_text_indices,
                                            "POTENTIAL DUPLICATE: Text block not in streamed_indices but total_text is non-empty"
                                        );
                                    }

                                    if !text.is_empty() {
                                        if !text_started {
                                            tracing::trace!("Starting text stream (from assistant message)");
                                            yield StreamChunk::TextStart;
                                            text_started = true;
                                        }
                                        tracing::trace!(
                                            text_len = text.len(),
                                            text_preview = %text.chars().take(20).collect::<String>(),
                                            block_index = block_idx,
                                            "Yielding TextDelta (from assistant message)"
                                        );
                                        total_text.push_str(&text);
                                        yield StreamChunk::TextDelta(text);
                                    }
                                }
                                ContentBlock::ToolUse { id, name, input } => {
                                    // Skip tool_use blocks that were already streamed via stream_event
                                    // This prevents duplicate tool calls when the full "assistant" message arrives
                                    if streamed_tool_indices.contains(&(block_index as u32)) {
                                        tracing::trace!(block_index, tool_id = %id, "Skipping already-streamed tool_use block");
                                        continue;
                                    }

                                    // Tool use - emit as observed since CLI executes tools
                                    debug!(id = %id, name = %name, "Emitting ToolObserved for CLI tool use (from assistant message)");

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
                        // Update CLI-reported context sizes
                        if let Some(ref usage) = message.usage {
                            // Full context size (includes cache reads)
                            cli_context_input_tokens = usage.total_input_tokens();
                            // Delta input tokens (NEW tokens, excludes cache reads)
                            cli_delta_input_tokens = usage.delta_input_tokens();
                            // Cache read tokens for this turn
                            cli_cache_read_tokens = usage.cache_read_tokens();
                            if let Some(o) = usage.output_tokens {
                                cli_output_tokens = o;
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
                    Ok(CliMessage::Result { result, is_error, usage, session_id, total_cost_usd, num_turns }) => {
                        // Log the received usage data for debugging
                        debug!(
                            has_usage = usage.is_some(),
                            usage_input = ?usage.as_ref().and_then(|u| u.input_tokens),
                            usage_output = ?usage.as_ref().and_then(|u| u.output_tokens),
                            total_cost_usd = ?total_cost_usd,
                            num_turns = ?num_turns,
                            "Claude CLI Result message received"
                        );

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
                            debug!(result_len = result.len(), "Using result as text (no streaming)");
                            yield StreamChunk::TextDelta(result);
                        }

                        // Update CLI-reported context sizes from Result message
                        if let Some(ref u) = usage {
                            // Full context size (includes cache reads)
                            cli_context_input_tokens = u.total_input_tokens();
                            // Delta input tokens (NEW tokens, excludes cache reads)
                            cli_delta_input_tokens = u.delta_input_tokens();
                            // Cache read tokens for this turn
                            cli_cache_read_tokens = u.cache_read_tokens();
                            if let Some(o) = u.output_tokens {
                                cli_output_tokens = o;
                            }
                            debug!(
                                input = ?u.input_tokens,
                                cache_creation = ?u.cache_creation_input_tokens,
                                cache_read = ?u.cache_read_input_tokens,
                                total_context = cli_context_input_tokens,
                                delta_input = cli_delta_input_tokens,
                                output = cli_output_tokens,
                                "Received CLI context sizes from Result"
                            );
                        } else {
                            warn!(
                                cli_context_input_tokens = cli_context_input_tokens,
                                cli_delta_input_tokens = cli_delta_input_tokens,
                                cli_cache_read_tokens = cli_cache_read_tokens,
                                cli_output_tokens = cli_output_tokens,
                                "No usage in Result message - using values from Assistant message"
                            );
                        }

                        // Store cost if available
                        cli_cost = total_cost_usd;

                        if text_started {
                            yield StreamChunk::TextEnd;
                        }

                        // Update accumulated usage with the DELTA tokens (excluding cache reads)
                        // The delta is: input_tokens + cache_creation_input_tokens (NOT cache_read)
                        let (input_delta, output_delta) = {
                            let mut acc = accumulated_usage_handle.write().await;
                            let deltas = acc.update_from_cli_turn(
                                cli_delta_input_tokens,  // Delta (excludes cache reads)
                                cli_output_tokens,
                                cli_context_input_tokens, // Full context for tracking
                                cli_cost,
                            );
                            debug!(
                                cli_context_input = cli_context_input_tokens,
                                cli_delta_input = cli_delta_input_tokens,
                                cli_output = cli_output_tokens,
                                input_delta = deltas.0,
                                output_delta = deltas.1,
                                accumulated_input = acc.total_input_tokens,
                                accumulated_output = acc.total_output_tokens,
                                num_turns = acc.num_turns,
                                "Token usage computed (delta excludes cache reads)"
                            );
                            deltas
                        };

                        // Always use EndTurn since Claude CLI executes tools internally
                        // (whether via built-in tools or MCP, we don't want the runner
                        // to try to execute them again)
                        let finish_reason = crate::stream::FinishReason::EndTurn;

                        // Get the accumulated usage to send with FinishStep
                        let accumulated = {
                            let acc = accumulated_usage_handle.read().await;
                            crate::stream::AccumulatedUsage {
                                total_input_tokens: acc.total_input_tokens,
                                total_output_tokens: acc.total_output_tokens,
                                total_cache_read_tokens: cli_cache_read_tokens, // Cache read for this turn
                                total_cache_write_tokens: 0,
                                total_reasoning_tokens: 0,
                                total_cost: acc.total_cost,
                                // Include the actual context size for the last/current request
                                last_request_input: cli_context_input_tokens,
                                last_request_output: cli_output_tokens,
                                last_request_cache_read: cli_cache_read_tokens,
                            }
                        };

                        // Yield the DELTA values for this step plus ACCUMULATED totals
                        yield StreamChunk::FinishStep {
                            usage: crate::stream::Usage {
                                input_tokens: input_delta as u32,
                                output_tokens: output_delta as u32,
                                ..Default::default()
                            },
                            accumulated_usage: Some(accumulated.clone()),
                            finish_reason,
                        };

                        debug!(
                            input_delta = input_delta,
                            output_delta = output_delta,
                            accumulated_input = accumulated.total_input_tokens,
                            accumulated_output = accumulated.total_output_tokens,
                            accumulated_cost = accumulated.total_cost,
                            "Stream completed with token usage"
                        );
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
                    Ok(CliMessage::StreamEvent { event }) => {
                        // Real-time streaming events for token-by-token output
                        match event.event_type.as_str() {
                            "content_block_start" => {
                                // A new content block is starting
                                if let Some(ref content_block) = event.content_block {
                                    match content_block.block_type.as_str() {
                                        "text" => {
                                            // Text block starting - emit TextStart if not already started
                                            if !text_started {
                                                tracing::trace!(index = event.index, "Starting text stream (from stream_event)");
                                                yield StreamChunk::TextStart;
                                                text_started = true;
                                            }
                                            // Mark this index as being streamed
                                            streamed_text_indices.insert(event.index);
                                        }
                                        "tool_use" => {
                                            // Tool use block starting - initialize accumulator
                                            let tool_id = content_block.id.clone().unwrap_or_default();
                                            let tool_name = content_block.name.clone().unwrap_or_default();
                                            tracing::trace!(
                                                index = event.index,
                                                tool_id = %tool_id,
                                                tool_name = %tool_name,
                                                "Tool use block starting (from stream_event)"
                                            );
                                            streaming_tool_inputs.insert(event.index, (tool_id, tool_name, String::new()));
                                        }
                                        _ => {
                                            tracing::trace!(
                                                block_type = %content_block.block_type,
                                                index = event.index,
                                                "Unknown content block type in stream_event"
                                            );
                                        }
                                    }
                                }
                            }
                            "content_block_delta" => {
                                // Delta within a content block - this is the token-by-token streaming
                                if let Some(ref delta) = event.delta {
                                    match delta.delta_type.as_str() {
                                        "text_delta" => {
                                            // Text token - emit immediately
                                            if let Some(ref text) = delta.text {
                                                if !text.is_empty() {
                                                    // Always track this content block index to prevent duplicate
                                                    // emission when the full "assistant" message arrives
                                                    streamed_text_indices.insert(event.index);
                                                    
                                                    if !text_started {
                                                        // Safety: start text if we somehow missed content_block_start
                                                        tracing::trace!("Starting text stream (late, from text_delta)");
                                                        yield StreamChunk::TextStart;
                                                        text_started = true;
                                                    }
                                                    tracing::trace!(
                                                        text_len = text.len(),
                                                        text_preview = %text.chars().take(20).collect::<String>(),
                                                        index = event.index,
                                                        "Yielding TextDelta (from stream_event)"
                                                    );
                                                    total_text.push_str(text);
                                                    yield StreamChunk::TextDelta(text.clone());
                                                }
                                            }
                                        }
                                        "input_json_delta" => {
                                            // Partial tool input JSON - accumulate
                                            if let Some(ref partial) = delta.partial_json {
                                                if let Some(entry) = streaming_tool_inputs.get_mut(&event.index) {
                                                    entry.2.push_str(partial);
                                                    tracing::trace!(
                                                        index = event.index,
                                                        partial_len = partial.len(),
                                                        total_len = entry.2.len(),
                                                        "Accumulating tool input JSON"
                                                    );
                                                }
                                            }
                                        }
                                        _ => {
                                            tracing::trace!(
                                                delta_type = %delta.delta_type,
                                                index = event.index,
                                                "Unknown delta type in stream_event"
                                            );
                                        }
                                    }
                                }
                            }
                            "content_block_stop" => {
                                // Content block finished
                                tracing::trace!(index = event.index, "Content block stopped");

                                // If this was a tool input, emit the complete tool call
                                if let Some((tool_id, tool_name, accumulated_json)) = streaming_tool_inputs.remove(&event.index) {
                                    // End text block if it was started
                                    if text_started {
                                        yield StreamChunk::TextEnd;
                                        text_started = false;
                                    }

                                    // Parse the accumulated JSON or use empty object
                                    let input_str = if accumulated_json.is_empty() {
                                        "{}".to_string()
                                    } else {
                                        accumulated_json
                                    };

                                    debug!(
                                        id = %tool_id,
                                        name = %tool_name,
                                        input_len = input_str.len(),
                                        "Emitting ToolObserved from stream_event"
                                    );

                                    // Track the tool call for matching with results
                                    tool_calls.push((tool_id.clone(), tool_name.clone(), input_str.clone()));

                                    // Mark this tool index as streamed
                                    streamed_tool_indices.insert(event.index);

                                    // Emit observed tool (not ToolCall, so runner won't execute)
                                    yield StreamChunk::ToolObserved {
                                        id: tool_id,
                                        name: tool_name,
                                        input: input_str,
                                    };
                                }
                            }
                            "message_start" | "message_delta" | "message_stop" => {
                                // Message-level events - we handle these via the full message types
                                tracing::trace!(event_type = %event.event_type, "Ignoring message-level stream event");
                            }
                            _ => {
                                tracing::trace!(
                                    event_type = %event.event_type,
                                    "Unknown stream event type"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        // Not all lines are valid JSON messages (could be debug output)
                        tracing::trace!(error = %e, line_preview = %line.chars().take(100).collect::<String>(), "Failed to parse JSON line (may be debug output)");
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

    fn tool_timeout(&self) -> Option<std::time::Duration> {
        // Claude CLI has a ~60 second timeout for MCP tool calls.
        // We use 45 seconds to ensure we cancel before Claude CLI times out,
        // giving a clear error message rather than a generic timeout.
        Some(std::time::Duration::from_secs(45))
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

    #[test]
    fn test_stream_event_text_delta_parsing() {
        // Test that we can parse the exact format from Claude CLI stream-json output
        let json = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}}"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        match msg {
            CliMessage::StreamEvent { event } => {
                assert_eq!(event.event_type, "content_block_delta");
                assert_eq!(event.index, 0);
                let delta = event.delta.unwrap();
                assert_eq!(delta.delta_type, "text_delta");
                assert_eq!(delta.text.unwrap(), "Hello");
            }
            _ => panic!("Expected StreamEvent"),
        }
    }

    #[test]
    fn test_stream_event_content_block_start_parsing() {
        // Test content_block_start for text blocks
        let json = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text"}}}"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        match msg {
            CliMessage::StreamEvent { event } => {
                assert_eq!(event.event_type, "content_block_start");
                assert_eq!(event.index, 0);
                let content_block = event.content_block.unwrap();
                assert_eq!(content_block.block_type, "text");
            }
            _ => panic!("Expected StreamEvent"),
        }
    }

    #[test]
    fn test_stream_event_tool_use_parsing() {
        // Test content_block_start for tool_use blocks
        let json = r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_123","name":"Read"}}}"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        match msg {
            CliMessage::StreamEvent { event } => {
                assert_eq!(event.event_type, "content_block_start");
                assert_eq!(event.index, 1);
                let content_block = event.content_block.unwrap();
                assert_eq!(content_block.block_type, "tool_use");
                assert_eq!(content_block.id.unwrap(), "toolu_123");
                assert_eq!(content_block.name.unwrap(), "Read");
            }
            _ => panic!("Expected StreamEvent"),
        }
    }

    #[test]
    fn test_stream_event_input_json_delta_parsing() {
        // Test input_json_delta for tool input streaming
        let json = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}}"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        match msg {
            CliMessage::StreamEvent { event } => {
                assert_eq!(event.event_type, "content_block_delta");
                assert_eq!(event.index, 1);
                let delta = event.delta.unwrap();
                assert_eq!(delta.delta_type, "input_json_delta");
                assert_eq!(delta.partial_json.unwrap(), r#"{"path":"#);
            }
            _ => panic!("Expected StreamEvent"),
        }
    }
}
