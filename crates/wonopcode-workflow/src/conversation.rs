//! Conversation engine for LLM interactions

use crate::config::EngineConfig;
use crate::error::{Result, WorkflowError};
use crate::io::{WorkflowEvent, WorkflowIO};
use crate::provider::{CollectedToolCall, ProviderClient};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use wonop_templates::{Artifact, ArtifactExtractor, ArtifactType};
use wonopcode_provider::{ContentPart, Message, Role};
use wonopcode_tools::{ToolContext, ToolRegistry};

/// Result of executing a conversation
pub struct ConversationResult {
    pub artifacts: Vec<Artifact>,
    pub conversation: Vec<Message>,
    pub turns: usize,
    pub elapsed_time: Duration,
    pub total_tokens: Option<usize>,
}

/// Conversation engine for managing LLM interactions
pub struct ConversationEngine {
    messages: Vec<Message>,
    provider: ProviderClient,
    config: EngineConfig,
    tool_registry: Option<Arc<ToolRegistry>>,
    max_turns: usize,
    start_time: Instant,
    total_tokens: usize,
}

impl ConversationEngine {
    pub fn new(
        provider: ProviderClient,
        config: EngineConfig,
        max_turns: usize,
        tool_registry: Option<Arc<ToolRegistry>>,
    ) -> Self {
        Self {
            messages: Vec::new(),
            provider,
            config,
            tool_registry,
            max_turns,
            start_time: Instant::now(),
            total_tokens: 0,
        }
    }

    pub async fn execute<IO: WorkflowIO>(
        &mut self,
        initial_prompt: String,
        output_dir: &Path,
        stop_on_artifact: &[String],
        system_message: Option<String>,
        io: &IO,
    ) -> Result<ConversationResult> {
        self.start_time = Instant::now();
        log::debug!("Starting conversation execution");

        let options = self.provider.build_options(&self.config, system_message);
        self.messages.push(Message::user(initial_prompt));

        let mut turn_count = 0;

        loop {
            turn_count += 1;

            io.emit(WorkflowEvent::ConversationTurnStarted {
                turn: turn_count,
                max_turns: self.max_turns,
            })
            .await;

            if io.is_cancelled() {
                return Err(WorkflowError::Cancelled);
            }

            if turn_count > self.max_turns {
                io.emit(WorkflowEvent::Warning {
                    message: format!("Max conversation turns ({}) reached", self.max_turns),
                })
                .await;
                break;
            }

            io.emit(WorkflowEvent::Progress {
                message: format!(
                    "Sending request to {} ({})...",
                    self.provider.provider_name(),
                    self.provider.model_name()
                ),
            })
            .await;

            let response = self
                .provider
                .send_message_with_io(self.messages.clone(), options.clone(), Some(io))
                .await?;

            if let Some((input, output)) = response.usage {
                self.total_tokens += (input + output) as usize;
            }

            // Handle tool calls
            if !response.tool_calls.is_empty() {
                let tool_results =
                    self.handle_tool_calls(&response.tool_calls, output_dir, io).await?;

                let mut assistant_content = vec![];
                if !response.content.is_empty() {
                    assistant_content.push(ContentPart::Text {
                        text: response.content.clone(),
                    });
                }
                for call in &response.tool_calls {
                    assistant_content.push(ContentPart::ToolUse {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        input: serde_json::from_str(&call.arguments).unwrap_or_default(),
                    });
                }
                self.messages.push(Message {
                    role: Role::Assistant,
                    content: assistant_content,
                });

                for (call, result) in response.tool_calls.iter().zip(tool_results.iter()) {
                    self.messages.push(Message {
                        role: Role::User,
                        content: vec![ContentPart::ToolResult {
                            tool_use_id: call.id.clone(),
                            content: result.clone(),
                            is_error: None,
                        }],
                    });
                }

                continue;
            }

            // Process response content
            let content = &response.content;
            self.messages.push(Message::assistant(content.clone()));

            // Extract artifacts
            let artifacts = ArtifactExtractor::extract(content)
                .unwrap_or_else(|e| {
                    log::warn!("Failed to extract artifacts: {}", e);
                    Vec::new()
                });

            // Check for stop conditions
            let should_stop = artifacts.iter().any(|artifact| {
                let type_str = artifact.artifact_type.as_str();
                stop_on_artifact.contains(&type_str.to_string())
            });

            for artifact in &artifacts {
                io.emit(WorkflowEvent::ArtifactCreated {
                    artifact: artifact.clone(),
                })
                .await;
            }

            if should_stop || !artifacts.is_empty() {
                // Save artifacts
                for artifact in &artifacts {
                    if let Err(e) = self.save_artifact(artifact, output_dir) {
                        log::warn!("Failed to save artifact: {}", e);
                    }
                }

                io.emit(WorkflowEvent::ConversationCompleted {
                    turns: turn_count,
                    total_tokens: self.total_tokens,
                    duration: self.start_time.elapsed(),
                })
                .await;

                return Ok(ConversationResult {
                    artifacts,
                    conversation: self.messages.clone(),
                    turns: turn_count,
                    elapsed_time: self.start_time.elapsed(),
                    total_tokens: Some(self.total_tokens),
                });
            }

            break;
        }

        io.emit(WorkflowEvent::ConversationCompleted {
            turns: turn_count,
            total_tokens: self.total_tokens,
            duration: self.start_time.elapsed(),
        })
        .await;

        Ok(ConversationResult {
            artifacts: Vec::new(),
            conversation: self.messages.clone(),
            turns: turn_count,
            elapsed_time: self.start_time.elapsed(),
            total_tokens: Some(self.total_tokens),
        })
    }

    async fn handle_tool_calls<IO: WorkflowIO>(
        &self,
        tool_calls: &[CollectedToolCall],
        output_dir: &Path,
        io: &IO,
    ) -> Result<Vec<String>> {
        let registry = self.tool_registry.as_ref().ok_or_else(|| {
            WorkflowError::ToolExecution("Tool registry not configured".to_string())
        })?;

        let mut results = Vec::new();

        for call in tool_calls {
            io.emit(WorkflowEvent::ToolCallStarted {
                tool_name: call.name.clone(),
                tool_id: call.id.clone(),
            })
            .await;

            let tool = registry.get(&call.name).ok_or_else(|| {
                WorkflowError::ToolExecution(format!("Tool not found: {}", call.name))
            })?;

            let args: serde_json::Value =
                serde_json::from_str(&call.arguments).unwrap_or_default();

            let ctx = ToolContext {
                session_id: "workflow-session".to_string(),
                message_id: call.id.clone(),
                agent: "workflow".to_string(),
                abort: CancellationToken::new(),
                root_dir: output_dir.to_path_buf(),
                cwd: output_dir.to_path_buf(),
                snapshot: None,
                file_time: None,
                sandbox: None,
                event_tx: None,
                ticket_service: None,
                memory_service: None,
                ace_service: None,
                hms_service: None,
                permission_checker: None,
                workstream_ticket_id: None,
                workstream_default_tracker_id: None,
            };

            match tool.execute(args, &ctx).await {
                Ok(output) => {
                    let result = format_tool_output(&output);
                    io.emit(WorkflowEvent::ToolCallCompleted {
                        tool_name: call.name.clone(),
                        tool_id: call.id.clone(),
                        success: true,
                        output: Some(result.clone()),
                    })
                    .await;
                    results.push(result);
                }
                Err(e) => {
                    let error_msg = format!("Tool error: {}", e);
                    io.emit(WorkflowEvent::ToolCallCompleted {
                        tool_name: call.name.clone(),
                        tool_id: call.id.clone(),
                        success: false,
                        output: Some(error_msg.clone()),
                    })
                    .await;
                    results.push(error_msg);
                }
            }
        }

        Ok(results)
    }

    fn save_artifact(&self, artifact: &Artifact, output_dir: &Path) -> std::io::Result<()> {
        if artifact.artifact_type == ArtifactType::File {
            if let Some(ref content) = artifact.content {
                let path = output_dir.join(&artifact.name);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, content)?;
                log::info!("Saved artifact: {}", path.display());
            }
        }
        Ok(())
    }
}

fn format_tool_output(output: &wonopcode_tools::ToolOutput) -> String {
    let mut result = String::new();
    if !output.title.is_empty() {
        result.push_str(&output.title);
        result.push_str("\n\n");
    }
    result.push_str(&output.output);
    result
}

