//! Custom command system for defining slash commands.
//!
//! Commands are defined in:
//! - Configuration file: `wonopcode.json` -> `command` section
//! - Markdown files: `.wonopcode/command/**/*.md` with frontmatter

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, warn};

/// A custom command definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    /// Command name (used as /name).
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Prompt template with placeholders.
    pub template: String,
    /// Override agent for this command.
    #[serde(default)]
    pub agent: Option<String>,
    /// Override model for this command.
    #[serde(default)]
    pub model: Option<String>,
    /// Run as subtask.
    #[serde(default)]
    pub subtask: bool,
}

impl Command {
    /// Create a new command.
    pub fn new(name: impl Into<String>, template: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            template: template.into(),
            agent: None,
            model: None,
            subtask: false,
        }
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the agent.
    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = Some(agent.into());
        self
    }

    /// Set the model.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Expand the template with arguments.
    pub fn expand(&self, arguments: &str) -> String {
        let args: Vec<&str> = arguments.split_whitespace().collect();
        expand_template(&self.template, &args, arguments)
    }
}

/// Command frontmatter in YAML format.
#[derive(Debug, Deserialize)]
struct CommandFrontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    subtask: Option<bool>,
}

/// Command registry for managing custom commands.
#[derive(Debug, Default)]
pub struct CommandRegistry {
    commands: HashMap<String, Command>,
}

impl CommandRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry with built-in commands.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register_builtins();
        registry
    }

    /// Register built-in commands.
    fn register_builtins(&mut self) {
        // /init - Initialize project configuration
        self.register(Command::new("init", r#"Analyze this codebase and create or update the AGENTS.md file with project-specific conventions.

Look for:
1. Code style patterns (formatting, naming conventions)
2. Project structure and architecture
3. Testing patterns and frameworks
4. Build tools and scripts
5. Common patterns and idioms

Create a comprehensive AGENTS.md that will help me work more effectively with this codebase.
$ARGUMENTS"#)
            .with_description("Initialize or update project configuration")
            .with_agent("plan"));

        // /review - Code review
        self.register(
            Command::new(
                "review",
                r#"Please review the code changes. Focus on:

1. Code quality and best practices
2. Potential bugs or edge cases
3. Performance considerations
4. Security implications
5. Suggestions for improvement

$ARGUMENTS"#,
            )
            .with_description("Review code changes"),
        );

        // /explain - Explain code
        self.register(
            Command::new(
                "explain",
                r#"Please explain the following code or concept in detail:

$ARGUMENTS

Include:
1. What the code does at a high level
2. Key implementation details
3. Any patterns or techniques used
4. Potential improvements or alternatives"#,
            )
            .with_description("Explain code or concepts"),
        );

        // /fix - Fix issues
        self.register(
            Command::new(
                "fix",
                r#"Please fix the following issue:

$ARGUMENTS

Make sure to:
1. Identify the root cause
2. Implement a proper fix
3. Add any necessary tests
4. Document the changes"#,
            )
            .with_description("Fix issues or bugs"),
        );

        // /test - Generate tests
        self.register(
            Command::new(
                "test",
                r#"Please generate tests for:

$ARGUMENTS

Include:
1. Unit tests for individual functions
2. Edge cases and error handling
3. Integration tests if appropriate
4. Clear test descriptions"#,
            )
            .with_description("Generate tests"),
        );

        // /doc - Generate documentation
        self.register(
            Command::new(
                "doc",
                r#"Please generate or improve documentation for:

$ARGUMENTS

Include:
1. Function/module documentation
2. Usage examples
3. Parameter descriptions
4. Return value documentation"#,
            )
            .with_description("Generate documentation"),
        );

        // /refactor - Refactor code
        self.register(
            Command::new(
                "refactor",
                r#"Please refactor the following code:

$ARGUMENTS

Focus on:
1. Improving code structure
2. Reducing complexity
3. Better naming
4. Removing duplication
5. Following best practices"#,
            )
            .with_description("Refactor code"),
        );

        // /sandbox - Sandbox control
        self.register(
            Command::new(
                "sandbox",
                r#"Control the sandbox environment.

Usage:
  /sandbox start  - Start the sandbox container/VM
  /sandbox stop   - Stop the sandbox
  /sandbox status - Show current sandbox status
  /sandbox shell  - Open an interactive shell in the sandbox

$ARGUMENTS"#,
            )
            .with_description("Control sandbox environment"),
        );
    }

    /// Register a command.
    pub fn register(&mut self, command: Command) {
        self.commands.insert(command.name.clone(), command);
    }

    /// Register commands from configuration.
    pub fn register_from_config(&mut self, config: &HashMap<String, CommandConfig>) {
        for (name, cfg) in config {
            let mut command = Command::new(name, &cfg.template);
            if let Some(desc) = &cfg.description {
                command.description = desc.clone();
            }
            if let Some(agent) = &cfg.agent {
                command.agent = Some(agent.clone());
            }
            if let Some(model) = &cfg.model {
                command.model = Some(model.clone());
            }
            if let Some(subtask) = cfg.subtask {
                command.subtask = subtask;
            }
            self.register(command);
        }
    }

    /// Discover commands from directories.
    pub async fn discover(&mut self, directories: &[PathBuf]) {
        for dir in directories {
            let command_dir = dir.join(".wonopcode").join("command");
            if command_dir.exists() {
                self.scan_directory(&command_dir).await;
            }
        }
    }

    /// Scan a directory for command markdown files.
    async fn scan_directory(&mut self, dir: &Path) {
        let walker = walkdir::WalkDir::new(dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok());

        for entry in walker {
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                match Self::load_command(path).await {
                    Ok(command) => {
                        debug!(name = %command.name, path = %path.display(), "Loaded command");
                        self.register(command);
                    }
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "Failed to load command");
                    }
                }
            }
        }
    }

    /// Load a command from a markdown file.
    async fn load_command(path: &Path) -> Result<Command, String> {
        let content = fs::read_to_string(path)
            .await
            .map_err(|e| format!("Failed to read file: {}", e))?;

        let (frontmatter, body) = parse_frontmatter(&content)?;

        // Use filename as name if not specified in frontmatter
        let name = frontmatter.name.unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed")
                .to_string()
        });

        Ok(Command {
            name,
            description: frontmatter.description.unwrap_or_default(),
            template: body,
            agent: frontmatter.agent,
            model: frontmatter.model,
            subtask: frontmatter.subtask.unwrap_or(false),
        })
    }

    /// Get a command by name.
    pub fn get(&self, name: &str) -> Option<&Command> {
        self.commands.get(name)
    }

    /// List all commands.
    pub fn list(&self) -> Vec<&Command> {
        self.commands.values().collect()
    }

    /// Get command names.
    pub fn names(&self) -> Vec<&str> {
        self.commands.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a command exists.
    pub fn contains(&self, name: &str) -> bool {
        self.commands.contains_key(name)
    }

    /// Get the number of commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// Command configuration from config file.
#[derive(Debug, Clone, Deserialize)]
pub struct CommandConfig {
    pub template: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub subtask: Option<bool>,
}

/// Parse YAML frontmatter from markdown content.
fn parse_frontmatter(content: &str) -> Result<(CommandFrontmatter, String), String> {
    let content = content.trim();

    // Check for frontmatter delimiter
    if !content.starts_with("---") {
        // No frontmatter, use entire content as template
        return Ok((
            CommandFrontmatter {
                name: None,
                description: None,
                agent: None,
                model: None,
                subtask: None,
            },
            content.to_string(),
        ));
    }

    // Find the end of frontmatter
    let rest = &content[3..];
    let end_idx = rest
        .find("\n---")
        .ok_or("Missing closing frontmatter delimiter")?;

    let frontmatter_str = &rest[..end_idx].trim();
    let body = rest[end_idx + 4..].trim();

    // Parse YAML frontmatter
    let frontmatter: CommandFrontmatter = serde_yaml::from_str(frontmatter_str)
        .map_err(|e| format!("Invalid frontmatter YAML: {}", e))?;

    Ok((frontmatter, body.to_string()))
}

/// Expand template with arguments.
fn expand_template(template: &str, args: &[&str], full_arguments: &str) -> String {
    let mut result = template.to_string();

    // Replace $ARGUMENTS with full argument string
    result = result.replace("$ARGUMENTS", full_arguments);

    // Find the highest numbered placeholder
    let mut max_placeholder = 0;
    for i in 1..=20 {
        if result.contains(&format!("${}", i)) {
            max_placeholder = i;
        }
    }

    // Replace numbered placeholders
    for i in 1..=max_placeholder {
        let placeholder = format!("${}", i);
        let value = if i == max_placeholder && i <= args.len() {
            // Last placeholder gets all remaining arguments
            args[i - 1..].join(" ")
        } else if i <= args.len() {
            args[i - 1].to_string()
        } else {
            String::new()
        };
        result = result.replace(&placeholder, &value);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_template_arguments() {
        let result = expand_template(
            "Review: $ARGUMENTS",
            &["file.rs", "line", "20"],
            "file.rs line 20",
        );
        assert_eq!(result, "Review: file.rs line 20");
    }

    #[test]
    fn test_expand_template_numbered() {
        let result = expand_template("$1 and $2", &["first", "second"], "first second");
        assert_eq!(result, "first and second");
    }

    #[test]
    fn test_expand_template_last_swallows() {
        let result = expand_template(
            "File: $1, Rest: $2",
            &["main.rs", "extra", "args"],
            "main.rs extra args",
        );
        assert_eq!(result, "File: main.rs, Rest: extra args");
    }

    #[test]
    fn test_parse_frontmatter() {
        let content = r#"---
name: test
description: A test command
agent: plan
---

This is the template $ARGUMENTS
"#;

        let (fm, body) = parse_frontmatter(content).unwrap();
        assert_eq!(fm.name, Some("test".to_string()));
        assert_eq!(fm.description, Some("A test command".to_string()));
        assert_eq!(fm.agent, Some("plan".to_string()));
        assert!(body.contains("template"));
    }

    #[test]
    fn test_parse_no_frontmatter() {
        let content = "Just a template with $ARGUMENTS";
        let (fm, body) = parse_frontmatter(content).unwrap();
        assert!(fm.name.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn test_command_expand() {
        // When $1 is the only numbered placeholder and there are multiple args,
        // $1 gets all the args (last placeholder swallows remaining)
        let cmd = Command::new("test", "Process $1 with $ARGUMENTS");
        let result = cmd.expand("file.rs extra stuff");
        // $1 swallows all args when it's the last placeholder
        assert_eq!(
            result,
            "Process file.rs extra stuff with file.rs extra stuff"
        );
    }

    #[test]
    fn test_builtin_commands() {
        let registry = CommandRegistry::with_builtins();
        assert!(registry.contains("init"));
        assert!(registry.contains("review"));
        assert!(registry.contains("explain"));
        assert!(registry.contains("fix"));
        assert!(registry.contains("test"));
        assert!(registry.contains("doc"));
        assert!(registry.contains("refactor"));
    }
}
