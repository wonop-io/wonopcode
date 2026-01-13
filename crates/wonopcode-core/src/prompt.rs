//! Prompt loop - the core AI conversation engine.
//!
//! This module handles the back-and-forth conversation with AI providers:
//! - Sending messages to the provider
//! - Processing streaming responses
//! - Executing tool calls
//! - Handling continuation (tool_use -> continue)

use crate::bus::{Bus, PartUpdated, SessionStatus, Status};
use crate::error::CoreResult;
use crate::message::{ModelRef, UserMessage};
use crate::session::{Session, SessionRepository};
use futures::StreamExt;
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use wonopcode_provider::{
    stream::{FinishReason, StreamChunk},
    ContentPart, GenerateOptions, LanguageModel, Message as ProviderMessage, Role, ToolDefinition,
};
use wonopcode_snapshot::SnapshotStore;
use wonopcode_tools::{ToolContext, ToolOutput, ToolRegistry};
use wonopcode_util::FileTimeState;

/// Maximum number of continuation steps to prevent infinite loops.
const MAX_STEPS: usize = 100;

/// Configuration for the prompt loop.
#[derive(Debug, Clone)]
pub struct PromptConfig {
    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,
    /// Temperature for sampling.
    pub temperature: Option<f32>,
    /// System prompt.
    pub system: Option<String>,
    /// Maximum steps before stopping.
    pub max_steps: usize,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            max_tokens: Some(8192),
            temperature: Some(0.7),
            system: None,
            max_steps: MAX_STEPS,
        }
    }
}

/// Result of a prompt loop execution.
#[derive(Debug)]
pub struct PromptResult {
    /// The assistant's final text response.
    pub text: String,
    /// Tool calls that were made.
    pub tool_calls: Vec<ToolCallResult>,
    /// Total tokens used.
    pub tokens_input: u32,
    pub tokens_output: u32,
    /// Finish reason.
    pub finish_reason: FinishReason,
    /// Number of steps taken.
    pub steps: usize,
}

/// Result of a tool call.
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub tool: String,
    pub input: Value,
    pub output: String,
    pub success: bool,
}

/// The prompt loop executor.
///
/// This is a reusable core for AI conversation loops that can be used by:
/// - Server APIs for headless operation
/// - CLI tools for scripted usage  
/// - Tests for integration testing
///
/// For TUI-integrated usage, see the `Runner` in the `wonopcode` crate which
/// provides additional features like compaction, MCP integration, and UI updates.
///
/// # Example
///
/// ```ignore
/// let loop_executor = PromptLoop::new(provider, tools, session_repo, bus, cancel);
/// let result = loop_executor.run(&session, "Hello", PromptConfig::default()).await?;
/// println!("Response: {}", result.text);
/// ```
pub struct PromptLoop {
    provider: Arc<dyn LanguageModel>,
    tools: Arc<ToolRegistry>,
    /// Session repository for persistence support.
    /// Used for saving session state; messages are also kept in memory for performance.
    #[expect(dead_code, reason = "stored for future session persistence features")]
    session_repo: Arc<SessionRepository>,
    bus: Bus,
    cancel: CancellationToken,
    /// Optional snapshot store for file versioning.
    snapshot: Option<Arc<SnapshotStore>>,
    /// Optional file time tracker for concurrent edit detection.
    file_time: Option<Arc<FileTimeState>>,
}

impl PromptLoop {
    /// Create a new prompt loop.
    pub fn new(
        provider: Arc<dyn LanguageModel>,
        tools: Arc<ToolRegistry>,
        session_repo: Arc<SessionRepository>,
        bus: Bus,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            provider,
            tools,
            session_repo,
            bus,
            cancel,
            snapshot: None,
            file_time: None,
        }
    }

    /// Create a new prompt loop with snapshot and file time tracking.
    pub fn with_tracking(
        provider: Arc<dyn LanguageModel>,
        tools: Arc<ToolRegistry>,
        session_repo: Arc<SessionRepository>,
        bus: Bus,
        cancel: CancellationToken,
        snapshot: Option<Arc<SnapshotStore>>,
        file_time: Option<Arc<FileTimeState>>,
    ) -> Self {
        Self {
            provider,
            tools,
            session_repo,
            bus,
            cancel,
            snapshot,
            file_time,
        }
    }

    /// Execute the prompt loop for a user message.
    pub async fn run(
        &self,
        session: &Session,
        user_input: &str,
        config: PromptConfig,
    ) -> CoreResult<PromptResult> {
        let mut messages: Vec<ProviderMessage> = Vec::new();
        let mut total_input_tokens = 0u32;
        let mut total_output_tokens = 0u32;
        let mut all_tool_calls = Vec::new();
        let mut final_text = String::new();
        let mut finish_reason = FinishReason::EndTurn;
        let mut steps = 0;

        // Create user message
        let user_msg = UserMessage::new(
            &session.id,
            "default",
            ModelRef {
                provider_id: self.provider.provider_id().to_string(),
                model_id: self.provider.model_info().id.clone(),
            },
        );

        // Add user message to provider messages
        messages.push(ProviderMessage::user(user_input));

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

        // Update session status
        self.bus
            .publish(SessionStatus {
                session_id: session.id.clone(),
                status: Status::Running,
            })
            .await;

        // Main loop
        loop {
            if self.cancel.is_cancelled() {
                warn!("Prompt loop cancelled");
                break;
            }

            if steps >= config.max_steps {
                warn!("Max steps reached: {}", config.max_steps);
                break;
            }

            steps += 1;
            debug!(step = steps, "Starting prompt step");

            // Build generate options
            let options = GenerateOptions {
                temperature: config.temperature,
                max_tokens: config.max_tokens,
                system: config.system.clone(),
                tools: tool_defs.clone(),
                abort: Some(self.cancel.clone()),
                ..Default::default()
            };

            // Call the provider
            let stream = match self.provider.generate(messages.clone(), options).await {
                Ok(s) => s,
                Err(e) => {
                    error!("Provider error: {}", e);
                    return Err(crate::error::CoreError::from(
                        crate::error::SessionError::Locked {
                            id: format!("Provider error: {e}"),
                        },
                    ));
                }
            };

            // Process the stream
            let mut current_text = String::new();
            let mut tool_calls: Vec<(String, String, String)> = Vec::new(); // (id, name, args)
            let mut step_input_tokens = 0u32;
            let mut step_output_tokens = 0u32;

            tokio::pin!(stream);

            while let Some(chunk_result) = stream.next().await {
                if self.cancel.is_cancelled() {
                    break;
                }

                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("Stream error: {}", e);
                        break;
                    }
                };

                match chunk {
                    StreamChunk::TextStart => {
                        debug!("Text generation started");
                    }
                    StreamChunk::TextDelta(delta) => {
                        current_text.push_str(&delta);
                        // Emit partial update event
                        self.bus
                            .publish(PartUpdated {
                                session_id: session.id.clone(),
                                message_id: user_msg.id.clone(),
                                part_id: "text".to_string(),
                                delta: Some(delta),
                            })
                            .await;
                    }
                    StreamChunk::TextEnd => {
                        debug!("Text generation ended");
                    }
                    StreamChunk::ToolCallStart { id, name } => {
                        debug!(id = %id, name = %name, "Tool call started");
                        tool_calls.push((id, name, String::new()));
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
                        // Find and update or add
                        if let Some(call) = tool_calls.iter_mut().find(|c| c.0 == id) {
                            call.2 = arguments;
                        } else {
                            tool_calls.push((id, name, arguments));
                        }
                    }
                    StreamChunk::ReasoningStart => {}
                    StreamChunk::ReasoningDelta(_) => {}
                    StreamChunk::ReasoningEnd => {}
                    StreamChunk::ToolObserved { .. } => {
                        // Tool observed from external execution (e.g., Claude CLI)
                        // For the basic prompt loop, we just ignore these
                    }
                    StreamChunk::ToolResultObserved { .. } => {
                        // Tool result observed from external execution
                        // For the basic prompt loop, we just ignore these
                    }
                    StreamChunk::FinishStep {
                        usage,
                        finish_reason: reason,
                    } => {
                        step_input_tokens = usage.input_tokens;
                        step_output_tokens = usage.output_tokens;
                        finish_reason = reason;
                        debug!(
                            input = step_input_tokens,
                            output = step_output_tokens,
                            reason = ?finish_reason,
                            "Step finished"
                        );
                    }
                    StreamChunk::Error(err) => {
                        warn!("Stream error: {}", err);
                    }
                }
            }

            // Update totals
            total_input_tokens += step_input_tokens;
            total_output_tokens += step_output_tokens;
            final_text = current_text.clone();

            // Add assistant message to history
            let mut assistant_content = vec![];
            if !current_text.is_empty() {
                assistant_content.push(ContentPart::text(&current_text));
            }
            for (id, name, args) in &tool_calls {
                let input: Value = serde_json::from_str(args).unwrap_or(Value::Null);
                assistant_content.push(ContentPart::tool_use(id, name, input));
            }

            if !assistant_content.is_empty() {
                messages.push(ProviderMessage {
                    role: Role::Assistant,
                    content: assistant_content,
                });
            }

            // Handle tool calls
            if !tool_calls.is_empty() {
                info!("Executing {} tool calls", tool_calls.len());

                for (call_id, tool_name, args_str) in tool_calls {
                    let input: Value = serde_json::from_str(&args_str).unwrap_or(Value::Null);

                    // Execute the tool
                    let result = self.execute_tool(session, &tool_name, input.clone()).await;

                    let (output, success) = match result {
                        Ok(out) => (out.output, true),
                        Err(e) => (format!("Error: {e}"), false),
                    };

                    all_tool_calls.push(ToolCallResult {
                        tool: tool_name.clone(),
                        input: input.clone(),
                        output: output.clone(),
                        success,
                    });

                    // Add tool result to messages
                    messages.push(ProviderMessage::tool_result(&call_id, &output));
                }

                // Continue the loop for tool_use finish reason
                if finish_reason == FinishReason::ToolUse {
                    continue;
                }
            }

            // End turn or other finish reasons - stop the loop
            break;
        }

        // Update session status to idle
        self.bus
            .publish(SessionStatus {
                session_id: session.id.clone(),
                status: Status::Idle,
            })
            .await;

        Ok(PromptResult {
            text: final_text,
            tool_calls: all_tool_calls,
            tokens_input: total_input_tokens,
            tokens_output: total_output_tokens,
            finish_reason,
            steps,
        })
    }

    /// Execute a single tool.
    async fn execute_tool(
        &self,
        session: &Session,
        tool_name: &str,
        input: Value,
    ) -> Result<ToolOutput, wonopcode_tools::ToolError> {
        let tool = self.tools.get(tool_name).ok_or_else(|| {
            wonopcode_tools::ToolError::validation(format!("Unknown tool: {tool_name}"))
        })?;

        let ctx = ToolContext {
            session_id: session.id.clone(),
            message_id: "current".to_string(),
            agent: "default".to_string(),
            abort: self.cancel.clone(),
            root_dir: std::path::PathBuf::from(&session.directory),
            cwd: std::path::PathBuf::from(&session.directory),
            snapshot: self.snapshot.clone(),
            file_time: self.file_time.clone(),
            sandbox: None, // Sandbox not used in prompt executor (yet)
            event_tx: None,
        };

        let _timing = wonopcode_util::TimingGuard::tool(tool_name);

        tool.execute(input, &ctx).await
    }
}

/// A simple synchronous prompt function for basic usage.
pub async fn prompt_once(
    provider: Arc<dyn LanguageModel>,
    tools: Arc<ToolRegistry>,
    user_input: &str,
    system: Option<&str>,
    _cwd: &std::path::Path,
) -> CoreResult<String> {
    let cancel = CancellationToken::new();

    // Build tool definitions
    let tool_defs: Vec<ToolDefinition> = tools
        .all()
        .map(|t| ToolDefinition {
            name: t.id().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters_schema(),
        })
        .collect();

    let options = GenerateOptions {
        system: system.map(|s| s.to_string()),
        tools: tool_defs,
        abort: Some(cancel.clone()),
        ..Default::default()
    };

    let messages = vec![ProviderMessage::user(user_input)];

    let stream = provider.generate(messages, options).await.map_err(|e| {
        crate::error::CoreError::from(crate::error::SessionError::Locked {
            id: format!("Provider error: {e}"),
        })
    })?;

    let mut text = String::new();
    tokio::pin!(stream);

    while let Some(chunk_result) = stream.next().await {
        if let Ok(StreamChunk::TextDelta(delta)) = chunk_result {
            text.push_str(&delta);
        }
    }

    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_config_default() {
        let config = PromptConfig::default();
        assert_eq!(config.max_steps, MAX_STEPS);
        assert!(config.max_tokens.is_some());
    }
}
