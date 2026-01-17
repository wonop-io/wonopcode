//! Hooks system for automation and event handling.
//!
//! Supports hooks that run commands in response to events like:
//! - file_edited: After a file is edited
//! - session_completed: After a session ends

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, error};

/// A hook definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    /// Command to execute.
    pub command: Vec<String>,
    /// Environment variables.
    #[serde(default)]
    pub environment: HashMap<String, String>,
}

impl Hook {
    /// Create a new hook.
    pub fn new(command: Vec<String>) -> Self {
        Self {
            command,
            environment: HashMap::new(),
        }
    }

    /// Add environment variables.
    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.environment = env;
        self
    }

    /// Execute the hook with the given context variables.
    pub async fn execute(&self, context: &HookContext) -> Result<HookResult, HookError> {
        if self.command.is_empty() {
            return Err(HookError::InvalidCommand("Empty command".into()));
        }

        // Substitute variables in command
        let args: Vec<String> = self
            .command
            .iter()
            .map(|arg| substitute_variables(arg, context))
            .collect();

        let (program, args) = args
            .split_first()
            .ok_or_else(|| HookError::InvalidCommand("No program specified".into()))?;

        debug!(program = %program, "Executing hook");

        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Add environment variables (from hook definition)
        for (key, value) in &self.environment {
            cmd.env(key, substitute_variables(value, context));
        }

        // Add context as environment variables
        for (key, value) in &context.env {
            cmd.env(key, value);
        }

        // Set working directory if provided
        if let Some(cwd) = &context.cwd {
            cmd.current_dir(cwd);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| HookError::ExecutionFailed(format!("Failed to execute {program}: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(HookResult {
                success: true,
                stdout,
                stderr,
                exit_code: output.status.code(),
            })
        } else {
            Err(HookError::ExecutionFailed(format!(
                "Hook failed with exit code {:?}: {}",
                output.status.code(),
                stderr.trim()
            )))
        }
    }
}

/// Result of a hook execution.
#[derive(Debug)]
pub struct HookResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

/// Context passed to hooks.
#[derive(Debug, Default)]
pub struct HookContext {
    /// Environment variables to set.
    pub env: HashMap<String, String>,
    /// Working directory.
    pub cwd: Option<std::path::PathBuf>,
}

impl HookContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn with_cwd(mut self, cwd: impl Into<std::path::PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }
}

/// Substitute context variables in a string.
fn substitute_variables(input: &str, context: &HookContext) -> String {
    let mut result = input.to_string();
    for (key, value) in &context.env {
        result = result.replace(&format!("${key}"), value);
        result = result.replace(&format!("${{{key}}}"), value);
    }
    result
}

/// Hook error types.
#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("Invalid command: {0}")]
    InvalidCommand(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
}

/// Hook event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    /// File was edited.
    FileEdited,
    /// Session was completed.
    SessionCompleted,
    /// Message was sent.
    MessageSent,
    /// Tool was executed.
    ToolExecuted,
}

impl HookEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            HookEvent::FileEdited => "file_edited",
            HookEvent::SessionCompleted => "session_completed",
            HookEvent::MessageSent => "message_sent",
            HookEvent::ToolExecuted => "tool_executed",
        }
    }

    /// Parse hook event from string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "file_edited" => Some(HookEvent::FileEdited),
            "session_completed" => Some(HookEvent::SessionCompleted),
            "message_sent" => Some(HookEvent::MessageSent),
            "tool_executed" => Some(HookEvent::ToolExecuted),
            _ => None,
        }
    }
}

/// Hook registry for managing hooks.
#[derive(Debug, Default)]
pub struct HookRegistry {
    /// Hooks by event type.
    hooks: HashMap<HookEvent, Vec<Hook>>,
    /// File-pattern based hooks for file_edited.
    file_hooks: HashMap<String, Vec<Hook>>,
}

impl HookRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a hook for an event.
    pub fn register(&mut self, event: HookEvent, hook: Hook) {
        self.hooks.entry(event).or_default().push(hook);
    }

    /// Register a file-pattern hook (for file_edited event).
    pub fn register_file_hook(&mut self, pattern: impl Into<String>, hook: Hook) {
        self.file_hooks
            .entry(pattern.into())
            .or_default()
            .push(hook);
    }

    /// Trigger hooks for an event.
    pub async fn trigger(&self, event: HookEvent, context: &HookContext) {
        if let Some(hooks) = self.hooks.get(&event) {
            for hook in hooks {
                match hook.execute(context).await {
                    Ok(result) => {
                        debug!(
                            event = event.as_str(),
                            success = result.success,
                            "Hook executed"
                        );
                    }
                    Err(e) => {
                        error!(event = event.as_str(), error = %e, "Hook failed");
                    }
                }
            }
        }
    }

    /// Trigger file hooks for a specific file.
    pub async fn trigger_file_edited(&self, file_path: &Path, context: &HookContext) {
        // First trigger generic file_edited hooks
        self.trigger(HookEvent::FileEdited, context).await;

        // Then trigger pattern-matched hooks
        let file_str = file_path.display().to_string();
        let extension = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        for (pattern, hooks) in &self.file_hooks {
            // Simple pattern matching: *.ext or exact path
            let matches = if pattern.starts_with("*.") {
                let ext = &pattern[1..];
                file_str.ends_with(ext) || format!(".{extension}") == ext
            } else if pattern.contains('*') {
                // Basic glob matching
                glob_match(pattern, &file_str)
            } else {
                // Exact match
                file_str == *pattern
                    || file_path.file_name().map(|n| n.to_str()) == Some(Some(pattern))
            };

            if matches {
                for hook in hooks {
                    match hook.execute(context).await {
                        Ok(result) => {
                            debug!(
                                pattern = %pattern,
                                file = %file_str,
                                success = result.success,
                                "File hook executed"
                            );
                        }
                        Err(e) => {
                            error!(
                                pattern = %pattern,
                                file = %file_str,
                                error = %e,
                                "File hook failed"
                            );
                        }
                    }
                }
            }
        }
    }

    /// Check if any hooks are registered for an event.
    pub fn has_hooks(&self, event: HookEvent) -> bool {
        self.hooks
            .get(&event)
            .map(|h| !h.is_empty())
            .unwrap_or(false)
    }

    /// Get the number of hooks registered for an event.
    pub fn count(&self, event: HookEvent) -> usize {
        self.hooks.get(&event).map(|h| h.len()).unwrap_or(0)
    }
}

/// Simple glob pattern matching.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern_parts: Vec<&str> = pattern.split('*').collect();

    if pattern_parts.is_empty() {
        return text.is_empty();
    }

    let mut text_pos = 0;

    // First part must match at start (if not empty)
    if !pattern_parts[0].is_empty() {
        if !text.starts_with(pattern_parts[0]) {
            return false;
        }
        text_pos = pattern_parts[0].len();
    }

    // Last part must match at end (if not empty and not the only part)
    if pattern_parts.len() > 1 {
        let last = pattern_parts[pattern_parts.len() - 1];
        if !last.is_empty() && !text.ends_with(last) {
            return false;
        }
    }

    // Middle parts must appear in order
    for part in &pattern_parts[1..pattern_parts.len().saturating_sub(1)] {
        if part.is_empty() {
            continue;
        }
        match text[text_pos..].find(part) {
            Some(pos) => text_pos += pos + part.len(),
            None => return false,
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substitute_variables() {
        let context = HookContext::new()
            .with_env("FILE", "/path/to/file.rs")
            .with_env("EXT", "rs");

        assert_eq!(
            substitute_variables("Processing $FILE", &context),
            "Processing /path/to/file.rs"
        );

        assert_eq!(
            substitute_variables("Extension: ${EXT}", &context),
            "Extension: rs"
        );
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.rs", "src/lib.rs"));
        assert!(glob_match("src/*.rs", "src/main.rs"));
        assert!(!glob_match("*.rs", "main.go"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("test*file", "test_some_file"));
    }

    #[test]
    fn test_hook_event_conversion() {
        assert_eq!(HookEvent::FileEdited.as_str(), "file_edited");
        assert_eq!(HookEvent::parse("file_edited"), Some(HookEvent::FileEdited));
        assert_eq!(HookEvent::parse("unknown"), None);
    }

    #[test]
    fn test_hook_new() {
        let hook = Hook::new(vec!["echo".to_string(), "hello".to_string()]);
        assert_eq!(hook.command, vec!["echo", "hello"]);
        assert!(hook.environment.is_empty());
    }

    #[test]
    fn test_hook_with_env() {
        let mut env = HashMap::new();
        env.insert("FOO".to_string(), "bar".to_string());

        let hook = Hook::new(vec!["test".to_string()]).with_env(env);
        assert_eq!(hook.environment.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_hook_serialization() {
        let hook = Hook::new(vec!["echo".to_string(), "$FILE".to_string()]);
        let json = serde_json::to_string(&hook).unwrap();
        let parsed: Hook = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.command, hook.command);
    }

    #[test]
    fn test_hook_context_default() {
        let context = HookContext::default();
        assert!(context.env.is_empty());
        assert!(context.cwd.is_none());
    }

    #[test]
    fn test_hook_context_with_cwd() {
        let context = HookContext::new().with_cwd("/tmp/test");
        assert_eq!(context.cwd, Some(std::path::PathBuf::from("/tmp/test")));
    }

    #[tokio::test]
    async fn test_hook_execute_empty_command() {
        let hook = Hook::new(vec![]);
        let context = HookContext::new();
        let result = hook.execute(&context).await;
        assert!(matches!(result, Err(HookError::InvalidCommand(_))));
    }

    #[tokio::test]
    async fn test_hook_execute_success() {
        let hook = Hook::new(vec!["echo".to_string(), "hello".to_string()]);
        let context = HookContext::new();
        let result = hook.execute(&context).await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("hello"));
        assert_eq!(result.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_hook_execute_with_variable_substitution() {
        let hook = Hook::new(vec!["echo".to_string(), "$MSG".to_string()]);
        let context = HookContext::new().with_env("MSG", "test_message");
        let result = hook.execute(&context).await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("test_message"));
    }

    #[tokio::test]
    async fn test_hook_execute_with_cwd() {
        let hook = Hook::new(vec!["pwd".to_string()]);
        let context = HookContext::new().with_cwd("/tmp");
        let result = hook.execute(&context).await.unwrap();
        assert!(result.success);
        // On macOS /tmp is a symlink to /private/tmp
        assert!(result.stdout.contains("tmp"));
    }

    #[tokio::test]
    async fn test_hook_execute_failure() {
        let hook = Hook::new(vec!["false".to_string()]);
        let context = HookContext::new();
        let result = hook.execute(&context).await;
        assert!(matches!(result, Err(HookError::ExecutionFailed(_))));
    }

    #[test]
    fn test_hook_registry_new() {
        let registry = HookRegistry::new();
        assert!(!registry.has_hooks(HookEvent::FileEdited));
        assert_eq!(registry.count(HookEvent::FileEdited), 0);
    }

    #[test]
    fn test_hook_registry_register() {
        let mut registry = HookRegistry::new();
        registry.register(HookEvent::FileEdited, Hook::new(vec!["test".to_string()]));

        assert!(registry.has_hooks(HookEvent::FileEdited));
        assert_eq!(registry.count(HookEvent::FileEdited), 1);
        assert!(!registry.has_hooks(HookEvent::SessionCompleted));
    }

    #[test]
    fn test_hook_registry_register_multiple() {
        let mut registry = HookRegistry::new();
        registry.register(HookEvent::FileEdited, Hook::new(vec!["cmd1".to_string()]));
        registry.register(HookEvent::FileEdited, Hook::new(vec!["cmd2".to_string()]));

        assert_eq!(registry.count(HookEvent::FileEdited), 2);
    }

    #[test]
    fn test_hook_registry_register_file_hook() {
        let mut registry = HookRegistry::new();
        registry.register_file_hook("*.rs", Hook::new(vec!["rustfmt".to_string()]));

        assert!(registry.file_hooks.contains_key("*.rs"));
    }

    #[tokio::test]
    async fn test_hook_registry_trigger() {
        let mut registry = HookRegistry::new();
        registry.register(
            HookEvent::FileEdited,
            Hook::new(vec!["echo".to_string(), "triggered".to_string()]),
        );

        let context = HookContext::new();
        registry.trigger(HookEvent::FileEdited, &context).await;
        // Just verifying it doesn't panic
    }

    #[tokio::test]
    async fn test_hook_registry_trigger_file_edited() {
        let mut registry = HookRegistry::new();
        registry.register(
            HookEvent::FileEdited,
            Hook::new(vec!["echo".to_string(), "generic".to_string()]),
        );
        registry.register_file_hook(
            "*.rs",
            Hook::new(vec!["echo".to_string(), "rust file".to_string()]),
        );

        let context = HookContext::new().with_env("FILE", "/test/main.rs");
        registry
            .trigger_file_edited(Path::new("/test/main.rs"), &context)
            .await;
        // Just verifying it doesn't panic
    }

    #[tokio::test]
    async fn test_hook_registry_trigger_file_edited_exact_match() {
        let mut registry = HookRegistry::new();
        registry.register_file_hook(
            "Cargo.toml",
            Hook::new(vec!["echo".to_string(), "cargo file".to_string()]),
        );

        let context = HookContext::new();
        registry
            .trigger_file_edited(Path::new("/project/Cargo.toml"), &context)
            .await;
    }

    #[test]
    fn test_hook_error_display() {
        let invalid_cmd = HookError::InvalidCommand("empty".to_string());
        assert!(invalid_cmd.to_string().contains("Invalid command"));

        let exec_failed = HookError::ExecutionFailed("process failed".to_string());
        assert!(exec_failed.to_string().contains("Execution failed"));
    }

    #[test]
    fn test_hook_event_all_variants() {
        assert_eq!(HookEvent::FileEdited.as_str(), "file_edited");
        assert_eq!(HookEvent::SessionCompleted.as_str(), "session_completed");
        assert_eq!(HookEvent::MessageSent.as_str(), "message_sent");
        assert_eq!(HookEvent::ToolExecuted.as_str(), "tool_executed");

        assert_eq!(HookEvent::parse("file_edited"), Some(HookEvent::FileEdited));
        assert_eq!(
            HookEvent::parse("session_completed"),
            Some(HookEvent::SessionCompleted)
        );
        assert_eq!(
            HookEvent::parse("message_sent"),
            Some(HookEvent::MessageSent)
        );
        assert_eq!(
            HookEvent::parse("tool_executed"),
            Some(HookEvent::ToolExecuted)
        );
        assert_eq!(HookEvent::parse("invalid"), None);
    }

    #[test]
    fn test_hook_event_equality() {
        assert_eq!(HookEvent::FileEdited, HookEvent::FileEdited);
        assert_ne!(HookEvent::FileEdited, HookEvent::SessionCompleted);
    }

    #[test]
    fn test_glob_match_edge_cases() {
        // Multiple wildcards
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXc"));

        // Pattern at start
        assert!(glob_match("hello*", "hello world"));
        assert!(!glob_match("hello*", "world hello"));

        // Pattern at end
        assert!(glob_match("*.txt", "file.txt"));
        assert!(!glob_match("*.txt", "file.rs"));
    }

    #[test]
    fn test_substitute_variables_no_variables() {
        let context = HookContext::new();
        assert_eq!(
            substitute_variables("no variables here", &context),
            "no variables here"
        );
    }

    #[test]
    fn test_substitute_variables_multiple() {
        let context = HookContext::new().with_env("A", "1").with_env("B", "2");

        assert_eq!(substitute_variables("$A and $B", &context), "1 and 2");
        assert_eq!(substitute_variables("${A}${B}", &context), "12");
    }
}
