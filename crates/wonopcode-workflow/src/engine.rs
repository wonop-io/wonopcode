//! Workflow execution engine

use crate::config::{EngineConfig, WorkflowConfig};
use crate::conversation::ConversationEngine;
use crate::error::{Result, WorkflowError};
use crate::io::{WorkflowEvent, WorkflowIO};
use crate::provider::ProviderClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wonop_templates::types::{Artifact, ArtifactMapping, Template, WorkflowStep};
use wonop_templates::TemplateManager;
use wonopcode_tools::ToolRegistry;

/// Result of executing a workflow
pub struct WorkflowResult {
    pub artifacts: Vec<Artifact>,
    pub context: serde_json::Value,
    pub total_time: Duration,
    pub step_count: usize,
    pub step_timings: Vec<StepTiming>,
    pub metrics: WorkflowMetrics,
}

/// Timing information for a single step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepTiming {
    pub step_name: String,
    pub step_type: String,
    pub duration: Duration,
    pub skipped: bool,
}

/// Detailed metrics for workflow execution
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowMetrics {
    pub steps_executed: usize,
    pub steps_skipped: usize,
    pub workflow_steps: usize,
    pub command_steps: usize,
    pub total_artifacts: usize,
    pub max_depth_reached: usize,
    pub template_render_time: Duration,
    pub command_execution_time: Duration,
}

/// Output from a command execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub success: bool,
    pub elapsed_ms: u64,
}

/// Workflow execution engine
pub struct WorkflowEngine {
    template_manager: TemplateManager,
    provider: ProviderClient,
    config: EngineConfig,
    tool_registry: Option<Arc<ToolRegistry>>,
    tera: tera::Tera,
}

impl WorkflowEngine {
    pub fn new(
        template_manager: TemplateManager,
        provider: ProviderClient,
        config: EngineConfig,
    ) -> Self {
        Self {
            template_manager,
            provider,
            config,
            tool_registry: None,
            tera: tera::Tera::default(),
        }
    }

    pub fn with_tools(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    fn workflow_config(&self) -> &WorkflowConfig {
        &self.config.workflow
    }

    pub async fn execute<IO: WorkflowIO>(
        &mut self,
        template: &Template,
        user_inputs: HashMap<String, serde_json::Value>,
        output_dir: &Path,
        io: &IO,
    ) -> Result<WorkflowResult> {
        self.execute_at_depth(template, user_inputs, output_dir, 0, io)
            .await
    }

    async fn execute_at_depth<IO: WorkflowIO>(
        &mut self,
        template: &Template,
        user_inputs: HashMap<String, serde_json::Value>,
        output_dir: &Path,
        current_depth: usize,
        io: &IO,
    ) -> Result<WorkflowResult> {
        let start_time = Instant::now();

        let step_count = template.steps.as_ref().map(|s| s.len()).unwrap_or(0);
        io.emit(WorkflowEvent::WorkflowStarted {
            template_name: template.title.clone(),
            total_steps: step_count,
        })
        .await;

        if current_depth > self.workflow_config().max_depth {
            return Err(WorkflowError::DepthExceeded(self.workflow_config().max_depth));
        }

        let mut context = tera::Context::new();
        for (key, value) in &user_inputs {
            context.insert(key, value);
        }

        let Some(steps) = &template.steps else {
            return self.execute_simple(template, user_inputs, output_dir, io).await;
        };

        if steps.len() > self.workflow_config().max_steps {
            return Err(WorkflowError::TooManySteps {
                count: steps.len(),
                max: self.workflow_config().max_steps,
            });
        }

        let mut steps_context: HashMap<String, serde_json::Value> = HashMap::new();
        let mut all_artifacts = Vec::new();
        let mut step_timings = Vec::new();
        let mut metrics = WorkflowMetrics::default();
        metrics.max_depth_reached = current_depth;

        for (idx, step) in steps.iter().enumerate() {
            if io.is_cancelled() {
                return Err(WorkflowError::Cancelled);
            }

            context.insert("steps", &steps_context);

            io.emit(WorkflowEvent::StepStarted {
                step_index: idx,
                step_name: step.name().to_string(),
                step_type: step.type_name().to_string(),
                description: step.description().map(|s| s.to_string()),
            })
            .await;

            let step_start = Instant::now();
            let mut skipped = false;

            match step {
                WorkflowStep::Workflow {
                    name,
                    template: tmpl,
                    inputs,
                    output_mapping,
                    condition,
                    ..
                } => {
                    if !self.should_execute(condition, &context)? {
                        skipped = true;
                        metrics.steps_skipped += 1;
                        io.emit(WorkflowEvent::StepSkipped {
                            step_index: idx,
                            step_name: name.clone(),
                            reason: "Condition not met".to_string(),
                        })
                        .await;
                    } else {
                        let render_start = Instant::now();
                        let step_inputs = self.render_inputs(inputs, &context)?;
                        metrics.template_render_time += render_start.elapsed();

                        let result = self
                            .execute_subworkflow(tmpl, step_inputs, output_dir, current_depth + 1, io)
                            .await?;

                        all_artifacts.extend(result.artifacts.clone());

                        metrics.max_depth_reached =
                            metrics.max_depth_reached.max(result.metrics.max_depth_reached);
                        metrics.template_render_time += result.metrics.template_render_time;
                        metrics.command_execution_time += result.metrics.command_execution_time;

                        let captured = self.capture_artifacts(&result.artifacts, output_mapping, name)?;

                        context.insert(name, &captured);

                        let step_result = serde_json::json!({ "artifacts": captured });
                        steps_context.insert(name.clone(), step_result);

                        metrics.steps_executed += 1;
                        metrics.workflow_steps += 1;
                    }
                }

                WorkflowStep::Command {
                    name,
                    command,
                    output_as,
                    condition,
                    ..
                } => {
                    if !self.should_execute(condition, &context)? {
                        skipped = true;
                        metrics.steps_skipped += 1;
                        io.emit(WorkflowEvent::StepSkipped {
                            step_index: idx,
                            step_name: name.clone(),
                            reason: "Condition not met".to_string(),
                        })
                        .await;
                    } else {
                        let render_start = Instant::now();
                        let rendered_cmd = self.render_template_string(command, &context)?;
                        metrics.template_render_time += render_start.elapsed();

                        let cmd_start = Instant::now();
                        let output = self.execute_command(&rendered_cmd).await?;
                        metrics.command_execution_time += cmd_start.elapsed();

                        context.insert(output_as, &output);

                        let step_result = serde_json::json!({
                            "output": output.stdout,
                            "stdout": output.stdout,
                            "stderr": output.stderr,
                            "exit_code": output.exit_code,
                            "success": output.success,
                        });
                        steps_context.insert(name.clone(), step_result);

                        metrics.steps_executed += 1;
                        metrics.command_steps += 1;
                    }
                }
            }

            let step_duration = step_start.elapsed();

            step_timings.push(StepTiming {
                step_name: step.name().to_string(),
                step_type: step.type_name().to_string(),
                duration: step_duration,
                skipped,
            });

            if !skipped {
                io.emit(WorkflowEvent::StepCompleted {
                    step_index: idx,
                    step_name: step.name().to_string(),
                    duration_ms: step_duration.as_millis() as u64,
                    artifacts: vec![],
                })
                .await;
            }
        }

        context.insert("steps", &steps_context);

        let render_start = Instant::now();
        let rendered = self.render_template_string(&template.template_body, &context)?;
        metrics.template_render_time += render_start.elapsed();

        io.emit(WorkflowEvent::Progress {
            message: "Executing LLM call for final template...".to_string(),
        })
        .await;

        let mut conversation = ConversationEngine::new(
            self.provider.clone(),
            self.config.clone(),
            self.config.max_conversation_turns,
            self.tool_registry.clone(),
        );

        let stop_on_artifact = template
            .stop_on_artifact
            .clone()
            .unwrap_or_else(|| vec!["file".to_string()]);

        let llm_result = conversation
            .execute(
                rendered,
                output_dir,
                &stop_on_artifact,
                template.system_message.clone(),
                io,
            )
            .await?;

        all_artifacts.extend(llm_result.artifacts);
        metrics.total_artifacts = all_artifacts.len();

        let total_time = start_time.elapsed();

        io.emit(WorkflowEvent::WorkflowCompleted {
            template_name: template.title.clone(),
            total_duration_ms: total_time.as_millis() as u64,
            artifacts_count: all_artifacts.len(),
        })
        .await;

        Ok(WorkflowResult {
            artifacts: all_artifacts,
            context: context.into_json(),
            total_time,
            step_count: steps.len(),
            step_timings,
            metrics,
        })
    }

    async fn execute_simple<IO: WorkflowIO>(
        &mut self,
        template: &Template,
        inputs: HashMap<String, serde_json::Value>,
        output_dir: &Path,
        io: &IO,
    ) -> Result<WorkflowResult> {
        let start_time = Instant::now();

        let mut context = tera::Context::new();
        for (key, value) in &inputs {
            context.insert(key, value);
        }

        let rendered = self.render_template_string(&template.template_body, &context)?;

        let mut conversation = ConversationEngine::new(
            self.provider.clone(),
            self.config.clone(),
            self.config.max_conversation_turns,
            self.tool_registry.clone(),
        );

        let stop_on_artifact = template
            .stop_on_artifact
            .clone()
            .unwrap_or_else(|| vec!["file".to_string()]);

        let result = conversation
            .execute(
                rendered,
                output_dir,
                &stop_on_artifact,
                template.system_message.clone(),
                io,
            )
            .await?;

        let artifact_count = result.artifacts.len();

        Ok(WorkflowResult {
            artifacts: result.artifacts,
            context: context.into_json(),
            total_time: start_time.elapsed(),
            step_count: 0,
            step_timings: Vec::new(),
            metrics: WorkflowMetrics {
                total_artifacts: artifact_count,
                ..Default::default()
            },
        })
    }

    fn execute_subworkflow<'a, IO: WorkflowIO + 'a>(
        &'a mut self,
        template_name: &'a str,
        inputs: HashMap<String, serde_json::Value>,
        output_dir: &'a Path,
        depth: usize,
        io: &'a IO,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<WorkflowResult>> + 'a>> {
        Box::pin(async move {
            let template = self
                .template_manager
                .load_template(template_name)
                .map_err(|_| WorkflowError::TemplateNotFound(template_name.to_string()))?;

            self.execute_at_depth(&template, inputs, output_dir, depth, io)
                .await
        })
    }

    fn should_execute(
        &mut self,
        condition: &Option<String>,
        context: &tera::Context,
    ) -> Result<bool> {
        let Some(cond_str) = condition else {
            return Ok(true);
        };

        let rendered = self.render_template_string(cond_str, context)?;

        match rendered.trim().to_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" | "" => Ok(false),
            other => Err(WorkflowError::ConditionEval(format!(
                "Invalid condition value: {}",
                other
            ))),
        }
    }

    fn render_inputs(
        &mut self,
        inputs: &HashMap<String, String>,
        context: &tera::Context,
    ) -> Result<HashMap<String, serde_json::Value>> {
        let mut rendered = HashMap::new();
        for (key, template_str) in inputs {
            let value = self.render_template_string(template_str, context)?;
            rendered.insert(key.clone(), serde_json::Value::String(value));
        }
        Ok(rendered)
    }

    fn render_template_string(
        &mut self,
        template_str: &str,
        context: &tera::Context,
    ) -> Result<String> {
        if !template_str.contains("{{") && !template_str.contains("{%") {
            return Ok(template_str.to_string());
        }

        self.tera
            .render_str(template_str, context)
            .map_err(|e| WorkflowError::Template(e.to_string()))
    }

    async fn execute_command(&self, command: &str) -> Result<CommandOutput> {
        use tokio::process::Command;

        let start_time = Instant::now();
        let timeout_duration = self.workflow_config().step_timeout;

        let result = tokio::time::timeout(timeout_duration, async {
            Command::new("sh").arg("-c").arg(command).output().await
        })
        .await;

        let elapsed_ms = start_time.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(output)) => Ok(CommandOutput {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code().unwrap_or(-1),
                success: output.status.success(),
                elapsed_ms,
            }),
            Ok(Err(e)) => Err(WorkflowError::CommandExecution(e.to_string())),
            Err(_) => Err(WorkflowError::StepTimeout {
                step_name: "command".to_string(),
                timeout_secs: timeout_duration.as_secs(),
            }),
        }
    }

    fn capture_artifacts(
        &self,
        artifacts: &[Artifact],
        mappings: &[ArtifactMapping],
        step_name: &str,
    ) -> Result<HashMap<String, serde_json::Value>> {
        let mut captured = HashMap::new();

        for mapping in mappings {
            let matched = artifacts.iter().find(|a| {
                if let Some(ref id) = mapping.artifact_id {
                    if a.id.as_ref() == Some(id) {
                        return true;
                    }
                }
                if let Some(ref tag) = mapping.tag {
                    if a.tag.as_ref() == Some(tag) {
                        return true;
                    }
                }
                if let Some(ref atype) = mapping.artifact_type {
                    if a.artifact_type.as_str() == atype {
                        return true;
                    }
                }
                if let Some(ref pattern) = mapping.artifact {
                    if a.name == *pattern || glob_match(pattern, &a.name) {
                        return true;
                    }
                }
                false
            });

            if let Some(artifact) = matched {
                captured.insert(
                    mapping.as_field.clone(),
                    serde_json::json!({
                        "name": artifact.name,
                        "content": artifact.content,
                        "type": artifact.artifact_type.as_str(),
                    }),
                );
            } else if mapping.required {
                return Err(WorkflowError::OutputMapping(format!(
                    "Required artifact not found for mapping '{}' in step '{}'",
                    mapping.as_field, step_name
                )));
            }
        }

        Ok(captured)
    }
}

fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.starts_with('*') && pattern.ends_with('*') {
        let middle = &pattern[1..pattern.len() - 1];
        return text.contains(middle);
    }
    if pattern.starts_with('*') {
        return text.ends_with(&pattern[1..]);
    }
    if pattern.ends_with('*') {
        return text.starts_with(&pattern[..pattern.len() - 1]);
    }
    pattern == text
}

