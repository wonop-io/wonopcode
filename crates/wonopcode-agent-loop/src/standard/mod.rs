//! Standard agent loop implementation.
//!
//! This module provides `StandardLoop`, the default `AgentLoop` implementation
//! that replicates the current behavior of the runner's `run_prompt()` method.
//!
//! # Observational Memory Support
//!
//! When `LoopContext.om_enabled` is true and the memory state fields are set,
//! StandardLoop uses Observational Memory (OM) for context compression instead
//! of legacy message compaction. OM provides:
//!
//! - Bounded context window via token-based thresholds
//! - Event-level observation extraction (not summaries)
//! - Prompt cache optimization

mod streaming;
mod tools;

use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use tracing::{debug, info, warn};
use uuid::Uuid;

use wonopcode_observational_memory::{
    ObserverAgent, ObserverConfig, Priority, ReflectorAgent, ReflectorConfig, TokenCounter,
    persistence::ObservationPersistence,
};
use wonopcode_provider::stream::{FinishReason, StreamChunk, Usage};
use wonopcode_provider::{ContentPart, GenerateOptions, Message as ProviderMessage};

use wonopcode_codemode::{format_hints_for_injection, select_hints};

use crate::context::{ObservationalMemoryStateSnapshot, ObservationSnapshot};
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
/// - Observational Memory for context compression (when enabled)
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
    /// Maximum iterations before stopping. None means unlimited.
    max_iterations: Option<u32>,
    /// Doom loop detector (interior mutability for &self methods).
    doom_detector: Mutex<DoomLoopDetector>,
}

impl StandardLoop {
    /// Create a new StandardLoop with default settings.
    pub fn new() -> Self {
        Self {
            max_iterations: None,
            doom_detector: Mutex::new(DoomLoopDetector::new()),
        }
    }

    /// Create with custom iteration limit. None means unlimited.
    pub fn with_max_iterations(max_iterations: Option<u32>) -> Self {
        Self {
            max_iterations,
            doom_detector: Mutex::new(DoomLoopDetector::new()),
        }
    }

    /// Create an OM snapshot from the current context for UI updates.
    fn create_om_snapshot(ctx: &LoopContext<'_>) -> ObservationalMemoryStateSnapshot {
        let (observations, stats) = if let Some(ref ms) = ctx.memory_state {
            let obs: Vec<ObservationSnapshot> = ms.observations.iter()
                .map(|o| Self::observation_to_snapshot(o))
                .collect();
            (obs, ms.stats.clone())
        } else {
            (Vec::new(), wonopcode_observational_memory::MemoryStats::default())
        };

        let (observation_tokens, message_tokens, reflector_threshold, observer_threshold) = 
            if let Some(ref sm) = ctx.token_state_machine {
                let thresholds = sm.thresholds();
                (
                    sm.observation_tokens(),
                    sm.message_tokens(),
                    thresholds.reflector_threshold,
                    thresholds.observer_threshold,
                )
            } else {
                (0, 0, 20_000, 10_000)
            };

        let avg_compression = if stats.tokens_observed > 0 {
            stats.tokens_observed as f32 / stats.tokens_after_compression.max(1) as f32
        } else {
            1.0
        };

        // Get cross-session info from memory state
        let (loaded_from_previous_session, loaded_session_date) = if let Some(ref ms) = ctx.memory_state {
            let date = if ms.loaded_from_previous_session && !ms.observations.is_empty() {
                // Format the oldest observation date as the session date
                ms.observations.first()
                    .map(|o| o.observation_date.format("%B %d").to_string())
            } else {
                None
            };
            (ms.loaded_from_previous_session, date)
        } else {
            (false, None)
        };

        ObservationalMemoryStateSnapshot {
            enabled: ctx.om_enabled,
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
            loaded_from_previous_session,
            loaded_session_date,
        }
    }

    /// Convert an Observation to an ObservationSnapshot for UI display.
    fn observation_to_snapshot(obs: &wonopcode_observational_memory::Observation) -> ObservationSnapshot {
        ObservationSnapshot {
            id: obs.id.clone(),
            priority: match obs.priority {
                Priority::High => "high".to_string(),
                Priority::Medium => "medium".to_string(),
                Priority::Low => "low".to_string(),
            },
            timestamp: obs.observation_date.format("%H:%M").to_string(),
            content: obs.content.clone(),
            children: obs.children.iter()
                .map(|c| Self::observation_to_snapshot(c))
                .collect(),
            pinned: obs.pinned,
        }
    }

    /// Run Observer/Reflector if thresholds are met.
    ///
    /// This is called after each prompt turn when OM is enabled.
    /// Returns true if observation or reflection was triggered.
    async fn maybe_observe_and_reflect(
        ctx: &mut LoopContext<'_>,
    ) -> Result<bool, LoopError> {
        // Check if OM is enabled and ready
        if !ctx.is_om_ready() {
            return Ok(false);
        }

        let mut any_triggered = false;

        // First, count message tokens (read-only access to ctx)
        let chars_per_token = ctx.provider.model_info().tokenizer.chars_per_token;
        let token_counter = TokenCounter::from_ratio(chars_per_token);
        let message_tokens: u32 = ctx.messages.iter()
            .map(|m| {
                m.content.iter()
                    .map(|p| match p {
                        ContentPart::Text { text } => token_counter.count_str(text),
                        ContentPart::ToolUse { input, .. } => {
                            token_counter.count_str(&input.to_string())
                        }
                        ContentPart::ToolResult { content, .. } => {
                            token_counter.count_str(content)
                        }
                        _ => 0,
                    })
                    .sum::<u32>()
            })
            .sum();

        // Get thresholds (needs token_state_machine)
        let thresholds = ctx.token_state_machine.as_ref().ok_or_else(|| LoopError::internal("OM token_state_machine not initialized"))?.thresholds();
        let should_observe = thresholds.should_observe(message_tokens);
        
        // Update token state machine
        ctx.token_state_machine.as_mut().ok_or_else(|| LoopError::internal("OM token_state_machine not initialized"))?.update_message_tokens(message_tokens);

        // Check if observation should be triggered
        if should_observe {
            debug!("OM: Observer threshold reached ({} tokens)", message_tokens);
            
            // Get messages to observe (read memory_state)
            // Always use ctx.messages.len() as end - the stored range may have a stale end value
            // if new messages were added since the last observation
            let start = ctx.memory_state.as_ref().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?
                .unobserved_message_range
                .map(|(s, _)| s)
                .unwrap_or(0);
            let end = ctx.messages.len();
            
            if start < end && start < ctx.messages.len() {
                let messages_to_observe: Vec<_> = ctx.messages[start..end.min(ctx.messages.len())]
                    .to_vec();
                
                // Run Observer (release mutable borrow during async call)
                ctx.token_state_machine.as_mut().ok_or_else(|| LoopError::internal("OM token_state_machine not initialized"))?.start_observer();
                
                let provider = ctx.provider.clone();
                let observer = ObserverAgent::new(ObserverConfig::default(), provider);
                
                match observer.run(&messages_to_observe, Utc::now()).await {
                    Ok(result) => {
                        let obs_count = result.observations.len();
                        let compression = result.compression_ratio();
                        
                        info!(
                            "OM: Observer completed: {} messages → {} observations ({:.1}x compression)",
                            end - start, obs_count, compression
                        );
                        
                        // Update memory state with new observations
                        {
                            let memory_state = ctx.memory_state.as_mut().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?;
                            memory_state.add_pending_batch(result.observations);
                            memory_state.commit_observations(end - start);
                        }
                        
                        // Save observations to disk immediately (continuous persistence)
                        // This ensures observations survive Cmd+Q or unexpected termination
                        if let Some(ref project_dir) = ctx.om_project_dir {
                            let persistence = ObservationPersistence::new(project_dir);
                            let memory_state = ctx.memory_state.as_ref().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?;
                            match persistence.save(memory_state) {
                                Ok(()) => {
                                    info!(
                                        "OM: Saved {} observations to {}",
                                        memory_state.observations.len(),
                                        persistence.observations_path().display()
                                    );
                                }
                                Err(e) => {
                                    warn!("OM: Failed to persist observations after observer: {:?}", e);
                                }
                            }
                        }
                        
                        // CRITICAL: Emit new messages for persistence BEFORE draining!
                        // The runner needs these messages for session persistence.
                        // After the drain, ctx.messages will be empty/reduced.
                        {
                            let new_messages: Vec<_> = ctx.messages
                                .iter()
                                .skip(ctx.messages_count_at_start)
                                .cloned()
                                .collect();
                            
                            if !new_messages.is_empty() {
                                info!(
                                    "OM: Emitting {} new messages for persistence before drain",
                                    new_messages.len()
                                );
                                ctx.send_update(LoopUpdate::MessagesForPersistence(new_messages));
                            }
                        }
                        
                        // Remove observed messages from context, keeping recent ones.
                        // This is the key step that bounds the context window!
                        // We keep the system message (if any) and the most recent messages
                        // that weren't observed yet.
                        let messages_to_remove = end - start;
                        if messages_to_remove > 0 && obs_count > 0 {
                            // Find system message if present (always at index 0)
                            let has_system = ctx.messages.first()
                                .map(|m| matches!(m.role, wonopcode_provider::Role::System))
                                .unwrap_or(false);
                            
                            // Determine how many recent messages to keep (unobserved)
                            let _keep_recent = ctx.messages.len().saturating_sub(end);
                            
                            if has_system {
                                // Keep system message, remove observed, keep recent
                                // [system][observed...][recent...] -> [system][recent...]
                                let system_msg = ctx.messages.remove(0);
                                ctx.messages.drain(0..messages_to_remove.min(ctx.messages.len()));
                                ctx.messages.insert(0, system_msg);
                            } else {
                                // No system message, just remove observed
                                ctx.messages.drain(start..end.min(ctx.messages.len()));
                            }
                            
                            info!(
                                "OM: Removed {} observed messages, {} messages remain",
                                messages_to_remove, ctx.messages.len()
                            );
                            
                            // Reset unobserved range to track all remaining messages
                            // (excluding system message at index 0 if present)
                            let new_start = if has_system { 1 } else { 0 };
                            ctx.memory_state.as_mut().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?.unobserved_message_range = 
                                Some((new_start, ctx.messages.len()));
                        }
                        
                        // Complete observer in state machine
                        let observation_tokens = ctx.memory_state.as_ref().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?.observation_tokens;
                        ctx.token_state_machine.as_mut().ok_or_else(|| LoopError::internal("OM token_state_machine not initialized"))?
                            .complete_observer(message_tokens, observation_tokens);
                        
                        // Send OM snapshot to UI
                        let snapshot = Self::create_om_snapshot(ctx);
                        let total_obs = snapshot.observations.len();
                        info!(
                            "🧠 [StandardLoop] Sending OM update after observer: {} observations",
                            total_obs
                        );
                        ctx.send_update(LoopUpdate::ObservationalMemoryUpdate(snapshot));
                        
                        // Also send a status message for the chat indicator
                        ctx.send_update(LoopUpdate::Status(format!(
                            "Memory updated: {} observations", total_obs
                        )));
                        
                        any_triggered = true;
                    }
                    Err(e) => {
                        warn!("OM: Observer failed: {:?}", e);
                    }
                }
            }
        }

        // Check if reflection should be triggered
        let observation_tokens = ctx.memory_state.as_ref().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?.observation_tokens;
        let obs_count = ctx.memory_state.as_ref().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?.observations.len();
        let should_reflect = ctx.token_state_machine.as_ref().ok_or_else(|| LoopError::internal("OM token_state_machine not initialized"))?
            .thresholds().should_reflect(observation_tokens) && obs_count >= 10;
        
        if should_reflect {
            debug!("OM: Reflector threshold reached ({} tokens)", observation_tokens);
            
            ctx.token_state_machine.as_mut().ok_or_else(|| LoopError::internal("OM token_state_machine not initialized"))?.start_reflector();
            
            let observations_to_reflect = ctx.memory_state.as_ref().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?.observations.clone();
            let provider = ctx.provider.clone();
            let reflector = ReflectorAgent::new(ReflectorConfig::default(), provider);
            
            match reflector.run(&observations_to_reflect, Utc::now()).await {
                Ok(result) => {
                    info!(
                        "OM: Reflector completed: {} → {} observations ({:.0}% reduction)",
                        result.input_count,
                        result.output_count,
                        (1.0 - result.compression_ratio()) * 100.0
                    );
                    
                    // Update memory state with restructured observations
                    let reflection_stats = wonopcode_observational_memory::ReflectionStats {
                        merged: result.merged_count as u32,
                        dropped: result.dropped_count as u32,
                        meta_added: result.patterns.len() as u32,
                    };
                    ctx.memory_state.as_mut().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?
                        .commit_reflection(result.observations, reflection_stats);
                    
                    // Save observations to disk immediately (continuous persistence)
                    // This ensures observations survive Cmd+Q or unexpected termination
                    if let Some(ref project_dir) = ctx.om_project_dir {
                        let persistence = ObservationPersistence::new(project_dir);
                        let memory_state = ctx.memory_state.as_ref().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?;
                        match persistence.save(memory_state) {
                            Ok(()) => {
                                info!(
                                    "OM: Saved {} observations to {} (post-reflection)",
                                    memory_state.observations.len(),
                                    persistence.observations_path().display()
                                );
                            }
                            Err(e) => {
                                warn!("OM: Failed to persist observations after reflector: {:?}", e);
                            }
                        }
                    }
                    
                    // Complete reflector in state machine
                    let new_observation_tokens = ctx.memory_state.as_ref().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?.observation_tokens;
                    ctx.token_state_machine.as_mut().ok_or_else(|| LoopError::internal("OM token_state_machine not initialized"))?
                        .complete_reflector(new_observation_tokens);
                    
                    // Send OM snapshot to UI after reflection
                    let snapshot = Self::create_om_snapshot(ctx);
                    info!(
                        "🧠 [StandardLoop] Sending OM update after reflector: {} observations",
                        snapshot.observations.len()
                    );
                    ctx.send_update(LoopUpdate::ObservationalMemoryUpdate(snapshot));
                    
                    any_triggered = true;
                }
                Err(e) => {
                    warn!("OM: Reflector failed: {:?}", e);
                }
            }
        }

        Ok(any_triggered)
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
        debug!(loop_name = "standard", "Starting prompt execution");

        // Reset doom loop detector for new prompt
        if let Ok(mut detector) = self.doom_detector.lock() {
            detector.reset();
        }

        // Select contextual hints based on user message and inject into the prompt
        let hints = select_hints(user_input, &[]);
        let hints_text = format_hints_for_injection(&hints);
        let augmented_input = if hints_text.is_empty() {
            debug!("No contextual hints matched for user input");
            user_input.to_string()
        } else {
            debug!(
                hints_count = hints.len(),
                "Injecting contextual hints into user message"
            );
            format!("{}\n\n{}", user_input, hints_text)
        };

        // Add user message with optional images
        let user_msg = if ctx.prompt_images.is_empty() {
            ProviderMessage::user(&augmented_input)
        } else {
            // Build multi-part message with images first, then text
            let mut content = Vec::with_capacity(ctx.prompt_images.len() + 1);

            // Add images
            for image in &ctx.prompt_images {
                content.push(ContentPart::image_base64(&image.media_type, &image.data));
            }

            // Add text (with hints)
            content.push(ContentPart::text(&augmented_input));

            ProviderMessage {
                role: wonopcode_provider::Role::User,
                content,
            }
        };
        ctx.messages.push(user_msg);

        // RAG: Search memory for relevant context based on user input
        // Only on first call (not tool result iterations)
        let memory_context = if let Some(ref mem_svc) = ctx.memory_service {
            match mem_svc
                .search(wonop_memory::MemorySearchParams {
                    query: user_input.to_string(),
                    scopes: None, // Search all scopes
                    threshold: Some(0.3), // Reasonable similarity threshold
                    limit: Some(3), // Limit to top 3 relevant memories
                })
                .await
            {
                Ok(result) if !result.entries.is_empty() => {
                    let context_parts: Vec<String> = result
                        .entries
                        .iter()
                        .map(|e| format!("- {}: {}", e.key, e.content))
                        .collect();
                    debug!(
                        memories_found = result.entries.len(),
                        "RAG: Found relevant memories"
                    );
                    Some(format!(
                        "\n\n[Relevant context from memory]\n{}",
                        context_parts.join("\n")
                    ))
                }
                Ok(_) => None, // No relevant memories found
                Err(e) => {
                    debug!(error = %e, "RAG: Failed to search memory");
                    None
                }
            }
        } else {
            None
        };

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
            if let Some(max) = self.max_iterations {
                if iteration > max {
                    return Err(LoopError::MaxIterations(max));
                }
            }

            debug!(iteration, "Starting loop iteration");

            // Build generation options with optional RAG context and OM observations
            // 
            // The prompt structure for optimal cache hits:
            // 1. System prompt (static) - cached
            // 2. Observations (semi-static, changes after Observer runs) - cached  
            // 3. RAG context (changes per query) - not cached
            // 4. Messages (changes every turn) - not cached
            let system_prompt = {
                let mut parts: Vec<String> = Vec::new();
                
                // 1. Base system prompt
                if let Some(base) = &ctx.config.system_prompt {
                    parts.push(base.clone());
                }
                
                // 2. Observational Memory observations (if enabled and have observations)
                if ctx.is_om_ready() {
                    let observations_block = ctx.memory_state.as_ref().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?.format_for_prompt();
                    if !observations_block.is_empty() {
                        let obs_count = ctx.memory_state.as_ref().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?.observations.len();
                        let obs_tokens = ctx.memory_state.as_ref().ok_or_else(|| LoopError::internal("OM memory_state not initialized"))?.observation_tokens;
                        debug!(
                            "OM: Injecting {} observations ({} tokens) into system prompt",
                            obs_count, obs_tokens
                        );
                        parts.push(observations_block);
                    }
                }
                
                // 3. RAG context (only on first iteration)
                if iteration == 1 {
                    if let Some(rag) = &memory_context {
                        parts.push(rag.clone());
                    }
                }
                
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join("\n\n"))
                }
            };

            let options = GenerateOptions {
                temperature: ctx.config.temperature,
                max_tokens: ctx.config.max_tokens,
                system: system_prompt,
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

            // Timing capture for completion statistics
            let completion_id = Uuid::new_v4().to_string();
            let completion_start = Instant::now();
            let completion_timestamp = Utc::now().timestamp_millis() as f64;
            let mut first_token_time: Option<Instant> = None;

            // Capture request for developer statistics
            let request_json = serde_json::json!({
                "messages": ctx.messages.clone(),
                "system": options.system.clone(),
                "tools": options.tools.iter().map(|t| serde_json::json!({
                    "name": &t.name,
                    "description": &t.description,
                    "parameters": &t.parameters,
                })).collect::<Vec<_>>(),
                "temperature": options.temperature,
                "max_tokens": options.max_tokens,
            });

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
                        // Send error to UI so the user sees it
                        let error_msg = format!("Provider error: {}", e);
                        warn!(error = %e, "Stream error");
                        ctx.send_update(LoopUpdate::Error(error_msg.clone()));
                        
                        // For critical errors, stop processing and return
                        // This ensures the user sees the error instead of an empty response
                        if !current_text.is_empty() {
                            ctx.messages.push(ProviderMessage::assistant(&current_text));
                        }
                        return Err(crate::LoopError::from_provider_error(e));
                    }
                };

                match chunk {
                    StreamChunk::TextStart => {}
                    StreamChunk::TextDelta(delta) => {
                        // Track first token time for latency measurement
                        if first_token_time.is_none() {
                            first_token_time = Some(Instant::now());
                        }
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
                        // Only add if not already present (avoid duplicates)
                        if !tool_calls.iter().any(|(tid, _, _)| tid == &id) {
                            tool_calls.push((id, name, arguments));
                        } else {
                            debug!(id = %id, "Skipping duplicate tool call");
                        }
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
                        debug!(
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
                        let step_cost = model_info
                            .cost
                            .calculate(step_usage.input_tokens, step_usage.output_tokens);

                        // Extract accumulated values from provider (if available)
                        let (
                            accumulated_input,
                            accumulated_output,
                            accumulated_cost,
                            last_request_input,
                            last_request_output,
                            last_request_cache_read,
                        ) = if let Some(acc) = &accumulated_usage {
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

                        debug!(
                            step_input = step_usage.input_tokens,
                            step_output = step_usage.output_tokens,
                            step_cost = step_cost,
                            accumulated_input = ?accumulated_input,
                            accumulated_output = ?accumulated_output,
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
                        // Note: CompletionRecorded is emitted after the stream loop completes
                        // to avoid duplicate events from providers that send multiple FinishStep chunks
                    }
                    StreamChunk::Error(e) => {
                        // Send error to UI so the user sees it
                        let error_msg = format!("Stream error: {}", e);
                        warn!(error = %e, "Stream error");
                        ctx.send_update(LoopUpdate::Error(error_msg.clone()));
                        
                        // For critical errors, stop processing and return
                        if !current_text.is_empty() {
                            ctx.messages.push(ProviderMessage::assistant(&current_text));
                        }
                        return Err(crate::LoopError::Internal(error_msg));
                    }
                }
            }

            // Finalize any pending tool calls that didn't get a ToolCall chunk
            for (id, (name, args)) in pending_tool_calls {
                if !tool_calls.iter().any(|(tid, _, _)| tid == &id) {
                    tool_calls.push((id, name, args));
                }
            }

            // Emit CompletionRecorded after the stream loop completes (once per completion)
            // This avoids duplicate events from providers that send multiple FinishStep chunks
            {
                let model_info = ctx.model_info();
                let step_cost = model_info
                    .cost
                    .calculate(step_usage.input_tokens, step_usage.output_tokens);
                let total_duration_ms = completion_start.elapsed().as_millis() as u64;
                let latency_ms = first_token_time
                    .map(|t| t.duration_since(completion_start).as_millis() as u64)
                    .unwrap_or(total_duration_ms);
                let cache_read = step_usage.cache_read_tokens as u64;

                // Capture response for developer statistics
                let response_json = serde_json::json!({
                    "text": &current_text,
                    "tool_calls": tool_calls.iter().map(|(id, name, args)| {
                        serde_json::json!({
                            "id": id,
                            "name": name,
                            "arguments": serde_json::from_str::<serde_json::Value>(args).unwrap_or(serde_json::Value::String(args.clone())),
                        })
                    }).collect::<Vec<_>>(),
                    "finish_reason": format!("{:?}", finish_reason),
                    "usage": {
                        "input_tokens": step_usage.input_tokens,
                        "output_tokens": step_usage.output_tokens,
                        "cache_read_tokens": cache_read,
                    },
                });

                ctx.send_update(LoopUpdate::CompletionRecorded {
                    id: completion_id.clone(),
                    timestamp: completion_timestamp,
                    model: model_info.name.clone(),
                    input_tokens: step_usage.input_tokens as u64,
                    output_tokens: step_usage.output_tokens as u64,
                    cache_read_tokens: cache_read,
                    cost: step_cost,
                    latency_ms,
                    total_duration_ms,
                    finish_reason: format!("{:?}", finish_reason),
                    request: Some(request_json.clone()),
                    response: Some(response_json),
                });
            }

            debug!(
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
            let tool_executor = ToolExecutor::with_services(
                ctx.tools,
                ctx.snapshot_store.cloned(),
                ctx.file_time.clone(),
                ctx.sandbox.clone(),
                ctx.tool_event_tx.clone(),
                ctx.permission_checker.clone(),
                tool_timeout,
                ctx.ticket_service.clone(),
                ctx.memory_service.clone(),
                ctx.hms_service.clone(),
                ctx.ts_permission_checker.clone(),
                ctx.workstream_ticket_id.clone(),
                ctx.workstream_default_tracker_id.clone(),
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

        debug!(
            loop_name = "standard",
            iterations = iteration,
            response_len = final_text.len(),
            "Prompt execution complete"
        );

        // Run Observational Memory compression if enabled
        if ctx.is_om_ready() {
            if let Err(e) = Self::maybe_observe_and_reflect(ctx).await {
                warn!("OM: Error during observation/reflection: {:?}", e);
                // Don't fail the prompt, just log the error
            }
        }

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
        assert_eq!(loop_impl.max_iterations, None);
    }

    #[test]
    fn test_standard_loop_with_max_iterations() {
        let loop_impl = StandardLoop::with_max_iterations(Some(100));
        assert_eq!(loop_impl.max_iterations, Some(100));
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