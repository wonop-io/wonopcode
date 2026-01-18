//! Agent management command handlers.
//!
//! Handles listing available agents and showing detailed agent configuration.

use clap::Subcommand;
use std::path::Path;

/// Agent subcommands.
#[derive(Subcommand)]
pub enum AgentCommands {
    /// List available agents
    List,
    /// Show details for an agent
    Show {
        /// Agent name
        name: String,
    },
}

/// Handle agent commands.
#[allow(clippy::cognitive_complexity)]
pub async fn handle_agent(command: AgentCommands, cwd: &Path) -> anyhow::Result<()> {
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
                        println!("Description: {desc}");
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
                        println!("  Model:    {model}");
                    }
                    if let Some(temp) = agent.temperature {
                        println!("  Temp:     {temp}");
                    }
                    if let Some(top_p) = agent.top_p {
                        println!("  Top-p:    {top_p}");
                    }
                    if let Some(max_steps) = agent.max_steps {
                        println!("  Max steps: {max_steps}");
                    }
                    if let Some(color) = &agent.color {
                        println!("  Color:    {color}");
                    }

                    println!();
                    println!("Permissions:");
                    println!("  Edit:     {:?}", agent.permission.edit);
                    println!("  Webfetch: {:?}", agent.permission.webfetch);
                    if let Some(doom) = &agent.permission.doom_loop {
                        println!("  Doom loop: {doom:?}");
                    }
                    if let Some(ext) = &agent.permission.external_directory {
                        println!("  External dir: {ext:?}");
                    }

                    // Show bash permissions
                    if !agent.permission.bash.is_empty() {
                        println!();
                        println!("Bash permissions:");
                        for (pattern, perm) in &agent.permission.bash {
                            println!("  {pattern:<20} {perm:?}");
                        }
                    }

                    // Show tools
                    if !agent.tools.is_empty() {
                        println!();
                        println!("Tool overrides:");
                        for (tool, enabled) in &agent.tools {
                            let status = if *enabled { "enabled" } else { "disabled" };
                            println!("  {tool:<12} {status}");
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
                    println!("Agent '{name}' not found.");
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
