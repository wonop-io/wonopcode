//! Run command handlers.
//!
//! Handles the execution of single prompts in non-interactive mode,
//! as well as headless server mode for remote operation.

use crate::runner::{Runner, RunnerConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

/// Run the wonopcode server in headless mode.
///
/// Starts an HTTP server that can be accessed remotely, allowing
/// clients to connect and interact with the AI assistant.
///
/// # Arguments
/// * `address` - The socket address to bind to
/// * `cwd` - The current working directory for the server
pub async fn run_server(address: SocketAddr, cwd: &std::path::Path) -> anyhow::Result<()> {
    info!("Starting wonopcode server on {}", address);

    // Create instance
    let instance = wonopcode_core::Instance::new(cwd).await?;
    let bus = instance.bus().clone();

    // Create server state
    let state = wonopcode_server::AppState::new(instance, bus);

    // Create router
    let app = wonopcode_server::create_router(state);

    // Start server
    let listener = tokio::net::TcpListener::bind(address).await?;
    info!("Server listening on http://{}", address);

    axum::serve(listener, app).await?;

    Ok(())
}

/// Run a single command and exit (non-interactive mode).
///
/// This function executes a single prompt, prints the response, and exits.
/// It's designed for scripting and automation use cases.
///
/// # Arguments
/// * `cwd` - The current working directory
/// * `message` - The message parts to join as the prompt
/// * `model` - Optional model specification (provider/model format)
/// * `_continue_session` - Whether to continue the last session (currently unused)
/// * `_session` - Optional session ID to resume (currently unused)
/// * `format` - Output format ("json" or plain text)
/// * `default_provider` - Default provider to use if not specified in model
/// * `cli_secret` - Optional API secret for server authentication
#[allow(clippy::too_many_arguments)]
#[allow(clippy::cognitive_complexity)]
pub async fn run_command(
    cwd: &std::path::Path,
    message: Vec<String>,
    model: Option<String>,
    _continue_session: bool,
    _session: Option<String>,
    format: &str,
    default_provider: String,
    cli_secret: Option<String>,
) -> anyhow::Result<()> {
    use std::io::{self, Write};

    // Join message parts
    let prompt = if message.is_empty() {
        // Read from stdin if no message provided
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        input.trim().to_string()
    } else {
        message.join(" ")
    };

    if prompt.is_empty() {
        eprintln!("Error: No message provided");
        return Ok(());
    }

    // Create instance
    let instance = wonopcode_core::Instance::new(cwd).await?;

    // Parse model specification (provider/model format)
    let (provider, model_id) = if let Some(ref m) = model {
        tracing::debug!(model_spec = %m, default_provider = %default_provider, "Parsing model spec");
        super::model::parse_model_spec(m, &default_provider)
    } else {
        tracing::debug!(default_provider = %default_provider, "Using default provider");
        (
            default_provider.clone(),
            super::model::get_default_model(&default_provider),
        )
    };
    tracing::debug!(provider = %provider, model_id = %model_id, "Using provider and model");

    // Load API key (may be empty for CLI-based auth)
    let api_key = crate::runner::load_api_key(&provider).unwrap_or_default();

    // Check if we have authentication
    if api_key.is_empty() {
        use wonopcode_provider::claude_cli::ClaudeCliProvider;

        // For Anthropic, allow CLI-based subscription auth
        if provider != "anthropic"
            || !ClaudeCliProvider::is_available()
            || !ClaudeCliProvider::is_authenticated()
        {
            eprintln!("Error: No API key found for provider '{provider}'");
            eprintln!("Run: wonopcode auth login {provider}");
            return Ok(());
        }
    }

    // Load config before starting MCP server (needed for permission config)
    let core_config = instance.config().await;

    // Create shared Bus and PermissionManager for MCP server and Runner
    // For 'run' command (non-interactive), we allow all operations since there's no TUI to prompt
    let shared_bus = wonopcode_core::bus::Bus::new();
    let shared_permission_manager =
        Arc::new(wonopcode_core::PermissionManager::new(shared_bus.clone()));

    // Initialize permission rules (allow-all for non-interactive mode)
    for rule in wonopcode_core::PermissionManager::default_rules() {
        shared_permission_manager.add_rule(rule).await;
    }
    // Allow all dangerous tools since we can't prompt the user
    for rule in wonopcode_core::PermissionManager::sandbox_allow_all_rules() {
        shared_permission_manager.add_rule(rule).await;
    }

    // Initialize shared todo storage early so MCP server and Runner use the same store
    let todo_path = wonopcode_tools::todo::SharedFileTodoStore::init_env();
    info!(path = %todo_path.display(), "Initialized shared todo storage");

    // Get API key for MCP server authentication
    // Priority: CLI arg > environment variable > config file
    let secret = cli_secret
        .or_else(|| std::env::var("WONOPCODE_SECRET").ok())
        .or_else(|| core_config.server.as_ref().and_then(|s| s.api_key.clone()));

    // Start background MCP HTTP server for Claude CLI integration
    let (mcp_url, mcp_server_handle) = match super::start_mcp_server(
        cwd,
        shared_permission_manager.clone(),
    )
    .await
    {
        Ok((url, handle)) => (Some(url), Some(handle)),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to start MCP server, Claude CLI will not use custom tools");
            (None, None)
        }
    };

    // Create runner config
    let allow_all_in_sandbox = core_config
        .permission
        .as_ref()
        .and_then(|p| p.allow_all_in_sandbox)
        .unwrap_or(true);
    let config = RunnerConfig {
        provider: provider.clone(),
        model_id: model_id.clone(),
        api_key,
        system_prompt: None,
        max_tokens: Some(8192),
        temperature: Some(0.7),
        doom_loop: wonopcode_core::permission::Decision::Ask,
        test_provider_settings: None,
        allow_all: false,
        allow_all_in_sandbox,
        mcp_url, // Use background MCP server for custom tools
        mcp_secret: secret,
        external_mcp_servers: std::collections::HashMap::new(),
    };

    // Create runner with shared permission manager (allow-all for non-interactive mode)
    let runner = match Runner::new_with_shared(
        config.clone(),
        instance.clone(),
        None,
        Some(shared_bus),
        Some(shared_permission_manager),
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error creating runner: {e}");
            return Ok(());
        }
    };

    // Create channels
    let (action_tx, action_rx) = tokio::sync::mpsc::unbounded_channel();
    let (update_tx, mut update_rx) = tokio::sync::mpsc::unbounded_channel();

    // Spawn runner
    let runner_handle = tokio::spawn(async move {
        runner.run(action_rx, update_tx).await;
    });

    // Send prompt
    let _ = action_tx.send(wonopcode_tui::AppAction::SendPrompt(prompt));

    // Collect response
    let is_json = format == "json";
    let mut response_text = String::new();

    while let Some(update) = update_rx.recv().await {
        match update {
            wonopcode_tui::AppUpdate::TextDelta(delta) => {
                if !is_json {
                    print!("{delta}");
                    io::stdout().flush()?;
                }
                response_text.push_str(&delta);
            }
            wonopcode_tui::AppUpdate::ToolStarted { name, id, input } => {
                if is_json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "type": "tool_start",
                            "name": name,
                            "id": id,
                            "input": input
                        })
                    );
                }
            }
            wonopcode_tui::AppUpdate::ToolCompleted {
                id,
                success,
                output,
                metadata,
            } => {
                if is_json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "type": "tool_result",
                            "id": id,
                            "success": success,
                            "output": output,
                            "metadata": metadata
                        })
                    );
                }
            }
            wonopcode_tui::AppUpdate::Completed { text } => {
                if is_json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "type": "response",
                            "text": text
                        })
                    );
                } else if response_text.is_empty() {
                    println!("{text}");
                }
                break;
            }
            wonopcode_tui::AppUpdate::Error(e) => {
                if is_json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "type": "error",
                            "message": e
                        })
                    );
                } else {
                    eprintln!("\nError: {e}");
                }
                break;
            }
            _ => {}
        }
    }

    if !is_json && !response_text.is_empty() {
        println!(); // Final newline
    }

    // Shutdown
    let _ = action_tx.send(wonopcode_tui::AppAction::Quit);
    // Shutdown MCP server
    if let Some(handle) = mcp_server_handle {
        handle.abort();
    }

    runner_handle.abort();
    instance.dispose().await;

    Ok(())
}
