//! Formatter integration for auto-formatting files after edits.
//!
//! Supports multiple formatters based on file extension.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tracing::debug;

/// A formatter definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Formatter {
    /// Formatter name.
    pub name: String,
    /// Command to run (with $FILE placeholder).
    pub command: Vec<String>,
    /// Environment variables.
    #[serde(default)]
    pub environment: HashMap<String, String>,
    /// File extensions this formatter handles.
    pub extensions: Vec<String>,
    /// Whether this formatter is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Formatter {
    /// Create a new formatter.
    pub fn new(name: impl Into<String>, command: Vec<String>, extensions: Vec<String>) -> Self {
        Self {
            name: name.into(),
            command,
            environment: HashMap::new(),
            extensions,
            enabled: true,
        }
    }

    /// Check if this formatter handles the given file extension.
    pub fn handles(&self, extension: &str) -> bool {
        self.extensions.iter().any(|ext| {
            ext.eq_ignore_ascii_case(extension)
                || ext.eq_ignore_ascii_case(&format!(".{extension}"))
        })
    }

    /// Format a file.
    pub async fn format(&self, file_path: &Path) -> Result<(), FormatterError> {
        if !self.enabled {
            return Ok(());
        }

        if self.command.is_empty() {
            return Err(FormatterError::InvalidCommand("Empty command".into()));
        }

        let file_str = file_path.display().to_string();

        // Build command with $FILE substitution
        let args: Vec<String> = self
            .command
            .iter()
            .map(|arg| arg.replace("$FILE", &file_str))
            .collect();

        let (program, args) = args
            .split_first()
            .ok_or_else(|| FormatterError::InvalidCommand("No program specified".into()))?;

        debug!(formatter = %self.name, file = %file_str, "Running formatter");

        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Add environment variables
        for (key, value) in &self.environment {
            cmd.env(key, value);
        }

        let output = cmd.output().await.map_err(|e| {
            FormatterError::ExecutionFailed(format!("Failed to execute {program}: {e}"))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(FormatterError::ExecutionFailed(format!(
                "{} failed with exit code {:?}: {}",
                self.name,
                output.status.code(),
                stderr.trim()
            )));
        }

        debug!(formatter = %self.name, file = %file_str, "Format successful");
        Ok(())
    }
}

/// Formatter error types.
#[derive(Debug, thiserror::Error)]
pub enum FormatterError {
    #[error("Invalid command: {0}")]
    InvalidCommand(String),

    #[error("Formatter not found: {0}")]
    NotFound(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
}

/// Formatter registry for managing formatters.
#[derive(Debug, Default)]
pub struct FormatterRegistry {
    formatters: Vec<Formatter>,
    /// Whether formatting is globally disabled.
    disabled: bool,
}

impl FormatterRegistry {
    /// Create a new registry with built-in formatters.
    pub fn with_builtins() -> Self {
        let mut registry = Self::default();
        registry.register_builtins();
        registry
    }

    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Disable all formatting.
    pub fn disable(&mut self) {
        self.disabled = true;
    }

    /// Enable formatting.
    pub fn enable(&mut self) {
        self.disabled = false;
    }

    /// Register built-in formatters.
    fn register_builtins(&mut self) {
        // Go
        self.formatters.push(Formatter::new(
            "gofmt",
            vec!["gofmt".into(), "-w".into(), "$FILE".into()],
            vec![".go".into()],
        ));

        // Rust
        self.formatters.push(Formatter::new(
            "rustfmt",
            vec!["rustfmt".into(), "$FILE".into()],
            vec![".rs".into()],
        ));

        // JavaScript/TypeScript (Prettier)
        self.formatters.push(Formatter::new(
            "prettier",
            vec![
                "npx".into(),
                "prettier".into(),
                "--write".into(),
                "$FILE".into(),
            ],
            vec![
                ".js".into(),
                ".jsx".into(),
                ".ts".into(),
                ".tsx".into(),
                ".json".into(),
                ".css".into(),
                ".scss".into(),
                ".less".into(),
                ".html".into(),
                ".vue".into(),
                ".svelte".into(),
                ".md".into(),
                ".yaml".into(),
                ".yml".into(),
            ],
        ));

        // Python (Ruff)
        self.formatters.push(Formatter::new(
            "ruff",
            vec!["ruff".into(), "format".into(), "$FILE".into()],
            vec![".py".into(), ".pyi".into()],
        ));

        // Zig
        self.formatters.push(Formatter::new(
            "zig",
            vec!["zig".into(), "fmt".into(), "$FILE".into()],
            vec![".zig".into(), ".zon".into()],
        ));

        // C/C++ (clang-format)
        self.formatters.push(Formatter::new(
            "clang-format",
            vec!["clang-format".into(), "-i".into(), "$FILE".into()],
            vec![
                ".c".into(),
                ".cpp".into(),
                ".cc".into(),
                ".cxx".into(),
                ".h".into(),
                ".hpp".into(),
                ".hxx".into(),
            ],
        ));

        // Shell (shfmt)
        self.formatters.push(Formatter::new(
            "shfmt",
            vec!["shfmt".into(), "-w".into(), "$FILE".into()],
            vec![".sh".into(), ".bash".into()],
        ));

        // Dart
        self.formatters.push(Formatter::new(
            "dart",
            vec!["dart".into(), "format".into(), "$FILE".into()],
            vec![".dart".into()],
        ));

        // Kotlin (ktlint)
        self.formatters.push(Formatter::new(
            "ktlint",
            vec!["ktlint".into(), "-F".into(), "$FILE".into()],
            vec![".kt".into(), ".kts".into()],
        ));

        // Ruby (rubocop)
        self.formatters.push(Formatter::new(
            "rubocop",
            vec!["rubocop".into(), "-A".into(), "$FILE".into()],
            vec![".rb".into(), ".rake".into(), ".gemspec".into()],
        ));

        // Elixir (mix format)
        self.formatters.push(Formatter::new(
            "mix",
            vec!["mix".into(), "format".into(), "$FILE".into()],
            vec![".ex".into(), ".exs".into(), ".eex".into(), ".heex".into()],
        ));

        // OCaml
        self.formatters.push(Formatter::new(
            "ocamlformat",
            vec!["ocamlformat".into(), "-i".into(), "$FILE".into()],
            vec![".ml".into(), ".mli".into()],
        ));

        // Terraform
        self.formatters.push(Formatter::new(
            "terraform",
            vec!["terraform".into(), "fmt".into(), "$FILE".into()],
            vec![".tf".into(), ".tfvars".into()],
        ));

        // Gleam
        self.formatters.push(Formatter::new(
            "gleam",
            vec!["gleam".into(), "format".into(), "$FILE".into()],
            vec![".gleam".into()],
        ));
    }

    /// Register a custom formatter.
    pub fn register(&mut self, formatter: Formatter) {
        self.formatters.push(formatter);
    }

    /// Find a formatter for the given file.
    pub fn find_for_file(&self, path: &Path) -> Option<&Formatter> {
        if self.disabled {
            return None;
        }

        let extension = path.extension()?.to_str()?;

        self.formatters
            .iter()
            .find(|f| f.enabled && f.handles(extension))
    }

    /// Format a file if a formatter is available.
    pub async fn format_file(&self, path: &Path) -> Result<bool, FormatterError> {
        if self.disabled {
            return Ok(false);
        }

        match self.find_for_file(path) {
            Some(formatter) => {
                formatter.format(path).await?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Check if a formatter is available (binary exists).
    pub async fn is_available(&self, name: &str) -> bool {
        let formatter = match self.formatters.iter().find(|f| f.name == name) {
            Some(f) => f,
            None => return false,
        };

        if formatter.command.is_empty() {
            return false;
        }

        let program = &formatter.command[0];

        // Try to run with --version or --help to check availability
        match Command::new(program)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
        {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    }

    /// Get all registered formatters.
    pub fn list(&self) -> &[Formatter] {
        &self.formatters
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_formatter_handles_extension() {
        let formatter = Formatter::new(
            "test",
            vec!["test".into()],
            vec![".rs".into(), ".go".into()],
        );

        assert!(formatter.handles("rs"));
        assert!(formatter.handles(".rs"));
        assert!(formatter.handles("go"));
        assert!(!formatter.handles("py"));
    }

    #[test]
    fn test_find_formatter() {
        let registry = FormatterRegistry::with_builtins();

        let rs_file = PathBuf::from("src/main.rs");
        let formatter = registry.find_for_file(&rs_file);
        assert!(formatter.is_some());
        assert_eq!(formatter.unwrap().name, "rustfmt");

        let go_file = PathBuf::from("main.go");
        let formatter = registry.find_for_file(&go_file);
        assert!(formatter.is_some());
        assert_eq!(formatter.unwrap().name, "gofmt");

        let unknown_file = PathBuf::from("file.xyz");
        assert!(registry.find_for_file(&unknown_file).is_none());
    }

    #[test]
    fn test_registry_disabled() {
        let mut registry = FormatterRegistry::with_builtins();
        registry.disable();

        let rs_file = PathBuf::from("src/main.rs");
        assert!(registry.find_for_file(&rs_file).is_none());
    }

    #[test]
    fn test_registry_enable() {
        let mut registry = FormatterRegistry::with_builtins();
        registry.disable();
        assert!(registry.find_for_file(&PathBuf::from("main.rs")).is_none());

        registry.enable();
        assert!(registry.find_for_file(&PathBuf::from("main.rs")).is_some());
    }

    #[test]
    fn test_formatter_new() {
        let formatter = Formatter::new(
            "test-fmt",
            vec!["fmt".to_string(), "$FILE".to_string()],
            vec![".txt".to_string()],
        );

        assert_eq!(formatter.name, "test-fmt");
        assert!(formatter.enabled);
        assert!(formatter.environment.is_empty());
        assert!(formatter.handles("txt"));
    }

    #[test]
    fn test_formatter_handles_case_insensitive() {
        let formatter = Formatter::new("test", vec!["test".into()], vec![".RS".into()]);

        assert!(formatter.handles("rs"));
        assert!(formatter.handles("RS"));
        assert!(formatter.handles(".rs"));
        assert!(formatter.handles(".RS"));
    }

    #[test]
    fn test_registry_new() {
        let registry = FormatterRegistry::new();
        assert!(registry.list().is_empty());
    }

    #[test]
    fn test_registry_register() {
        let mut registry = FormatterRegistry::new();
        let formatter = Formatter::new(
            "custom",
            vec!["custom-fmt".into(), "$FILE".into()],
            vec![".custom".into()],
        );

        registry.register(formatter);
        assert_eq!(registry.list().len(), 1);
        assert_eq!(registry.list()[0].name, "custom");
    }

    #[test]
    fn test_registry_list() {
        let registry = FormatterRegistry::with_builtins();
        let formatters = registry.list();

        assert!(!formatters.is_empty());

        // Check some built-in formatters exist
        assert!(formatters.iter().any(|f| f.name == "rustfmt"));
        assert!(formatters.iter().any(|f| f.name == "gofmt"));
        assert!(formatters.iter().any(|f| f.name == "prettier"));
    }

    #[test]
    fn test_find_formatter_no_extension() {
        let registry = FormatterRegistry::with_builtins();
        let file = PathBuf::from("Makefile");
        assert!(registry.find_for_file(&file).is_none());
    }

    #[test]
    fn test_find_formatter_prettier_extensions() {
        let registry = FormatterRegistry::with_builtins();

        let extensions = [
            ".js", ".jsx", ".ts", ".tsx", ".json", ".css", ".html", ".md", ".yaml", ".yml",
        ];

        for ext in extensions {
            let file = PathBuf::from(format!("file{}", ext));
            let formatter = registry.find_for_file(&file);
            assert!(formatter.is_some(), "Expected prettier for {}", ext);
            assert_eq!(formatter.unwrap().name, "prettier");
        }
    }

    #[test]
    fn test_find_formatter_python() {
        let registry = FormatterRegistry::with_builtins();

        let py_file = PathBuf::from("script.py");
        let formatter = registry.find_for_file(&py_file);
        assert!(formatter.is_some());
        assert_eq!(formatter.unwrap().name, "ruff");

        let pyi_file = PathBuf::from("types.pyi");
        assert!(registry.find_for_file(&pyi_file).is_some());
    }

    #[test]
    fn test_find_formatter_c_cpp() {
        let registry = FormatterRegistry::with_builtins();

        let c_exts = [".c", ".cpp", ".cc", ".cxx", ".h", ".hpp", ".hxx"];
        for ext in c_exts {
            let file = PathBuf::from(format!("file{}", ext));
            let formatter = registry.find_for_file(&file);
            assert!(formatter.is_some(), "Expected clang-format for {}", ext);
            assert_eq!(formatter.unwrap().name, "clang-format");
        }
    }

    #[test]
    fn test_formatter_disabled() {
        let mut formatter = Formatter::new("test", vec!["echo".into()], vec![".test".into()]);
        formatter.enabled = false;

        let mut registry = FormatterRegistry::new();
        registry.register(formatter);

        let file = PathBuf::from("file.test");
        assert!(registry.find_for_file(&file).is_none());
    }

    #[tokio::test]
    async fn test_format_file_no_formatter() {
        let registry = FormatterRegistry::with_builtins();
        let file = PathBuf::from("file.xyz");

        let result = registry.format_file(&file).await;
        assert!(result.is_ok());
        assert!(!result.unwrap()); // No formatter found
    }

    #[tokio::test]
    async fn test_format_file_disabled() {
        let mut registry = FormatterRegistry::with_builtins();
        registry.disable();

        let file = PathBuf::from("main.rs");
        let result = registry.format_file(&file).await;
        assert!(result.is_ok());
        assert!(!result.unwrap()); // Disabled, so nothing formatted
    }

    #[tokio::test]
    async fn test_format_disabled_formatter() {
        let mut formatter = Formatter::new("test", vec!["echo".into()], vec![".test".into()]);
        formatter.enabled = false;

        let file = PathBuf::from("/tmp/test.file");
        let result = formatter.format(&file).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_format_empty_command() {
        let formatter = Formatter::new("empty", vec![], vec![".test".into()]);

        let file = PathBuf::from("/tmp/test.file");
        let result = formatter.format(&file).await;
        assert!(result.is_err());

        if let Err(FormatterError::InvalidCommand(msg)) = result {
            assert!(msg.contains("Empty command"));
        } else {
            panic!("Expected InvalidCommand error");
        }
    }

    #[tokio::test]
    async fn test_format_nonexistent_command() {
        let formatter = Formatter::new(
            "nonexistent",
            vec!["this-command-does-not-exist-12345".into(), "$FILE".into()],
            vec![".test".into()],
        );

        let file = PathBuf::from("/tmp/test.file");
        let result = formatter.format(&file).await;
        assert!(result.is_err());

        if let Err(FormatterError::ExecutionFailed(msg)) = result {
            assert!(msg.contains("Failed to execute"));
        } else {
            panic!("Expected ExecutionFailed error");
        }
    }

    #[test]
    fn test_formatter_error_display() {
        let err = FormatterError::InvalidCommand("test error".into());
        assert_eq!(err.to_string(), "Invalid command: test error");

        let err = FormatterError::NotFound("rustfmt".into());
        assert_eq!(err.to_string(), "Formatter not found: rustfmt");

        let err = FormatterError::ExecutionFailed("exit code 1".into());
        assert_eq!(err.to_string(), "Execution failed: exit code 1");
    }

    #[test]
    fn test_formatter_serialization() {
        let formatter = Formatter {
            name: "test".to_string(),
            command: vec!["fmt".to_string(), "$FILE".to_string()],
            environment: {
                let mut env = HashMap::new();
                env.insert("KEY".to_string(), "VALUE".to_string());
                env
            },
            extensions: vec![".test".to_string()],
            enabled: true,
        };

        let json = serde_json::to_string(&formatter).unwrap();
        let deserialized: Formatter = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.name, "test");
        assert_eq!(deserialized.command.len(), 2);
        assert_eq!(
            deserialized.environment.get("KEY"),
            Some(&"VALUE".to_string())
        );
        assert!(deserialized.enabled);
    }

    #[test]
    fn test_formatter_default_enabled() {
        let json = r#"{"name":"test","command":["fmt"],"extensions":[".test"]}"#;
        let formatter: Formatter = serde_json::from_str(json).unwrap();
        assert!(formatter.enabled); // defaults to true
    }

    #[tokio::test]
    async fn test_is_available_nonexistent_formatter() {
        let registry = FormatterRegistry::with_builtins();
        let available = registry.is_available("nonexistent-formatter-name").await;
        assert!(!available);
    }

    #[tokio::test]
    async fn test_is_available_empty_command() {
        let mut registry = FormatterRegistry::new();
        let formatter = Formatter {
            name: "empty".to_string(),
            command: vec![],
            environment: HashMap::new(),
            extensions: vec![".test".to_string()],
            enabled: true,
        };
        registry.register(formatter);

        let available = registry.is_available("empty").await;
        assert!(!available);
    }

    #[test]
    fn test_default_registry() {
        let registry = FormatterRegistry::default();
        assert!(registry.list().is_empty());
    }
}
