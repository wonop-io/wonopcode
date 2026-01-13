//! Bash tool - execute shell commands.
//!
//! Executes shell commands with:
//! - Configurable timeout (default 2 minutes, max 10 minutes)
//! - Working directory support
//! - Output truncation for large outputs
//! - Background execution support
//! - Permission-based command validation

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::{debug, info, warn};
use wonopcode_sandbox::SandboxCapabilities;
use wonopcode_util::{BashPermission, BashPermissionConfig};

/// Default timeout in milliseconds (2 minutes).
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// Maximum timeout in milliseconds (10 minutes).
const MAX_TIMEOUT_MS: u64 = 600_000;

/// Maximum output size in bytes before truncation.
const MAX_OUTPUT_SIZE: usize = 30_000;

/// Execute shell commands.
pub struct BashTool;

#[derive(Debug, Deserialize)]
struct BashArgs {
    command: String,
    /// Description field sent by agent for logging purposes (not used in execution).
    #[serde(default)]
    _description: Option<String>,
    workdir: Option<String>,
    timeout: Option<u64>,
    #[serde(default)]
    run_in_background: bool,
}

#[async_trait]
impl Tool for BashTool {
    fn id(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        r#"Executes a given bash command with optional timeout.

Usage notes:
- The command argument is required.
- You can specify an optional timeout in milliseconds (up to 600000ms / 10 minutes).
- Commands will time out after 120000ms (2 minutes) by default.
- Use workdir parameter to run in a specific directory.
- Output is truncated if it exceeds 30000 characters."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["command", "description"],
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command to execute"
                },
                "description": {
                    "type": "string",
                    "description": "Clear, concise description of what this command does"
                },
                "workdir": {
                    "type": "string",
                    "description": "The working directory to run the command in"
                },
                "timeout": {
                    "type": "number",
                    "description": "Optional timeout in milliseconds (max 600000)"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "Run the command in background"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: BashArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        // Validate command is not empty
        if args.command.trim().is_empty() {
            return Err(ToolError::validation("Command cannot be empty"));
        }

        // Check bash permissions using default config
        // NOTE: Permission configuration could be loaded from session/config,
        // but for security, we use the restrictive defaults. Customization is
        // available via the permission manager at the Instance level.
        let permission_config = BashPermissionConfig::default();
        let permission = permission_config.check(&args.command);

        match permission {
            BashPermission::Deny => {
                warn!(command = %args.command, "Bash command denied by permission config");
                return Err(ToolError::permission_denied(format!(
                    "Command '{}' is not allowed. This command has been blocked by the permission configuration.",
                    truncate_command(&args.command)
                )));
            }
            BashPermission::Ask => {
                // For now, log a warning that the command requires approval
                // In full integration, this would trigger a permission request through the bus
                info!(command = %args.command, "Bash command requires user approval");
                // Continue execution but note this should ideally be confirmed
                // The permission system in the runner should have already asked
            }
            BashPermission::Allow => {
                debug!(command = %args.command, "Bash command allowed by permission config");
            }
        }

        // Determine working directory
        let workdir = args
            .workdir
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.cwd.clone());

        // Calculate timeout
        let timeout_ms = args
            .timeout
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS);
        let timeout = Duration::from_millis(timeout_ms);

        // Route through sandbox if available
        if let Some(sandbox) = ctx.sandbox() {
            info!(
                command = %args.command,
                "Routing bash command through sandbox"
            );
            return self
                .execute_sandboxed(sandbox.as_ref(), &args.command, &workdir, timeout, ctx)
                .await;
        } else {
            debug!(
                command = %args.command,
                "No sandbox available, executing directly"
            );
        }

        // Non-sandboxed execution: additional security checks required

        // Verify workdir is within the project root (security check)
        if let Ok(canonical_workdir) = workdir.canonicalize() {
            if let Ok(canonical_root) = ctx.root_dir.canonicalize() {
                if !canonical_workdir.starts_with(&canonical_root) {
                    warn!(
                        workdir = %workdir.display(),
                        root = %ctx.root_dir.display(),
                        "Bash command attempted to run outside project root"
                    );
                    return Err(ToolError::permission_denied(format!(
                        "Cannot execute commands in '{}' - directory is outside the project root",
                        workdir.display()
                    )));
                }
            }
        }

        // Validate working directory exists
        if !workdir.exists() {
            return Err(ToolError::validation(format!(
                "Working directory does not exist: {}",
                workdir.display()
            )));
        }

        debug!(
            command = %args.command,
            workdir = %workdir.display(),
            timeout_ms = timeout_ms,
            sandboxed = false,
            "Executing bash command"
        );

        self.execute_direct(&args.command, &workdir, timeout, args.run_in_background)
            .await
    }
}

impl BashTool {
    /// Execute command through sandbox runtime.
    async fn execute_sandboxed(
        &self,
        sandbox: &dyn wonopcode_sandbox::SandboxRuntime,
        command: &str,
        workdir: &Path,
        timeout: Duration,
        ctx: &ToolContext,
    ) -> ToolResult<ToolOutput> {
        // Convert host path to sandbox path
        let sandbox_workdir = ctx.to_sandbox_path(workdir);

        debug!(
            command = %command,
            host_workdir = %workdir.display(),
            sandbox_workdir = %sandbox_workdir.display(),
            timeout_ms = timeout.as_millis(),
            sandboxed = true,
            "Executing bash command in sandbox"
        );

        // Use default capabilities for now.
        //
        // Elevated capabilities for specific commands is a future enhancement.
        // This would allow commands like `git`, `npm`, `cargo` to have network
        // access while still running in the sandbox. Implementation would require:
        // 1. A whitelist of commands that can request elevated capabilities
        // 2. Pattern matching on the command to identify which capability is needed
        // 3. Audit logging for all elevated capability usage
        // 4. User confirmation for first use of each elevated command
        //
        // For now, all sandbox commands use default (restricted) capabilities.
        // Network access can be enabled globally via sandbox.network config.
        let capabilities = SandboxCapabilities::default();

        let result = sandbox
            .execute(command, &sandbox_workdir, timeout, &capabilities)
            .await
            .map_err(|e| ToolError::execution_failed(format!("Sandbox execution failed: {e}")))?;

        // Combine output
        let combined = result.combined();
        let (output, truncated) = truncate_output(&combined, MAX_OUTPUT_SIZE);

        if truncated {
            warn!(
                command = %command,
                "Output truncated to {} chars",
                MAX_OUTPUT_SIZE
            );
        }

        // Determine title based on exit code
        let title = if result.success {
            truncate_command(command)
        } else {
            format!(
                "{} (exit code: {})",
                truncate_command(command),
                result.exit_code
            )
        };

        Ok(ToolOutput::new(title, output).with_metadata(json!({
            "exit_code": result.exit_code,
            "workdir": workdir.display().to_string(),
            "sandbox_workdir": sandbox_workdir.display().to_string(),
            "sandboxed": true,
            "truncated": truncated
        })))
    }

    /// Execute command directly on host.
    async fn execute_direct(
        &self,
        command: &str,
        workdir: &PathBuf,
        timeout: Duration,
        run_in_background: bool,
    ) -> ToolResult<ToolOutput> {
        // Build the command
        let mut cmd = Command::new("bash");
        cmd.arg("-c")
            .arg(command)
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Set environment to avoid interactive prompts
        cmd.env("TERM", "dumb");
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        cmd.env("NO_COLOR", "1");

        // Spawn the process
        let mut child = cmd
            .spawn()
            .map_err(|e| ToolError::execution_failed(format!("Failed to spawn process: {e}")))?;

        // Handle background execution
        if run_in_background {
            // Don't wait for the process
            return Ok(ToolOutput::new(
                format!("Started in background: {}", truncate_command(command)),
                "Command started in background. Output will be available as it becomes ready.",
            )
            .with_metadata(json!({
                "background": true,
                "workdir": workdir.display().to_string()
            })));
        }

        // Wait for the process with timeout
        let timeout_ms = timeout.as_millis() as u64;
        let result = tokio::time::timeout(timeout, async {
            // Get stdout and stderr handles
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();

            // Read output concurrently
            let stdout_handle = tokio::spawn(async move {
                let mut buf = Vec::new();
                if let Some(mut stdout) = stdout {
                    stdout.read_to_end(&mut buf).await.ok();
                }
                buf
            });

            let stderr_handle = tokio::spawn(async move {
                let mut buf = Vec::new();
                if let Some(mut stderr) = stderr {
                    stderr.read_to_end(&mut buf).await.ok();
                }
                buf
            });

            // Wait for process to complete
            let status = child.wait().await?;

            // Collect output
            let stdout_bytes = stdout_handle.await.unwrap_or_default();
            let stderr_bytes = stderr_handle.await.unwrap_or_default();

            Ok::<_, std::io::Error>((status, stdout_bytes, stderr_bytes))
        })
        .await;

        match result {
            Ok(Ok((status, stdout_bytes, stderr_bytes))) => {
                let exit_code = status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&stdout_bytes);
                let stderr = String::from_utf8_lossy(&stderr_bytes);

                // Combine output
                let mut output = String::new();

                if !stdout.is_empty() {
                    output.push_str(&stdout);
                }

                if !stderr.is_empty() {
                    if !output.is_empty() {
                        output.push_str("\n\n--- stderr ---\n");
                    }
                    output.push_str(&stderr);
                }

                // Truncate if too long
                let (output, truncated) = truncate_output(&output, MAX_OUTPUT_SIZE);

                if truncated {
                    warn!(
                        command = %command,
                        "Output truncated to {} chars",
                        MAX_OUTPUT_SIZE
                    );
                }

                // Determine title based on exit code
                let title = if status.success() {
                    truncate_command(command)
                } else {
                    format!("{} (exit code: {})", truncate_command(command), exit_code)
                };

                Ok(ToolOutput::new(title, output).with_metadata(json!({
                    "exit_code": exit_code,
                    "workdir": workdir.display().to_string(),
                    "truncated": truncated
                })))
            }
            Ok(Err(e)) => Err(ToolError::execution_failed(format!("Process error: {e}"))),
            Err(_) => {
                // Timeout - process is killed by kill_on_drop
                Err(ToolError::execution_failed(format!(
                    "Command timed out after {timeout_ms}ms"
                )))
            }
        }
    }
}

/// Truncate command for display in title.
fn truncate_command(cmd: &str) -> String {
    let first_line = cmd.lines().next().unwrap_or(cmd);
    if first_line.len() > 50 {
        format!("{}...", &first_line[..47])
    } else {
        first_line.to_string()
    }
}

/// Truncate output if too long.
fn truncate_output(output: &str, max_size: usize) -> (String, bool) {
    if output.len() <= max_size {
        return (output.to_string(), false);
    }

    // Keep first half and last portion
    let keep_start = max_size * 2 / 3;
    let keep_end = max_size - keep_start - 100; // Leave room for truncation message

    let start = &output[..keep_start];
    let end_start = output.len().saturating_sub(keep_end);
    let end = &output[end_start..];

    let truncated = format!(
        "{}\n\n... [truncated {} chars] ...\n\n{}",
        start,
        output.len() - keep_start - keep_end,
        end
    );

    (truncated, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::sync::CancellationToken;

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
        }
    }

    #[tokio::test]
    #[cfg_attr(windows, ignore)]
    async fn test_simple_command() {
        let tool = BashTool;
        let ctx = test_context();

        let result = tool
            .execute(
                json!({
                    "command": "echo hello",
                    "description": "Print hello"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.output.trim().contains("hello"));
    }

    #[tokio::test]
    #[cfg_attr(windows, ignore)]
    async fn test_command_with_exit_code() {
        let tool = BashTool;
        let ctx = test_context();

        let result = tool
            .execute(
                json!({
                    "command": "exit 42",
                    "description": "Exit with code 42"
                }),
                &ctx,
            )
            .await
            .unwrap();

        let metadata: Value = result.metadata;
        assert_eq!(metadata["exit_code"], 42);
    }

    #[tokio::test]
    #[cfg_attr(windows, ignore)]
    async fn test_command_stderr() {
        let tool = BashTool;
        let ctx = test_context();

        let result = tool
            .execute(
                json!({
                    "command": "echo error >&2",
                    "description": "Print to stderr"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.output.contains("error"));
    }

    #[tokio::test]
    #[cfg_attr(windows, ignore)]
    async fn test_command_timeout() {
        let tool = BashTool;
        let ctx = test_context();

        let result = tool
            .execute(
                json!({
                    "command": "sleep 10",
                    "description": "Sleep for 10 seconds",
                    "timeout": 100  // 100ms timeout
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn test_empty_command() {
        let tool = BashTool;
        let ctx = test_context();

        let result = tool
            .execute(
                json!({
                    "command": "   ",
                    "description": "Empty command"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    #[cfg_attr(windows, ignore)]
    async fn test_workdir() {
        let tool = BashTool;
        let ctx = test_context();

        let result = tool
            .execute(
                json!({
                    "command": "pwd",
                    "description": "Print working directory",
                    "workdir": "/tmp"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(
            result.output.trim().contains("/tmp") || result.output.trim().contains("/private/tmp")
        );
    }

    #[test]
    fn test_truncate_output() {
        let short = "hello";
        let (result, truncated) = truncate_output(short, 1000);
        assert_eq!(result, "hello");
        assert!(!truncated);

        let long = "x".repeat(50000);
        let (result, truncated) = truncate_output(&long, 1000);
        assert!(result.len() < long.len());
        assert!(truncated);
        assert!(result.contains("[truncated"));
    }

    #[test]
    fn test_truncate_command() {
        assert_eq!(truncate_command("echo hello"), "echo hello");
        assert_eq!(
            truncate_command(
                "echo hello world this is a very long command that should be truncated"
            ),
            "echo hello world this is a very long command th..."
        );
        assert_eq!(truncate_command("line1\nline2\nline3"), "line1");
    }
}
