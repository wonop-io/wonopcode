//! Authentication command handlers.
//!
//! Handles login, logout, and status commands for various AI providers.

use crate::runner;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::Command;

use clap::Subcommand;
use wonopcode_provider::claude_cli::ClaudeCliProvider;

/// Authentication subcommands.
#[derive(Subcommand)]
pub enum AuthCommands {
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

/// Handle authentication commands.
pub async fn handle_auth(command: AuthCommands) -> anyhow::Result<()> {
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
    // Validate provider and get key URL
    let key_url = match provider {
        "anthropic" => "https://console.anthropic.com/settings/keys",
        "openai" => "https://platform.openai.com/api-keys",
        "openrouter" => "https://openrouter.ai/keys",
        _ => {
            eprintln!("Unknown provider: {provider}");
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
            eprintln!("Error running Claude CLI: {e}");
            eprintln!();
            eprintln!("Make sure Claude Code CLI is installed:");
            eprintln!("  npm install -g @anthropic-ai/claude-code");
        }
    }

    Ok(())
}

/// Login with a manual API key.
async fn auth_login_api_key(provider: &str, key_url: &str) -> anyhow::Result<()> {
    // Check if already authenticated
    if let Some(key) = runner::load_api_key(provider) {
        if !key.is_empty() {
            println!("Already authenticated with {provider}.");
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
    println!("To get an API key for {provider}, visit:");
    println!("  {key_url}");
    println!();

    // Prompt for API key
    print!("Enter your {provider} API key: ");
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
        println!("Warning: The API key doesn't match the expected format for {provider}.");
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
    println!("✓ API key saved for {provider}.");
    println!();
    println!("You can now use wonopcode with --provider {provider}");

    Ok(())
}

/// Log out from a provider by removing the API key from config.
async fn auth_logout(provider: &str) -> anyhow::Result<()> {
    // Validate provider
    match provider {
        "anthropic" | "openai" | "openrouter" => {}
        _ => {
            eprintln!("Unknown provider: {provider}");
            return Ok(());
        }
    }

    // Remove from config
    remove_api_key(provider).await?;

    println!("✓ Logged out from {provider}.");

    Ok(())
}

/// Show authentication status.
async fn auth_status() -> anyhow::Result<()> {
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
        println!("  {:<12} {}", format!("{provider}:"), status);
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
fn read_password_or_line() -> io::Result<String> {
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// Get environment variable name for a provider.
pub fn get_env_var(provider: &str) -> &'static str {
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
    format!("{prefix}...{suffix}")
}

/// Get the credentials file path.
pub fn get_credentials_path() -> PathBuf {
    wonopcode_core::config::Config::global_config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("credentials.json")
}

/// Save an API key to the credentials file.
pub async fn save_api_key(provider: &str, api_key: &str) -> anyhow::Result<()> {
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
pub async fn remove_api_key(provider: &str) -> anyhow::Result<()> {
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
