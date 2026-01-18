//! PR checkout and git operations for GitHub integration.
//!
//! TODO: Complete GitHub Actions integration. This module provides git operations
//! for PR management. Currently only `checkout_pr` is used (via CLI). The remaining
//! functions will be used when we implement automated PR creation from agent output:
//! - `create_branch`, `checkout_branch` - for creating PR branches
//! - `stage_all`, `commit`, `push` - for committing agent changes
//! - `has_changes` - to check if there are changes to commit
//!
//! Priority: Low - GitHub Actions is an advanced deployment feature.

#![allow(dead_code)]

use anyhow::Result;
use std::process::Command;
use tracing::{debug, info};

/// Checkout a PR by number.
pub async fn checkout_pr(pr_number: u64) -> Result<()> {
    info!("Checking out PR #{}", pr_number);

    // Use gh CLI to checkout the PR
    let output = Command::new("gh")
        .args(["pr", "checkout", &pr_number.to_string()])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to checkout PR: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    debug!("gh pr checkout output: {}", stdout);

    info!("Successfully checked out PR #{}", pr_number);
    Ok(())
}

/// Get the current branch name.
pub fn current_branch() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to get current branch: {stderr}");
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();

    Ok(branch)
}

/// Check if a branch exists locally.
pub fn branch_exists(branch: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", branch])
        .output()?;

    Ok(output.status.success())
}

/// Create and checkout a new branch.
pub fn create_branch(branch: &str, from: Option<&str>) -> Result<()> {
    let mut args = vec!["checkout", "-b", branch];
    if let Some(base) = from {
        args.push(base);
    }

    let output = Command::new("git").args(&args).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to create branch: {stderr}");
    }

    Ok(())
}

/// Checkout an existing branch.
pub fn checkout_branch(branch: &str) -> Result<()> {
    let output = Command::new("git").args(["checkout", branch]).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to checkout branch: {stderr}");
    }

    Ok(())
}

/// Stage all changes.
pub fn stage_all() -> Result<()> {
    let output = Command::new("git").args(["add", "-A"]).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to stage changes: {stderr}");
    }

    Ok(())
}

/// Create a commit.
pub fn commit(message: &str, coauthor: Option<&str>) -> Result<()> {
    let mut full_message = message.to_string();

    if let Some(author) = coauthor {
        full_message.push_str(&format!("\n\nCo-authored-by: {author}"));
    }

    let output = Command::new("git")
        .args(["commit", "-m", &full_message])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Check if there's nothing to commit
        if stderr.contains("nothing to commit") {
            return Ok(());
        }
        anyhow::bail!("Failed to commit: {stderr}");
    }

    Ok(())
}

/// Push to remote.
pub fn push(branch: &str, force: bool) -> Result<()> {
    let mut args = vec!["push", "origin", branch];
    if force {
        args.insert(1, "--force-with-lease");
    }

    let output = Command::new("git").args(&args).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to push: {stderr}");
    }

    Ok(())
}

/// Check if there are uncommitted changes.
pub fn has_changes() -> Result<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to check git status: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(!stdout.trim().is_empty())
}

/// Get the default branch (usually main or master).
pub fn default_branch() -> Result<String> {
    // Try to get from remote
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .output()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(branch) = stdout.trim().strip_prefix("refs/remotes/origin/") {
            return Ok(branch.to_string());
        }
    }

    // Fallback to checking common defaults
    if branch_exists("origin/main")? {
        return Ok("main".to_string());
    }
    if branch_exists("origin/master")? {
        return Ok("master".to_string());
    }

    // Last resort
    Ok("main".to_string())
}

/// Parse a GitHub remote URL to get owner and repo.
pub fn parse_github_remote(url: &str) -> Option<(String, String)> {
    // Handle various formats:
    // git@github.com:owner/repo.git
    // https://github.com/owner/repo.git
    // https://github.com/owner/repo
    // git://github.com/owner/repo.git

    let url = url.trim();

    // Remove .git suffix
    let url = url.strip_suffix(".git").unwrap_or(url);

    // SSH format
    if url.starts_with("git@github.com:") {
        let path = url.strip_prefix("git@github.com:")?;
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }

    // HTTPS/git format
    if let Some(path) = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git://github.com/"))
    {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }

    None
}

/// Get the origin remote URL.
pub fn get_origin_url() -> Result<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to get origin URL: {stderr}");
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();

    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_remote_ssh() {
        let (owner, repo) = parse_github_remote("git@github.com:wonop-io/wonopcode.git").unwrap();
        assert_eq!(owner, "wonop-io");
        assert_eq!(repo, "wonopcode");
    }

    #[test]
    fn test_parse_github_remote_https() {
        let (owner, repo) =
            parse_github_remote("https://github.com/wonop-io/wonopcode.git").unwrap();
        assert_eq!(owner, "wonop-io");
        assert_eq!(repo, "wonopcode");
    }

    #[test]
    fn test_parse_github_remote_https_no_git() {
        let (owner, repo) = parse_github_remote("https://github.com/wonop-io/wonopcode").unwrap();
        assert_eq!(owner, "wonop-io");
        assert_eq!(repo, "wonopcode");
    }
}
