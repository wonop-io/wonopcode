//! Web server and MCP-related functions.
//!
//! This module contains functionality for running the wonopcode web server in headless mode,
//! as well as MCP (Model Context Protocol) server setup for tool execution over HTTP/SSE.

use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

/// Wrapper for executing tools through the MCP interface.
///
/// This wrapper handles permission checks, sandbox integration, and tool execution
/// for MCP HTTP requests.
struct ToolExecutorWrapper {
    tool: Arc<dyn wonopcode_tools::Tool>,
    snapshot: Option<Arc<wonopcode_snapshot::SnapshotStore>>,
    file_time: Arc<wonopcode_util::FileTimeState>,
    cancel: tokio_util::sync::CancellationToken,
    permissions: Arc<wonopcode_core::permission::PermissionManager>,
}

#[async_trait::async_trait]
impl wonopcode_mcp::McpToolExecutor for ToolExecutorWrapper {
    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &wonopcode_mcp::McpToolContext,
    ) -> Result<String, String> {
        use wonopcode_tools::ToolContext;

        let tool_name = self.tool.id();

        // Extract path from args for file-related tools
        let path = args
            .get("filePath")
            .or_else(|| args.get("path"))
            .or_else(|| args.get("file"))
            .and_then(|v| v.as_str())
            .map(String::from);

        // Check permission - this will prompt the user if needed via the shared Bus.
        // When using a shared permission manager with a TUI, "ask" rules will send
        // a permission request to the TUI and wait for user response.
        // When sandbox is running, all tools are allowed.
        // Note: We check sandbox state from the permission manager rather than self.sandbox
        // because the sandbox may be started after the MCP server is created.
        let has_sandbox = self.permissions.is_sandbox_running();
        let check = wonopcode_core::permission::PermissionCheck {
            id: uuid::Uuid::new_v4().to_string(),
            tool: tool_name.to_string(),
            action: "execute".to_string(),
            path: path.clone(),
            description: format!("Execute tool: {tool_name}"),
            details: args.clone(),
        };

        let allowed = self
            .permissions
            .check_with_sandbox(&ctx.session_id, check, has_sandbox)
            .await;

        if !allowed {
            return Err(format!("Permission denied for tool '{tool_name}'."));
        }

        // Get sandbox runtime from permission manager if sandbox is running
        let sandbox: Option<Arc<dyn wonopcode_sandbox::SandboxRuntime>> = if has_sandbox {
            self.permissions
                .sandbox_runtime_any()
                .await
                .and_then(|any| {
                    any.downcast::<crate::runner::SandboxRuntimeWrapper>()
                        .ok()
                        .map(|wrapper| wrapper.0.clone())
                })
        } else {
            None
        };

        // Create tool context
        let tool_ctx = ToolContext {
            session_id: ctx.session_id.clone(),
            message_id: "mcp".to_string(),
            agent: "mcp-http".to_string(),
            abort: self.cancel.clone(),
            root_dir: ctx.root_dir.clone(),
            cwd: ctx.cwd.clone(),
            snapshot: self.snapshot.clone(),
            file_time: Some(self.file_time.clone()),
            sandbox,
            event_tx: None, // MCP HTTP doesn't need event_tx
        };

        tracing::info!(
            tool = %tool_name,
            path = ?path,
            has_sandbox = has_sandbox,
            "MCP HTTP tool execution"
        );

        let _timing = wonopcode_util::TimingGuard::mcp_tool(tool_name);
        match self.tool.execute(args, &tool_ctx).await {
            Ok(output) => {
                // Truncate very long outputs
                let mut text = if output.output.len() > 50000 {
                    format!(
                        "{}\n\n... [Output truncated: {} chars total]",
                        &output.output[..50000],
                        output.output.len()
                    )
                } else {
                    output.output
                };

                // Add sandbox indicator for bash tool
                if has_sandbox && tool_name == "bash" {
                    text = format!("[sandbox] {text}");
                }

                // For file-modifying tools, append metadata as JSON so the TUI can parse it
                if !output.metadata.is_null()
                    && matches!(tool_name, "edit" | "write" | "multiedit" | "patch")
                {
                    text = format!("{}\n\n<!-- TOOL_METADATA: {} -->", text, output.metadata);
                }

                Ok(text)
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Start a background HTTP server for MCP tools.
///
/// This starts an HTTP server on a random available port that serves only the MCP endpoints.
/// Returns the MCP SSE URL and a server handle that can be used to shutdown the server.
///
/// # Arguments
/// * `cwd` - Working directory for the MCP server
/// * `shared_permission_manager` - Shared permission manager for tool authorization.
///   If provided, permission requests will be sent via its Bus to the TUI for user prompts.
///
/// # Returns
/// A tuple of (mcp_sse_url, server_handle) where:
/// - `mcp_sse_url` is the URL to use for MCP connections (e.g., "http://127.0.0.1:12345/mcp/sse")
/// - `server_handle` is a tokio task handle for the server (can be aborted to shutdown)
pub async fn start_mcp_server(
    cwd: &std::path::Path,
    shared_permission_manager: Arc<wonopcode_core::PermissionManager>,
) -> anyhow::Result<(String, tokio::task::JoinHandle<()>)> {
    use axum::Router;
    use wonopcode_mcp::create_mcp_router;

    // Bind to a random available port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let local_addr = listener.local_addr()?;

    info!(address = %local_addr, "Starting background MCP HTTP server");

    // Build the URL for the MCP message endpoint
    let mcp_message_url = format!("http://{local_addr}/mcp/message");

    // Create MCP state with shared permission manager
    let mcp_state =
        create_mcp_http_state(cwd, &mcp_message_url, Some(shared_permission_manager)).await?;

    // Create router with just MCP endpoints (no CORS needed for localhost)
    let mcp_router = create_mcp_router(mcp_state);
    let app = Router::new().nest("/mcp", mcp_router);

    // Build the SSE URL to return
    let mcp_sse_url = format!("http://{local_addr}/mcp/sse");

    // Spawn the server in the background
    let server_handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(error = %e, "MCP HTTP server error");
        }
        info!("MCP HTTP server shutdown");
    });

    info!(mcp_url = %mcp_sse_url, "MCP HTTP server started");

    Ok((mcp_sse_url, server_handle))
}

/// Create MCP HTTP state for the headless server.
///
/// This sets up the MCP tools to be served over HTTP/SSE instead of stdio,
/// allowing Claude CLI to connect to the headless server.
///
/// # Arguments
/// * `cwd` - Working directory for tools
/// * `message_url` - URL for MCP message endpoint
/// * `permission_manager` - Optional shared permission manager. If None, creates a standalone one that allows all.
#[allow(clippy::cognitive_complexity)]
pub async fn create_mcp_http_state(
    cwd: &std::path::Path,
    message_url: &str,
    permission_manager: Option<std::sync::Arc<wonopcode_core::PermissionManager>>,
) -> anyhow::Result<wonopcode_mcp::McpHttpState> {
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;
    use wonopcode_core::bus::Bus;
    use wonopcode_core::permission::PermissionManager;
    use wonopcode_mcp::McpToolContext;
    use wonopcode_snapshot::{SnapshotConfig, SnapshotStore};
    use wonopcode_tools::ToolRegistry;
    use wonopcode_util::FileTimeState;

    let session_id = "headless-mcp".to_string();
    let root_dir = cwd.to_path_buf();

    // Create tool context
    let mcp_context = McpToolContext {
        session_id: session_id.clone(),
        cwd: cwd.to_path_buf(),
        root_dir: root_dir.clone(),
    };

    // Use provided permission manager or create a standalone one for headless mode
    let permission_manager = if let Some(pm) = permission_manager {
        // Use shared permission manager - rules are already loaded
        // Permission checks will use the shared Bus to prompt users
        pm
    } else {
        // Create standalone permission manager for headless mode (no TUI)
        // This allows all operations since there's no UI to prompt users
        let bus = Bus::new();
        let pm = Arc::new(PermissionManager::new(bus));

        // Add default rules
        for rule in PermissionManager::default_rules() {
            pm.add_rule(rule).await;
        }

        // Allow all dangerous tools in standalone headless mode
        // (no TUI means we can't prompt users, so must allow)
        for rule in PermissionManager::sandbox_allow_all_rules() {
            pm.add_rule(rule).await;
        }

        pm
    };

    // Initialize snapshot store
    let snapshot_dir = root_dir.join(".wonopcode").join("snapshots");
    let snapshot_store =
        SnapshotStore::new(snapshot_dir, root_dir.clone(), SnapshotConfig::default())
            .await
            .ok()
            .map(Arc::new);

    // Initialize file time tracker
    let file_time = Arc::new(FileTimeState::new());

    // Use shared file todo store
    let todo_store: Arc<dyn wonopcode_tools::todo::TodoStore> = if let Some(store) =
        wonopcode_tools::todo::SharedFileTodoStore::from_env()
    {
        tracing::info!(
            path = %store.path().display(),
            "MCP HTTP using SharedFileTodoStore"
        );
        Arc::new(store)
    } else {
        tracing::warn!("WONOPCODE_TODO_FILE not set, MCP HTTP using InMemoryTodoStore - todos will NOT sync with TUI!");
        Arc::new(wonopcode_tools::todo::InMemoryTodoStore::new())
    };

    // Create tool registry with all tools
    let mut tools = ToolRegistry::with_builtins();
    tools.register(Arc::new(wonopcode_tools::bash::BashTool));
    tools.register(Arc::new(wonopcode_tools::webfetch::WebFetchTool));
    tools.register(Arc::new(wonopcode_tools::todo::TodoWriteTool::new(
        todo_store.clone(),
    )));
    tools.register(Arc::new(wonopcode_tools::todo::TodoReadTool::new(
        todo_store,
    )));
    tools.register(Arc::new(wonopcode_tools::lsp::LspTool::new()));

    // Build MCP server tools map
    let mut mcp_tools = std::collections::HashMap::new();
    let cancel = CancellationToken::new();

    for tool in tools.all() {
        let tool_clone = tool.clone();
        let snapshot = snapshot_store.clone();
        let ft = file_time.clone();
        let cancel_clone = cancel.clone();
        let perm = permission_manager.clone();

        let executor = ToolExecutorWrapper {
            tool: tool_clone,
            snapshot,
            file_time: ft,
            cancel: cancel_clone,
            permissions: perm,
        };

        mcp_tools.insert(
            tool.id().to_string(),
            wonopcode_mcp::McpServerTool {
                name: tool.id().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters_schema(),
                executor: Arc::new(executor),
            },
        );
    }

    info!(
        tools = mcp_tools.len(),
        message_url = %message_url,
        "Created MCP HTTP state"
    );

    Ok(wonopcode_mcp::McpHttpState::new(
        "wonopcode-tools",
        env!("CARGO_PKG_VERSION"),
        mcp_tools,
        mcp_context,
        message_url,
    ))
}

/// Run web server (headless mode).
///
/// Starts the wonopcode web server on the specified address, optionally opening
/// a browser to the web interface.
///
/// # Arguments
/// * `address` - Socket address to bind the server to
/// * `open_browser` - Whether to automatically open a browser to the web interface
/// * `cwd` - Current working directory for the server instance
pub async fn run_web_server(
    address: SocketAddr,
    open_browser: bool,
    cwd: &std::path::Path,
) -> anyhow::Result<()> {
    println!();
    println!("  ╭─────────────────────────────────────╮");
    println!("  │           Wonopcode Web             │");
    println!("  ╰─────────────────────────────────────╯");
    println!();

    // Create instance
    let instance = wonopcode_core::Instance::new(cwd).await?;
    let bus = instance.bus().clone();

    // Create server state
    let state = wonopcode_server::AppState::new(instance, bus);

    // Create router
    let app = wonopcode_server::create_router(state);

    // Determine URLs to display
    let display_url = if address.ip().is_unspecified() {
        // Show localhost for local access
        let local_url = format!("http://localhost:{}", address.port());
        println!("  Local access:      {local_url}");

        // Try to find network IPs
        if let Ok(interfaces) = get_network_ips() {
            for ip in interfaces {
                println!("  Network access:    http://{}:{}", ip, address.port());
            }
        }
        local_url
    } else {
        let url = format!("http://{address}");
        println!("  Web interface:     {url}");
        url
    };

    println!();
    println!("  Press Ctrl+C to stop the server");
    println!();

    // Open browser if requested
    if open_browser {
        let url_clone = display_url.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let _ = open::that(url_clone);
        });
    }

    // Start server
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Get network IP addresses for display.
///
/// Attempts to discover local network IP addresses on the current machine
/// for displaying in the web server output. This helps users access the
/// server from other devices on the network.
///
/// # Returns
/// A vector of IP address strings, or an IO error if discovery fails.
#[allow(unused_mut)]
pub fn get_network_ips() -> std::io::Result<Vec<String>> {
    let mut ips = Vec::new();

    // On Unix, we can try to get interfaces
    #[cfg(unix)]
    {
        if let Ok(output) = std::process::Command::new("hostname").arg("-I").output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for ip in stdout.split_whitespace() {
                    // Skip IPv6 and internal addresses
                    if !ip.contains(':') && !ip.starts_with("127.") && !ip.starts_with("172.") {
                        ips.push(ip.to_string());
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if ips.is_empty() {
            if let Ok(output) = std::process::Command::new("ipconfig")
                .arg("getifaddr")
                .arg("en0")
                .output()
            {
                if output.status.success() {
                    let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !ip.is_empty() {
                        ips.push(ip);
                    }
                }
            }
        }
    }

    Ok(ips)
}
