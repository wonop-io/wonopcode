//! Export and import command handlers.
//!
//! Handles exporting and importing sessions in JSON or Markdown format.

use std::path::{Path, PathBuf};
use wonopcode_core::message::MessagePart;
use wonopcode_core::session::MessageWithParts;

/// Handle export command.
pub async fn handle_export(
    cwd: &Path,
    session_id: Option<String>,
    output: PathBuf,
    format: &str,
) -> anyhow::Result<()> {
    let instance = wonopcode_core::Instance::new(cwd).await?;
    let project_id = instance.project_id().await;

    // Collect sessions to export
    let sessions: Vec<_> = if let Some(id) = session_id {
        match instance.get_session(&id).await {
            Some(session) => vec![session],
            None => {
                eprintln!("Session not found: {id}");
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
            export_json(&instance, &project_id, &sessions, &output).await?;
        }
        "markdown" | "md" => {
            export_markdown(&instance, &project_id, &sessions, &output).await?;
        }
        _ => {
            eprintln!("Unknown export format: {format}. Use 'json' or 'markdown'.");
        }
    }

    instance.dispose().await;
    Ok(())
}

/// Export sessions as JSON.
async fn export_json(
    instance: &wonopcode_core::Instance,
    project_id: &str,
    sessions: &[wonopcode_core::session::Session],
    output: &Path,
) -> anyhow::Result<()> {
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

    for session in sessions {
        let messages = instance
            .session_repo()
            .messages(project_id, &session.id, None)
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
    tokio::fs::write(output, json).await?;

    println!(
        "Exported {} session(s) to {}",
        sessions.len(),
        output.display()
    );

    Ok(())
}

/// Export sessions as Markdown.
async fn export_markdown(
    instance: &wonopcode_core::Instance,
    project_id: &str,
    sessions: &[wonopcode_core::session::Session],
    output: &Path,
) -> anyhow::Result<()> {
    let mut content = String::new();

    content.push_str("# Wonopcode Session Export\n\n");
    content.push_str(&format!(
        "Exported: {}\n\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    ));
    content.push_str(&format!("Sessions: {}\n\n", sessions.len()));
    content.push_str("---\n\n");

    for session in sessions {
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
            .messages(project_id, &session.id, None)
            .await
            .unwrap_or_default();

        for msg_with_parts in &messages {
            let role = if msg_with_parts.message.is_user() {
                "User"
            } else {
                "Assistant"
            };
            content.push_str(&format!("### {role}\n\n"));

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
                            wonopcode_core::message::ToolState::Pending { input, .. } => {
                                Some(input)
                            }
                            wonopcode_core::message::ToolState::Running { input, .. } => {
                                Some(input)
                            }
                            wonopcode_core::message::ToolState::Completed { input, .. } => {
                                Some(input)
                            }
                            wonopcode_core::message::ToolState::Error { input, .. } => Some(input),
                        };
                        if let Some(input) = input {
                            content.push_str("```json\n");
                            content
                                .push_str(&serde_json::to_string_pretty(input).unwrap_or_default());
                            content.push_str("\n```\n");
                        }
                        // Get output from completed state
                        if let wonopcode_core::message::ToolState::Completed { output, .. } =
                            &tool_part.state
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

    tokio::fs::write(output, content).await?;

    println!(
        "Exported {} session(s) to {}",
        sessions.len(),
        output.display()
    );

    Ok(())
}

/// Handle import command.
pub async fn handle_import(cwd: &Path, input: PathBuf) -> anyhow::Result<()> {
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
    println!("Import complete: {imported} imported, {skipped} skipped");

    instance.dispose().await;
    Ok(())
}
