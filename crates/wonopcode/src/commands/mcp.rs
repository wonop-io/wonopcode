//! MCP (Model Context Protocol) management command handlers.
//!
//! Handles adding, listing, and authenticating MCP servers.

use clap::Subcommand;
use std::path::Path;
use wonopcode_core::config::{Config, McpConfig, McpLocalConfig, McpRemoteConfig};

/// MCP subcommands.
#[derive(Subcommand)]
pub enum McpCommands {
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

/// Handle MCP commands.
pub async fn handle_mcp(command: McpCommands, cwd: &Path) -> anyhow::Result<()> {
    match command {
        McpCommands::Add {
            name,
            server_type,
            command,
            url,
        } => {
            add_mcp_server(cwd, &name, &server_type, command, url).await?;
        }
        McpCommands::List => {
            list_mcp_servers(cwd).await?;
        }
        McpCommands::Auth { name } => {
            auth_mcp_server(cwd, &name).await?;
        }
        McpCommands::Logout { name } => {
            logout_mcp_server(&name).await?;
        }
    }
    Ok(())
}

/// Add an MCP server to the configuration.
async fn add_mcp_server(
    cwd: &Path,
    name: &str,
    server_type: &str,
    command: Option<String>,
    url: Option<String>,
) -> anyhow::Result<()> {
    // Load existing config
    let (mut config, _) = Config::load(Some(cwd)).await?;

    // Validate inputs and create config
    let mcp_config = match server_type {
        "local" => {
            let cmd = command
                .ok_or_else(|| anyhow::anyhow!("Local MCP server requires --command argument"))?;
            // Simple split on whitespace - for complex commands, use config file
            let parts: Vec<String> = cmd.split_whitespace().map(|s| s.to_string()).collect();
            if parts.is_empty() {
                return Err(anyhow::anyhow!("Command cannot be empty"));
            }
            McpConfig::Local(McpLocalConfig {
                command: parts,
                environment: None,
                enabled: Some(true),
                timeout: None,
            })
        }
        "remote" | "http" | "sse" => {
            let remote_url =
                url.ok_or_else(|| anyhow::anyhow!("Remote MCP server requires --url argument"))?;
            McpConfig::Remote(McpRemoteConfig {
                url: remote_url,
                enabled: Some(true),
                headers: None,
                oauth: None,
                timeout: None,
            })
        }
        _ => {
            return Err(anyhow::anyhow!(
                "Unknown server type: {server_type}. Use 'local' or 'remote'."
            ));
        }
    };

    // Add to config
    let mcp_servers = config.mcp.get_or_insert_with(Default::default);
    mcp_servers.insert(name.to_string(), mcp_config);

    // Save config
    config.save_partial(Some(cwd)).await?;

    println!("✓ Added MCP server '{name}'");
    println!();
    println!("Configuration saved. The server will be available on next start.");

    Ok(())
}

/// List configured MCP servers.
async fn list_mcp_servers(cwd: &Path) -> anyhow::Result<()> {
    let (config, _) = Config::load(Some(cwd)).await?;

    let Some(mcp_servers) = config.mcp else {
        println!("No MCP servers configured.");
        return Ok(());
    };

    if mcp_servers.is_empty() {
        println!("No MCP servers configured.");
        return Ok(());
    }

    println!("MCP Servers:");
    println!();
    println!("{:<20} {:<10} {}", "NAME", "TYPE", "ENDPOINT");
    println!("{}", "-".repeat(60));

    for (name, cfg) in &mcp_servers {
        let (server_type, endpoint) = match cfg {
            McpConfig::Local(local) => {
                let cmd = local.command.join(" ");
                ("local", cmd)
            }
            McpConfig::Remote(remote) => ("remote", remote.url.clone()),
        };

        // Truncate endpoint if too long
        let endpoint_display = if endpoint.len() > 30 {
            format!("{}...", &endpoint[..27])
        } else {
            endpoint
        };

        println!("{:<20} {:<10} {}", name, server_type, endpoint_display);
    }

    Ok(())
}

/// Authenticate with an OAuth-enabled MCP server.
async fn auth_mcp_server(cwd: &Path, name: &str) -> anyhow::Result<()> {
    let (config, _) = Config::load(Some(cwd)).await?;

    let Some(mcp_servers) = config.mcp else {
        return Err(anyhow::anyhow!("No MCP servers configured"));
    };

    let Some(server_config) = mcp_servers.get(name) else {
        return Err(anyhow::anyhow!("MCP server '{name}' not found"));
    };

    let McpConfig::Remote(remote) = server_config else {
        return Err(anyhow::anyhow!(
            "MCP server '{name}' is not a remote server (OAuth only available for remote servers)"
        ));
    };

    // Check if OAuth is configured
    if remote.oauth.is_none() {
        return Err(anyhow::anyhow!(
            "MCP server '{name}' does not have OAuth configured"
        ));
    }

    println!("OAuth authentication for MCP servers is not yet implemented in CLI.");
    println!("Server URL: {}", remote.url);
    println!();
    println!("To configure OAuth, add the oauth section to your config:");
    println!("  {{");
    println!("    \"mcp\": {{");
    println!("      \"{name}\": {{");
    println!("        \"type\": \"remote\",");
    println!("        \"url\": \"{}\",", remote.url);
    println!("        \"oauth\": {{");
    println!("          \"client_id\": \"your-client-id\",");
    println!("          \"client_secret\": \"your-client-secret\"");
    println!("        }}");
    println!("      }}");
    println!("    }}");
    println!("  }}");

    Ok(())
}

/// Remove OAuth credentials for an MCP server.
async fn logout_mcp_server(name: &str) -> anyhow::Result<()> {
    println!("OAuth logout for MCP server '{name}' is not yet implemented.");
    println!();
    println!("To remove credentials manually, delete any stored tokens from:");
    println!("  ~/.config/wonopcode/mcp-tokens/{name}.json");

    Ok(())
}
