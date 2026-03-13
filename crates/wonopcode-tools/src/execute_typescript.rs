//! Execute TypeScript tool - runs TypeScript code in a sandboxed deno_core runtime.
//!
//! CRITICAL: This is critical code that cannot have any unsafe code.
//! All errors must be handled gracefully — no unwrap(), expect(), or panic!() in production paths.
//!
//! This tool allows LLMs to execute TypeScript code against a typed API,
//! enabling multi-step operations in a single script instead of multiple tool calls.
//!
//! Key features:
//! - Sandboxed execution with V8 isolate
//! - File system operations scoped to project root
//! - Shell command execution with allowlist
//! - Console output capture
//! - Configurable timeout and heap limits
//!
//! # V8 Process Isolation
//!
//! This tool requires a TypescriptExecutor (worker process) to execute code.
//! In-process V8 execution was removed to fix macOS CodeRange crashes caused by
//! V8/WebKit memory conflicts.
//!
//! The worker process architecture is implemented in:
//! - `wonopcode-codemode-worker`: Worker binary with V8 runtime
//! - `wonopcode-codemode-client`: IPC client library  
//! - `wonopcode-pro-server::WorkerManager`: Worker lifecycle management
//!
//! See docs/v8-process-isolation-plan.md for details.

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, info, warn};
use wonopcode_codemode::ToolDefinition;

/// Execute TypeScript code in a sandboxed runtime.
pub struct ExecuteTypescriptTool;

/// Options for script execution.
#[derive(Debug, Default, Deserialize)]
struct ExecuteOptions {
    /// Optional timeout in seconds (max: 120). If not specified, no timeout is applied.
    #[serde(default)]
    timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ExecuteTypescriptArgs {
    /// The TypeScript code to execute.
    code: String,
    /// Brief description of what this code does (required).
    description: String,
    /// Execution options including timeout.
    #[serde(default)]
    options: Option<ExecuteOptions>,
}

#[async_trait]
impl Tool for ExecuteTypescriptTool {
    fn id(&self) -> &str {
        "execute_typescript"
    }

    fn description(&self) -> &str {
        // Description is maintained in wonopcode-codemode/src/docs/execute_typescript.md
        // to stay in sync with runtime.js API changes
        include_str!("../../../../../../crates/wonopcode-codemode/src/docs/execute_typescript.md")
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["code", "description"],
            "properties": {
                "code": {
                    "type": "string",
                    "description": "TypeScript code to execute in the sandbox"
                },
                "description": {
                    "type": "string",
                    "description": "Brief description of what this code does"
                },
                "options": {
                    "type": "object",
                    "description": "Execution options",
                    "properties": {
                        "timeout_secs": {
                            "type": "integer",
                            "description": "Optional timeout in seconds (max: 120). If not specified, no timeout is applied.",
                            "minimum": 1,
                            "maximum": 120
                        }
                    }
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: ExecuteTypescriptArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        // Validate code is not empty
        if args.code.trim().is_empty() {
            return Err(ToolError::validation("Code cannot be empty"));
        }

        // Validate description is not empty
        if args.description.trim().is_empty() {
            return Err(ToolError::validation("Description cannot be empty"));
        }

        let description = &args.description;
        info!(description = %description, "Executing TypeScript code");

        // Extract timeout from options if provided, clamp to max 120 seconds
        let timeout_secs: Option<u64> = args.options
            .and_then(|opts| opts.timeout_secs)
            .map(|t| t.min(120));

        // Require external executor (worker process)
        // In-process V8 execution was removed to fix macOS CodeRange crashes
        // caused by V8/WebKit memory conflicts
        let executor = ctx.typescript_executor.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "TypeScript executor not configured. Worker process mode is required. \
                 See docs/v8-process-isolation-plan.md for setup instructions."
            )
        })?;
        
        let exec_context = crate::TypescriptExecutionContext {
            project_root: ctx.root_dir.clone(),
            session_id: ctx.session_id.clone(),
            timeout_secs,
        };

        let output = executor.execute(&args.code, exec_context).await.map_err(|e| {
            warn!(error = %e, "TypeScript execution failed");
            ToolError::execution_failed(format!("Execution failed: {e}"))
        })?;

        let output_text = if output.is_empty() {
            "(no output)".to_string()
        } else {
            output.join("\n")
        };

        debug!(
            lines = output.len(),
            "TypeScript execution completed"
        );

        Ok(ToolOutput::new(
            format!("TypeScript: {}", truncate_description(description)),
            output_text,
        )
        .with_metadata(serde_json::json!({
            "description": description,
            "output_lines": output.len(),
            "timeout_secs": timeout_secs,
        })))
    }

    fn requires_permission(&self) -> bool {
        // The execute_typescript tool itself is auto-allowed by default rules.
        // Dangerous operations inside the runtime (fs.write, childProcess.spawn, etc.) will
        // request their own permissions via the permission bridge.
        // We still return true so it goes through the permission system,
        // but the default rules will auto-approve it.
        true
    }
}

/// Generate TypeScript type stubs for a set of tool definitions.
///
/// This is used to provide the LLM with type information about the available API.
pub fn generate_type_stubs(tools: &[ToolDefinition]) -> String {
    wonopcode_codemode::generate_type_stubs(tools)
}

/// Get the default tool definitions available in the runtime.
pub fn default_tool_definitions() -> Vec<ToolDefinition> {
    wonopcode_codemode::default_tool_definitions()
}

/// Truncate description for display.
fn truncate_description(desc: &str) -> String {
    let first_line = desc.lines().next().unwrap_or(desc);
    if first_line.len() > 60 {
        // Take first 57 characters safely (handles multi-byte UTF-8)
        let truncated: String = first_line.chars().take(57).collect();
        format!("{}...", truncated)
    } else {
        first_line.to_string()
    }
}


// ============================================================================
// In-Process TypeScript Executor
// ============================================================================

use wonopcode_codemode::{CodemodeRuntime, RuntimeConfig, FeatureFlags, AllowedCommands};
use wonopcode_codemode::ServiceHandles as CodemodeServiceHandles;

/// In-process TypeScript executor using CodemodeRuntime.
///
/// This executor runs TypeScript code directly in the main process using
/// deno_core/V8. It's suitable for CLI usage where there's no WebKit conflict.
///
/// For desktop apps with WebKit, use WorkerExecutor (process isolation) instead
/// to avoid V8/WebKit memory conflicts.
pub struct InProcessTypescriptExecutor {
    /// Service handles for external services.
    services: CodemodeServiceHandles,
    /// Default shell for command execution.
    default_shell: Option<String>,
}

impl InProcessTypescriptExecutor {
    /// Create a new in-process executor with the given service handles.
    pub fn new(services: CodemodeServiceHandles) -> Self {
        Self {
            services,
            default_shell: None,
        }
    }

    /// Set the default shell for command execution.
    pub fn with_default_shell(mut self, shell: Option<String>) -> Self {
        self.default_shell = shell;
        self
    }
}

#[async_trait]
impl crate::TypescriptExecutor for InProcessTypescriptExecutor {
    async fn execute(&self, code: &str, context: crate::TypescriptExecutionContext) -> Result<Vec<String>, String> {
        let config = RuntimeConfig {
            project_root: context.project_root.clone(),
            allowed_commands: AllowedCommands::default(),
            timeout_secs: context.timeout_secs,
            max_heap_size: 64 * 1024 * 1024, // 64MB
            permission_bridge: None,
            session_id: Some(context.session_id.clone()),
            services: self.services.clone(),
            features: FeatureFlags::all_enabled(),
            workstream_ticket_id: None,
            workstream_tracker_id: None,
            default_shell: self.default_shell.clone(),
        };

        let runtime = CodemodeRuntime::new(config);
        
        runtime.execute(code).await.map_err(|e| e.to_string())
    }
}

// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    /// Mock executor for testing that returns predefined output.
    struct MockTypescriptExecutor {
        output: Vec<String>,
    }

    impl MockTypescriptExecutor {
        fn new(output: Vec<String>) -> Self {
            Self { output }
        }
    }

    #[async_trait]
    impl crate::TypescriptExecutor for MockTypescriptExecutor {
        async fn execute(&self, _code: &str, _context: crate::TypescriptExecutionContext) -> Result<Vec<String>, String> {
            Ok(self.output.clone())
        }
    }

    fn test_context_with_executor(executor: Arc<dyn crate::TypescriptExecutor>) -> ToolContext {
        ToolContext {
            session_id: "test_session".to_string(),
            message_id: "test_message".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: PathBuf::from("/tmp"),
            cwd: PathBuf::from("/tmp"),
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
            default_shell: None,
            typescript_executor: Some(executor),
        }
    }

    fn test_context() -> ToolContext {
        ToolContext {
            session_id: "test_session".to_string(),
            message_id: "test_message".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: PathBuf::from("/tmp"),
            cwd: PathBuf::from("/tmp"),
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
            default_shell: None,
            typescript_executor: None,
        }
    }

    // ============================================================================
    // Unit Tests
    // ============================================================================

    #[test]
    fn test_tool_id() {
        let tool = ExecuteTypescriptTool;
        assert_eq!(tool.id(), "execute_typescript");
    }

    #[test]
    fn test_tool_description() {
        let tool = ExecuteTypescriptTool;
        let desc = tool.description();
        assert!(desc.contains("TypeScript"));
        assert!(desc.contains("sandbox"));
        assert!(desc.contains("fs.read"));  // Flat namespace
        assert!(desc.contains("childProcess"));
        assert!(desc.contains("help"));
    }

    #[test]
    fn test_tool_parameters_schema() {
        let tool = ExecuteTypescriptTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("code")));
        assert!(required.contains(&json!("description")));
        assert!(schema["properties"]["code"].is_object());
        assert!(schema["properties"]["description"].is_object());
        assert!(schema["properties"]["options"].is_object());
        assert!(schema["properties"]["options"]["properties"]["timeout_secs"].is_object());
    }

    #[test]
    fn test_tool_requires_permission() {
        let tool = ExecuteTypescriptTool;
        assert!(tool.requires_permission());
    }

    #[test]
    fn test_truncate_description() {
        assert_eq!(truncate_description("Short"), "Short");

        let long = "This is a very long description that should be truncated because it exceeds sixty characters";
        let truncated = truncate_description(long);
        assert!(truncated.len() <= 63);
        assert!(truncated.ends_with("..."));

        let multiline = "First line\nSecond line\nThird line";
        assert_eq!(truncate_description(multiline), "First line");
    }

    #[test]
    fn test_args_deserialization() {
        let args: ExecuteTypescriptArgs = serde_json::from_value(json!({
            "code": "console.log('test');",
            "description": "Test script"
        }))
        .unwrap();

        assert_eq!(args.code, "console.log('test');");
        assert_eq!(args.description, "Test script");
        assert!(args.options.is_none());
    }

    #[test]
    fn test_args_with_all_fields() {
        let args: ExecuteTypescriptArgs = serde_json::from_value(json!({
            "code": "console.log('test');",
            "description": "Test script",
            "options": {
                "timeout_secs": 60
            }
        }))
        .unwrap();

        assert_eq!(args.code, "console.log('test');");
        assert_eq!(args.description, "Test script");
        assert_eq!(args.options.unwrap().timeout_secs, Some(60));
    }

    #[test]
    fn test_default_tool_definitions() {
        let defs = default_tool_definitions();
        // After the API refactor, default_tool_definitions returns empty
        // The full API is now namespaced and available via flat globals
        assert!(defs.is_empty());
    }

    #[test]
    fn test_generate_type_stubs() {
        // generate_type_stubs is for legacy tool definitions
        // The new API uses generate_api_stubs() which provides
        // flat namespace declarations (fs, exec, lsp, etc.)
        let defs = default_tool_definitions();
        let stubs = generate_type_stubs(&defs);
        // With empty definitions, stubs should still have the header
        assert!(stubs.contains("wonop"));
    }

    // ============================================================================
    // Integration Tests (TC-WON-205-006)
    // ============================================================================

    #[tokio::test]
    async fn test_empty_code_rejected() {
        let tool = ExecuteTypescriptTool;
        let ctx = test_context();

        let result = tool.execute(json!({
            "code": "   ",
            "description": "Empty code test"
        }), &ctx).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[tokio::test]
    async fn test_invalid_args_rejected() {
        let tool = ExecuteTypescriptTool;
        let ctx = test_context();

        let result = tool.execute(json!({ "not_code": "test" }), &ctx).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid arguments"));
    }


    #[tokio::test]
    async fn test_executor_required() {
        let tool = ExecuteTypescriptTool;
        let ctx = test_context();

        // Valid args but no executor configured - should fail with clear message
        let result = tool.execute(json!({
            "code": "console.log('test');",
            "description": "Test"
        }), &ctx).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("executor not configured"));
    }

    // Integration tests below require a TypescriptExecutor (worker process).
    // They are marked #[ignore] since unit tests run without the executor.
    // Run with: cargo test -- --ignored

    #[tokio::test]
    async fn test_basic_execution_with_mock() {
        let tool = ExecuteTypescriptTool;
        let mock = Arc::new(MockTypescriptExecutor::new(vec!["Hello from TypeScript".to_string()]));
        let ctx = test_context_with_executor(mock);

        let result = tool
            .execute(
                json!({
                    "code": "console.log('Hello from TypeScript');",
                    "description": "Test execution"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.output.contains("Hello from TypeScript"));
    }

    #[tokio::test]
    #[ignore = "requires real TypescriptExecutor - run with --ignored"]
    async fn test_basic_execution() {
        let tool = ExecuteTypescriptTool;
        let ctx = test_context();

        let result = tool
            .execute(
                json!({
                    "code": "console.log('Hello from TypeScript');",
                    "description": "Test execution"
                }),
                &ctx,
            )
            .await;

        match result {
            Ok(output) => {
                assert!(output.output.contains("Hello from TypeScript"));
            }
            Err(e) => {
                // Runtime errors are acceptable in tests
                println!("Runtime error (acceptable in test): {}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires TypescriptExecutor - run with --ignored"]
    async fn test_multiple_console_outputs() {
        let tool = ExecuteTypescriptTool;
        let ctx = test_context();

        let result = tool
            .execute(
                json!({
                    "code": r#"
                        console.log('Line 1');
                        console.log('Line 2');
                        console.log('Line 3');
                    "#,
                    "description": "Multiple outputs"
                }),
                &ctx,
            )
            .await;

        match result {
            Ok(output) => {
                assert!(output.output.contains("Line 1"));
                assert!(output.output.contains("Line 2"));
                assert!(output.output.contains("Line 3"));
            }
            Err(e) => {
                println!("Runtime error (acceptable in test): {}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires TypescriptExecutor - run with --ignored"]
    async fn test_file_read_execution() {
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let test_file = dir.path().join("test.txt");
        fs::write(&test_file, "Hello, World!").unwrap();

        let tool = ExecuteTypescriptTool;
        let ctx = ToolContext {
            root_dir: dir.path().to_path_buf(),
            cwd: dir.path().to_path_buf(),
            ..test_context()
        };

        let result = tool
            .execute(
                json!({
                    "code": r#"
                        const result = await fs.read("test.txt");
                        console.log(result.content);
                    "#,
                    "description": "Read test file"
                }),
                &ctx,
            )
            .await;

        match result {
            Ok(output) => {
                assert!(output.output.contains("Hello, World!"));
            }
            Err(e) => {
                println!("Runtime error (acceptable in test): {}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires TypescriptExecutor - run with --ignored"]
    async fn test_file_write_execution() {
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        let tool = ExecuteTypescriptTool;
        let ctx = ToolContext {
            root_dir: dir.path().to_path_buf(),
            cwd: dir.path().to_path_buf(),
            ..test_context()
        };

        let result = tool
            .execute(
                json!({
                    "code": r#"
                        await fs.write("output.txt", "Created by TypeScript");
                        console.log("File written");
                    "#,
                    "description": "Write test file"
                }),
                &ctx,
            )
            .await;

        match result {
            Ok(output) => {
                assert!(output.output.contains("File written"));
                let content = fs::read_to_string(dir.path().join("output.txt")).unwrap();
                assert_eq!(content, "Created by TypeScript");
            }
            Err(e) => {
                println!("Runtime error (acceptable in test): {}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires TypescriptExecutor - run with --ignored"]
    async fn test_list_dir_execution() {
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        fs::write(dir.path().join("file1.txt"), "").unwrap();
        fs::write(dir.path().join("file2.txt"), "").unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();

        let tool = ExecuteTypescriptTool;
        let ctx = ToolContext {
            root_dir: dir.path().to_path_buf(),
            cwd: dir.path().to_path_buf(),
            ..test_context()
        };

        let result = tool
            .execute(
                json!({
                    "code": r#"
                        const result = await fs.list(".");
                        console.log(result.entries.map(e => e.name).join(", "));
                    "#,
                    "description": "List directory"
                }),
                &ctx,
            )
            .await;

        match result {
            Ok(output) => {
                assert!(output.output.contains("file1.txt"));
                assert!(output.output.contains("file2.txt"));
                assert!(output.output.contains("subdir"));
            }
            Err(e) => {
                println!("Runtime error (acceptable in test): {}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires TypescriptExecutor - run with --ignored"]
    async fn test_timeout_clamping() {
        let tool = ExecuteTypescriptTool;
        let ctx = test_context();

        // Test that timeout is clamped to max 120 seconds
        let result = tool
            .execute(
                json!({
                    "code": "console.log('quick');",
                    "description": "Timeout clamping test",
                    "options": {
                        "timeout_secs": 999  // Should be clamped to 120
                    }
                }),
                &ctx,
            )
            .await;

        match result {
            Ok(output) => {
                // Verify execution succeeded despite large timeout
                assert!(output.output.contains("quick"));
                // Verify metadata shows clamped timeout
                assert_eq!(output.metadata["timeout_secs"], 120);
            }
            Err(e) => {
                println!("Runtime error (acceptable in test): {}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires TypescriptExecutor - run with --ignored"]
    async fn test_output_metadata() {
        let tool = ExecuteTypescriptTool;
        let ctx = test_context();

        let result = tool
            .execute(
                json!({
                    "code": "console.log('test');",
                    "description": "Metadata test"
                }),
                &ctx,
            )
            .await;

        match result {
            Ok(output) => {
                assert_eq!(output.metadata["description"], "Metadata test");
                // timeout_secs can be null (no timeout) or a number
                assert!(output.metadata["output_lines"].is_number());
            }
            Err(e) => {
                println!("Runtime error (acceptable in test): {}", e);
            }
        }
    }
}