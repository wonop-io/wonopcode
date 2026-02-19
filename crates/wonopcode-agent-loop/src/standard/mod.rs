//! Standard agent loop implementation.
//!
//! This module provides `StandardLoop`, the default `AgentLoop` implementation
//! that replicates the current behavior of the runner's `run_prompt()` method.

mod streaming;
mod tools;

use async_trait::async_trait;
use futures::StreamExt;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Mutex;
use tracing::{debug, info, warn};

use wonopcode_provider::stream::{FinishReason, StreamChunk, Usage};
use wonopcode_provider::{ContentPart, GenerateOptions, Message as ProviderMessage};

use crate::{AgentLoop, LoopCapabilities, LoopContext, LoopError, LoopUpdate};

pub use streaming::StreamProcessor;
pub use tools::ToolExecutor;

/// Doom loop detection threshold - number of consecutive identical tool calls.
const DOOM_LOOP_THRESHOLD: usize = 3;

/// Maximum tool calls to keep in doom loop detector before pruning.
const DOOM_LOOP_MAX_RECORDS: usize = 100;

/// Record of a tool call for doom loop detection.
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
        Self::default()
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

/// Standard agent loop implementation.
///
/// This is the default loop that replicates the behavior of the original
/// `run_prompt()` method in `runner.rs`. It provides:
///
/// - Streaming responses from the provider
/// - Parallel tool execution
/// - Automatic message compaction (TODO)
/// - Doom loop detection
/// - Extended thinking support
///
/// # Example
///
/// ```rust,ignore
/// use wonopcode_agent_loop::StandardLoop;
///
/// let loop_impl = StandardLoop::new();
/// ```
pub struct StandardLoop {
    /// Maximum iterations before doom loop detection triggers.
    max_iterations: u32,
    /// Doom loop detector (interior mutability for &self methods).
    doom_detector: Mutex<DoomLoopDetector>,
}

impl StandardLoop {
    /// Create a new StandardLoop with default settings.
    pub fn new() -> Self {
        Self {
            max_iterations: 50,
            doom_detector: Mutex::new(DoomLoopDetector::new()),
        }
    }

    /// Create with custom iteration limit.
    pub fn with_max_iterations(max_iterations: u32) -> Self {
        Self {
            max_iterations,
            doom_detector: Mutex::new(DoomLoopDetector::new()),
        }
    }
}

impl Default for StandardLoop {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentLoop for StandardLoop {
    async fn run_prompt(
        &self,
        ctx: &mut LoopContext<'_>,
        user_input: &str,
    ) -> Result<String, LoopError> {
        info!(loop_name = "standard", "Starting prompt execution");

        // Reset doom loop detector for new prompt
        if let Ok(mut detector) = self.doom_detector.lock() {
            detector.reset();
        }

        // Add user message with optional images
        let user_msg = if ctx.prompt_images.is_empty() {
            ProviderMessage::user(user_input)
        } else {
            // Build multi-part message with images first, then text
            let mut content = Vec::with_capacity(ctx.prompt_images.len() + 1);

            // Add images
            for image in &ctx.prompt_images {
                content.push(ContentPart::image_base64(&image.media_type, &image.data));
            }

            // Add text
            content.push(ContentPart::text(user_input));

            ProviderMessage {
                role: wonopcode_provider::Role::User,
                content,
            }
        };
        ctx.messages.push(user_msg);

        let mut final_text = String::new();
        let mut iteration = 0;

        // Main agentic loop
        loop {
            // Check cancellation
            if ctx.is_cancelled() {
                // Save partial history before returning
                if !final_text.is_empty() {
                    ctx.messages.push(ProviderMessage::assistant(&final_text));
                }
                info!(
                    partial_text_len = final_text.len(),
                    "Prompt cancelled, partial response preserved"
                );
                return Ok(final_text);
            }

            // Check iteration limit
            iteration += 1;
            if iteration > self.max_iterations {
                return Err(LoopError::MaxIterations(self.max_iterations));
            }

            debug!(iteration, "Starting loop iteration");

            // Build generation options
            let options = GenerateOptions {
                temperature: ctx.config.temperature,
                max_tokens: ctx.config.max_tokens,
                system: ctx.config.system_prompt.clone(),
                tools: ctx.tool_defs.clone(),
                abort: Some(ctx.cancel.clone()),
                ..Default::default()
            };

            // Call provider
            debug!(
                iteration,
                message_count = ctx.messages.len(),
                tool_count = ctx.tool_defs.len(),
                "Calling provider"
            );

            let stream = ctx
                .provider
                .generate(ctx.messages.clone(), options)
                .await
                .map_err(LoopError::from_provider_error)?;

            tokio::pin!(stream);

            let mut current_text = String::new();
            let mut tool_calls: Vec<(String, String, String)> = Vec::new(); // (id, name, args)
            let mut finish_reason = FinishReason::EndTurn;
            let mut step_usage = Usage::default();
            // Track pending tool calls during streaming
            let mut pending_tool_calls: HashMap<String, (String, String)> = HashMap::new();
            // Track observed tools in order (for external tool execution like CLI providers)
            // Using IndexMap to preserve insertion order
            let mut observed_tools: IndexMap<String, (String, String)> = IndexMap::new(); // id -> (name, input)
                                                                                          // Track observed tool results (success, output) - separate from observed_tools
                                                                                          // because results arrive later via ToolResultObserved events
            let mut observed_tool_results: HashMap<String, (bool, String)> = HashMap::new();
            // Track content parts in order for correct interleaving
            // This is needed because text and tools can be interleaved:
            // text1 -> tool1 -> text2 -> tool2
            // We need to preserve this order in the persisted message
            let mut ordered_content: Vec<ContentPart> = Vec::new();

            // Process stream
            while let Some(chunk_result) = stream.next().await {
                if ctx.is_cancelled() {
                    // Save partial text and return it (not an error)
                    if !current_text.is_empty() {
                        ctx.messages.push(ProviderMessage::assistant(&current_text));
                    }
                    info!(
                        partial_text_len = current_text.len(),
                        "Stream cancelled, partial response preserved"
                    );
                    return Ok(current_text);
                }

                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(error = %e, "Stream error (will be skipped)");
                        continue;
                    }
                };

                match chunk {
                    StreamChunk::TextStart => {}
                    StreamChunk::TextDelta(delta) => {
                        current_text.push_str(&delta);
                        ctx.send_update(LoopUpdate::TextDelta(delta));
                    }
                    StreamChunk::TextEnd => {}
                    StreamChunk::ToolCallStart { id, name } => {
                        debug!(id = %id, name = %name, "Tool call started");
                        pending_tool_calls.insert(id.clone(), (name.clone(), String::new()));
                    }
                    StreamChunk::ToolCallDelta { id, delta } => {
                        if let Some((_, args)) = pending_tool_calls.get_mut(&id) {
                            args.push_str(&delta);
                        }
                    }
                    StreamChunk::ToolCall {
                        id,
                        name,
                        arguments,
                    } => {
                        debug!(id = %id, name = %name, "Tool call complete");
                        tool_calls.push((id, name, arguments));
                    }
                    StreamChunk::ReasoningStart => {}
                    StreamChunk::ReasoningDelta(delta) => {
                        ctx.send_update(LoopUpdate::ThinkingDelta(delta));
                    }
                    StreamChunk::ReasoningEnd => {}
                    StreamChunk::ToolObserved { id, name, input } => {
                        // Tool was observed being executed externally (e.g., by Claude CLI)
                        debug!(id = %id, name = %name, "Tool observed (external execution)");

                        // Flush any accumulated text BEFORE the tool to preserve order
                        // This ensures text1 -> tool1 -> text2 ordering is maintained
                        if !current_text.is_empty() {
                            ordered_content.push(ContentPart::text(&current_text));
                            current_text.clear();
                        }

                        // Add the tool to ordered content
                        let input_val: serde_json::Value =
                            serde_json::from_str(&input).unwrap_or(serde_json::Value::Null);
                        ordered_content.push(ContentPart::tool_use(&id, &name, input_val));

                        // Track the tool for result matching
                        observed_tools.insert(id.clone(), (name.clone(), input.clone()));
                        ctx.send_update(LoopUpdate::ToolStarted { id, name, input });
                    }
                    StreamChunk::ToolResultObserved {
                        id,
                        success,
                        output,
                    } => {
                        // Tool result was observed (external execution completed)
                        // Look up the tool name from when we observed it starting
                        let name = observed_tools
                            .get(&id)
                            .map(|(n, _)| n.clone())
                            .unwrap_or_else(|| "unknown".to_string());
                        debug!(id = %id, name = %name, success = %success, "Tool result observed");

                        // Store the result for later persistence
                        // This allows us to update the tool state from Pending to Completed
                        observed_tool_results.insert(id.clone(), (success, output.clone()));

                        ctx.send_update(LoopUpdate::ToolCompleted {
                            id,
                            name,
                            success,
                            output,
                            metadata: None,
                        });
                    }
                    StreamChunk::FinishStep {
                        usage,
                        accumulated_usage,
                        finish_reason: reason,
                    } => {
                        info!(
                            step_input = usage.input_tokens,
                            step_output = usage.output_tokens,
                            has_accumulated = accumulated_usage.is_some(),
                            "Received FinishStep with usage"
                        );
                        step_usage.merge(&usage);
                        finish_reason = reason;

                        // Calculate cost for THIS STEP only (delta, not cumulative)
                        // The receiver will accumulate these deltas
                        let model_info = ctx.model_info();
                        let step_cost = model_info.cost.calculate(
                            step_usage.input_tokens,
                            step_usage.output_tokens,
                        );

                        // Extract accumulated values from provider (if available)
                        let (accumulated_input, accumulated_output, accumulated_cost, last_request_input, last_request_output, last_request_cache_read) = 
                            if let Some(acc) = &accumulated_usage {
                                (
                                    Some(acc.total_input_tokens), 
                                    Some(acc.total_output_tokens), 
                                    Some(acc.total_cost),
                                    Some(acc.last_request_input),
                                    Some(acc.last_request_output),
                                    Some(acc.last_request_cache_read),
                                )
                            } else {
                                (None, None, None, None, None, None)
                            };

                        info!(
                            step_input = step_usage.input_tokens,
                            step_output = step_usage.output_tokens,
                            step_cost = step_cost,
                            accumulated_input = ?accumulated_input,
                            accumulated_output = ?accumulated_output,
                            accumulated_cost = ?accumulated_cost,
                            last_request_input = ?last_request_input,
                            last_request_output = ?last_request_output,
                            last_request_cache_read = ?last_request_cache_read,
                            "Sending TokenUsage update"
                        );

                        // Send token usage update with DELTA values and optional ACCUMULATED totals
                        ctx.send_update(LoopUpdate::TokenUsage {
                            input: step_usage.input_tokens,
                            output: step_usage.output_tokens,
                            cost: step_cost,
                            context_limit: model_info.limit.context,
                            accumulated_input,
                            accumulated_output,
                            accumulated_cost,
                            last_request_input,
                            last_request_output,
                            last_request_cache_read,
                        });
                    }
                    StreamChunk::Error(e) => {
                        warn!(error = %e, "Stream error");
                    }
                }
            }

            // Finalize any pending tool calls that didn't get a ToolCall chunk
            for (id, (name, args)) in pending_tool_calls {
                if !tool_calls.iter().any(|(tid, _, _)| tid == &id) {
                    tool_calls.push((id, name, args));
                }
            }

            info!(
                iteration,
                text_len = current_text.len(),
                tool_calls = tool_calls.len(),
                finish_reason = ?finish_reason,
                "Step completed"
            );

            // Update final text
            final_text = current_text.clone();

            // Add assistant message to history
            // Include both regular tool_calls AND observed_tools (from external execution like Claude CLI)
            let has_observed_tools = !observed_tools.is_empty();
            if !current_text.is_empty() || !tool_calls.is_empty() || has_observed_tools {
                let content = if has_observed_tools {
                    // For observed tools (external execution like Claude CLI),
                    // use ordered_content which preserves text/tool interleaving.
                    // First, flush any remaining text that came after the last tool.
                    if !current_text.is_empty() {
                        ordered_content.push(ContentPart::text(&current_text));
                    }
                    ordered_content.clone()
                } else {
                    // For internal tool execution, build content the traditional way
                    let mut content = vec![];
                    if !current_text.is_empty() {
                        content.push(ContentPart::text(&current_text));
                    }
                    // Add regular tool calls (from internal execution)
                    for (id, name, args) in &tool_calls {
                        let input: serde_json::Value =
                            serde_json::from_str(args).unwrap_or(serde_json::Value::Null);
                        content.push(ContentPart::tool_use(id, name, input));
                    }
                    content
                };

                debug!(
                    text_len = current_text.len(),
                    tool_calls = tool_calls.len(),
                    observed_tools = observed_tools.len(),
                    ordered_content_len = if has_observed_tools {
                        ordered_content.len()
                    } else {
                        0
                    },
                    "Adding assistant message to history"
                );

                ctx.messages.push(ProviderMessage {
                    role: wonopcode_provider::Role::Assistant,
                    content,
                });

                // Add tool result messages for observed tools (like internal execution does)
                // This ensures the tool results are available for persistence
                for (id, (success, output)) in &observed_tool_results {
                    debug!(
                        tool_id = %id,
                        success = %success,
                        output_len = output.len(),
                        "Adding observed tool result to history"
                    );
                    ctx.messages.push(ProviderMessage::tool_result(id, output));
                }
            }

            // If no tool calls to execute internally, we're done
            // Note: observed_tools are already executed by the external process (e.g., Claude CLI)
            // so we don't iterate based on them - they're only recorded for history
            if tool_calls.is_empty() {
                break;
            }

            // Execute tool calls
            // Get the provider's tool timeout (if any) for permission checking
            let tool_timeout = ctx.provider.tool_timeout();
            let tool_executor = ToolExecutor::with_permissions(
                ctx.tools,
                ctx.snapshot_store.cloned(),
                ctx.file_time.clone(),
                ctx.sandbox.clone(),
                ctx.tool_event_tx.clone(),
                ctx.permission_checker.clone(),
                tool_timeout,
            );

            let mut tool_results = Vec::new();

            for (call_id, tool_name, args_str) in tool_calls {
                let input: serde_json::Value =
                    serde_json::from_str(&args_str).unwrap_or(serde_json::Value::Null);

                // Check doom loop
                let is_doom_loop = self
                    .doom_detector
                    .lock()
                    .map(|mut d| d.record_and_check(&tool_name, &input))
                    .unwrap_or(false);
                if is_doom_loop {
                    warn!(
                        tool = %tool_name,
                        "Doom loop detected: {} consecutive identical calls",
                        DOOM_LOOP_THRESHOLD
                    );
                    ctx.send_update(LoopUpdate::Status(format!(
                        "Doom loop detected: '{tool_name}' called {DOOM_LOOP_THRESHOLD} times with identical args"
                    )));

                    let error_msg = format!(
                        "Tool execution blocked: doom loop detected. \
                        You have called '{tool_name}' {DOOM_LOOP_THRESHOLD} times in a row with identical arguments. \
                        Please try a different approach or use different arguments."
                    );

                    ctx.send_update(LoopUpdate::ToolStarted {
                        name: tool_name.clone(),
                        id: call_id.clone(),
                        input: "{}".to_string(),
                    });
                    ctx.send_update(LoopUpdate::ToolCompleted {
                        id: call_id.clone(),
                        name: tool_name.clone(),
                        success: false,
                        output: error_msg.clone(),
                        metadata: None,
                    });

                    tool_results.push((call_id, error_msg));
                    continue;
                }

                // Send tool started
                ctx.send_update(LoopUpdate::ToolStarted {
                    name: tool_name.clone(),
                    id: call_id.clone(),
                    input: args_str.clone(),
                });

                // Execute tool
                let result = tool_executor
                    .execute(&tool_name, input, ctx.cwd, &ctx.session_id, ctx.cancel)
                    .await;

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
                        (output, true, Some(out.metadata))
                    }
                    Err(e) => (format!("Error: {e}"), false, None),
                };

                info!(
                    tool = %tool_name,
                    call_id = %call_id,
                    success = success,
                    output_len = output.len(),
                    "Tool completed"
                );

                ctx.send_update(LoopUpdate::ToolCompleted {
                    id: call_id.clone(),
                    name: tool_name.clone(),
                    success,
                    output: output.clone(),
                    metadata,
                });

                tool_results.push((call_id, output));
            }

            // Add tool results to messages
            for (call_id, output) in tool_results {
                ctx.messages
                    .push(ProviderMessage::tool_result(&call_id, &output));
            }
        }

        info!(
            loop_name = "standard",
            iterations = iteration,
            response_len = final_text.len(),
            "Prompt execution complete"
        );

        // Send completion signal to TUI to exit "thinking" state
        ctx.send_update(LoopUpdate::ResponseComplete {
            text: final_text.clone(),
        });

        Ok(final_text)
    }

    fn name(&self) -> &'static str {
        "standard"
    }

    fn capabilities(&self) -> LoopCapabilities {
        LoopCapabilities::standard()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doom_loop_detector() {
        let mut detector = DoomLoopDetector::new();
        let args = serde_json::json!({"file": "test.txt"});

        // First two calls should not trigger
        assert!(!detector.record_and_check("read", &args));
        assert!(!detector.record_and_check("read", &args));

        // Third identical call should trigger
        assert!(detector.record_and_check("read", &args));

        // Different tool should not trigger
        assert!(!detector.record_and_check("write", &args));
    }

    #[test]
    fn test_doom_loop_detector_reset() {
        let mut detector = DoomLoopDetector::new();
        let args = serde_json::json!({});

        detector.record_and_check("test", &args);
        detector.record_and_check("test", &args);
        detector.reset();

        // After reset, should not trigger immediately
        assert!(!detector.record_and_check("test", &args));
    }

    #[test]
    fn test_standard_loop_default() {
        let loop_impl = StandardLoop::default();
        assert_eq!(loop_impl.max_iterations, 50);
    }

    #[test]
    fn test_standard_loop_with_max_iterations() {
        let loop_impl = StandardLoop::with_max_iterations(100);
        assert_eq!(loop_impl.max_iterations, 100);
    }

    #[test]
    fn test_standard_loop_name() {
        let loop_impl = StandardLoop::new();
        assert_eq!(loop_impl.name(), "standard");
    }

    #[test]
    fn test_standard_loop_capabilities() {
        let loop_impl = StandardLoop::new();
        let caps = loop_impl.capabilities();
        assert!(caps.streaming);
        assert!(caps.parallel_tools);
        assert!(caps.extended_thinking);
    }
}
