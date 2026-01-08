//! Wonopcode - AI-powered coding assistant.
//!
//! This is the main entry point for the wonopcode CLI.

mod compaction;
#[cfg(feature = "github")]
mod github;
mod publish;
mod runner;
mod stats;
mod upgrade;

use clap::{Parser, Subcommand};
use runner::{Runner, RunnerConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

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
    /// Run as MCP server (for Claude CLI integration)
    #[command(name = "mcp-serve")]
    McpServe {
        /// Working directory
        #[arg(short, long)]
        cwd: Option<std::path::PathBuf>,
        /// Session ID for tool context
        #[arg(long)]
        session_id: Option<String>,
        /// Allow all tool executions without permission checks (use in trusted environments)
        #[arg(long, default_value = "false")]
        allow_all: bool,
    },
}

#[derive(Subcommand)]
enum AuthCommands {
    /// Log in to a provider
    Login {
        /// Provider name
        provider: String,
    },
    /// Log out from a provider
    Logout {
        /// Provider name
        provider: String,
    },
    /// Show authentication status
    Status,
}

#[derive(Subcommand)]
enum SessionCommands {
    /// List all sessions
    List,
    /// Show session details
    Show {
        /// Session ID
        id: String,
    },
    /// Delete a session
    Delete {
        /// Session ID
        id: String,
    },
}

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

#[derive(Subcommand)]
enum McpCommands {
    /// Add an MCP server
    Add {
        /// Server name
        name: String,
        /// Server type: local or remote
        #[arg(short, long, default_value = "local")]
        server_type: String,
        /// Command for local servers or URL for remote
        #[arg(short, long)]
        command: Option<String>,
        /// URL for remote servers
        #[arg(short, long)]
        url: Option<String>,
    },
    /// List MCP servers and their status
    List,
    /// Authenticate with an OAuth-enabled MCP server
    Auth {
        /// Server name
        name: String,
    },
    /// Remove OAuth credentials for an MCP server
    Logout {
        /// Server name
        name: String,
    },
}

#[derive(Subcommand)]
enum AgentCommands {
    /// List available agents
    List,
    /// Show details for an agent
    Show {
        /// Agent name
        name: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging and get log file path
    // In headless mode, log to stdout instead of file
    let log_file = init_logging(cli.verbose, cli.headless);

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
            run_command(
                &cwd,
                message,
                model,
                continue_session,
                session,
                &format,
                cli.provider.clone(),
            )
            .await
        }
        Some(Commands::Serve { address }) => run_server(address, &cwd).await,
        Some(Commands::Models) => {
            list_models();
            Ok(())
        }
        Some(Commands::Config) => show_config(&cwd).await,
        Some(Commands::Version) => {
            print_version();
            Ok(())
        }
        Some(Commands::Auth { command }) => handle_auth(command).await,
        Some(Commands::Session { command }) => handle_session(command, &cwd).await,
        Some(Commands::Export {
            session,
            output,
            format,
        }) => handle_export(&cwd, session, output, &format).await,
        Some(Commands::Import { input }) => handle_import(&cwd, input).await,
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
        Some(Commands::Web { address, open }) => run_web_server(address, open, &cwd).await,
        Some(Commands::Mcp { command }) => handle_mcp(command, &cwd).await,
        Some(Commands::Check { channel, json }) => {
            let channel = channel.and_then(|s| parse_release_channel(&s));
            upgrade::handle_check(channel, json).await
        }
        Some(Commands::Upgrade {
            yes,
            channel,
            version,
            force,
        }) => {
            let channel = channel.and_then(|s| parse_release_channel(&s));
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
        Some(Commands::Agent { command }) => handle_agent(command, &cwd).await,
        Some(Commands::McpServe {
            cwd: mcp_cwd,
            session_id,
            allow_all,
        }) => {
            let working_dir = mcp_cwd.unwrap_or_else(|| cwd.clone());
            run_mcp_server(&working_dir, session_id, allow_all).await
        }
        None => {
            // Check for headless or connect mode
            if cli.headless {
                run_headless(&cwd, cli.address, &cli).await
            } else if let Some(ref address) = cli.connect {
                run_connect(address, &cli).await
            } else {
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
fn init_logging(verbose: bool, headless: bool) -> Option<std::path::PathBuf> {
    let filter = if verbose {
        "wonopcode=debug,wonopcode_core=debug,wonopcode_provider=debug,wonopcode_tools=debug,tower_http=debug"
    } else if headless {
        // In headless mode, include info-level HTTP request logging
        "wonopcode=info,tower_http=info"
    } else {
        "wonopcode=info"
    };

    if headless {
        // In headless mode, log to stdout with colors
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .with_ansi(true)
            .init();
        return None;
    }

    // Get log directory
    let log_dir = get_log_dir();

    // Create log directory if needed
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!("Warning: Could not create log directory: {}", e);
        return None;
    }

    // Create log file path
    let log_file = log_dir.join("wonopcode.log");

    // Open log file for appending
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: Could not open log file: {}", e);
            return None;
        }
    };

    // Initialize tracing to file
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(false)
        .with_writer(file)
        .init();

    Some(log_file)
}

/// Get the log directory path.
fn get_log_dir() -> std::path::PathBuf {
    // macOS: ~/Library/Logs/wonopcode
    // Linux: ~/.local/state/wonopcode/logs
    // Windows: %LOCALAPPDATA%/wonopcode/logs

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            return home.join("Library/Logs/wonopcode");
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(state_dir) = dirs::state_dir() {
            return state_dir.join("wonopcode/logs");
        }
        if let Some(home) = dirs::home_dir() {
            return home.join(".local/state/wonopcode/logs");
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app) = dirs::data_local_dir() {
            return local_app.join("wonopcode/logs");
        }
    }

    // Fallback
    std::path::PathBuf::from(".wonopcode/logs")
}

/// Run the HTTP server.
async fn run_server(address: SocketAddr, cwd: &std::path::Path) -> anyhow::Result<()> {
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

/// Run command - execute a single prompt and exit.
async fn run_command(
    cwd: &std::path::Path,
    message: Vec<String>,
    model: Option<String>,
    _continue_session: bool,
    _session: Option<String>,
    format: &str,
    default_provider: String,
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
        parse_model_spec(m, &default_provider)
    } else {
        tracing::debug!(default_provider = %default_provider, "Using default provider");
        (
            default_provider.clone(),
            get_default_model(&default_provider),
        )
    };
    tracing::debug!(provider = %provider, model_id = %model_id, "Using provider and model");

    // Load API key (may be empty for CLI-based auth)
    let api_key = runner::load_api_key(&provider).unwrap_or_default();

    // Check if we have authentication
    if api_key.is_empty() {
        use wonopcode_provider::claude_cli::ClaudeCliProvider;

        // For Anthropic, allow CLI-based subscription auth
        if provider != "anthropic"
            || !ClaudeCliProvider::is_available()
            || !ClaudeCliProvider::is_authenticated()
        {
            eprintln!("Error: No API key found for provider '{}'", provider);
            eprintln!("Run: wonopcode auth login {}", provider);
            return Ok(());
        }
    }

    // Create runner config
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
        mcp_url: None, // Use stdio transport for normal TUI mode
    };

    // Create runner with snapshot support
    let runner = match Runner::new_with_snapshots(config.clone(), instance.clone()).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error creating runner: {}", e);
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
                    print!("{}", delta);
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
                    println!("{}", text);
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
                    eprintln!("\nError: {}", e);
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
    runner_handle.abort();
    instance.dispose().await;

    Ok(())
}

/// Parse release channel from string.
fn parse_release_channel(s: &str) -> Option<wonopcode_core::version::ReleaseChannel> {
    match s.to_lowercase().as_str() {
        "stable" => Some(wonopcode_core::version::ReleaseChannel::Stable),
        "beta" => Some(wonopcode_core::version::ReleaseChannel::Beta),
        "nightly" => Some(wonopcode_core::version::ReleaseChannel::Nightly),
        _ => {
            eprintln!("Unknown release channel: {}. Using 'stable'.", s);
            None
        }
    }
}

/// Parse model specification in provider/model format.
/// Also tries to infer provider from well-known model names.
fn parse_model_spec(spec: &str, default_provider: &str) -> (String, String) {
    if let Some((provider, model)) = spec.split_once('/') {
        (provider.to_string(), model.to_string())
    } else {
        // Try to infer provider from model name
        let provider = infer_provider_from_model(spec).unwrap_or(default_provider);
        (provider.to_string(), spec.to_string())
    }
}

/// Infer the provider from a model name.
fn infer_provider_from_model(model: &str) -> Option<&'static str> {
    let model_lower = model.to_lowercase();

    // OpenAI models
    if model_lower.starts_with("gpt-")
        || model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("chatgpt")
    {
        return Some("openai");
    }

    // Anthropic models
    if model_lower.starts_with("claude") {
        return Some("anthropic");
    }

    // Google models
    if model_lower.starts_with("gemini") {
        return Some("google");
    }

    None
}

/// Get default model for a provider.
fn get_default_model(provider: &str) -> String {
    match provider {
        "anthropic" => "claude-sonnet-4-5-20250929".to_string(),
        "openai" => "gpt-4o".to_string(),
        "openrouter" => "anthropic/claude-sonnet-4-5".to_string(),
        _ => "claude-sonnet-4-5-20250929".to_string(),
    }
}

/// List available models.
fn list_models() {
    println!("Available models:");
    println!();
    println!("Anthropic (Latest - Claude 4.5):");
    println!("  claude-sonnet-4-5-20250929  Claude Sonnet 4.5 (recommended)");
    println!("  claude-haiku-4-5-20251001   Claude Haiku 4.5 (fastest)");
    println!("  claude-opus-4-5-20251101    Claude Opus 4.5 (most intelligent)");
    println!();
    println!("Anthropic (Legacy - Claude 4.x):");
    println!("  claude-sonnet-4-20250514    Claude Sonnet 4");
    println!("  claude-opus-4-1-20250805    Claude Opus 4.1");
    println!("  claude-opus-4-20250514      Claude Opus 4");
    println!();
    println!("Anthropic (Legacy - Claude 3.x):");
    println!("  claude-3-7-sonnet-20250219  Claude 3.7 Sonnet (extended thinking)");
    println!("  claude-3-haiku-20240307     Claude 3 Haiku (economical)");
    println!();
    println!("OpenAI (GPT-5):");
    println!("  gpt-5.2                     GPT-5.2 (best for coding & agents)");
    println!("  gpt-5.1                     GPT-5.1 (configurable reasoning)");
    println!("  gpt-5                       GPT-5 (intelligent reasoning)");
    println!("  gpt-5-mini                  GPT-5 mini (fast, cost-efficient)");
    println!("  gpt-5-nano                  GPT-5 nano (fastest, cheapest)");
    println!();
    println!("OpenAI (GPT-4.1):");
    println!("  gpt-4.1                     GPT-4.1 (smartest non-reasoning)");
    println!("  gpt-4.1-mini                GPT-4.1 mini (fast, 1M context)");
    println!("  gpt-4.1-nano                GPT-4.1 nano (cheapest, 1M context)");
    println!();
    println!("OpenAI (O-Series):");
    println!("  o3                          o3 (reasoning model)");
    println!("  o3-mini                     o3-mini (fast reasoning)");
    println!("  o4-mini                     o4-mini (cost-efficient reasoning)");
    println!();
    println!("OpenAI (Legacy):");
    println!("  gpt-4o                      GPT-4o (previous flagship)");
    println!("  gpt-4o-mini                 GPT-4o mini (fast, affordable)");
    println!("  o1                          o1 (legacy reasoning)");
    println!();
    println!("OpenRouter:");
    println!("  Use any model ID from https://openrouter.ai/models");
}

/// Show configuration.
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

/// Handle authentication commands.
async fn handle_auth(command: AuthCommands) -> anyhow::Result<()> {
    match command {
        AuthCommands::Login { provider } => {
            auth_login(&provider).await?;
        }
        AuthCommands::Logout { provider } => {
            auth_logout(&provider).await?;
        }
        AuthCommands::Status => {
            auth_status().await?;
        }
    }

    Ok(())
}

/// Log in to a provider.
async fn auth_login(provider: &str) -> anyhow::Result<()> {
    use std::io::{self, Write};
    use wonopcode_provider::claude_cli::ClaudeCliProvider;

    // Validate provider and get key URL
    let key_url = match provider {
        "anthropic" => "https://console.anthropic.com/settings/keys",
        "openai" => "https://platform.openai.com/api-keys",
        "openrouter" => "https://openrouter.ai/keys",
        _ => {
            eprintln!("Unknown provider: {}", provider);
            eprintln!("Supported providers: anthropic, openai, openrouter");
            return Ok(());
        }
    };

    // Special handling for Anthropic - offer subscription option
    if provider == "anthropic" {
        println!();
        println!("Anthropic Authentication");
        println!("========================");
        println!();

        // Check if Claude CLI is available
        if ClaudeCliProvider::is_available() {
            println!("Choose authentication method:");
            println!();
            println!("  1. Claude subscription (via Claude Code CLI)");
            println!("     - Uses your Claude Max/Pro subscription");
            println!("     - No per-token usage costs");
            println!();
            println!("  2. API key");
            println!("     - Pay-per-use via Anthropic API");
            println!("     - Requires API key from console.anthropic.com");
            println!();
            print!("Enter choice [1/2]: ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if input.trim() == "1" {
                // Check if already authenticated before starting setup
                print!("Checking current authentication status... ");
                io::stdout().flush()?;

                if ClaudeCliProvider::is_authenticated() {
                    println!("already authenticated!");
                    println!();
                    println!("You are already authenticated via Claude CLI subscription.");
                    println!("You can use wonopcode with your subscription now.");
                    return Ok(());
                }
                println!("not authenticated.");
                return auth_login_claude_subscription().await;
            }
            // Otherwise fall through to API key
        }
    }

    auth_login_api_key(provider, key_url).await
}

/// Login using Claude CLI subscription.
async fn auth_login_claude_subscription() -> anyhow::Result<()> {
    use std::process::Command;

    println!();
    println!("Claude Subscription Login");
    println!("=========================");
    println!();
    println!("This will open the Claude Code CLI setup-token flow.");
    println!("You'll be redirected to claude.ai to authenticate.");
    println!();
    println!("Note: If this is your first time, you may need to run");
    println!("      'claude' once interactively to complete initial setup.");
    println!();

    // Run claude setup-token (the correct command for auth)
    let status = Command::new("claude").arg("setup-token").status();

    match status {
        Ok(exit_status) if exit_status.success() => {
            println!();
            println!("Authentication successful!");
            println!();
            println!("You can now use wonopcode with your Claude subscription.");
            println!("No API key is required - your subscription covers usage.");
        }
        Ok(_) => {
            eprintln!();
            eprintln!("Authentication failed or was cancelled.");
            eprintln!();
            eprintln!("If this is your first time, try running 'claude' once");
            eprintln!("to complete the initial setup, then retry:");
            eprintln!("  wonopcode auth login anthropic");
        }
        Err(e) => {
            eprintln!();
            eprintln!("Error running Claude CLI: {}", e);
            eprintln!();
            eprintln!("Make sure Claude Code CLI is installed:");
            eprintln!("  npm install -g @anthropic-ai/claude-code");
        }
    }

    Ok(())
}

/// Login with a manual API key.
async fn auth_login_api_key(provider: &str, key_url: &str) -> anyhow::Result<()> {
    use std::io::{self, Write};

    // Check if already authenticated
    if let Some(key) = runner::load_api_key(provider) {
        if !key.is_empty() {
            println!("Already authenticated with {}.", provider);
            print!("Do you want to replace the existing key? [y/N] ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Cancelled.");
                return Ok(());
            }
        }
    }

    println!();
    println!("To get an API key for {}, visit:", provider);
    println!("  {}", key_url);
    println!();

    // Prompt for API key
    print!("Enter your {} API key: ", provider);
    io::stdout().flush()?;

    // Read API key
    let api_key = read_password_or_line()?;

    if api_key.is_empty() {
        println!("No API key provided. Cancelled.");
        return Ok(());
    }

    // Validate API key format
    let valid = match provider {
        "anthropic" => api_key.starts_with("sk-ant-"),
        "openai" => api_key.starts_with("sk-"),
        "openrouter" => api_key.starts_with("sk-or-"),
        _ => true,
    };

    if !valid {
        println!();
        println!(
            "Warning: The API key doesn't match the expected format for {}.",
            provider
        );
        print!("Continue anyway? [y/N] ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    // Save to config file
    save_api_key(provider, &api_key).await?;

    println!();
    println!("✓ API key saved for {}.", provider);
    println!();
    println!("You can now use wonopcode with --provider {}", provider);

    Ok(())
}

/// Log out from a provider by removing the API key from config.
async fn auth_logout(provider: &str) -> anyhow::Result<()> {
    // Validate provider
    match provider {
        "anthropic" | "openai" | "openrouter" => {}
        _ => {
            eprintln!("Unknown provider: {}", provider);
            return Ok(());
        }
    }

    // Remove from config
    remove_api_key(provider).await?;

    println!("✓ Logged out from {}.", provider);

    Ok(())
}

/// Show authentication status.
async fn auth_status() -> anyhow::Result<()> {
    use wonopcode_provider::claude_cli::ClaudeCliProvider;

    println!("Authentication status:");
    println!();

    let providers = ["anthropic", "openai", "openrouter"];

    for provider in providers {
        let status = if let Ok(key) = std::env::var(get_env_var(provider)) {
            format!("✓ {} (env)", mask_api_key(&key))
        } else if let Some(key) = runner::load_api_key(provider) {
            format!("✓ {} (config)", mask_api_key(&key))
        } else if provider == "anthropic"
            && ClaudeCliProvider::is_available()
            && ClaudeCliProvider::is_authenticated()
        {
            "✓ subscription (claude cli)".to_string()
        } else if provider == "anthropic" && ClaudeCliProvider::is_available() {
            "✗ claude cli not logged in".to_string()
        } else {
            "✗ not authenticated".to_string()
        };
        println!("  {:<12} {}", format!("{}:", provider), status);
    }

    println!();
    println!("Config file: {}", get_credentials_path().display());

    // Show Claude CLI status
    if ClaudeCliProvider::is_available() {
        println!();
        println!("Claude CLI:  installed");
        if ClaudeCliProvider::is_authenticated() {
            println!("             authenticated (subscription)");
        } else {
            println!("             not logged in (run: claude login)");
        }
    }

    Ok(())
}

/// Read a line from stdin.
/// Note: Input is not hidden. For production, consider adding rpassword dependency.
fn read_password_or_line() -> std::io::Result<String> {
    use std::io::{self, BufRead};

    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// Get environment variable name for a provider.
fn get_env_var(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        _ => "",
    }
}

/// Mask an API key for display.
fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        return "*".repeat(key.len());
    }
    let prefix = &key[..4];
    let suffix = &key[key.len() - 4..];
    format!("{}...{}", prefix, suffix)
}

/// Get the credentials file path.
fn get_credentials_path() -> std::path::PathBuf {
    wonopcode_core::config::Config::global_config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("credentials.json")
}

/// Save an API key to the credentials file.
async fn save_api_key(provider: &str, api_key: &str) -> anyhow::Result<()> {
    use std::collections::HashMap;

    let path = get_credentials_path();

    // Create directory if needed
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Load existing credentials
    let mut credentials: HashMap<String, String> = if path.exists() {
        let content = tokio::fs::read_to_string(&path).await?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        HashMap::new()
    };

    // Update credential
    credentials.insert(provider.to_string(), api_key.to_string());

    // Save back
    let content = serde_json::to_string_pretty(&credentials)?;
    tokio::fs::write(&path, content).await?;

    // Set restrictive permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&path, perms)?;
    }

    Ok(())
}

/// Remove a credential from the credentials file.
async fn remove_api_key(provider: &str) -> anyhow::Result<()> {
    use std::collections::HashMap;

    let path = get_credentials_path();

    if !path.exists() {
        return Ok(());
    }

    // Load existing credentials
    let content = tokio::fs::read_to_string(&path).await?;
    let mut credentials: HashMap<String, String> =
        serde_json::from_str(&content).unwrap_or_default();

    // Remove credential
    credentials.remove(provider);

    // Save back
    let content = serde_json::to_string_pretty(&credentials)?;
    tokio::fs::write(&path, content).await?;

    Ok(())
}

/// Handle session commands.
async fn handle_session(command: SessionCommands, cwd: &std::path::Path) -> anyhow::Result<()> {
    let instance = wonopcode_core::Instance::new(cwd).await?;

    match command {
        SessionCommands::List => {
            let sessions = instance.list_sessions().await;

            if sessions.is_empty() {
                println!("No sessions found.");
            } else {
                println!("Sessions:");
                println!();
                println!("{:<28} {:<30} {:<20}", "ID", "TITLE", "UPDATED");
                println!("{}", "-".repeat(78));

                for session in sessions {
                    let updated = session.updated_at().format("%Y-%m-%d %H:%M:%S");
                    let title = if session.title.len() > 28 {
                        format!("{}...", &session.title[..25])
                    } else {
                        session.title.clone()
                    };
                    println!("{:<28} {:<30} {:<20}", session.id, title, updated);
                }
            }
        }
        SessionCommands::Show { id } => match instance.get_session(&id).await {
            Some(session) => {
                println!("Session: {}", session.id);
                println!("Title: {}", session.title);
                println!("Project: {}", session.project_id);
                println!("Directory: {}", session.directory);
                println!(
                    "Created: {}",
                    session.created_at().format("%Y-%m-%d %H:%M:%S")
                );
                println!(
                    "Updated: {}",
                    session.updated_at().format("%Y-%m-%d %H:%M:%S")
                );
                if let Some(parent) = &session.parent_id {
                    println!("Parent: {}", parent);
                }
            }
            None => {
                println!("Session not found: {}", id);
            }
        },
        SessionCommands::Delete { id } => {
            let project_id = instance.project_id().await;
            match instance.session_repo().delete(&project_id, &id).await {
                Ok(_) => println!("Session deleted: {}", id),
                Err(e) => println!("Error deleting session: {}", e),
            }
        }
    }

    instance.dispose().await;
    Ok(())
}

/// Handle export command.
async fn handle_export(
    cwd: &std::path::Path,
    session_id: Option<String>,
    output: std::path::PathBuf,
    format: &str,
) -> anyhow::Result<()> {
    use wonopcode_core::message::MessagePart;
    use wonopcode_core::session::MessageWithParts;

    let instance = wonopcode_core::Instance::new(cwd).await?;
    let project_id = instance.project_id().await;

    // Collect sessions to export
    let sessions: Vec<_> = if let Some(id) = session_id {
        match instance.get_session(&id).await {
            Some(session) => vec![session],
            None => {
                eprintln!("Session not found: {}", id);
                instance.dispose().await;
                return Ok(());
            }
        }
    } else {
        instance.list_sessions().await
    };

    if sessions.is_empty() {
        println!("No sessions to export.");
        instance.dispose().await;
        return Ok(());
    }

    match format {
        "json" => {
            // Export as JSON
            #[derive(serde::Serialize)]
            struct ExportData {
                version: String,
                exported_at: String,
                sessions: Vec<SessionExport>,
            }

            #[derive(serde::Serialize)]
            struct SessionExport {
                session: wonopcode_core::session::Session,
                messages: Vec<MessageWithParts>,
            }

            let mut session_exports = Vec::new();

            for session in &sessions {
                let messages = instance
                    .session_repo()
                    .messages(&project_id, &session.id, None)
                    .await
                    .unwrap_or_default();

                session_exports.push(SessionExport {
                    session: session.clone(),
                    messages,
                });
            }

            let export_data = ExportData {
                version: env!("CARGO_PKG_VERSION").to_string(),
                exported_at: chrono::Utc::now().to_rfc3339(),
                sessions: session_exports,
            };

            let json = serde_json::to_string_pretty(&export_data)?;
            tokio::fs::write(&output, json).await?;

            println!(
                "Exported {} session(s) to {}",
                sessions.len(),
                output.display()
            );
        }
        "markdown" | "md" => {
            // Export as Markdown
            let mut content = String::new();

            content.push_str("# Wonopcode Session Export\n\n");
            content.push_str(&format!(
                "Exported: {}\n\n",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
            ));
            content.push_str(&format!("Sessions: {}\n\n", sessions.len()));
            content.push_str("---\n\n");

            for session in &sessions {
                content.push_str(&format!("## Session: {}\n\n", session.title));
                content.push_str(&format!("- **ID**: {}\n", session.id));
                content.push_str(&format!(
                    "- **Created**: {}\n",
                    session.created_at().format("%Y-%m-%d %H:%M:%S")
                ));
                content.push_str(&format!(
                    "- **Updated**: {}\n\n",
                    session.updated_at().format("%Y-%m-%d %H:%M:%S")
                ));

                let messages = instance
                    .session_repo()
                    .messages(&project_id, &session.id, None)
                    .await
                    .unwrap_or_default();

                for msg_with_parts in &messages {
                    let role = if msg_with_parts.message.is_user() {
                        "User"
                    } else {
                        "Assistant"
                    };
                    content.push_str(&format!("### {}\n\n", role));

                    for part in &msg_with_parts.parts {
                        match part {
                            MessagePart::Text(text_part) => {
                                content.push_str(&text_part.text);
                                content.push_str("\n\n");
                            }
                            MessagePart::Tool(tool_part) => {
                                content.push_str(&format!("**Tool: {}**\n", tool_part.tool));
                                // Get input from state
                                let input = match &tool_part.state {
                                    wonopcode_core::message::ToolState::Pending {
                                        input, ..
                                    } => Some(input),
                                    wonopcode_core::message::ToolState::Running {
                                        input, ..
                                    } => Some(input),
                                    wonopcode_core::message::ToolState::Completed {
                                        input, ..
                                    } => Some(input),
                                    wonopcode_core::message::ToolState::Error { input, .. } => {
                                        Some(input)
                                    }
                                };
                                if let Some(input) = input {
                                    content.push_str("```json\n");
                                    content.push_str(
                                        &serde_json::to_string_pretty(input).unwrap_or_default(),
                                    );
                                    content.push_str("\n```\n");
                                }
                                // Get output from completed state
                                if let wonopcode_core::message::ToolState::Completed {
                                    output,
                                    ..
                                } = &tool_part.state
                                {
                                    content.push_str("\n**Result:**\n```\n");
                                    content.push_str(output);
                                    content.push_str("\n```\n");
                                }
                                content.push('\n');
                            }
                            MessagePart::Reasoning(reasoning) => {
                                content.push_str("*Thinking...*\n\n");
                                content.push_str(&reasoning.text);
                                content.push_str("\n\n");
                            }
                            _ => {}
                        }
                    }
                }

                content.push_str("---\n\n");
            }

            tokio::fs::write(&output, content).await?;

            println!(
                "Exported {} session(s) to {}",
                sessions.len(),
                output.display()
            );
        }
        _ => {
            eprintln!(
                "Unknown export format: {}. Use 'json' or 'markdown'.",
                format
            );
        }
    }

    instance.dispose().await;
    Ok(())
}

/// Handle import command.
async fn handle_import(cwd: &std::path::Path, input: std::path::PathBuf) -> anyhow::Result<()> {
    use wonopcode_core::session::MessageWithParts;

    let instance = wonopcode_core::Instance::new(cwd).await?;
    let project_id = instance.project_id().await;

    // Read the file
    let content = tokio::fs::read_to_string(&input).await?;

    // Parse as JSON
    #[derive(serde::Deserialize)]
    struct ImportData {
        /// Export format version (for future compatibility checks).
        #[serde(default)]
        _version: Option<String>,
        /// Export timestamp (for informational purposes).
        #[serde(default)]
        _exported_at: Option<String>,
        sessions: Vec<SessionImport>,
    }

    #[derive(serde::Deserialize)]
    struct SessionImport {
        session: wonopcode_core::session::Session,
        messages: Vec<MessageWithParts>,
    }

    let import_data: ImportData = serde_json::from_str(&content)?;

    let mut imported = 0;
    let mut skipped = 0;

    for session_import in import_data.sessions {
        // Check if session already exists
        if instance
            .get_session(&session_import.session.id)
            .await
            .is_some()
        {
            println!(
                "Skipping existing session: {} ({})",
                session_import.session.id, session_import.session.title
            );
            skipped += 1;
            continue;
        }

        // Create a new session with the imported data
        let mut session = session_import.session.clone();
        session.project_id = project_id.clone();

        // Save session via the repository
        let repo = instance.session_repo();
        match repo.create(session.clone()).await {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error importing session {}: {}", session.id, e);
                continue;
            }
        }

        // Save messages and parts
        for msg_with_parts in &session_import.messages {
            if let Err(e) = repo.save_message(&msg_with_parts.message).await {
                eprintln!("Error importing message in session {}: {}", session.id, e);
            }
            for part in &msg_with_parts.parts {
                if let Err(e) = repo.save_part(part).await {
                    eprintln!(
                        "Error importing message part in session {}: {}",
                        session.id, e
                    );
                }
            }
        }

        println!("Imported session: {} ({})", session.id, session.title);
        imported += 1;
    }

    println!();
    println!(
        "Import complete: {} imported, {} skipped",
        imported, skipped
    );

    instance.dispose().await;
    Ok(())
}

/// Run interactive mode.
async fn run_interactive(cwd: &std::path::Path, cli: Cli) -> anyhow::Result<()> {
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

    // Create runner config
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
        mcp_url: None, // Use stdio transport for normal TUI mode
    };

    // Get MCP config from config file
    let mcp_configs = config_file.mcp.clone();

    // Check if update notification is ready (with timeout)
    let update_msg = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        update_notification,
    )
    .await
    .ok()
    .and_then(|r| r.ok())
    .flatten();

    if cli.basic {
        // Basic mode: simple line-based input
        // Show update notification if available
        if let Some(ref msg) = update_msg {
            eprintln!("{}", msg);
            eprintln!();
        }
        run_basic_mode(&instance, &config, cli.prompt, mcp_configs).await?;
    } else {
        // TUI mode - pass update notification to show as toast
        run_tui_mode(&instance, config, cli.prompt, mcp_configs, &config_file, update_msg).await?;
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
) -> anyhow::Result<()> {
    use std::io::{self, BufRead, Write};

    println!("Wonopcode v{}", env!("CARGO_PKG_VERSION"));
    println!("Working directory: {}", instance.directory().display());
    println!("Provider: {} / {}", config.provider, config.model_id);
    println!();

    // Create runner with full feature support
    let runner =
        match Runner::new_with_features(config.clone(), instance.clone(), mcp_configs).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error creating runner: {}", e);
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
        println!("You: {}", prompt);
        println!();
        let _ = action_tx.send(wonopcode_tui::AppAction::SendPrompt(prompt));

        // Wait for response
        let mut response_text = String::new();
        while let Some(update) = update_rx.recv().await {
            match update {
                wonopcode_tui::AppUpdate::TextDelta(delta) => {
                    print!("{}", delta);
                    io::stdout().flush()?;
                    response_text.push_str(&delta);
                }
                wonopcode_tui::AppUpdate::ToolStarted { name, .. } => {
                    println!("\n[Running tool: {}]", name);
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
                        println!("{}", text);
                    }
                    println!();
                    break;
                }
                wonopcode_tui::AppUpdate::Error(e) => {
                    eprintln!("\nError: {}", e);
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
                        print!("{}", delta);
                        io::stdout().flush()?;
                        response_text.push_str(&delta);
                    }
                    wonopcode_tui::AppUpdate::ToolStarted { name, .. } => {
                        println!("\n[Running tool: {}]", name);
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
                            println!("{}", text);
                        }
                        println!();
                        break;
                    }
                    wonopcode_tui::AppUpdate::Error(e) => {
                        eprintln!("\nError: {}", e);
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
async fn run_tui_mode(
    instance: &wonopcode_core::Instance,
    config: RunnerConfig,
    initial_prompt: Option<String>,
    mcp_configs: Option<std::collections::HashMap<String, wonopcode_core::config::McpConfig>>,
    app_config: &wonopcode_core::config::Config,
    update_notification: Option<String>,
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

    // Create runner with full feature support
    let runner = match Runner::new_with_features(config, instance.clone(), mcp_configs).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error creating runner: {}", e);
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
async fn run_headless(
    cwd: &std::path::Path,
    address: std::net::SocketAddr,
    cli: &Cli,
) -> anyhow::Result<()> {
    use tokio::sync::mpsc;
    use wonopcode_protocol::{Action, Update};
    use wonopcode_server::{create_headless_router_with_mcp, HeadlessState};

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

    // Build MCP HTTP URL for headless mode
    let mcp_sse_url = format!("http://{}/mcp/sse", address);

    // Create runner config with MCP HTTP transport
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
        mcp_url: Some(mcp_sse_url), // Use HTTP transport for MCP
    };

    // Get MCP config
    let mcp_configs = config_file.mcp.clone();

    // Create channels for action/update communication
    let (protocol_action_tx, mut protocol_action_rx) = mpsc::unbounded_channel::<Action>();
    let (app_action_tx, app_action_rx) = mpsc::unbounded_channel::<wonopcode_tui::AppAction>();
    let (app_update_tx, mut app_update_rx) = mpsc::unbounded_channel::<wonopcode_tui::AppUpdate>();

    // Create headless state
    let headless_state = HeadlessState::new(protocol_action_tx);
    let update_broadcast = headless_state.update_tx.clone();
    let state_handle = headless_state.current_state.clone();
    let _shutdown_flag = headless_state.shutdown.clone();

    // Set initial state
    {
        let mut state = state_handle.write().await;
        state.project = cwd.display().to_string();
        state.model = format!("{}/{}", provider, model_id);

        // Set initial sandbox state based on config
        if let Some(sandbox_cfg) = &config_file.sandbox {
            if sandbox_cfg.enabled.unwrap_or(false) {
                state.sandbox.state = "stopped".to_string();
                state.sandbox.runtime_type = Some("Auto".to_string());
            }
        }

        // Set config for settings dialog
        state.config = Some(wonopcode_protocol::ConfigState {
            sandbox: config_file.sandbox.as_ref().map(|s| {
                wonopcode_protocol::SandboxConfigState {
                    enabled: s.enabled.unwrap_or(false),
                    runtime: s.runtime.clone(),
                }
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
                                            } => {
                                                ("completed".to_string(), Some(output.clone()), true)
                                            }
                                            wonopcode_core::message::ToolState::Error {
                                                error,
                                                ..
                                            } => {
                                                ("failed".to_string(), Some(error.clone()), false)
                                            }
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
                                            } => {
                                                serde_json::to_string(input).unwrap_or_default()
                                            }
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
                                        content.push(wonopcode_protocol::MessageSegment::Thinking {
                                            text: reasoning.text,
                                        });
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
                });
                info!(session_id = %new_session_id, "Created new empty session for headless mode");
            }
        }
    }

    // Create runner
    let runner = match Runner::new_with_features(config, instance.clone(), mcp_configs).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error creating runner: {}", e);
            return Err(anyhow::anyhow!("Failed to create runner: {}", e));
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
                Action::Revert { message_id } => {
                    wonopcode_tui::AppAction::Revert { message_id }
                }
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
        // Track the current assistant message being built with ordered segments
        let mut current_message_segments: Vec<wonopcode_protocol::MessageSegment> = Vec::new();
        let mut current_message_id: Option<String> = None;

        while let Some(update) = app_update_rx.recv().await {
            // Update the current state based on the update type
            match &update {
                wonopcode_tui::AppUpdate::Started => {
                    // Start a new assistant message
                    current_message_segments.clear();
                    current_message_id = Some(uuid::Uuid::new_v4().to_string());
                }
                wonopcode_tui::AppUpdate::TextDelta(delta) => {
                    // Append to last text segment or create new one
                    if let Some(wonopcode_protocol::MessageSegment::Text { text }) =
                        current_message_segments.last_mut()
                    {
                        text.push_str(delta);
                    } else {
                        current_message_segments.push(wonopcode_protocol::MessageSegment::Text {
                            text: delta.clone(),
                        });
                    }
                }
                wonopcode_tui::AppUpdate::ToolStarted { id, name, input } => {
                    // Add tool segment in order
                    current_message_segments.push(wonopcode_protocol::MessageSegment::Tool {
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
                wonopcode_tui::AppUpdate::ToolCompleted {
                    id,
                    success,
                    output,
                    ..
                } => {
                    // Update tool status in segments
                    for segment in &mut current_message_segments {
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
                wonopcode_tui::AppUpdate::Completed { .. } => {
                    // Finalize current message and add to session
                    if let Some(msg_id) = current_message_id.take() {
                        let mut state = state_for_updates.write().await;
                        // Get model and agent from current state
                        let model = Some(state.model.clone());
                        let agent = Some(state.agent.clone());
                        if let Some(ref mut session) = state.session {
                            session.messages.push(wonopcode_protocol::Message {
                                id: msg_id,
                                role: "assistant".to_string(),
                                content: current_message_segments.clone(),
                                timestamp: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                                tool_calls: vec![], // Tools are now inline in content
                                model,
                                agent,
                            });
                        }
                        current_message_segments.clear();
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
                wonopcode_tui::AppUpdate::TodosUpdated(todos) => {
                    let mut state = state_for_updates.write().await;
                    state.todos = todos
                        .iter()
                        .map(|t| wonopcode_protocol::TodoInfo {
                            id: t.id.clone(),
                            content: t.content.clone(),
                            status: t.status.clone(),
                            priority: t.priority.clone(),
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
                        .map(|(id, title, timestamp)| wonopcode_protocol::SessionListItem {
                            id: id.clone(),
                            title: title.clone(),
                            timestamp: timestamp.clone(),
                        })
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
                wonopcode_tui::AppUpdate::TodosUpdated(todos) => Update::TodosUpdated {
                    todos: todos
                        .into_iter()
                        .map(|t| wonopcode_protocol::TodoInfo {
                            id: t.id,
                            content: t.content,
                            status: t.status,
                            priority: t.priority,
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
            };

            let _ = update_broadcast.send(protocol_update);
        }
    });

    // Create MCP HTTP state for tool serving
    let mcp_message_url = format!("http://{}/mcp/message", address);
    let mcp_state = create_mcp_http_state(cwd, &mcp_message_url).await.ok();
    let has_mcp = mcp_state.is_some();

    // Create router with MCP support
    let app = create_headless_router_with_mcp(headless_state, mcp_state);

    // Start server
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Server running on http://{}", address);
    if has_mcp {
        println!("MCP endpoint: http://{}/mcp/sse", address);
    }
    println!("Press Ctrl+C to stop");

    // Run server until shutdown
    axum::serve(listener, app).await?;

    // Wait for runner to complete
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), runner_handle).await;

    instance.dispose().await;
    Ok(())
}

/// Connect to a remote headless server.
async fn run_connect(address: &str, _cli: &Cli) -> anyhow::Result<()> {
    use wonopcode_tui::{App, Backend, RemoteBackend, SandboxStatusUpdate};

    // Parse address
    let address = if address.starts_with(':') {
        format!("127.0.0.1{}", address)
    } else {
        address.to_string()
    };

    println!("Connecting to {}...", address);

    // Create remote backend
    let backend = RemoteBackend::new(&address)?;

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
    let _ = update_tx.send(wonopcode_tui::AppUpdate::SandboxUpdated(sandbox_update));

    // Apply todos
    if !state.todos.is_empty() {
        let todos: Vec<wonopcode_tui::TodoUpdate> = state
            .todos
            .into_iter()
            .map(|t| wonopcode_tui::TodoUpdate {
                id: t.id,
                content: t.content,
                status: t.status,
                priority: t.priority,
            })
            .collect();
        let _ = update_tx.send(wonopcode_tui::AppUpdate::TodosUpdated(todos));
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
        let _ = update_tx.send(wonopcode_tui::AppUpdate::McpUpdated(servers));
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
        let _ = update_tx.send(wonopcode_tui::AppUpdate::LspUpdated(servers));
    }

    // Apply sessions list
    if !state.sessions.is_empty() {
        let sessions: Vec<(String, String, String)> = state
            .sessions
            .into_iter()
            .map(|s| (s.id, s.title, s.timestamp))
            .collect();
        let _ = update_tx.send(wonopcode_tui::AppUpdate::Sessions(sessions));
    }

    // Apply token usage
    let _ = update_tx.send(wonopcode_tui::AppUpdate::TokenUsage {
        input: state.token_usage.input,
        output: state.token_usage.output,
        cost: state.token_usage.cost,
        context_limit: state.context_limit,
    });

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
        let _ = update_tx.send(wonopcode_tui::AppUpdate::ModifiedFilesUpdated(files));
    }

    // Apply current session with messages
    if let Some(session) = state.session {
        let messages: Vec<wonopcode_tui::DisplayMessage> = session
            .messages
            .into_iter()
            .map(|msg| {
                // Convert protocol message to display message
                match msg.role.as_str() {
                    "user" => {
                        // Extract text from content segments
                        let text: String = msg
                            .content
                            .iter()
                            .filter_map(|seg| match seg {
                                wonopcode_protocol::MessageSegment::Text { text } => {
                                    Some(text.clone())
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        wonopcode_tui::DisplayMessage::user(text)
                    }
                    "assistant" => {
                        // Helper to convert protocol ToolCall to TUI DisplayToolCall
                        let convert_tool =
                            |tool: &wonopcode_protocol::ToolCall| -> wonopcode_tui::DisplayToolCall {
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
                                        "*Thinking:* {}",
                                        text
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
                                wonopcode_protocol::MessageSegment::Text { text } => {
                                    Some(text.clone())
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        wonopcode_tui::DisplayMessage::system(text)
                    }
                    _ => wonopcode_tui::DisplayMessage::system(format!(
                        "Unknown role: {}",
                        msg.role
                    )),
                }
            })
            .collect();

        let _ = update_tx.send(wonopcode_tui::AppUpdate::SessionLoaded {
            id: session.id,
            title: session.title,
            messages,
        });
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

/// Run MCP server for Claude CLI integration.
///
/// This exposes wonopcode's tools via the MCP protocol, allowing Claude CLI
/// to use our custom tools instead of its built-in ones.
async fn run_mcp_server(
    cwd: &std::path::Path,
    session_id: Option<String>,
    allow_all: bool,
) -> anyhow::Result<()> {
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;
    use tracing::{info, warn};
    use wonopcode_core::bus::Bus;
    use wonopcode_core::permission::{PermissionManager, PermissionRule};
    use wonopcode_mcp::{McpServer, McpToolContext};
    use wonopcode_snapshot::{SnapshotConfig, SnapshotStore};
    use wonopcode_tools::ToolRegistry;
    use wonopcode_util::FileTimeState;

    // Get session ID from arg or environment
    let session_id = session_id
        .or_else(|| std::env::var("WONOPCODE_SESSION_ID").ok())
        .unwrap_or_else(|| "mcp-default".to_string());

    // Get root dir from environment or use cwd
    let root_dir = std::env::var("WONOPCODE_ROOT_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| cwd.to_path_buf());

    // Create tool context
    let mcp_context = McpToolContext {
        session_id: session_id.clone(),
        cwd: cwd.to_path_buf(),
        root_dir: root_dir.clone(),
    };

    // Initialize permission manager
    // In MCP mode, we can't prompt the user, so we use rules-based decisions only.
    // If allow_all is set, we allow everything. Otherwise, we use default rules
    // and deny anything that would require user prompting.
    let bus = Bus::new();
    let permission_manager = Arc::new(PermissionManager::new(bus));

    // Add default rules for safe tools (read-only operations)
    for rule in PermissionManager::default_rules() {
        permission_manager.add_rule(rule).await;
    }

    // If allow_all is set, add a wildcard allow rule at the end
    if allow_all {
        permission_manager
            .add_rule(PermissionRule::allow("*"))
            .await;
        info!("MCP server running with --allow-all: all tools permitted");
    } else {
        // For write operations, allow within the project directory
        let project_path = format!("{}/**", root_dir.display());
        permission_manager
            .add_rule(PermissionRule::allow("write").with_path(&project_path))
            .await;
        permission_manager
            .add_rule(PermissionRule::allow("edit").with_path(&project_path))
            .await;
        permission_manager
            .add_rule(PermissionRule::allow("multiedit").with_path(&project_path))
            .await;
        permission_manager
            .add_rule(PermissionRule::allow("patch").with_path(&project_path))
            .await;
        // Allow todowrite (no path needed)
        permission_manager
            .add_rule(PermissionRule::allow("todowrite"))
            .await;
        // Allow bash with limited scope (project directory)
        permission_manager
            .add_rule(PermissionRule::allow("bash").with_path(&project_path))
            .await;
        info!(
            project_dir = %root_dir.display(),
            "MCP server running with project-scoped permissions"
        );
    }

    // Initialize snapshot store
    let snapshot_dir = root_dir.join(".wonopcode").join("snapshots");
    let snapshot_store =
        SnapshotStore::new(snapshot_dir, root_dir.clone(), SnapshotConfig::default())
            .await
            .ok()
            .map(Arc::new);

    // Initialize file time tracker
    let file_time = Arc::new(FileTimeState::new());

    // Initialize sandbox if configured
    // Keep the manager for cleanup on exit
    let (sandbox_runtime, sandbox_manager): (
        Option<Arc<dyn wonopcode_sandbox::SandboxRuntime>>,
        Option<Arc<wonopcode_sandbox::SandboxManager>>,
    ) = {
        use wonopcode_core::Instance;
        use wonopcode_sandbox::{SandboxConfig, SandboxManager, SandboxRuntimeType};

        // Check if sandbox is explicitly enabled/disabled via environment variable
        // This allows the parent process to override the config file setting
        let sandbox_env_var = std::env::var("WONOPCODE_SANDBOX_ENABLED").ok();
        let sandbox_env_override = sandbox_env_var.as_ref().map(|v| v == "true" || v == "1");

        info!(
            sandbox_env_var = ?sandbox_env_var,
            sandbox_env_override = ?sandbox_env_override,
            "MCP server checking sandbox env override"
        );

        // Load config to check sandbox settings
        let instance = Instance::new(cwd).await.ok();
        let sandbox_config = instance
            .as_ref()
            .map(|i| futures::executor::block_on(i.config()));

        if let Some(config) = sandbox_config {
            if let Some(sandbox_cfg) = &config.sandbox {
                // Use env override if set, otherwise use config file setting
                let sandbox_enabled =
                    sandbox_env_override.unwrap_or_else(|| sandbox_cfg.enabled.unwrap_or(false));

                info!(
                    sandbox_enabled = sandbox_enabled,
                    config_enabled = ?sandbox_cfg.enabled,
                    "MCP server sandbox decision"
                );

                if sandbox_enabled {
                    // Convert core config to sandbox config
                    let runtime = match sandbox_cfg.runtime.as_deref() {
                        Some("docker") => SandboxRuntimeType::Docker,
                        Some("podman") => SandboxRuntimeType::Podman,
                        Some("lima") => SandboxRuntimeType::Lima,
                        Some("none") => SandboxRuntimeType::None,
                        _ => SandboxRuntimeType::Auto,
                    };

                    let sandbox_config = SandboxConfig {
                        enabled: true,
                        runtime,
                        image: sandbox_cfg.image.clone(),
                        ..SandboxConfig::default()
                    };

                    match SandboxManager::new(sandbox_config, root_dir.clone()).await {
                        Ok(manager) => {
                            let manager = Arc::new(manager);
                            if manager.is_available() {
                                // Start the sandbox
                                if let Err(e) = manager.start().await {
                                    warn!(error = %e, "Failed to start sandbox in MCP server");
                                    (None, Some(manager))
                                } else {
                                    info!("Sandbox started in MCP server");
                                    let runtime = manager.runtime().await.ok();
                                    (runtime, Some(manager))
                                }
                            } else {
                                warn!("Sandbox enabled but no runtime available");
                                (None, None)
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to initialize sandbox in MCP server");
                            (None, None)
                        }
                    }
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            }
        } else {
            (None, None)
        }
    };

    // Use shared file todo store - reads path from WONOPCODE_TODO_FILE env var set by TUI.
    // This enables cross-process communication between MCP server and TUI.
    let todo_store: Arc<dyn wonopcode_tools::todo::TodoStore> =
        if let Some(store) = wonopcode_tools::todo::SharedFileTodoStore::from_env() {
            Arc::new(store)
        } else {
            // Fallback to in-memory if env var not set (shouldn't happen in normal usage)
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
    // Note: TaskTool and SkillTool require more complex setup, skipping for now

    // Create MCP server
    let mut server = McpServer::new("wonopcode-tools", env!("CARGO_PKG_VERSION"))
        .with_context(mcp_context.clone());

    // Register all tools from the registry
    let cancel = CancellationToken::new();
    for tool in tools.all() {
        let tool_clone = tool.clone();
        let snapshot = snapshot_store.clone();
        let ft = file_time.clone();
        let ctx_clone = mcp_context.clone();
        let cancel_clone = cancel.clone();
        let perm = permission_manager.clone();
        let sandbox = sandbox_runtime.clone();

        // Create an executor that wraps the wonopcode tool
        let has_sandbox = sandbox.is_some();
        let executor = ToolExecutorWrapper {
            tool: tool_clone,
            snapshot,
            file_time: ft,
            cancel: cancel_clone,
            permissions: perm,
            sandbox,
        };
        // Note: ctx_clone is not used as execute() receives context as parameter
        let _ = ctx_clone;

        if has_sandbox {
            info!(tool = tool.id(), "Registered tool with sandbox support");
        }

        server.register(wonopcode_mcp::McpServerTool {
            name: tool.id().to_string(),
            description: tool.description().to_string(),
            parameters: tool.parameters_schema(),
            executor: Arc::new(executor),
        });
    }

    info!(
        cwd = %cwd.display(),
        session_id = %session_id,
        tools = server.tool_count(),
        "Starting MCP server"
    );

    // Run the server on stdio (async version since we're in an async context)
    server.serve_stdio().await?;

    // Cleanup: stop sandbox on exit
    if let Some(ref manager) = sandbox_manager {
        if manager.is_ready().await {
            info!("Stopping sandbox container on MCP server exit");
            if let Err(e) = manager.stop().await {
                warn!(error = %e, "Failed to stop sandbox on MCP server exit");
            } else {
                info!("Sandbox container stopped");
            }
        }
    }

    Ok(())
}

/// Wrapper to execute wonopcode tools via MCP.
struct ToolExecutorWrapper {
    tool: Arc<dyn wonopcode_tools::Tool>,
    snapshot: Option<Arc<wonopcode_snapshot::SnapshotStore>>,
    file_time: Arc<wonopcode_util::FileTimeState>,
    cancel: tokio_util::sync::CancellationToken,
    permissions: Arc<wonopcode_core::permission::PermissionManager>,
    sandbox: Option<Arc<dyn wonopcode_sandbox::SandboxRuntime>>,
}

#[async_trait::async_trait]
impl wonopcode_mcp::McpToolExecutor for ToolExecutorWrapper {
    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &wonopcode_mcp::McpToolContext,
    ) -> Result<String, String> {
        let tool_name = self.tool.id();

        // Extract path from args for file-related tools
        let path = args
            .get("filePath")
            .or_else(|| args.get("path"))
            .or_else(|| args.get("file"))
            .and_then(|v| v.as_str())
            .map(String::from);

        // Check permission using rules only (no user prompting in MCP mode)
        let allowed = self
            .permissions
            .check_rules_only(&ctx.session_id, tool_name, Some("execute"), path.as_deref())
            .await;

        if !allowed {
            return Err(format!(
                "Permission denied for tool '{}'. Path: {:?}. \
                Use --allow-all flag when starting the MCP server to permit all operations.",
                tool_name, path
            ));
        }

        let tool_ctx = wonopcode_tools::ToolContext {
            session_id: ctx.session_id.clone(),
            message_id: "mcp".to_string(),
            agent: "claude-cli".to_string(),
            abort: self.cancel.clone(),
            root_dir: ctx.root_dir.clone(),
            cwd: ctx.cwd.clone(),
            snapshot: self.snapshot.clone(),
            file_time: Some(self.file_time.clone()),
            sandbox: self.sandbox.clone(),
        };

        let has_sandbox = self.sandbox.is_some();
        info!(
            tool = %tool_name,
            path = ?path,
            has_sandbox = has_sandbox,
            "MCP tool execution permitted"
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
                    text = format!("[sandbox] {}", text);
                }

                // For file-modifying tools, append metadata as JSON so the TUI can parse it
                if !output.metadata.is_null() && matches!(tool_name, "edit" | "write" | "multiedit" | "patch") {
                    text = format!("{}\n\n<!-- TOOL_METADATA: {} -->", text, output.metadata);
                }

                Ok(text)
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Create MCP HTTP state for the headless server.
///
/// This sets up the MCP tools to be served over HTTP/SSE instead of stdio,
/// allowing Claude CLI to connect to the headless server.
async fn create_mcp_http_state(
    cwd: &std::path::Path,
    message_url: &str,
) -> anyhow::Result<wonopcode_mcp::McpHttpState> {
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;
    use wonopcode_core::bus::Bus;
    use wonopcode_core::permission::{PermissionManager, PermissionRule};
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

    // Initialize permission manager - allow all in headless mode
    let bus = Bus::new();
    let permission_manager = Arc::new(PermissionManager::new(bus));

    // Add default rules and allow all for headless HTTP mode
    for rule in PermissionManager::default_rules() {
        permission_manager.add_rule(rule).await;
    }
    permission_manager
        .add_rule(PermissionRule::allow("*"))
        .await;

    // Initialize snapshot store
    let snapshot_dir = root_dir.join(".wonopcode").join("snapshots");
    let snapshot_store = SnapshotStore::new(snapshot_dir, root_dir.clone(), SnapshotConfig::default())
        .await
        .ok()
        .map(Arc::new);

    // Initialize file time tracker
    let file_time = Arc::new(FileTimeState::new());

    // Use shared file todo store
    let todo_store: Arc<dyn wonopcode_tools::todo::TodoStore> =
        if let Some(store) = wonopcode_tools::todo::SharedFileTodoStore::from_env() {
            Arc::new(store)
        } else {
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
            sandbox: None, // No sandbox for HTTP MCP (could be added later)
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

/// Handle GitHub commands (requires --features github).
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

/// Run web server (headless mode).
async fn run_web_server(
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
        println!("  Local access:      {}", local_url);

        // Try to find network IPs
        if let Ok(interfaces) = get_network_ips() {
            for ip in interfaces {
                println!("  Network access:    http://{}:{}", ip, address.port());
            }
        }
        local_url
    } else {
        let url = format!("http://{}", address);
        println!("  Web interface:     {}", url);
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
fn get_network_ips() -> std::io::Result<Vec<String>> {
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

/// Handle MCP commands.
async fn handle_mcp(command: McpCommands, cwd: &std::path::Path) -> anyhow::Result<()> {
    match command {
        McpCommands::Add {
            name,
            server_type,
            command,
            url,
        } => handle_mcp_add(&name, &server_type, command, url, cwd).await,
        McpCommands::List => handle_mcp_list(cwd).await,
        McpCommands::Auth { name } => handle_mcp_auth(&name, cwd).await,
        McpCommands::Logout { name } => handle_mcp_logout(&name).await,
    }
}

/// Add an MCP server.
async fn handle_mcp_add(
    name: &str,
    server_type: &str,
    command: Option<String>,
    url: Option<String>,
    _cwd: &std::path::Path,
) -> anyhow::Result<()> {
    use std::io::{self, Write};

    println!();
    println!("Add MCP Server");
    println!("==============");
    println!();

    match server_type {
        "local" => {
            let cmd = if let Some(c) = command {
                c
            } else {
                print!("Enter command to run: ");
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                input.trim().to_string()
            };

            if cmd.is_empty() {
                println!("Error: Command is required for local servers");
                return Ok(());
            }

            println!();
            println!("Add the following to your wonopcode.json:");
            println!();
            println!(r#"  "mcp": {{"#);
            println!(r#"    "{}": {{"#, name);
            println!(r#"      "type": "local","#);
            println!(r#"      "command": ["{}"]"#, cmd.replace('"', r#"\""#));
            println!(r#"    }}"#);
            println!(r#"  }}"#);
        }
        "remote" => {
            let server_url = if let Some(u) = url {
                u
            } else {
                print!("Enter server URL: ");
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                input.trim().to_string()
            };

            if server_url.is_empty() {
                println!("Error: URL is required for remote servers");
                return Ok(());
            }

            println!();
            println!("Add the following to your wonopcode.json:");
            println!();
            println!(r#"  "mcp": {{"#);
            println!(r#"    "{}": {{"#, name);
            println!(r#"      "type": "remote","#);
            println!(r#"      "url": "{}""#, server_url);
            println!(r#"    }}"#);
            println!(r#"  }}"#);
        }
        _ => {
            println!(
                "Error: Invalid server type '{}'. Use 'local' or 'remote'.",
                server_type
            );
        }
    }

    println!();
    Ok(())
}

/// List MCP servers.
async fn handle_mcp_list(cwd: &std::path::Path) -> anyhow::Result<()> {
    println!();
    println!("MCP Servers");
    println!("===========");
    println!();

    // Load config to get MCP servers
    let (config, _) = wonopcode_core::config::Config::load(Some(cwd)).await?;

    match config.mcp {
        Some(servers) if !servers.is_empty() => {
            for (name, server_config) in &servers {
                let status_icon = "○"; // We'd need actual connection status
                let type_info = match server_config {
                    wonopcode_core::config::McpConfig::Local(local) => {
                        format!("local: {}", local.command.join(" "))
                    }
                    wonopcode_core::config::McpConfig::Remote(remote) => {
                        format!("remote: {}", remote.url)
                    }
                };
                println!("  {} {} ({})", status_icon, name, type_info);
            }
            println!();
            println!("{} server(s) configured", servers.len());
        }
        _ => {
            println!("No MCP servers configured.");
            println!();
            println!("Add servers with: wonopcode mcp add <name>");
            println!("Or add to wonopcode.json manually.");
        }
    }

    println!();
    Ok(())
}

/// Authenticate with an MCP server.
async fn handle_mcp_auth(name: &str, cwd: &std::path::Path) -> anyhow::Result<()> {
    println!();
    println!("MCP OAuth Authentication");
    println!("========================");
    println!();

    // Load config
    let (config, _) = wonopcode_core::config::Config::load(Some(cwd)).await?;

    let servers = match config.mcp {
        Some(s) => s,
        None => {
            println!("No MCP servers configured.");
            return Ok(());
        }
    };

    let server_config = match servers.get(name) {
        Some(c) => c,
        None => {
            println!("MCP server '{}' not found.", name);
            println!();
            println!("Available servers:");
            for name in servers.keys() {
                println!("  - {}", name);
            }
            return Ok(());
        }
    };

    match server_config {
        wonopcode_core::config::McpConfig::Remote(remote) => {
            println!("Server: {}", name);
            println!("URL: {}", remote.url);
            println!();
            println!("OAuth authentication is initiated when connecting to the server.");
            println!("The server will redirect you to complete authentication in your browser.");
            println!();
            println!("To test the connection, restart wonopcode and the OAuth flow will begin.");
        }
        wonopcode_core::config::McpConfig::Local(_) => {
            println!(
                "Server '{}' is a local server and doesn't support OAuth.",
                name
            );
        }
    }

    println!();
    Ok(())
}

/// Remove MCP OAuth credentials.
async fn handle_mcp_logout(name: &str) -> anyhow::Result<()> {
    println!();
    println!("MCP OAuth Logout");
    println!("================");
    println!();

    // Try to remove credentials
    let credentials_path = wonopcode_core::config::Config::global_config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("mcp-auth.json");

    if !credentials_path.exists() {
        println!("No MCP OAuth credentials stored.");
        return Ok(());
    }

    // Read and modify credentials
    let content = tokio::fs::read_to_string(&credentials_path).await?;
    let mut credentials: std::collections::HashMap<String, serde_json::Value> =
        serde_json::from_str(&content).unwrap_or_default();

    if credentials.remove(name).is_some() {
        let new_content = serde_json::to_string_pretty(&credentials)?;
        tokio::fs::write(&credentials_path, new_content).await?;
        println!("Removed OAuth credentials for '{}'.", name);
    } else {
        println!("No credentials found for '{}'.", name);
    }

    println!();
    Ok(())
}

/// Handle agent commands.
async fn handle_agent(command: AgentCommands, cwd: &std::path::Path) -> anyhow::Result<()> {
    use wonopcode_core::agent::AgentRegistry;
    use wonopcode_core::config::Config;

    // Load configuration
    let (config, _) = Config::load(Some(cwd)).await.unwrap_or_default();

    // Create agent registry
    let registry = AgentRegistry::new(&config);

    match command {
        AgentCommands::List => {
            println!();
            println!("Available Agents");
            println!("================");
            println!();

            // Primary agents
            let primary = registry.primary_agents();
            if !primary.is_empty() {
                println!("Primary Agents (user-selectable):");
                println!();
                for agent in primary {
                    let default_marker = if agent.is_default { " (default)" } else { "" };
                    let desc = agent.description.as_deref().unwrap_or("");
                    println!("  {:<12} {}{}", agent.name, desc, default_marker);
                }
                println!();
            }

            // Subagents
            let subagents: Vec<_> = registry
                .subagents()
                .into_iter()
                .filter(|a| !a.hidden)
                .collect();
            if !subagents.is_empty() {
                println!("Subagents (spawned by Task tool):");
                println!();
                for agent in subagents {
                    let desc = agent.description.as_deref().unwrap_or("");
                    println!("  {:<12} {}", agent.name, desc);
                }
                println!();
            }

            // Custom agents
            let custom: Vec<_> = registry.all().filter(|a| !a.native && !a.hidden).collect();
            if !custom.is_empty() {
                println!("Custom Agents:");
                println!();
                for agent in custom {
                    let desc = agent.description.as_deref().unwrap_or("");
                    println!("  {:<12} {}", agent.name, desc);
                }
                println!();
            }

            println!("Use 'wonopcode agent show <name>' for details.");
            println!();
        }
        AgentCommands::Show { name } => {
            match registry.get(&name) {
                Some(agent) => {
                    println!();
                    println!("Agent: {}", agent.name);
                    println!("======={}=", "=".repeat(agent.name.len()));
                    println!();

                    if let Some(desc) = &agent.description {
                        println!("Description: {}", desc);
                        println!();
                    }

                    println!("Properties:");
                    println!("  Mode:     {:?}", agent.mode);
                    println!("  Native:   {}", if agent.native { "yes" } else { "no" });
                    println!(
                        "  Default:  {}",
                        if agent.is_default { "yes" } else { "no" }
                    );
                    println!("  Hidden:   {}", if agent.hidden { "yes" } else { "no" });

                    if let Some(model) = &agent.model {
                        println!("  Model:    {}", model);
                    }
                    if let Some(temp) = agent.temperature {
                        println!("  Temp:     {}", temp);
                    }
                    if let Some(top_p) = agent.top_p {
                        println!("  Top-p:    {}", top_p);
                    }
                    if let Some(max_steps) = agent.max_steps {
                        println!("  Max steps: {}", max_steps);
                    }
                    if let Some(color) = &agent.color {
                        println!("  Color:    {}", color);
                    }

                    println!();
                    println!("Permissions:");
                    println!("  Edit:     {:?}", agent.permission.edit);
                    println!("  Webfetch: {:?}", agent.permission.webfetch);
                    if let Some(doom) = &agent.permission.doom_loop {
                        println!("  Doom loop: {:?}", doom);
                    }
                    if let Some(ext) = &agent.permission.external_directory {
                        println!("  External dir: {:?}", ext);
                    }

                    // Show bash permissions
                    if !agent.permission.bash.is_empty() {
                        println!();
                        println!("Bash permissions:");
                        for (pattern, perm) in &agent.permission.bash {
                            println!("  {:<20} {:?}", pattern, perm);
                        }
                    }

                    // Show tools
                    if !agent.tools.is_empty() {
                        println!();
                        println!("Tool overrides:");
                        for (tool, enabled) in &agent.tools {
                            let status = if *enabled { "enabled" } else { "disabled" };
                            println!("  {:<12} {}", tool, status);
                        }
                    }

                    if let Some(prompt) = &agent.prompt {
                        println!();
                        println!("Custom prompt:");
                        // Truncate long prompts
                        let display = if prompt.len() > 200 {
                            format!("{}...", &prompt[..200])
                        } else {
                            prompt.clone()
                        };
                        println!("  {}", display.replace('\n', "\n  "));
                    }

                    println!();
                }
                None => {
                    println!("Agent '{}' not found.", name);
                    println!();
                    println!("Available agents:");
                    for agent in registry.all().filter(|a| !a.hidden) {
                        println!("  - {}", agent.name);
                    }
                }
            }
        }
    }

    Ok(())
}
