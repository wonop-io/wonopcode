//! Task tool - spawn subagent sessions.
//!
//! The task tool allows spawning subagents to handle complex tasks.
//! Currently implements a simplified inline subagent that runs with
//! limited tools (read-only for explore agent).

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Arguments for the task tool.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskArgs {
    /// Short description of the task (3-5 words).
    pub description: String,
    /// The task/prompt for the agent to perform.
    pub prompt: String,
    /// The type of subagent to use.
    pub subagent_type: String,
    /// Optional session ID to continue an existing task.
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Result from a subagent execution.
#[derive(Debug, Clone)]
pub struct SubagentResult {
    /// The final response text from the subagent.
    pub response: String,
    /// Whether the task completed successfully.
    pub success: bool,
    /// Optional error message if failed.
    pub error: Option<String>,
}

impl SubagentResult {
    /// Create a successful result.
    pub fn success(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            success: true,
            error: None,
        }
    }

    /// Create a failed result.
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            response: String::new(),
            success: false,
            error: Some(error.into()),
        }
    }
}

/// Callback type for executing subagent tasks.
/// The callback receives the task arguments and context, and should return
/// a future that resolves to the subagent result.
pub type SubagentExecutor = Arc<
    dyn Fn(
            TaskArgs,
            ToolContext,
        ) -> Pin<Box<dyn Future<Output = Result<SubagentResult, String>> + Send>>
        + Send
        + Sync,
>;

/// Spawn a subagent to handle a task.
pub struct TaskTool {
    /// Executor for running subagent tasks.
    executor: RwLock<Option<SubagentExecutor>>,
}

impl TaskTool {
    /// Create a new task tool without an executor (will return not implemented).
    pub fn new() -> Self {
        Self {
            executor: RwLock::new(None),
        }
    }

    /// Create a task tool with a subagent executor.
    pub fn with_executor(executor: SubagentExecutor) -> Self {
        Self {
            executor: RwLock::new(Some(executor)),
        }
    }

    /// Set the subagent executor.
    pub async fn set_executor(&self, executor: SubagentExecutor) {
        let mut ex = self.executor.write().await;
        *ex = Some(executor);
    }

    /// Check if an executor is configured.
    pub async fn has_executor(&self) -> bool {
        self.executor.read().await.is_some()
    }
}

impl Default for TaskTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for TaskTool {
    fn id(&self) -> &str {
        "task"
    }

    fn description(&self) -> &str {
        r#"Launch a new agent to handle complex, multi-step tasks autonomously.

Available agent types and the tools they have access to:
- general: General-purpose agent for researching complex questions and executing multi-step tasks. Use this agent to execute multiple units of work in parallel.
- explore: Fast agent specialized for exploring codebases. Use this when you need to quickly find files by patterns (eg. "src/components/**/*.tsx"), search code for keywords (eg. "API endpoints"), or answer questions about the codebase (eg. "how do API endpoints work?"). When calling this agent, specify the desired thoroughness level: "quick" for basic searches, "medium" for moderate exploration, or "very thorough" for comprehensive analysis across multiple locations and naming conventions.

When using the Task tool, you must specify a subagent_type parameter to select which agent type to use.

When to use the Task tool:
- When you are instructed to execute custom slash commands
- For open-ended searches that may require multiple rounds of globbing and grepping
- For complex multi-step research tasks

When NOT to use the Task tool:
- If you want to read a specific file path, use the Read or Glob tool instead
- If you are searching for a specific class definition like "class Foo", use the Glob tool instead
- If you are searching for code within a specific file or set of 2-3 files, use the Read tool instead

Usage notes:
1. Launch multiple agents concurrently whenever possible to maximize performance
2. When the agent is done, it will return a single message back to you
3. Each agent invocation is stateless unless you provide a session_id
4. The agent's outputs should generally be trusted
5. Clearly tell the agent whether you expect it to write code or just to do research"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["description", "prompt", "subagent_type"],
            "properties": {
                "description": {
                    "type": "string",
                    "description": "A short (3-5 words) description of the task"
                },
                "prompt": {
                    "type": "string",
                    "description": "The task for the agent to perform"
                },
                "subagent_type": {
                    "type": "string",
                    "description": "The type of specialized agent to use for this task",
                    "enum": ["general", "explore"]
                },
                "session_id": {
                    "type": "string",
                    "description": "Existing Task session to continue"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: TaskArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {}", e)))?;

        // Check if we have an executor
        let executor = self.executor.read().await;
        let executor = executor.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "Task tool requires subagent support. Please ensure the runner is configured with subagent execution.",
            )
        })?;

        // Validate agent type
        match args.subagent_type.as_str() {
            "general" | "explore" => {}
            other => {
                return Err(ToolError::validation(format!(
                    "Unknown agent type: '{}'. Available types: general, explore",
                    other
                )));
            }
        }

        // Clone context for the executor
        let ctx_clone = ToolContext {
            session_id: ctx.session_id.clone(),
            message_id: ctx.message_id.clone(),
            agent: args.subagent_type.clone(),
            abort: ctx.abort.clone(),
            root_dir: ctx.root_dir.clone(),
            cwd: ctx.cwd.clone(),
            snapshot: ctx.snapshot.clone(),
            file_time: ctx.file_time.clone(),
            sandbox: ctx.sandbox.clone(),
        };

        // Execute the subagent
        let description = args.description.clone();
        let result = executor(args, ctx_clone)
            .await
            .map_err(ToolError::execution_failed)?;

        if result.success {
            Ok(ToolOutput::new(
                format!("Task completed: {}", description),
                result.response,
            ))
        } else {
            Err(ToolError::execution_failed(
                result.error.unwrap_or_else(|| "Task failed".to_string()),
            ))
        }
    }
}

/// Get the system prompt for a subagent type.
pub fn get_subagent_prompt(agent_type: &str) -> &'static str {
    match agent_type {
        "explore" => EXPLORE_PROMPT,
        "general" => GENERAL_PROMPT,
        _ => GENERAL_PROMPT,
    }
}

/// Get the tool configuration for a subagent type.
/// Returns a list of (tool_name, enabled) pairs.
pub fn get_subagent_tools(agent_type: &str) -> Vec<(&'static str, bool)> {
    match agent_type {
        "explore" => vec![
            ("read", true),
            ("glob", true),
            ("grep", true),
            ("bash", true),
            ("list", true),
            ("edit", false),  // Read-only
            ("write", false), // Read-only
            ("todowrite", false),
            ("todoread", false),
            ("task", false), // No recursive tasks
        ],
        "general" => vec![
            ("read", true),
            ("glob", true),
            ("grep", true),
            ("bash", true),
            ("list", true),
            ("edit", true),
            ("write", true),
            ("todowrite", false),
            ("todoread", false),
            ("task", false), // No recursive tasks
        ],
        _ => vec![],
    }
}

const EXPLORE_PROMPT: &str = r#"You are a file search specialist. You excel at thoroughly navigating and exploring codebases.

Your strengths:
- Rapidly finding files using glob patterns
- Searching code and text with powerful regex patterns
- Reading and analyzing file contents

Guidelines:
- Use glob for broad file pattern matching
- ALWAYS use the grep tool for searching file contents - it uses ripgrep which respects .gitignore and is much faster than bash grep commands
- NEVER use bash grep commands (grep -r, etc.) - always use the grep tool instead, as bash grep will search through ignored directories like target/, node_modules/, .git/ and will be very slow
- Use read when you know the specific file path you need to read
- Use bash only for listing directory contents (ls) or other simple operations, NOT for searching
- Adapt your search approach based on the thoroughness level specified by the caller
- Return file paths as absolute paths in your final response
- For clear communication, avoid using emojis
- Do not create any files, or run bash commands that modify the user's system state in any way

Be thorough but efficient. When you find what you're looking for, provide a clear summary of the results."#;

const GENERAL_PROMPT: &str = r#"You are a capable AI assistant that can help with a variety of tasks.

You have access to tools for reading, writing, and searching files. Use them effectively to complete the task at hand.

Guidelines:
- Be thorough in your research
- Provide clear, actionable results
- If you need to modify files, do so carefully
- Summarize your findings when done

Complete the task and report back with your results."#;
