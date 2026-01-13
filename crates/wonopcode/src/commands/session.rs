//! Session management command handlers.
//!
//! Handles listing, showing, and deleting sessions.

use clap::Subcommand;
use std::path::Path;

/// Session subcommands.
#[derive(Subcommand)]
pub enum SessionCommands {
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

/// Handle session commands.
pub async fn handle_session(command: SessionCommands, cwd: &Path) -> anyhow::Result<()> {
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
                    println!("Parent: {parent}");
                }
            }
            None => {
                println!("Session not found: {id}");
            }
        },
        SessionCommands::Delete { id } => {
            let project_id = instance.project_id().await;
            match instance.session_repo().delete(&project_id, &id).await {
                Ok(_) => println!("Session deleted: {id}"),
                Err(e) => println!("Error deleting session: {e}"),
            }
        }
    }

    instance.dispose().await;
    Ok(())
}
