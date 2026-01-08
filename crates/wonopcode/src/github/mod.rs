//! GitHub integration for wonopcode.
//!
//! This module provides GitHub Actions integration and PR management.

mod api;
mod event;
mod pr;

pub use api::GitHubClient;
pub use pr::checkout_pr;

use anyhow::Result;
use tracing::info;

/// Run the GitHub agent in response to a workflow event.
pub async fn run_agent(
    cwd: &std::path::Path,
    event_path: Option<&str>,
    token: Option<&str>,
) -> Result<()> {
    // Load event from environment or file
    let event = if let Some(path) = event_path {
        let content = tokio::fs::read_to_string(path).await?;
        serde_json::from_str(&content)?
    } else if let Ok(path) = std::env::var("GITHUB_EVENT_PATH") {
        let content = tokio::fs::read_to_string(&path).await?;
        serde_json::from_str(&content)?
    } else {
        anyhow::bail!("No GitHub event provided. Set GITHUB_EVENT_PATH or use --event");
    };

    // Get token
    let token = token
        .map(String::from)
        .or_else(|| std::env::var("GITHUB_TOKEN").ok())
        .ok_or_else(|| anyhow::anyhow!("No GitHub token provided"))?;

    // Parse event type
    let event_name = std::env::var("GITHUB_EVENT_NAME").unwrap_or_else(|_| "unknown".to_string());

    info!("Processing GitHub event: {}", event_name);

    // Create GitHub client
    let client = GitHubClient::new(&token)?;

    // Handle the event
    match event_name.as_str() {
        "issue_comment" => handle_issue_comment(&client, &event, cwd).await?,
        "pull_request_review_comment" => handle_pr_review_comment(&client, &event, cwd).await?,
        "issues" => handle_issue(&client, &event, cwd).await?,
        "pull_request" => handle_pull_request(&client, &event, cwd).await?,
        "workflow_dispatch" => handle_workflow_dispatch(&client, &event, cwd).await?,
        _ => {
            info!("Ignoring unsupported event type: {}", event_name);
        }
    }

    Ok(())
}

/// Handle issue comment event.
async fn handle_issue_comment(
    client: &GitHubClient,
    event: &serde_json::Value,
    _cwd: &std::path::Path,
) -> Result<()> {
    let action = event["action"].as_str().unwrap_or("");
    if action != "created" {
        return Ok(());
    }

    let comment_body = event["comment"]["body"].as_str().unwrap_or("");
    let issue_number = event["issue"]["number"].as_u64().unwrap_or(0);
    let repo = event["repository"]["full_name"].as_str().unwrap_or("");

    info!("Processing issue comment on {}: #{}", repo, issue_number);

    // Check if this is a trigger comment
    if !is_trigger_comment(comment_body) {
        info!("Comment does not contain trigger phrase, ignoring");
        return Ok(());
    }

    // Extract the prompt from the comment
    let prompt = extract_prompt(comment_body);

    // Add reaction to indicate processing
    let comment_id = event["comment"]["id"].as_u64().unwrap_or(0);
    let (owner, repo_name) = parse_repo_name(repo)?;
    client
        .add_reaction(&owner, &repo_name, comment_id, "eyes")
        .await?;

    // Process the request
    // DEFERRED: GitHub integration - run agent and create PR
    // Implementation should:
    // 1. Clone/checkout the repository to a temp directory
    // 2. Create a new Runner with the cloned repo as cwd
    // 3. Run the agent with the extracted prompt
    // 4. Collect code changes made by the agent
    // 5. Create a new branch and commit the changes
    // 6. Push the branch and create a PR via GitHub API
    // 7. Reply to the original comment with the PR link
    // Priority: Low (GitHub Actions integration is advanced feature)

    info!("Would process prompt: {}", prompt);

    Ok(())
}

/// Handle PR review comment event.
async fn handle_pr_review_comment(
    _client: &GitHubClient,
    event: &serde_json::Value,
    _cwd: &std::path::Path,
) -> Result<()> {
    let action = event["action"].as_str().unwrap_or("");
    if action != "created" {
        return Ok(());
    }

    let comment_body = event["comment"]["body"].as_str().unwrap_or("");
    let pr_number = event["pull_request"]["number"].as_u64().unwrap_or(0);

    if !is_trigger_comment(comment_body) {
        return Ok(());
    }

    info!("Processing PR review comment on PR #{}", pr_number);

    // DEFERRED: GitHub integration - handle PR review comments
    // Implementation should:
    // 1. Checkout the PR branch
    // 2. Parse the review comment to understand the request
    // 3. Run the agent to address the feedback
    // 4. Push changes to the PR branch
    // 5. Reply to the review comment with results
    // Priority: Low (requires GitHub Actions integration)

    Ok(())
}

/// Handle issue event.
async fn handle_issue(
    _client: &GitHubClient,
    event: &serde_json::Value,
    _cwd: &std::path::Path,
) -> Result<()> {
    let action = event["action"].as_str().unwrap_or("");
    if action != "opened" && action != "labeled" {
        return Ok(());
    }

    let issue_number = event["issue"]["number"].as_u64().unwrap_or(0);
    info!("Processing issue event: #{}", issue_number);

    // DEFERRED: GitHub integration - handle issue events
    // Implementation should:
    // 1. Parse the issue body to understand the request
    // 2. For labeled issues (e.g., "wonopcode"), run the agent
    // 3. Create a PR addressing the issue
    // 4. Comment on the issue with the PR link
    // Priority: Low (requires GitHub Actions integration)

    Ok(())
}

/// Handle pull request event.
async fn handle_pull_request(
    _client: &GitHubClient,
    event: &serde_json::Value,
    _cwd: &std::path::Path,
) -> Result<()> {
    let action = event["action"].as_str().unwrap_or("");
    let pr_number = event["pull_request"]["number"].as_u64().unwrap_or(0);

    info!("Processing pull request event: #{} ({})", pr_number, action);

    // DEFERRED: GitHub integration - handle PR events
    // Implementation should:
    // 1. Checkout the PR branch
    // 2. Run analysis or requested actions
    // 3. For "synchronize" action, re-run checks if needed
    // 4. Comment with results
    // Priority: Low (requires GitHub Actions integration)

    Ok(())
}

/// Handle workflow dispatch event.
async fn handle_workflow_dispatch(
    _client: &GitHubClient,
    event: &serde_json::Value,
    _cwd: &std::path::Path,
) -> Result<()> {
    let inputs = &event["inputs"];

    info!("Processing workflow dispatch with inputs: {:?}", inputs);

    // DEFERRED: GitHub integration - handle workflow dispatch
    // Implementation should:
    // 1. Parse workflow inputs (prompt, branch, etc.)
    // 2. Clone/checkout the specified branch
    // 3. Run the agent with the provided prompt
    // 4. Create a PR with the changes
    // Priority: Low (requires GitHub Actions integration)

    Ok(())
}

/// Check if a comment contains a trigger phrase.
fn is_trigger_comment(body: &str) -> bool {
    let triggers = ["/wonopcode", "/wc"];
    let lower = body.to_lowercase();
    triggers.iter().any(|t| lower.contains(t))
}

/// Extract the prompt from a trigger comment.
fn extract_prompt(body: &str) -> String {
    let triggers = ["/wonopcode", "/wc"];
    let mut text = body.to_string();

    for trigger in triggers {
        if let Some(pos) = text.to_lowercase().find(trigger) {
            text = text[pos + trigger.len()..].to_string();
            break;
        }
    }

    text.trim().to_string()
}

/// Parse owner/repo format.
fn parse_repo_name(full_name: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = full_name.split('/').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid repository name: {}", full_name);
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_trigger_comment() {
        assert!(is_trigger_comment("/wonopcode fix this bug"));
        assert!(is_trigger_comment("Hey /wc can you help?"));
        assert!(!is_trigger_comment("Just a regular comment"));
    }

    #[test]
    fn test_extract_prompt() {
        assert_eq!(extract_prompt("/wonopcode fix this bug"), "fix this bug");
        assert_eq!(extract_prompt("Hey /wc can you help?"), "can you help?");
    }

    #[test]
    fn test_parse_repo_name() {
        let (owner, repo) = parse_repo_name("wonop-io/wonopcode").unwrap();
        assert_eq!(owner, "wonop-io");
        assert_eq!(repo, "wonopcode");
    }
}
