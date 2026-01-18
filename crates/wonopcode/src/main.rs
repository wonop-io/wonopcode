//! Wonopcode - AI-powered coding assistant.
//!
//! This is the main entry point for the wonopcode CLI.
// @ace:implements COMP-T90R9Q-UR4

mod commands;
mod compaction;
#[cfg(feature = "github")]
mod github;
mod publish;
mod runner;
mod stats;
mod upgrade;

// Re-export command types for use in Commands enum
use commands::{
    create_mcp_http_state, parse_model_spec, parse_release_channel, start_mcp_server,
    AgentCommands, AuthCommands, McpCommands, SessionCommands,
};

use clap::{Parser, Subcommand};
use runner::{Runner, RunnerConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Parser)]
#[command(name = "wonopcode")]
#[command(author, version, about = "AI-powered coding assistant", long_about = None)]
struct Cli {
    /// Run in basic mode (no TUI)
    #[arg(long)]
    basic: bool,

    /// Prompt to send immediately
    #[arg(short, long)]
    prompt: Option<String>,

    /// Continue the last session
    #[arg(short, long, name = "continue")]
    continue_session: bool,

    /// Resume a specific session
    #[arg(short, long)]
    resume: Option<String>,

    /// Print output as JSON
    #[arg(long)]
    json: bool,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Provider to use (anthropic, openai, openrouter)
    #[arg(long, default_value = "anthropic")]
    provider: String,

    /// Model ID to use
    #[arg(long, short)]
    model: Option<String>,

    /// Run in headless mode (server only, no TUI)
    #[arg(long)]
    headless: bool,

    /// Address to bind to in headless mode
    #[arg(long, default_value = "127.0.0.1:3000")]
    address: std::net::SocketAddr,

    /// Connect to a remote headless server
    #[arg(long)]
    connect: Option<String>,

    /// Secret key for server authentication.
    /// When set, clients must provide this key via X-API-Key header or Authorization: Bearer header.
    /// Can also be set via WONOPCODE_SECRET environment variable.
    #[arg(long)]
    secret: Option<String>,

    /// Project ID (e.g., organization project identifier) for tracking.
    /// This is stored in the agent state and can be queried via /info endpoint.
    #[arg(long)]
    project_id: Option<String>,

    /// Work ID (e.g., ticket ID, issue number) for tracking.
    /// This is stored in the agent state and can be queried via /info endpoint.
    #[arg(long)]
    work_id: Option<String>,

    /// Advertise the server via mDNS for local network discovery (headless mode only).
    #[cfg(feature = "discover")]
    #[arg(long)]
    advertise: bool,

    /// Custom name for mDNS advertisement (default: hostname).
    #[cfg(feature = "discover")]
    #[arg(long)]
    name: Option<String>,

    /// Discover servers on the local network via mDNS instead of specifying an address.
    #[cfg(feature = "discover")]
    #[arg(long, conflicts_with = "connect")]
    discover: bool,

    /// Subcommand
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run with a message (non-interactive)
    Run {
        /// Model to use (provider/model format)
        #[arg(short, long)]
        model: Option<String>,
        /// Continue the last session
        #[arg(short, long)]
        continue_session: bool,
        /// Session ID to continue
        #[arg(short, long)]
        session: Option<String>,
        /// Output format: default or json
        #[arg(long, default_value = "default")]
        format: String,
        /// Message to send
        #[arg(num_args = 0..)]
        message: Vec<String>,
    },
    /// Start the HTTP server
    Serve {
        /// Address to bind to
        #[arg(short, long, default_value = "127.0.0.1:3000")]
        address: SocketAddr,
    },
    /// List available models
    Models,
    /// Show configuration
    Config,
    /// Print version information
    Version,
    /// Authenticate with a provider
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },
    /// Manage sessions
    Session {
        #[command(subcommand)]
        command: SessionCommands,
    },
    /// Export session(s) to a file
    Export {
        /// Session ID to export (exports all if not specified)
        #[arg(short, long)]
        session: Option<String>,
        /// Output file path
        #[arg(short, long)]
        output: std::path::PathBuf,
        /// Export format (json or markdown)
        #[arg(short, long, default_value = "json")]
        format: String,
    },
    /// Import session(s) from a file
    Import {
        /// Input file path
        #[arg(short, long)]
        input: std::path::PathBuf,
    },
    /// Start ACP (Agent Client Protocol) server for IDE integration
    Acp {
        /// Working directory
        #[arg(short, long)]
        cwd: Option<std::path::PathBuf>,
    },
    /// GitHub integration commands (requires --features github)
    #[cfg(feature = "github")]
    Github {
        #[command(subcommand)]
        command: GithubCommands,
    },
    /// Checkout a GitHub PR (requires --features github)
    #[cfg(feature = "github")]
    Pr {
        /// PR number to checkout
        number: u64,
    },
    /// Show token usage and cost statistics
    Stats {
        /// Show stats for the last N days (default: all time)
        #[arg(short, long)]
        days: Option<u32>,
        /// Number of tools to show (default: all)
        #[arg(short, long)]
        tools: Option<usize>,
        /// Filter by project (empty string for current project)
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Start web UI server (headless mode)
    Web {
        /// Address to bind to
        #[arg(short, long, default_value = "127.0.0.1:3000")]
        address: SocketAddr,
        /// Open browser automatically
        #[arg(long, default_value = "true")]
        open: bool,
    },
    /// Manage MCP servers
    Mcp {
        #[command(subcommand)]
        command: McpCommands,
    },
    /// Check for available updates
    Check {
        /// Release channel (stable, beta, nightly)
        #[arg(short, long)]
        channel: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Upgrade to the latest version
    Upgrade {
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
        /// Release channel (stable, beta, nightly)
        #[arg(short, long)]
        channel: Option<String>,
        /// Install a specific version
        #[arg(long)]
        version: Option<String>,
        /// Force reinstall even if up to date
        #[arg(long)]
        force: bool,
    },
    /// Publish a new release (for maintainers)
    Publish {
        /// Perform a dry run without creating the release
        #[arg(long)]
        dry_run: bool,
        /// GitHub token (or use GITHUB_TOKEN env var)
        #[arg(long)]
        token: Option<String>,
        /// Release channel (stable, beta, nightly)
        #[arg(short, long)]
        channel: Option<String>,
        /// Release notes (reads from CHANGELOG.md if not specified)
        #[arg(long)]
        notes: Option<String>,
    },
    /// List available agents
    Agent {
        #[command(subcommand)]
        command: AgentCommands,
    },
}

// AuthCommands, SessionCommands, and McpCommands are defined in commands module

#[cfg(feature = "github")]
#[derive(Subcommand)]
enum GithubCommands {
    /// Install GitHub integration
    Install,
    /// Run GitHub agent (for CI/GitHub Actions)
    Run {
        /// Path to GitHub event JSON
        #[arg(long)]
        event: Option<String>,
        /// GitHub token
        #[arg(long)]
        token: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging and get log file path
    // In headless mode, log to stdout instead of file
    let log_file = commands::init_logging(cli.verbose, cli.headless);

    // Initialize performance logging to separate file
    match wonopcode_util::perf::init() {
        Ok(perf_log_path) => {
            tracing::debug!(path = %perf_log_path.display(), "Performance logging initialized");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to initialize performance logging");
        }
    }

    // Get current directory
    let cwd = std::env::current_dir()?;

    // Handle subcommands
    let result = match cli.command {
        Some(Commands::Run {
            message,
            model,
            continue_session,
            session,
            format,
        }) => {
            commands::run_command(
                &cwd,
                message,
                model,
                continue_session,
                session,
                &format,
                cli.provider.clone(),
                cli.secret.clone(),
            )
            .await
        }
        Some(Commands::Serve { address }) => commands::run_server(address, &cwd).await,
        Some(Commands::Models) => {
            commands::list_models();
            Ok(())
        }
        Some(Commands::Config) => show_config(&cwd).await,
        Some(Commands::Version) => {
            print_version();
            Ok(())
        }
        Some(Commands::Auth { command }) => commands::handle_auth(command).await,
        Some(Commands::Session { command }) => commands::handle_session(command, &cwd).await,
        Some(Commands::Export {
            session,
            output,
            format,
        }) => commands::handle_export(&cwd, session, output, &format).await,
        Some(Commands::Import { input }) => commands::handle_import(&cwd, input).await,
        Some(Commands::Acp { cwd: acp_cwd }) => {
            let working_dir = acp_cwd.unwrap_or_else(|| cwd.clone());
            run_acp(&working_dir).await
        }
        #[cfg(feature = "github")]
        Some(Commands::Github { command }) => handle_github(command, &cwd).await,
        #[cfg(feature = "github")]
        Some(Commands::Pr { number }) => handle_pr(number).await,
        Some(Commands::Stats {
            days,
            tools,
            project,
        }) => handle_stats(&cwd, days, tools, project).await,
        Some(Commands::Web { address, open }) => {
            commands::run_web_server(address, open, &cwd).await
        }
        Some(Commands::Mcp { command }) => commands::handle_mcp(command, &cwd).await,
        Some(Commands::Check { channel, json }) => {
            let channel = channel.and_then(|s| commands::parse_release_channel(&s));
            upgrade::handle_check(channel, json).await
        }
        Some(Commands::Upgrade {
            yes,
            channel,
            version,
            force,
        }) => {
            let channel = channel.and_then(|s| commands::parse_release_channel(&s));
            upgrade::handle_upgrade(yes, channel, version, force).await
        }
        Some(Commands::Publish {
            dry_run,
            token,
            channel,
            notes,
        }) => {
            let channel = channel
                .and_then(|s| parse_release_channel(&s))
                .unwrap_or_default();
            publish::handle_publish(publish::PublishOptions {
                dry_run,
                token,
                channel,
                notes,
            })
            .await
        }
        Some(Commands::Agent { command }) => commands::handle_agent(command, &cwd).await,
        None => {
            // Check for headless, discover, or connect mode
            if cli.headless {
                run_headless(&cwd, cli.address, &cli).await
            } else if let Some(ref address) = cli.connect {
                run_connect(address, &cli).await
            } else {
                #[cfg(feature = "discover")]
                if cli.discover {
                    return run_discover(&cli).await;
                }
                // Default behavior: interactive mode
                run_interactive(&cwd, cli).await
            }
        }
    };

    // Print log file location on exit (for TUI mode)
    if let Some(path) = log_file {
        eprintln!("Logs: {}", path.display());
    }

    result
}

/// Initialize logging based on verbosity and mode.
/// In headless mode, logs are written to stdout.
/// Otherwise, logs are written to a file in the standard log directory.
/// Returns the log file path if logging to file.
async fn show_config(cwd: &std::path::Path) -> anyhow::Result<()> {
    let (config, sources) = wonopcode_core::config::Config::load(Some(cwd)).await?;

    println!("Configuration sources:");
    if sources.is_empty() {
        println!("  (none)");
    } else {
        for source in &sources {
            println!("  {}", source.display());
        }
    }
    println!();

    println!("Current configuration:");
    println!("{}", serde_json::to_string_pretty(&config)?);

    Ok(())
}

/// Print version information.
fn print_version() {
    println!("wonopcode {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("An AI-powered coding assistant for the terminal.");
    println!();
    println!("https://github.com/wonop-io/wonopcode");
}

/// Run interactive mode.
#[allow(clippy::cognitive_complexity)]
async fn run_interactive(cwd: &std::path::Path, cli: Cli) -> anyhow::Result<()> {
    // Initialize shared todo storage early so MCP server and Runner use the same store
    let todo_path = wonopcode_tools::todo::SharedFileTodoStore::init_env();
    info!(path = %todo_path.display(), "Initialized shared todo storage");

    // Check for updates on startup (non-blocking)
    let update_notification = {
        let cwd = cwd.to_path_buf();
        tokio::spawn(async move { upgrade::check_updates_on_startup(&cwd).await })
    };

    // Create instance (this already loads config internally)
    let instance = wonopcode_core::Instance::new(cwd).await?;

    // Get config from instance (avoid duplicate loading)
    let config_file = instance.config().await;

    // Determine provider and model
    // Priority: CLI > config file > recently used > defaults
    let model_state = wonopcode_tui::ModelState::load();

    let (provider, model_id) = if let Some(ref model_spec) = cli.model {
        // 1. CLI argument takes priority
        parse_model_spec(model_spec, &cli.provider)
    } else if let Some(ref model_spec) = config_file.model {
        // 2. Config file model
        parse_model_spec(model_spec, &cli.provider)
    } else if let Some(recent) = model_state.most_recent() {
        // 3. Most recently used model
        parse_model_spec(recent, &cli.provider)
    } else {
        // 4. Provider defaults
        let model_id = match cli.provider.as_str() {
            "anthropic" => "claude-sonnet-4-5-20250929".to_string(),
            "openai" => "gpt-4o".to_string(),
            "openrouter" => "anthropic/claude-sonnet-4-5".to_string(),
            _ => "claude-sonnet-4-5-20250929".to_string(),
        };
        (cli.provider.clone(), model_id)
    };

    // Load API key (may be empty for CLI-based auth)
    let api_key = runner::load_api_key(&provider).unwrap_or_default();

    // Log authentication status (but don't block startup)
    if api_key.is_empty() {
        use wonopcode_provider::claude_cli::ClaudeCliProvider;

        // For Anthropic, check if CLI-based subscription auth is available
        if provider == "anthropic"
            && ClaudeCliProvider::is_available()
            && ClaudeCliProvider::is_authenticated()
        {
            info!("Using Claude CLI subscription for authentication");
        } else {
            // No auth configured - app will still start, user can configure via /models or /connect
            info!(provider = %provider, "No API key configured - user can set up auth via UI");
        }
    }

    // Create shared Bus and PermissionManager for MCP server and Runner
    // This allows permission requests from MCP tools to reach the TUI for user prompts
    let shared_bus = wonopcode_core::bus::Bus::new();
    let shared_permission_manager =
        Arc::new(wonopcode_core::PermissionManager::new(shared_bus.clone()));

    // Initialize default permission rules
    for rule in wonopcode_core::PermissionManager::default_rules() {
        shared_permission_manager.add_rule(rule).await;
    }

    // Load permission rules from config (these take precedence over defaults)
    if let Some(perm_config) = &config_file.permission {
        for rule in wonopcode_core::PermissionManager::rules_from_config(perm_config) {
            shared_permission_manager.add_rule(rule).await;
        }
        info!("Permission manager initialized with default rules and config-based rules");
    } else {
        info!("Permission manager initialized with default rules");
    }

    // Start background MCP HTTP server for Claude CLI integration
    let (mcp_url, mcp_server_handle) = match start_mcp_server(
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
    let allow_all_in_sandbox = config_file
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
        mcp_url,          // Use background MCP server for custom tools
        mcp_secret: None, // No auth needed for local MCP server in TUI mode
        external_mcp_servers: std::collections::HashMap::new(), // Populated by Runner from mcp_configs
    };

    // Get MCP config from config file
    let mcp_configs = config_file.mcp.clone();

    // Check if update notification is ready (with timeout)
    let update_msg = tokio::time::timeout(std::time::Duration::from_secs(2), update_notification)
        .await
        .ok()
        .and_then(|r| r.ok())
        .flatten();

    if cli.basic {
        // Basic mode: simple line-based input (no TUI to prompt, need allow-all for dangerous tools)
        // Show update notification if available
        if let Some(ref msg) = update_msg {
            eprintln!("{msg}");
            eprintln!();
        }
        // In basic mode, add allow-all rules since we can't prompt
        for rule in wonopcode_core::PermissionManager::sandbox_allow_all_rules() {
            shared_permission_manager.add_rule(rule).await;
        }
        run_basic_mode(
            &instance,
            &config,
            cli.prompt,
            mcp_configs,
            shared_bus,
            shared_permission_manager,
        )
        .await?;
    } else {
        // TUI mode - pass shared permission manager for prompting
        run_tui_mode(
            &instance,
            config,
            cli.prompt,
            mcp_configs,
            &config_file,
            update_msg,
            shared_bus,
            shared_permission_manager,
        )
        .await?;
    }

    // Shutdown MCP server
    if let Some(handle) = mcp_server_handle {
        handle.abort();
        // Give it a moment to shutdown gracefully
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Cleanup
    instance.dispose().await;

    Ok(())
}

/// Run in basic mode (no TUI).
async fn run_basic_mode(
    instance: &wonopcode_core::Instance,
    config: &RunnerConfig,
    initial_prompt: Option<String>,
    mcp_configs: Option<std::collections::HashMap<String, wonopcode_core::config::McpConfig>>,
    shared_bus: wonopcode_core::bus::Bus,
    shared_permission_manager: Arc<wonopcode_core::PermissionManager>,
) -> anyhow::Result<()> {
    use std::io::{self, BufRead, Write};

    println!("Wonopcode v{}", env!("CARGO_PKG_VERSION"));
    println!("Working directory: {}", instance.directory().display());
    println!("Provider: {} / {}", config.provider, config.model_id);
    println!();

    // Create runner with shared permission manager
    let runner = match Runner::new_with_shared(
        config.clone(),
        instance.clone(),
        mcp_configs,
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

    // Handle initial prompt or interactive
    if let Some(prompt) = initial_prompt {
        println!("You: {prompt}");
        println!();
        let _ = action_tx.send(wonopcode_tui::AppAction::SendPrompt(prompt));

        // Wait for response
        let mut response_text = String::new();
        while let Some(update) = update_rx.recv().await {
            match update {
                wonopcode_tui::AppUpdate::TextDelta(delta) => {
                    print!("{delta}");
                    io::stdout().flush()?;
                    response_text.push_str(&delta);
                }
                wonopcode_tui::AppUpdate::ToolStarted { name, .. } => {
                    println!("\n[Running tool: {name}]");
                }
                wonopcode_tui::AppUpdate::ToolCompleted { success, .. } => {
                    if success {
                        println!("[Tool completed]");
                    } else {
                        println!("[Tool failed]");
                    }
                }
                wonopcode_tui::AppUpdate::Completed { text } => {
                    if response_text.is_empty() {
                        println!("{text}");
                    }
                    println!();
                    break;
                }
                wonopcode_tui::AppUpdate::Error(e) => {
                    eprintln!("\nError: {e}");
                    break;
                }
                _ => {}
            }
        }
    } else {
        println!("Type your message and press Enter. Type 'exit' or Ctrl+C to quit.");
        println!();

        let stdin = io::stdin();
        loop {
            print!("> ");
            io::stdout().flush()?;

            let mut line = String::new();
            if stdin.lock().read_line(&mut line)? == 0 {
                break;
            }

            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line == "exit" || line == "quit" {
                break;
            }

            let _ = action_tx.send(wonopcode_tui::AppAction::SendPrompt(line.to_string()));

            // Wait for response
            let mut response_text = String::new();
            while let Some(update) = update_rx.recv().await {
                match update {
                    wonopcode_tui::AppUpdate::TextDelta(delta) => {
                        print!("{delta}");
                        io::stdout().flush()?;
                        response_text.push_str(&delta);
                    }
                    wonopcode_tui::AppUpdate::ToolStarted { name, .. } => {
                        println!("\n[Running tool: {name}]");
                    }
                    wonopcode_tui::AppUpdate::ToolCompleted { success, .. } => {
                        if success {
                            println!("[Tool completed]");
                        } else {
                            println!("[Tool failed]");
                        }
                    }
                    wonopcode_tui::AppUpdate::Completed { text } => {
                        if response_text.is_empty() {
                            println!("{text}");
                        }
                        println!();
                        break;
                    }
                    wonopcode_tui::AppUpdate::Error(e) => {
                        eprintln!("\nError: {e}");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    // Shutdown
    let _ = action_tx.send(wonopcode_tui::AppAction::Quit);
    runner_handle.abort();

    Ok(())
}

/// Run in TUI mode.
#[allow(clippy::too_many_arguments)]
async fn run_tui_mode(
    instance: &wonopcode_core::Instance,
    config: RunnerConfig,
    initial_prompt: Option<String>,
    mcp_configs: Option<std::collections::HashMap<String, wonopcode_core::config::McpConfig>>,
    app_config: &wonopcode_core::config::Config,
    update_notification: Option<String>,
    shared_bus: wonopcode_core::bus::Bus,
    shared_permission_manager: Arc<wonopcode_core::PermissionManager>,
) -> anyhow::Result<()> {
    use wonopcode_tui::App;

    let mut app = App::new();

    // Apply saved settings from config (theme, render settings, etc.)
    app.apply_config(app_config);

    // Set project info
    app.set_project(instance.directory().display().to_string());
    app.set_model(format!("{}/{}", config.provider, config.model_id));

    // Show update notification as toast if available
    if let Some(msg) = update_notification {
        app.show_toast(&msg);
    }

    // Take action receiver for processing
    let action_rx = app
        .take_action_rx()
        .ok_or_else(|| anyhow::anyhow!("Action receiver already taken - app state corrupted"))?;
    let update_tx = app.update_sender();

    // Create runner with shared Bus and PermissionManager
    // This allows MCP tools to send permission requests to the TUI for user prompts
    let runner = match Runner::new_with_shared(
        config,
        instance.clone(),
        mcp_configs,
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

    // Spawn runner
    let runner_handle = tokio::spawn(async move {
        runner.run(action_rx, update_tx).await;
    });

    // Add initial prompt if provided
    if let Some(prompt) = initial_prompt {
        app.add_user_message(prompt);
    }

    // Run the TUI
    app.run().await?;

    // Wait for runner to complete (it will cleanup sandbox on exit)
    // The runner exits when action_rx is dropped (which happens when app exits)
    // Give it a short timeout to cleanup
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), runner_handle).await;

    match timeout {
        Ok(_) => {
            info!("Runner shutdown complete");
        }
        Err(_) => {
            info!("Runner shutdown timeout");
        }
    }

    Ok(())
}

/// Run in headless mode (server only, no TUI).
///
/// This starts an HTTP server that exposes the agent via REST API and SSE,
/// allowing remote TUI clients to connect.
#[allow(clippy::cognitive_complexity)]
async fn run_headless(
    cwd: &std::path::Path,
    address: std::net::SocketAddr,
    cli: &Cli,
) -> anyhow::Result<()> {
    use tokio::sync::mpsc;
    use wonopcode_protocol::{Action, Update};
    use wonopcode_server::{create_headless_router_with_options, HeadlessState};

    info!("Starting headless server on {}", address);
    println!("Wonopcode headless server v{}", env!("CARGO_PKG_VERSION"));
    println!("Working directory: {}", cwd.display());

    // Create instance
    let instance = wonopcode_core::Instance::new(cwd).await?;
    let config_file = instance.config().await;

    // Determine provider and model
    let model_state = wonopcode_tui::ModelState::load();
    let (provider, model_id) = if let Some(ref model_spec) = cli.model {
        parse_model_spec(model_spec, &cli.provider)
    } else if let Some(ref model_spec) = config_file.model {
        parse_model_spec(model_spec, &cli.provider)
    } else if let Some(recent) = model_state.most_recent() {
        parse_model_spec(recent, &cli.provider)
    } else {
        let model_id = match cli.provider.as_str() {
            "anthropic" => "claude-sonnet-4-5-20250929".to_string(),
            "openai" => "gpt-4o".to_string(),
            "openrouter" => "anthropic/claude-sonnet-4-5".to_string(),
            _ => "claude-sonnet-4-5-20250929".to_string(),
        };
        (cli.provider.clone(), model_id)
    };

    // Load API key
    let api_key = runner::load_api_key(&provider).unwrap_or_default();

    // Get secret for server authentication (needed early for runner config)
    // Priority: CLI arg > environment variable > config file
    let secret = cli
        .secret
        .clone()
        .or_else(|| std::env::var("WONOPCODE_SECRET").ok())
        .or_else(|| config_file.server.as_ref().and_then(|s| s.api_key.clone()));

    // Build MCP HTTP URL for headless mode
    let mcp_sse_url = format!("http://{address}/mcp/sse");

    // Create runner config with MCP HTTP transport
    let allow_all_in_sandbox = config_file
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
        allow_all: false, // Permissions flow from TUI via protocol
        allow_all_in_sandbox,
        mcp_url: Some(mcp_sse_url), // Use HTTP transport for MCP
        mcp_secret: secret.clone(),
        external_mcp_servers: std::collections::HashMap::new(), // Populated by Runner from mcp_configs
    };

    // Get MCP config
    let mcp_configs = config_file.mcp.clone();

    // Create shared permission manager for both Runner and MCP HTTP server.
    // This ensures sandbox state is shared between them - when sandbox is started
    // via Runner, MCP tools will see the updated sandbox state and runtime.
    let shared_bus = wonopcode_core::bus::Bus::new();
    let shared_permission_manager = std::sync::Arc::new(
        wonopcode_core::permission::PermissionManager::new(shared_bus.clone()),
    );

    // Initialize permission rules
    for rule in wonopcode_core::permission::PermissionManager::default_rules() {
        shared_permission_manager.add_rule(rule).await;
    }
    if let Some(perm_config) = &config_file.permission {
        for rule in wonopcode_core::permission::PermissionManager::rules_from_config(perm_config) {
            shared_permission_manager.add_rule(rule).await;
        }
    }
    info!("Shared permission manager initialized for headless mode");

    // Create channels for action/update communication
    let (protocol_action_tx, mut protocol_action_rx) = mpsc::unbounded_channel::<Action>();
    let (app_action_tx, app_action_rx) = mpsc::unbounded_channel::<wonopcode_tui::AppAction>();
    let (app_update_tx, mut app_update_rx) = mpsc::unbounded_channel::<wonopcode_tui::AppUpdate>();

    // Create shutdown channel for graceful server shutdown
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    // Create headless state with shutdown channel
    let headless_state = HeadlessState::new(protocol_action_tx).with_shutdown_tx(shutdown_tx);
    let update_broadcast = headless_state.update_tx.clone();
    let state_handle = headless_state.current_state.clone();
    let _shutdown_flag = headless_state.shutdown.clone();

    // Set initial state
    {
        let mut state = state_handle.write().await;
        state.project = cwd.display().to_string();
        state.model = format!("{provider}/{model_id}");
        state.project_id = cli.project_id.clone();
        state.work_id = cli.work_id.clone();

        // Set initial sandbox state based on config
        if let Some(sandbox_cfg) = &config_file.sandbox {
            if sandbox_cfg.enabled.unwrap_or(false) {
                state.sandbox.state = "stopped".to_string();
                state.sandbox.runtime_type = Some("Auto".to_string());
            }
        }

        // Set config for settings dialog
        state.config = Some(wonopcode_protocol::ConfigState {
            sandbox: config_file
                .sandbox
                .as_ref()
                .map(|s| wonopcode_protocol::SandboxConfigState {
                    enabled: s.enabled.unwrap_or(false),
                    runtime: s.runtime.clone(),
                }),
            permission: config_file.permission.as_ref().map(|p| {
                wonopcode_protocol::PermissionConfigState {
                    allow_all_in_sandbox: p.allow_all_in_sandbox,
                }
            }),
        });

        // Load sessions list
        let project_id = instance.project_id().await;
        let session_repo = instance.session_repo();
        if let Ok(sessions) = session_repo.list(&project_id).await {
            // Convert sessions to protocol format
            state.sessions = sessions
                .iter()
                .map(|s| wonopcode_protocol::SessionListItem {
                    id: s.id.clone(),
                    title: s.title.clone(),
                    timestamp: s.updated_at().format("%Y-%m-%d %H:%M").to_string(),
                })
                .collect();

            // Load the most recent session's messages
            if let Some(recent_session) = sessions.first() {
                if let Ok(messages) = session_repo
                    .messages(&project_id, &recent_session.id, None)
                    .await
                {
                    let protocol_messages: Vec<wonopcode_protocol::Message> = messages
                        .into_iter()
                        .map(|msg_with_parts| {
                            let role = if msg_with_parts.message.is_user() {
                                "user"
                            } else {
                                "assistant"
                            }
                            .to_string();

                            // Convert parts to content segments
                            let mut content = Vec::new();
                            let mut tool_calls = Vec::new();

                            for part in msg_with_parts.parts {
                                match part {
                                    wonopcode_core::message::MessagePart::Text(text_part) => {
                                        content.push(wonopcode_protocol::MessageSegment::Text {
                                            text: text_part.text,
                                        });
                                    }
                                    wonopcode_core::message::MessagePart::Tool(tool_part) => {
                                        let (status, output, success) = match &tool_part.state {
                                            wonopcode_core::message::ToolState::Pending {
                                                ..
                                            } => ("pending".to_string(), None, false),
                                            wonopcode_core::message::ToolState::Running {
                                                ..
                                            } => ("running".to_string(), None, false),
                                            wonopcode_core::message::ToolState::Completed {
                                                output,
                                                ..
                                            } => (
                                                "completed".to_string(),
                                                Some(output.clone()),
                                                true,
                                            ),
                                            wonopcode_core::message::ToolState::Error {
                                                error,
                                                ..
                                            } => ("failed".to_string(), Some(error.clone()), false),
                                        };

                                        let input = match &tool_part.state {
                                            wonopcode_core::message::ToolState::Pending {
                                                input,
                                                ..
                                            }
                                            | wonopcode_core::message::ToolState::Running {
                                                input,
                                                ..
                                            }
                                            | wonopcode_core::message::ToolState::Completed {
                                                input,
                                                ..
                                            }
                                            | wonopcode_core::message::ToolState::Error {
                                                input,
                                                ..
                                            } => serde_json::to_string(input).unwrap_or_default(),
                                        };

                                        tool_calls.push(wonopcode_protocol::ToolCall {
                                            id: tool_part.id,
                                            name: tool_part.tool,
                                            input,
                                            output,
                                            success,
                                            status,
                                        });
                                    }
                                    wonopcode_core::message::MessagePart::Reasoning(reasoning) => {
                                        content.push(
                                            wonopcode_protocol::MessageSegment::Thinking {
                                                text: reasoning.text,
                                            },
                                        );
                                    }
                                    _ => {}
                                }
                            }

                            // Convert i64 timestamp to string
                            let ts = msg_with_parts.message.created_at();
                            let timestamp = chrono::DateTime::from_timestamp(ts, 0)
                                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                                .unwrap_or_else(|| ts.to_string());

                            // Extract model and agent from assistant messages
                            let (model, agent) = match &msg_with_parts.message {
                                wonopcode_core::message::Message::Assistant(am) => {
                                    (Some(am.model_id.clone()), Some(am.agent.clone()))
                                }
                                wonopcode_core::message::Message::User(_) => (None, None),
                            };

                            wonopcode_protocol::Message {
                                id: msg_with_parts.message.id().to_string(),
                                role,
                                content,
                                timestamp,
                                tool_calls,
                                model,
                                agent,
                            }
                        })
                        .collect();

                    state.session = Some(wonopcode_protocol::SessionState {
                        id: recent_session.id.clone(),
                        title: recent_session.title.clone(),
                        messages: protocol_messages,
                        is_shared: false,
                        share_url: None,
                        is_streaming: false,
                        streaming_message: None,
                    });

                    info!(
                        session_id = %recent_session.id,
                        message_count = state.session.as_ref().map(|s| s.messages.len()).unwrap_or(0),
                        "Loaded most recent session"
                    );
                }
            } else {
                // No existing sessions - create a new empty session for message tracking
                let new_session_id = uuid::Uuid::new_v4().to_string();
                state.session = Some(wonopcode_protocol::SessionState {
                    id: new_session_id.clone(),
                    title: "New Session".to_string(),
                    messages: Vec::new(),
                    is_shared: false,
                    share_url: None,
                    is_streaming: false,
                    streaming_message: None,
                });
                info!(session_id = %new_session_id, "Created new empty session for headless mode");
            }
        }
    }

    // Create runner with shared bus and permission manager
    let runner = match Runner::new_with_shared(
        config,
        instance.clone(),
        mcp_configs,
        Some(shared_bus),
        Some(shared_permission_manager.clone()),
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error creating runner: {e}");
            return Err(anyhow::anyhow!("Failed to create runner: {e}"));
        }
    };

    // Spawn runner
    let runner_handle = tokio::spawn(async move {
        runner.run(app_action_rx, app_update_tx).await;
    });

    // Spawn task to convert protocol actions to app actions
    let state_for_actions = state_handle.clone();
    tokio::spawn(async move {
        while let Some(action) = protocol_action_rx.recv().await {
            let app_action = match action {
                Action::SendPrompt { prompt } => {
                    // Add user message to session state
                    {
                        let mut state = state_for_actions.write().await;
                        if let Some(ref mut session) = state.session {
                            session.messages.push(wonopcode_protocol::Message {
                                id: uuid::Uuid::new_v4().to_string(),
                                role: "user".to_string(),
                                content: vec![wonopcode_protocol::MessageSegment::Text {
                                    text: prompt.clone(),
                                }],
                                timestamp: chrono::Utc::now()
                                    .format("%Y-%m-%d %H:%M:%S")
                                    .to_string(),
                                tool_calls: vec![],
                                model: None,
                                agent: None,
                            });
                        }
                    }
                    wonopcode_tui::AppAction::SendPrompt(prompt)
                }
                Action::Cancel => wonopcode_tui::AppAction::Cancel,
                Action::Quit => {
                    // In headless mode, Quit from TUI just means client disconnected
                    // Don't kill the runner - it should keep serving other clients
                    info!("Client requested quit - ignoring in headless mode");
                    continue;
                }
                Action::ChangeModel { model } => wonopcode_tui::AppAction::ChangeModel(model),
                Action::ChangeAgent { agent } => wonopcode_tui::AppAction::ChangeAgent(agent),
                Action::NewSession => wonopcode_tui::AppAction::NewSession,
                Action::SwitchSession { session_id } => {
                    wonopcode_tui::AppAction::SwitchSession(session_id)
                }
                Action::RenameSession { title } => {
                    wonopcode_tui::AppAction::RenameSession { title }
                }
                Action::ForkSession { message_id } => {
                    wonopcode_tui::AppAction::ForkSession { message_id }
                }
                Action::Undo => wonopcode_tui::AppAction::Undo,
                Action::Redo => wonopcode_tui::AppAction::Redo,
                Action::Revert { message_id } => wonopcode_tui::AppAction::Revert { message_id },
                Action::Unrevert => wonopcode_tui::AppAction::Unrevert,
                Action::Compact => wonopcode_tui::AppAction::Compact,
                Action::SandboxStart => wonopcode_tui::AppAction::SandboxStart,
                Action::SandboxStop => wonopcode_tui::AppAction::SandboxStop,
                Action::SandboxRestart => wonopcode_tui::AppAction::SandboxRestart,
                Action::McpToggle { name } => wonopcode_tui::AppAction::McpToggle { name },
                Action::McpReconnect { name } => wonopcode_tui::AppAction::McpReconnect { name },
                Action::ShareSession => wonopcode_tui::AppAction::ShareSession,
                Action::UnshareSession => wonopcode_tui::AppAction::UnshareSession,
                Action::GotoMessage { message_id } => {
                    wonopcode_tui::AppAction::GotoMessage { message_id }
                }
                Action::SaveSettings { scope, config } => {
                    // Convert protocol scope to app scope
                    let app_scope = match scope {
                        wonopcode_protocol::SaveScope::Project => wonopcode_tui::SaveScope::Project,
                        wonopcode_protocol::SaveScope::Global => wonopcode_tui::SaveScope::Global,
                    };
                    // Try to deserialize config
                    if let Ok(parsed_config) =
                        serde_json::from_value::<wonopcode_core::config::Config>(config)
                    {
                        wonopcode_tui::AppAction::SaveSettings {
                            scope: app_scope,
                            config: Box::new(parsed_config),
                        }
                    } else {
                        continue;
                    }
                }
                Action::PermissionResponse {
                    request_id,
                    allow,
                    remember,
                } => wonopcode_tui::AppAction::PermissionResponse {
                    request_id,
                    allow,
                    remember,
                },
                Action::UpdateTestProviderSettings {
                    emulate_thinking,
                    emulate_tool_calls,
                    emulate_tool_observed,
                    emulate_streaming,
                } => wonopcode_tui::AppAction::UpdateTestProviderSettings {
                    emulate_thinking,
                    emulate_tool_calls,
                    emulate_tool_observed,
                    emulate_streaming,
                },
            };

            if app_action_tx.send(app_action).is_err() {
                break;
            }
        }
    });

    // Spawn task to convert app updates to protocol updates, update state, and broadcast
    let state_for_updates = state_handle.clone();
    tokio::spawn(async move {
        while let Some(update) = app_update_rx.recv().await {
            // Update the current state based on the update type
            match &update {
                wonopcode_tui::AppUpdate::Started => {
                    // Start a new assistant message - track in shared state
                    let mut state = state_for_updates.write().await;
                    let model = Some(state.model.clone());
                    let agent = Some(state.agent.clone());
                    if let Some(ref mut session) = state.session {
                        session.is_streaming = true;
                        session.streaming_message = Some(wonopcode_protocol::Message {
                            id: uuid::Uuid::new_v4().to_string(),
                            role: "assistant".to_string(),
                            content: vec![],
                            timestamp: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                            tool_calls: vec![],
                            model,
                            agent,
                        });
                    }
                }
                wonopcode_tui::AppUpdate::TextDelta(delta) => {
                    // Append to streaming message in shared state
                    let mut state = state_for_updates.write().await;
                    if let Some(ref mut session) = state.session {
                        if let Some(ref mut msg) = session.streaming_message {
                            // Append to last text segment or create new one
                            if let Some(wonopcode_protocol::MessageSegment::Text { text }) =
                                msg.content.last_mut()
                            {
                                text.push_str(delta);
                            } else {
                                msg.content.push(wonopcode_protocol::MessageSegment::Text {
                                    text: delta.clone(),
                                });
                            }
                        }
                    }
                }
                wonopcode_tui::AppUpdate::ToolStarted { id, name, input } => {
                    // Add tool segment to streaming message
                    let mut state = state_for_updates.write().await;
                    if let Some(ref mut session) = state.session {
                        if let Some(ref mut msg) = session.streaming_message {
                            msg.content.push(wonopcode_protocol::MessageSegment::Tool {
                                tool: wonopcode_protocol::ToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input: input.clone(),
                                    output: None,
                                    success: false,
                                    status: "running".to_string(),
                                },
                            });
                        }
                    }
                }
                wonopcode_tui::AppUpdate::ToolCompleted {
                    id,
                    success,
                    output,
                    ..
                } => {
                    // Update tool status in streaming message
                    let mut state = state_for_updates.write().await;
                    if let Some(ref mut session) = state.session {
                        if let Some(ref mut msg) = session.streaming_message {
                            for segment in &mut msg.content {
                                if let wonopcode_protocol::MessageSegment::Tool { tool } = segment {
                                    if &tool.id == id {
                                        tool.output = Some(output.clone());
                                        tool.success = *success;
                                        tool.status = if *success {
                                            "completed".to_string()
                                        } else {
                                            "failed".to_string()
                                        };
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                wonopcode_tui::AppUpdate::Completed { .. } => {
                    // Finalize streaming message and move to messages list
                    let mut state = state_for_updates.write().await;
                    if let Some(ref mut session) = state.session {
                        if let Some(msg) = session.streaming_message.take() {
                            session.messages.push(msg);
                        }
                        session.is_streaming = false;
                    }
                }
                wonopcode_tui::AppUpdate::Error(_) => {
                    // Clear streaming state on error
                    let mut state = state_for_updates.write().await;
                    if let Some(ref mut session) = state.session {
                        session.streaming_message = None;
                        session.is_streaming = false;
                    }
                }
                wonopcode_tui::AppUpdate::SandboxUpdated(status) => {
                    let mut state = state_for_updates.write().await;
                    state.sandbox.state = status.state.clone();
                    state.sandbox.runtime_type = status.runtime_type.clone();
                    state.sandbox.error = status.error.clone();
                }
                wonopcode_tui::AppUpdate::TokenUsage {
                    input,
                    output,
                    cost,
                    context_limit,
                } => {
                    let mut state = state_for_updates.write().await;
                    state.token_usage.input = *input;
                    state.token_usage.output = *output;
                    state.token_usage.cost = *cost;
                    state.context_limit = *context_limit;
                }
                wonopcode_tui::AppUpdate::ModelInfo { context_limit } => {
                    let mut state = state_for_updates.write().await;
                    state.context_limit = *context_limit;
                }
                wonopcode_tui::AppUpdate::AgentChanged(agent) => {
                    let mut state = state_for_updates.write().await;
                    state.agent = agent.clone();
                }
                wonopcode_tui::AppUpdate::TodosUpdated { phases, todos } => {
                    let mut state = state_for_updates.write().await;
                    state.phases = phases
                        .iter()
                        .map(|p| wonopcode_protocol::PhaseInfo {
                            id: p.id.clone(),
                            name: p.name.clone(),
                            status: p.status.clone(),
                            todos: p
                                .todos
                                .iter()
                                .map(|t| wonopcode_protocol::TodoInfo {
                                    id: t.id.clone(),
                                    content: t.content.clone(),
                                    status: t.status.clone(),
                                    priority: t.priority.clone(),
                                    phase_id: t.phase_id.clone(),
                                })
                                .collect(),
                        })
                        .collect();
                    state.todos = todos
                        .iter()
                        .map(|t| wonopcode_protocol::TodoInfo {
                            id: t.id.clone(),
                            content: t.content.clone(),
                            status: t.status.clone(),
                            priority: t.priority.clone(),
                            phase_id: t.phase_id.clone(),
                        })
                        .collect();
                }
                wonopcode_tui::AppUpdate::LspUpdated(servers) => {
                    let mut state = state_for_updates.write().await;
                    state.lsp_servers = servers
                        .iter()
                        .map(|s| wonopcode_protocol::LspInfo {
                            id: s.id.clone(),
                            name: s.name.clone(),
                            root: s.root.clone(),
                            connected: s.connected,
                        })
                        .collect();
                }
                wonopcode_tui::AppUpdate::McpUpdated(servers) => {
                    let mut state = state_for_updates.write().await;
                    state.mcp_servers = servers
                        .iter()
                        .map(|s| wonopcode_protocol::McpInfo {
                            name: s.name.clone(),
                            connected: s.connected,
                            error: s.error.clone(),
                        })
                        .collect();
                }
                wonopcode_tui::AppUpdate::ModifiedFilesUpdated(files) => {
                    let mut state = state_for_updates.write().await;
                    state.modified_files = files
                        .iter()
                        .map(|f| wonopcode_protocol::ModifiedFileInfo {
                            path: f.path.clone(),
                            added: f.added,
                            removed: f.removed,
                        })
                        .collect();
                }
                wonopcode_tui::AppUpdate::Sessions(sessions) => {
                    let mut state = state_for_updates.write().await;
                    state.sessions = sessions
                        .iter()
                        .map(
                            |(id, title, timestamp)| wonopcode_protocol::SessionListItem {
                                id: id.clone(),
                                title: title.clone(),
                                timestamp: timestamp.clone(),
                            },
                        )
                        .collect();
                }
                _ => {}
            }

            // Convert to protocol update
            let protocol_update = match update {
                wonopcode_tui::AppUpdate::Started => Update::Started,
                wonopcode_tui::AppUpdate::TextDelta(delta) => Update::TextDelta { delta },
                wonopcode_tui::AppUpdate::ToolStarted { name, id, input } => {
                    Update::ToolStarted { id, name, input }
                }
                wonopcode_tui::AppUpdate::ToolCompleted {
                    id,
                    success,
                    output,
                    metadata,
                } => Update::ToolCompleted {
                    id,
                    success,
                    output,
                    metadata,
                },
                wonopcode_tui::AppUpdate::Completed { text } => Update::Completed { text },
                wonopcode_tui::AppUpdate::Error(error) => Update::Error { error },
                wonopcode_tui::AppUpdate::Status(message) => Update::Status { message },
                wonopcode_tui::AppUpdate::TokenUsage {
                    input,
                    output,
                    cost,
                    context_limit,
                } => Update::TokenUsage {
                    input,
                    output,
                    cost,
                    context_limit,
                },
                wonopcode_tui::AppUpdate::ModelInfo { context_limit } => {
                    Update::ModelInfo { context_limit }
                }
                wonopcode_tui::AppUpdate::Sessions(sessions) => Update::Sessions {
                    sessions: sessions
                        .into_iter()
                        .map(|(id, title, timestamp)| wonopcode_protocol::SessionInfo {
                            id,
                            title,
                            timestamp,
                        })
                        .collect(),
                },
                wonopcode_tui::AppUpdate::TodosUpdated { phases, todos } => Update::TodosUpdated {
                    phases: phases
                        .into_iter()
                        .map(|p| wonopcode_protocol::PhaseInfo {
                            id: p.id,
                            name: p.name,
                            status: p.status,
                            todos: p
                                .todos
                                .into_iter()
                                .map(|t| wonopcode_protocol::TodoInfo {
                                    id: t.id,
                                    content: t.content,
                                    status: t.status,
                                    priority: t.priority,
                                    phase_id: t.phase_id,
                                })
                                .collect(),
                        })
                        .collect(),
                    todos: todos
                        .into_iter()
                        .map(|t| wonopcode_protocol::TodoInfo {
                            id: t.id,
                            content: t.content,
                            status: t.status,
                            priority: t.priority,
                            phase_id: t.phase_id,
                        })
                        .collect(),
                },
                wonopcode_tui::AppUpdate::LspUpdated(servers) => Update::LspUpdated {
                    servers: servers
                        .into_iter()
                        .map(|s| wonopcode_protocol::LspInfo {
                            id: s.id,
                            name: s.name,
                            root: s.root,
                            connected: s.connected,
                        })
                        .collect(),
                },
                wonopcode_tui::AppUpdate::McpUpdated(servers) => Update::McpUpdated {
                    servers: servers
                        .into_iter()
                        .map(|s| wonopcode_protocol::McpInfo {
                            name: s.name,
                            connected: s.connected,
                            error: s.error,
                        })
                        .collect(),
                },
                wonopcode_tui::AppUpdate::ModifiedFilesUpdated(files) => {
                    Update::ModifiedFilesUpdated {
                        files: files
                            .into_iter()
                            .map(|f| wonopcode_protocol::ModifiedFileInfo {
                                path: f.path,
                                added: f.added,
                                removed: f.removed,
                            })
                            .collect(),
                    }
                }
                wonopcode_tui::AppUpdate::PermissionsPending(count) => {
                    Update::PermissionsPending { count }
                }
                wonopcode_tui::AppUpdate::SandboxUpdated(status) => Update::SandboxUpdated {
                    state: status.state,
                    runtime_type: status.runtime_type,
                    error: status.error,
                },
                wonopcode_tui::AppUpdate::SystemMessage(message) => {
                    Update::SystemMessage { message }
                }
                wonopcode_tui::AppUpdate::AgentChanged(agent) => Update::AgentChanged { agent },
                wonopcode_tui::AppUpdate::PermissionRequest(req) => Update::PermissionRequest {
                    id: req.id,
                    tool: req.tool,
                    action: req.action,
                    description: req.description,
                    path: req.path,
                },
                wonopcode_tui::AppUpdate::SessionLoaded { .. } => {
                    // SessionLoaded is only used by the TUI when connecting to a server,
                    // it doesn't need to be broadcast from the headless server
                    continue;
                }
                // Git updates are handled via dedicated HTTP endpoints, not SSE
                wonopcode_tui::AppUpdate::GitStatusUpdated(_)
                | wonopcode_tui::AppUpdate::GitHistoryUpdated(_)
                | wonopcode_tui::AppUpdate::GitOperationResult { .. } => {
                    continue;
                }
            };

            let _ = update_broadcast.send(protocol_update);
        }
    });

    // Create MCP HTTP state for tool serving with shared permission manager.
    // This ensures MCP tools use the same sandbox state as the Runner.
    let mcp_message_url = format!("http://{address}/mcp/message");
    let mcp_state = create_mcp_http_state(cwd, &mcp_message_url, Some(shared_permission_manager))
        .await
        .ok();
    let has_mcp = mcp_state.is_some();

    // Create router with MCP support and secret authentication
    // The secret is passed separately so it can be applied to all endpoints
    let has_auth = secret.is_some();
    let app = create_headless_router_with_options(headless_state, mcp_state, secret.clone());

    // Start server
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Server running on http://{address}");
    if has_auth {
        println!("API key authentication: enabled");
    }
    if has_mcp {
        println!("MCP endpoint: http://{address}/mcp/sse");
    }

    // Start mDNS advertisement if enabled
    #[cfg(feature = "discover")]
    let _advertiser = if cli.advertise {
        use wonopcode_discover::{AdvertiseConfig, Advertiser};

        match Advertiser::new() {
            Ok(mut advertiser) => {
                // Determine the display name
                let name = cli.name.clone().unwrap_or_else(|| {
                    hostname::get()
                        .ok()
                        .and_then(|h| h.into_string().ok())
                        .unwrap_or_else(|| "wonopcode".to_string())
                });

                // Build advertise config with metadata
                let mut config =
                    AdvertiseConfig::new(&name, address.port(), env!("CARGO_PKG_VERSION"))
                        .with_model(&model_id)
                        .with_cwd(cwd.display().to_string())
                        .with_auth(secret.is_some());

                // Add project name from the worktree directory name
                if let Some(project_name) = cwd.file_name().and_then(|n| n.to_str()) {
                    config = config.with_project(project_name);
                }

                match advertiser.advertise(config) {
                    Ok(_) => {
                        println!("mDNS: advertising as '{name}'");
                        Some(advertiser)
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to advertise via mDNS");
                        None
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to create mDNS advertiser");
                None
            }
        }
    } else {
        None
    };

    println!("Press Ctrl+C to stop");

    // Run server until shutdown (graceful shutdown on channel signal or Ctrl+C)
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            // Wait for either shutdown signal or Ctrl+C
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received");
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Ctrl+C received");
                }
            }
        })
        .await?;

    // Wait for runner to complete
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), runner_handle).await;

    // Advertiser will be dropped here, stopping the mDNS advertisement

    instance.dispose().await;
    Ok(())
}

/// Discover and connect to a server on the local network via mDNS.
#[cfg(feature = "discover")]
async fn run_discover(cli: &Cli) -> anyhow::Result<()> {
    use std::io::Write;
    use std::time::Duration;
    use wonopcode_discover::{Browser, ServerInfo};

    println!("Discovering wonopcode servers on the local network...\n");

    let browser =
        Browser::new().map_err(|e| anyhow::anyhow!("Failed to create mDNS browser: {e}"))?;
    let servers = browser
        .browse(Duration::from_secs(3))
        .map_err(|e| anyhow::anyhow!("Failed to browse for servers: {e}"))?;

    if servers.is_empty() {
        println!("No servers found.");
        println!("\nMake sure a server is running with:");
        println!("  wonopcode --headless --advertise");
        return Ok(());
    }

    println!("Found {} server(s):\n", servers.len());

    for (i, server) in servers.iter().enumerate() {
        println!("  {}. {}", i + 1, server.name);
        println!("     Address: {}", server.address);
        if let Some(ref project) = server.project {
            println!("     Project: {project}");
        }
        if let Some(ref model) = server.model {
            println!("     Model: {model}");
        }
        if server.auth_required {
            println!("     Auth: required");
        }
        println!();
    }

    // Select a server
    let selected: &ServerInfo = if servers.len() == 1 {
        println!("Connecting to the only available server...\n");
        &servers[0]
    } else {
        print!("Select server (1-{}): ", servers.len());
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        let idx: usize = input
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid selection"))?;

        if idx < 1 || idx > servers.len() {
            return Err(anyhow::anyhow!("Selection out of range"));
        }

        &servers[idx - 1]
    };

    // Check if auth is required but no secret provided
    if selected.auth_required && cli.secret.is_none() {
        println!("Warning: Server requires authentication. Use --secret to provide credentials.\n");
    }

    // Connect to the selected server
    run_connect(&selected.address.to_string(), cli).await
}

/// Connect to a remote headless server.
#[allow(clippy::cognitive_complexity)]
async fn run_connect(address: &str, cli: &Cli) -> anyhow::Result<()> {
    use wonopcode_tui::{App, Backend, RemoteBackend, SandboxStatusUpdate};

    // Parse address
    let address = if address.starts_with(':') {
        format!("127.0.0.1{address}")
    } else {
        address.to_string()
    };

    println!("Connecting to {address}...");

    // Get secret for authentication
    // Priority: CLI arg > environment variable
    let secret = cli
        .secret
        .clone()
        .or_else(|| std::env::var("WONOPCODE_SECRET").ok());

    // Create remote backend with optional secret
    let backend = RemoteBackend::with_api_key(&address, secret)?;

    // Check connection
    backend.connect().await?;

    println!("Connected!");

    // Get initial state
    let state = backend.get_state().await?;

    // Create TUI app
    let mut app = App::new();

    // Apply initial state from server
    app.set_project(state.project);
    app.set_model(state.model);
    app.set_agent(state.agent);

    // Apply sandbox state
    let sandbox_update = SandboxStatusUpdate {
        state: state.sandbox.state,
        runtime_type: state.sandbox.runtime_type,
        error: state.sandbox.error,
    };
    // Send as update so it's processed correctly
    let update_tx = app.update_sender();
    if let Err(e) = update_tx.send(wonopcode_tui::AppUpdate::SandboxUpdated(sandbox_update)) {
        warn!("Failed to send sandbox update: {}", e);
    }

    // Apply todos (phases and flat list)
    if !state.phases.is_empty() || !state.todos.is_empty() {
        let phases: Vec<wonopcode_tui::PhaseUpdate> = state
            .phases
            .into_iter()
            .map(|p| wonopcode_tui::PhaseUpdate {
                id: p.id,
                name: p.name,
                status: p.status,
                todos: p
                    .todos
                    .into_iter()
                    .map(|t| wonopcode_tui::TodoUpdate {
                        id: t.id,
                        content: t.content,
                        status: t.status,
                        priority: t.priority,
                        phase_id: t.phase_id,
                    })
                    .collect(),
            })
            .collect();
        let todos: Vec<wonopcode_tui::TodoUpdate> = state
            .todos
            .into_iter()
            .map(|t| wonopcode_tui::TodoUpdate {
                id: t.id,
                content: t.content,
                status: t.status,
                priority: t.priority,
                phase_id: t.phase_id,
            })
            .collect();
        if let Err(e) = update_tx.send(wonopcode_tui::AppUpdate::TodosUpdated { phases, todos }) {
            warn!("Failed to send todos update: {}", e);
        }
    }

    // Apply MCP servers
    if !state.mcp_servers.is_empty() {
        let servers: Vec<wonopcode_tui::McpStatusUpdate> = state
            .mcp_servers
            .into_iter()
            .map(|s| wonopcode_tui::McpStatusUpdate {
                name: s.name,
                connected: s.connected,
                error: s.error,
            })
            .collect();
        if let Err(e) = update_tx.send(wonopcode_tui::AppUpdate::McpUpdated(servers)) {
            warn!("Failed to send MCP update: {}", e);
        }
    }

    // Apply LSP servers
    if !state.lsp_servers.is_empty() {
        let servers: Vec<wonopcode_tui::LspStatusUpdate> = state
            .lsp_servers
            .into_iter()
            .map(|s| wonopcode_tui::LspStatusUpdate {
                id: s.id,
                name: s.name,
                root: s.root,
                connected: s.connected,
            })
            .collect();
        if let Err(e) = update_tx.send(wonopcode_tui::AppUpdate::LspUpdated(servers)) {
            warn!("Failed to send LSP update: {}", e);
        }
    }

    // Apply sessions list
    if !state.sessions.is_empty() {
        let sessions: Vec<(String, String, String)> = state
            .sessions
            .into_iter()
            .map(|s| (s.id, s.title, s.timestamp))
            .collect();
        if let Err(e) = update_tx.send(wonopcode_tui::AppUpdate::Sessions(sessions)) {
            warn!("Failed to send sessions update: {}", e);
        }
    }

    // Apply token usage
    if let Err(e) = update_tx.send(wonopcode_tui::AppUpdate::TokenUsage {
        input: state.token_usage.input,
        output: state.token_usage.output,
        cost: state.token_usage.cost,
        context_limit: state.context_limit,
    }) {
        warn!("Failed to send token usage update: {}", e);
    }

    // Apply modified files
    if !state.modified_files.is_empty() {
        let files: Vec<wonopcode_tui::ModifiedFileUpdate> = state
            .modified_files
            .into_iter()
            .map(|f| wonopcode_tui::ModifiedFileUpdate {
                path: f.path,
                added: f.added,
                removed: f.removed,
            })
            .collect();
        if let Err(e) = update_tx.send(wonopcode_tui::AppUpdate::ModifiedFilesUpdated(files)) {
            warn!("Failed to send modified files update: {}", e);
        }
    }

    // Apply current session with messages
    if let Some(session) = state.session {
        // Helper to convert protocol ToolCall to TUI DisplayToolCall
        let convert_tool = |tool: &wonopcode_protocol::ToolCall| -> wonopcode_tui::DisplayToolCall {
            let status = match tool.status.as_str() {
                "completed" => wonopcode_tui::ToolStatus::Success,
                "failed" => wonopcode_tui::ToolStatus::Error,
                "running" => wonopcode_tui::ToolStatus::Running,
                _ => wonopcode_tui::ToolStatus::Pending,
            };
            wonopcode_tui::DisplayToolCall {
                id: tool.id.clone(),
                name: tool.name.clone(),
                input: Some(tool.input.clone()),
                output: tool.output.clone(),
                status,
                metadata: None,
                expanded: false,
            }
        };

        // Helper to convert a protocol message to display message
        let convert_message = |msg: &wonopcode_protocol::Message| -> wonopcode_tui::DisplayMessage {
            match msg.role.as_str() {
                "user" => {
                    // Extract text from content segments
                    let text: String = msg
                        .content
                        .iter()
                        .filter_map(|seg| match seg {
                            wonopcode_protocol::MessageSegment::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    wonopcode_tui::DisplayMessage::user(text)
                }
                "assistant" => {
                    // Convert segments to TUI segments (now includes inline tools)
                    let mut all_segments: Vec<wonopcode_tui::widgets::MessageSegment> = msg
                        .content
                        .iter()
                        .map(|seg| match seg {
                            wonopcode_protocol::MessageSegment::Text { text } => {
                                wonopcode_tui::widgets::MessageSegment::Text(text.clone())
                            }
                            wonopcode_protocol::MessageSegment::Code { code, .. } => {
                                wonopcode_tui::widgets::MessageSegment::Text(code.clone())
                            }
                            wonopcode_protocol::MessageSegment::Thinking { text } => {
                                wonopcode_tui::widgets::MessageSegment::Text(format!(
                                    "*Thinking:* {text}"
                                ))
                            }
                            wonopcode_protocol::MessageSegment::Tool { tool } => {
                                wonopcode_tui::widgets::MessageSegment::Tool(convert_tool(tool))
                            }
                        })
                        .collect();

                    // Also add any legacy tool_calls (for backward compatibility)
                    for tool in &msg.tool_calls {
                        all_segments.push(wonopcode_tui::widgets::MessageSegment::Tool(
                            convert_tool(tool),
                        ));
                    }

                    // Convert agent string to AgentMode
                    let agent_mode = msg
                        .agent
                        .as_ref()
                        .map(|a| wonopcode_tui::AgentMode::parse(a));

                    wonopcode_tui::DisplayMessage::assistant_with_segments(all_segments)
                        .with_model_agent(msg.model.clone(), agent_mode)
                }
                "system" => {
                    let text: String = msg
                        .content
                        .iter()
                        .filter_map(|seg| match seg {
                            wonopcode_protocol::MessageSegment::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    wonopcode_tui::DisplayMessage::system(text)
                }
                _ => wonopcode_tui::DisplayMessage::system(format!("Unknown role: {}", msg.role)),
            }
        };

        let messages: Vec<wonopcode_tui::DisplayMessage> =
            session.messages.iter().map(convert_message).collect();

        if let Err(e) = update_tx.send(wonopcode_tui::AppUpdate::SessionLoaded {
            id: session.id.clone(),
            title: session.title.clone(),
            messages,
        }) {
            warn!("Failed to send session loaded update: {}", e);
        }

        // If there's an in-progress streaming message, restore the streaming state
        if session.is_streaming {
            if let Some(ref streaming_msg) = session.streaming_message {
                info!(
                    "Restoring streaming state with {} content segments",
                    streaming_msg.content.len()
                );

                // Send Started to put TUI in streaming mode
                if let Err(e) = update_tx.send(wonopcode_tui::AppUpdate::Started) {
                    warn!("Failed to send Started update: {}", e);
                }

                // Send the accumulated content as events to build up the streaming display
                for segment in &streaming_msg.content {
                    match segment {
                        wonopcode_protocol::MessageSegment::Text { text } => {
                            if let Err(e) =
                                update_tx.send(wonopcode_tui::AppUpdate::TextDelta(text.clone()))
                            {
                                warn!("Failed to send TextDelta update: {}", e);
                            }
                        }
                        wonopcode_protocol::MessageSegment::Tool { tool } => {
                            // Send ToolStarted
                            if let Err(e) = update_tx.send(wonopcode_tui::AppUpdate::ToolStarted {
                                id: tool.id.clone(),
                                name: tool.name.clone(),
                                input: tool.input.clone(),
                            }) {
                                warn!("Failed to send ToolStarted update: {}", e);
                            }

                            // If tool is completed, send ToolCompleted
                            if tool.status == "completed" || tool.status == "failed" {
                                if let Err(e) =
                                    update_tx.send(wonopcode_tui::AppUpdate::ToolCompleted {
                                        id: tool.id.clone(),
                                        success: tool.success,
                                        output: tool.output.clone().unwrap_or_default(),
                                        metadata: None,
                                    })
                                {
                                    warn!("Failed to send ToolCompleted update: {}", e);
                                }
                            }
                        }
                        wonopcode_protocol::MessageSegment::Thinking { text } => {
                            // Send thinking as text delta with prefix
                            if let Err(e) = update_tx.send(wonopcode_tui::AppUpdate::TextDelta(
                                format!("*Thinking:* {text}"),
                            )) {
                                warn!("Failed to send Thinking update: {}", e);
                            }
                        }
                        wonopcode_protocol::MessageSegment::Code { code, .. } => {
                            if let Err(e) =
                                update_tx.send(wonopcode_tui::AppUpdate::TextDelta(code.clone()))
                            {
                                warn!("Failed to send Code update: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    // Take action receiver
    let action_rx = app
        .take_action_rx()
        .ok_or_else(|| anyhow::anyhow!("Failed to get action receiver"))?;

    // Start SSE listener to receive updates
    let _sse_handle = backend.subscribe_updates(update_tx);

    // Spawn task to forward actions to remote server
    let backend = std::sync::Arc::new(backend);
    let backend_clone = backend.clone();
    tokio::spawn(async move {
        let mut action_rx = action_rx;
        while let Some(action) = action_rx.recv().await {
            if let Err(e) = backend_clone.send_action(action).await {
                tracing::warn!("Failed to send action: {}", e);
            }
        }
    });

    // Run TUI
    app.run().await?;

    Ok(())
}

/// Run ACP (Agent Client Protocol) server for IDE integration.
async fn run_acp(cwd: &std::path::Path) -> anyhow::Result<()> {
    use wonopcode_acp::{serve, AgentConfig};

    info!("Starting ACP server in: {}", cwd.display());

    let config = AgentConfig {
        name: "Wonopcode".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        default_model: None,
    };

    serve(config).await;

    Ok(())
}

#[cfg(feature = "github")]
async fn handle_github(command: GithubCommands, cwd: &std::path::Path) -> anyhow::Result<()> {
    match command {
        GithubCommands::Install => {
            println!("GitHub Integration Setup");
            println!("========================");
            println!();
            println!("To enable GitHub integration:");
            println!();
            println!("1. Create a workflow file at .github/workflows/wonopcode.yml:");
            println!();
            println!("   name: Wonopcode");
            println!("   on:");
            println!("     issue_comment:");
            println!("       types: [created]");
            println!("     pull_request_review_comment:");
            println!("       types: [created]");
            println!();
            println!("   jobs:");
            println!("     run:");
            println!("       runs-on: ubuntu-latest");
            println!("       steps:");
            println!("         - uses: actions/checkout@v4");
            println!("         - name: Run Wonopcode");
            println!("           run: |");
            println!("             curl -fsSL https://wonopcode.dev/install.sh | sh");
            println!("             wonopcode github run");
            println!("           env:");
            println!("             GITHUB_TOKEN: ${{{{ secrets.GITHUB_TOKEN }}}}");
            println!("             ANTHROPIC_API_KEY: ${{{{ secrets.ANTHROPIC_API_KEY }}}}");
            println!();
            println!("2. Add your API key as a repository secret (Settings > Secrets)");
            println!();
            println!("3. Trigger by commenting `/wonopcode <your request>` on issues or PRs");
            Ok(())
        }
        GithubCommands::Run { event, token } => {
            github::run_agent(cwd, event.as_deref(), token.as_deref()).await
        }
    }
}

/// Handle PR checkout (requires --features github).
#[cfg(feature = "github")]
async fn handle_pr(number: u64) -> anyhow::Result<()> {
    github::checkout_pr(number).await
}

/// Handle stats command.
async fn handle_stats(
    cwd: &std::path::Path,
    days: Option<u32>,
    tools: Option<usize>,
    project: Option<String>,
) -> anyhow::Result<()> {
    println!();
    println!("Wonopcode Usage Statistics");
    println!("==========================");
    println!();

    let stats = stats::aggregate_session_stats(cwd, days, project).await?;
    stats::display_stats(&stats, tools);

    Ok(())
}
