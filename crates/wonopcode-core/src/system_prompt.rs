//! System prompt generation for wonopcode.
//!
//! This module provides a Tera template-based system prompt renderer that generates
//! system prompts with dynamic variables. The system prompt is re-rendered before
//! every LLM invocation to ensure fresh values for date, time, branch, AGENTS.md, etc.
//!
//! # Updating the System Prompt
//!
//! The system prompt is rendered from a Tera template. To modify it:
//!
//! 1. **Edit the default template**: Modify `DEFAULT_SYSTEM_PROMPT_TEMPLATE` in this file.
//!    The template uses Tera syntax (similar to Jinja2). Available variables:
//!    - `{{ agent_md }}` - Rendered AGENTS.md content (from HMS or disk)
//!    - `{{ date }}` - Current date (e.g., "Wed Mar 11 2026")
//!    - `{{ time }}` - Current time (e.g., "14:30:05")
//!    - `{{ branch }}` - Current git branch name
//!    - `{{ working_dir }}` - Absolute path to the working directory
//!    - `{{ model_name }}` - Name/ID of the current LLM model
//!    - `{{ provider }}` - Provider identifier (e.g., "anthropic", "openai")
//!    - `{{ platform }}` - OS platform (e.g., "macos", "linux")
//!    - `{{ is_git_repo }}` - Whether cwd is a git repository (boolean)
//!
//! 2. **Customize per-project**: Create a `.wonopcode/AGENTS.TEMPLATE.md` file to
//!    customize the agent instructions via the HMS (Hierarchical Memory System).
//!    The rendered content becomes the `{{ agent_md }}` variable.
//!
//! 3. **Legacy instruction files**: If no HMS template exists, the system falls back
//!    to reading raw instruction files: `AGENTS.md`, `CLAUDE.md`, etc.
//!
//! # Architecture
//!
//! The `SystemPromptRenderer` uses Tera templates to produce the system prompt.
//! It is called from the agent loop (`StandardLoop`) right before each LLM
//! `generate()` call, ensuring:
//! - Fresh date/time values
//! - Up-to-date AGENTS.md (re-rendered from HMS if available)
//! - Correct git branch after checkout operations
//!
//! This is a unified approach: all API-based providers (Anthropic, OpenAI, etc.)
//! use the same template. The Claude CLI provider is excluded since it manages
//! its own system prompt.

use std::path::Path;
use tera::{Context, Tera};

/// Default Tera template for the system prompt.
///
/// This template is used by all API-based providers. Edit this template to change
/// the system prompt across all providers uniformly.
///
/// ## Template Variables
///
/// | Variable | Description | Example |
/// |----------|-------------|---------|
/// | `agent_md` | Rendered AGENTS.md / custom instructions | Project-specific rules |
/// | `date` | Current date | "Wed Mar 11 2026" |
/// | `time` | Current time | "14:30:05" |
/// | `branch` | Git branch name | "main" |
/// | `working_dir` | Absolute working directory path | "/home/user/project" |
/// | `model_name` | LLM model identifier | "claude-sonnet-4-20250514" |
/// | `provider` | Provider identifier | "anthropic" |
/// | `platform` | OS platform | "macos" |
/// | `is_git_repo` | Whether cwd is in a git repo | true |
pub const DEFAULT_SYSTEM_PROMPT_TEMPLATE: &str = r#"You are Wonopcode, a powerful AI coding assistant.

You are an interactive agent that helps users with software engineering tasks. Use the instructions below and the tools available to you to assist the user.

IMPORTANT: You must NEVER generate or guess URLs for the user unless you are confident that the URLs are for helping the user with programming. You may use URLs provided by the user in their messages or local files.

If the user asks for help or wants to give feedback inform them of the following:
- ctrl+p to list available actions
- To give feedback, users should report the issue at
  https://github.com/wonop-io/wonopcode

When the user directly asks about Wonopcode (eg. "can Wonopcode do...", "does Wonopcode have..."), or asks in second person (eg. "are you able...", "can you do..."), or asks how to use a specific Wonopcode feature (eg. implement a hook, write a slash command, or install an MCP server), refer to the project documentation at https://github.com/wonop-io/wonopcode

# Tone and style
- Only use emojis if the user explicitly requests it. Avoid using emojis in all communication unless asked.
- Your output will be displayed on a command line interface. Your responses should be short and concise. You can use Github-flavored markdown for formatting, and will be rendered in a monospace font using the CommonMark specification.
- Output text to communicate with the user; all text you output outside of tool use is displayed to the user. Only use tools to complete tasks. Never use tools like Bash or code comments as means to communicate with the user during the session.
- NEVER create files unless they're absolutely necessary for achieving your goal. ALWAYS prefer editing an existing file to creating a new one. This includes markdown files.

# Professional objectivity
Prioritize technical accuracy and truthfulness over validating the user's beliefs. Focus on facts and problem-solving, providing direct, objective technical info without any unnecessary superlatives, praise, or emotional validation. It is best for the user if Wonopcode honestly applies the same rigorous standards to all ideas and disagrees when necessary, even if it may not be what the user wants to hear. Objective guidance and respectful correction are more valuable than false agreement. Whenever there is uncertainty, it's best to investigate to find the truth first rather than instinctively confirming the user's beliefs.

# Task Management
You have access to the TodoWrite tools to help you manage and plan tasks. Use these tools VERY frequently to ensure that you are tracking your tasks and giving the user visibility into your progress.
These tools are also EXTREMELY helpful for planning tasks, and for breaking down larger complex tasks into smaller steps. If you do not use this tool when planning, you may forget to do important tasks - and that is unacceptable.

It is critical that you mark todos as completed as soon as you are done with a task. Do not batch up multiple tasks before marking them as completed.

# Doing tasks
The user will primarily request you perform software engineering tasks. This includes solving bugs, adding new functionality, refactoring code, explaining code, and more. For these tasks the following steps are recommended:
- Use the TodoWrite tool to plan the task if required

- Tool results and user messages may include <system-reminder> tags. <system-reminder> tags contain useful information and reminders. They are automatically added by the system, and bear no direct relation to the specific tool results or user messages in which they appear.

# Tool usage policy
- When doing file search, prefer to use the Task tool in order to reduce context usage.
- You should proactively use the Task tool with specialized agents when the task at hand matches the agent's description.
- When WebFetch returns a message about a redirect to a different host, you should immediately make a new WebFetch request with the redirect URL provided in the response.
- You can call multiple tools in a single response. If you intend to call multiple tools and there are no dependencies between them, make all independent tool calls in parallel. Maximize use of parallel tool calls where possible to increase efficiency.
- Use specialized tools instead of bash commands when possible, as this provides a better user experience. For file operations, use dedicated tools: Read for reading files instead of cat/head/tail, Edit for editing instead of sed/awk, and Write for creating files instead of cat with heredoc or echo redirection. Reserve bash tools exclusively for actual system commands and terminal operations that require shell execution.
- VERY IMPORTANT: When exploring the codebase to gather context or to answer a question that is not a needle query for a specific file/class/function, it is CRITICAL that you use the Task tool instead of running search commands directly.

IMPORTANT: Always use the TodoWrite tool to plan and track tasks throughout the conversation.

IMPORTANT: Always plan your work using the planning module in TypeScript. Sessions are auto-created when you create use-cases or tasks. Call help("planning.feature") or help("planning.adHoc") to learn about available planning tools.

# Code References

When referencing specific functions or pieces of code include the pattern `file_path:line_number` to allow the user to easily navigate to the source code location.

{% if agent_md %}
# Project Instructions

{{ agent_md }}
{% endif %}

# Environment

<env>
Working directory: {{ working_dir }}
Is directory a git repo: {{ is_git_repo }}
Platform: {{ platform }}
Today's date: {{ date }}
Current time: {{ time }}
{% if branch %}Current branch: {{ branch }}{% endif %}
Model: {{ model_name }}
Provider: {{ provider }}
</env>
"#;

/// Explore agent prompt.
pub const EXPLORE_PROMPT: &str = r#"You are a file search specialist. You excel at thoroughly navigating and exploring codebases.

Your strengths:
- Rapidly finding files using glob patterns
- Searching code and text with powerful regex patterns
- Reading and analyzing file contents

Guidelines:
- Use Glob for broad file pattern matching
- Use Grep for searching file contents with regex
- Use Read when you know the specific file path you need to read
- Use Bash for file operations like copying, moving, or listing directory contents
- Adapt your search approach based on the thoroughness level specified by the caller
- Return file paths as absolute paths in your final response
- For clear communication, avoid using emojis
- Do not create any files, or run bash commands that modify the user's system state in any way

Complete the user's search request efficiently and report your findings clearly."#;

/// Compaction agent prompt.
pub const COMPACTION_PROMPT: &str = r#"You are a helpful AI assistant tasked with summarizing conversations.

When asked to summarize, provide a detailed but concise summary of the conversation.
Focus on information that would be helpful for continuing the conversation, including:
- What was done
- What is currently being worked on
- Which files are being modified
- What needs to be done next
- Key user requests, constraints, or preferences that should persist
- Important technical decisions and why they were made

Your summary should be comprehensive enough to provide context but concise enough to be quickly understood."#;

/// Title generation prompt.
pub const TITLE_PROMPT: &str = r#"You are a title generator. Generate a short, descriptive title for the conversation.

Guidelines:
- Keep it under 50 characters
- Be specific about the task or topic
- Use action verbs when appropriate
- No quotes or special formatting

Output only the title, nothing else."#;

/// Summary generation prompt.
pub const SUMMARY_PROMPT: &str = r#"You are a session summarizer. Create a brief summary of what was accomplished in this session.

Guidelines:
- List key accomplishments
- Note any remaining tasks
- Be concise (2-3 sentences max)

Output only the summary, no preamble."#;

/// Plan mode reminder prompt.
pub const PLAN_REMINDER: &str = r#"<system-reminder>
You are currently in PLAN MODE. In this mode:
- You can READ files but NOT edit or create them
- You can run READ-ONLY bash commands (ls, cat, grep, git log, etc.)
- Focus on planning, researching, and understanding the codebase
- Create a detailed plan using the TodoWrite tool

When the user is ready to implement, they will switch you to BUILD mode.
</system-reminder>"#;

/// Build switch notification.
pub const BUILD_SWITCH: &str = r#"<system-reminder>
Mode switched from PLAN to BUILD. You now have full access to edit files and run commands.
Continue implementing the plan that was created.
</system-reminder>"#;

/// Max steps warning.
pub const MAX_STEPS_WARNING: &str = r#"<system-reminder>
You have reached the maximum number of steps for this turn. Please summarize your progress
and let the user know what remains to be done. The user can continue in a new message.
</system-reminder>"#;

/// Variables for rendering the system prompt template.
#[derive(Debug, Clone)]
pub struct SystemPromptVars {
    /// Rendered AGENTS.md / custom instructions content.
    pub agent_md: Option<String>,
    /// Current date string (e.g., "Wed Mar 11 2026").
    pub date: String,
    /// Current time string (e.g., "14:30:05").
    pub time: String,
    /// Current git branch name.
    pub branch: Option<String>,
    /// Absolute path to the working directory.
    pub working_dir: String,
    /// LLM model identifier.
    pub model_name: String,
    /// Provider identifier.
    pub provider: String,
    /// OS platform.
    pub platform: String,
    /// Whether cwd is a git repo.
    pub is_git_repo: bool,
}

impl SystemPromptVars {
    /// Build variables from the current environment.
    ///
    /// This gathers date, time, platform, git branch, etc. from the live environment.
    /// The `agent_md` field must be set separately (from HMS or disk).
    pub fn from_env(cwd: &Path, model_name: &str, provider: &str) -> Self {
        let now = chrono::Local::now();
        Self {
            agent_md: None,
            date: now.format("%a %b %d %Y").to_string(),
            time: now.format("%H:%M:%S").to_string(),
            branch: get_git_branch(cwd),
            working_dir: cwd.display().to_string(),
            model_name: model_name.to_string(),
            provider: provider.to_string(),
            platform: std::env::consts::OS.to_string(),
            is_git_repo: cwd.join(".git").exists() || has_git_dir_ancestor(cwd),
        }
    }
}

/// Renders the system prompt from a Tera template and dynamic variables.
///
/// This is the main entry point for system prompt generation. It is called
/// before every LLM invocation in the agent loop.
pub struct SystemPromptRenderer {
    tera: Tera,
}

impl SystemPromptRenderer {
    /// Create a new renderer with the default template.
    pub fn new() -> Result<Self, String> {
        let mut tera = Tera::default();
        tera.add_raw_template("system_prompt", DEFAULT_SYSTEM_PROMPT_TEMPLATE)
            .map_err(|e| format!("Failed to parse default system prompt template: {}", e))?;
        Ok(Self { tera })
    }

    /// Create a renderer with a custom template string.
    ///
    /// Use this to override the default system prompt template entirely.
    pub fn with_template(template: &str) -> Result<Self, String> {
        let mut tera = Tera::default();
        tera.add_raw_template("system_prompt", template)
            .map_err(|e| format!("Invalid system prompt template: {}", e))?;
        Ok(Self { tera })
    }

    /// Render the system prompt with the given variables.
    ///
    /// This should be called right before each LLM invocation to get
    /// fresh date/time/branch values.
    pub fn render(&self, vars: &SystemPromptVars) -> Result<String, String> {
        let mut context = Context::new();

        // Insert all variables into the Tera context
        context.insert("agent_md", &vars.agent_md.as_deref().unwrap_or(""));
        context.insert("date", &vars.date);
        context.insert("time", &vars.time);
        context.insert("branch", &vars.branch.as_deref().unwrap_or(""));
        context.insert("working_dir", &vars.working_dir);
        context.insert("model_name", &vars.model_name);
        context.insert("provider", &vars.provider);
        context.insert("platform", &vars.platform);
        context.insert("is_git_repo", &if vars.is_git_repo { "yes" } else { "no" });

        self.tera
            .render("system_prompt", &context)
            .map_err(|e| format!("Failed to render system prompt: {}", e))
    }
}

/// Load custom instructions from AGENTS.md, CLAUDE.md, etc.
///
/// This is the fallback when HMS (Hierarchical Memory System) is not available.
/// It reads raw instruction files from the working directory.
pub fn load_custom_instructions(cwd: &Path) -> Option<String> {
    let instruction_files = [
        ".wonopcode/AGENTS.md",
        "AGENTS.md",
        ".claude/CLAUDE.md",
        "CLAUDE.md",
        ".wonopcode/instructions.md",
        ".cursor/rules",
    ];

    let mut instructions = Vec::new();

    for file in &instruction_files {
        let path = cwd.join(file);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if !content.trim().is_empty() {
                    instructions.push(content.trim().to_string());
                }
            }
        }
    }

    if instructions.is_empty() {
        None
    } else {
        Some(instructions.join("\n\n"))
    }
}

/// Get the current git branch name for the given directory.
fn get_git_branch(cwd: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd)
        .output()
        .ok()?;

    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !branch.is_empty() && branch != "HEAD" {
            Some(branch)
        } else {
            None
        }
    } else {
        None
    }
}

/// Check if any ancestor directory has a .git directory.
fn has_git_dir_ancestor(path: &Path) -> bool {
    let mut current = path.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return true;
        }
        if !current.pop() {
            return false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_default_template() {
        let renderer = SystemPromptRenderer::new().unwrap();
        let vars = SystemPromptVars {
            agent_md: Some("Always use rust.".to_string()),
            date: "Wed Mar 11 2026".to_string(),
            time: "14:30:05".to_string(),
            branch: Some("main".to_string()),
            working_dir: "/home/user/project".to_string(),
            model_name: "claude-sonnet-4-20250514".to_string(),
            provider: "anthropic".to_string(),
            platform: "linux".to_string(),
            is_git_repo: true,
        };

        let result = renderer.render(&vars).unwrap();
        assert!(result.contains("Wonopcode"));
        assert!(result.contains("Always use rust."));
        assert!(result.contains("Wed Mar 11 2026"));
        assert!(result.contains("14:30:05"));
        assert!(result.contains("main"));
        assert!(result.contains("/home/user/project"));
        assert!(result.contains("claude-sonnet-4-20250514"));
        assert!(result.contains("anthropic"));
    }

    #[test]
    fn test_renderer_no_agent_md() {
        let renderer = SystemPromptRenderer::new().unwrap();
        let vars = SystemPromptVars {
            agent_md: None,
            date: "Wed Mar 11 2026".to_string(),
            time: "14:30:05".to_string(),
            branch: None,
            working_dir: "/tmp".to_string(),
            model_name: "gpt-4".to_string(),
            provider: "openai".to_string(),
            platform: "macos".to_string(),
            is_git_repo: false,
        };

        let result = renderer.render(&vars).unwrap();
        assert!(result.contains("Wonopcode"));
        assert!(!result.contains("Project Instructions"));
    }

    #[test]
    fn test_renderer_custom_template() {
        let renderer = SystemPromptRenderer::with_template(
            "Hello {{ model_name }}! Date: {{ date }}"
        ).unwrap();
        let vars = SystemPromptVars {
            agent_md: None,
            date: "today".to_string(),
            time: "now".to_string(),
            branch: None,
            working_dir: "/tmp".to_string(),
            model_name: "test-model".to_string(),
            provider: "test".to_string(),
            platform: "test".to_string(),
            is_git_repo: false,
        };

        let result = renderer.render(&vars).unwrap();
        assert_eq!(result, "Hello test-model! Date: today");
    }

    #[test]
    fn test_renderer_invalid_template() {
        let result = SystemPromptRenderer::with_template("{{ unclosed");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_custom_instructions_missing() {
        let result = load_custom_instructions(Path::new("/nonexistent/path"));
        assert!(result.is_none());
    }

    #[test]
    fn test_system_prompt_vars_from_env() {
        let vars = SystemPromptVars::from_env(
            Path::new("/tmp"),
            "test-model",
            "test-provider",
        );
        assert_eq!(vars.model_name, "test-model");
        assert_eq!(vars.provider, "test-provider");
        assert!(!vars.date.is_empty());
        assert!(!vars.time.is_empty());
    }
}
