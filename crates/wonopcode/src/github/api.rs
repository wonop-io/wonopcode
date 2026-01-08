//! GitHub API client.
//!
//! TODO: Complete GitHub Actions integration. This module provides the API client
//! for managing PRs, issues, and reactions. Currently only `add_reaction` is used
//! from `handle_issue_comment`. The remaining methods will be used when we implement:
//! - Creating PRs from agent output
//! - Posting PR review comments
//! - Managing issue labels
//!
//! Priority: Low - GitHub Actions is an advanced deployment feature.

#![allow(dead_code)]

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

/// GitHub API client.
pub struct GitHubClient {
    client: Client,
    token: String,
    base_url: String,
}

impl GitHubClient {
    /// Create a new GitHub client.
    pub fn new(token: &str) -> Result<Self> {
        let client = Client::builder().user_agent("wonopcode/1.0").build()?;

        Ok(Self {
            client,
            token: token.to_string(),
            base_url: "https://api.github.com".to_string(),
        })
    }

    /// Create a new client with a custom base URL.
    pub fn with_base_url(token: &str, base_url: &str) -> Result<Self> {
        let mut client = Self::new(token)?;
        client.base_url = base_url.to_string();
        Ok(client)
    }

    /// Add a reaction to a comment.
    pub async fn add_reaction(
        &self,
        owner: &str,
        repo: &str,
        comment_id: u64,
        reaction: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/repos/{}/{}/issues/comments/{}/reactions",
            self.base_url, owner, repo, comment_id
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&serde_json::json!({ "content": reaction }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Failed to add reaction: {} - {}", status, body);
            anyhow::bail!("Failed to add reaction: {}", status);
        }

        debug!("Added {} reaction to comment {}", reaction, comment_id);
        Ok(())
    }

    /// Remove a reaction from a comment.
    pub async fn remove_reaction(
        &self,
        owner: &str,
        repo: &str,
        comment_id: u64,
        reaction_id: u64,
    ) -> Result<()> {
        let url = format!(
            "{}/repos/{}/{}/issues/comments/{}/reactions/{}",
            self.base_url, owner, repo, comment_id, reaction_id
        );

        let response = self
            .client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Failed to remove reaction: {} - {}", status, body);
            anyhow::bail!("Failed to remove reaction: {}", status);
        }

        Ok(())
    }

    /// Create a comment on an issue or PR.
    pub async fn create_comment(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
        body: &str,
    ) -> Result<Comment> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}/comments",
            self.base_url, owner, repo, issue_number
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&serde_json::json!({ "body": body }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Failed to create comment: {} - {}", status, body);
            anyhow::bail!("Failed to create comment: {}", status);
        }

        let comment: Comment = response.json().await?;
        debug!("Created comment: {}", comment.id);
        Ok(comment)
    }

    /// Update a comment.
    pub async fn update_comment(
        &self,
        owner: &str,
        repo: &str,
        comment_id: u64,
        body: &str,
    ) -> Result<Comment> {
        let url = format!(
            "{}/repos/{}/{}/issues/comments/{}",
            self.base_url, owner, repo, comment_id
        );

        let response = self
            .client
            .patch(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&serde_json::json!({ "body": body }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Failed to update comment: {} - {}", status, body);
            anyhow::bail!("Failed to update comment: {}", status);
        }

        let comment: Comment = response.json().await?;
        Ok(comment)
    }

    /// Get a pull request.
    pub async fn get_pull_request(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<PullRequest> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            self.base_url, owner, repo, number
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Failed to get pull request: {} - {}", status, body);
            anyhow::bail!("Failed to get pull request: {}", status);
        }

        let pr: PullRequest = response.json().await?;
        Ok(pr)
    }

    /// Create a pull request.
    pub async fn create_pull_request(
        &self,
        owner: &str,
        repo: &str,
        title: &str,
        body: &str,
        head: &str,
        base: &str,
    ) -> Result<PullRequest> {
        let url = format!("{}/repos/{}/{}/pulls", self.base_url, owner, repo);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&serde_json::json!({
                "title": title,
                "body": body,
                "head": head,
                "base": base
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Failed to create pull request: {} - {}", status, body);
            anyhow::bail!("Failed to create pull request: {}", status);
        }

        let pr: PullRequest = response.json().await?;
        debug!("Created pull request: #{}", pr.number);
        Ok(pr)
    }

    /// Get an issue.
    pub async fn get_issue(&self, owner: &str, repo: &str, number: u64) -> Result<Issue> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}",
            self.base_url, owner, repo, number
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Failed to get issue: {} - {}", status, body);
            anyhow::bail!("Failed to get issue: {}", status);
        }

        let issue: Issue = response.json().await?;
        Ok(issue)
    }

    /// Check user permission level.
    pub async fn get_permission_level(
        &self,
        owner: &str,
        repo: &str,
        username: &str,
    ) -> Result<String> {
        let url = format!(
            "{}/repos/{}/{}/collaborators/{}/permission",
            self.base_url, owner, repo, username
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                return Ok("none".to_string());
            }
            let body = response.text().await.unwrap_or_default();
            error!("Failed to get permission level: {} - {}", status, body);
            anyhow::bail!("Failed to get permission level: {}", status);
        }

        #[derive(Deserialize)]
        struct PermissionResponse {
            permission: String,
        }

        let resp: PermissionResponse = response.json().await?;
        Ok(resp.permission)
    }
}

/// GitHub comment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub body: String,
    pub user: User,
    pub created_at: String,
    pub updated_at: String,
}

/// GitHub user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub login: String,
    pub id: u64,
}

/// GitHub pull request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub user: User,
    pub head: PullRequestRef,
    pub base: PullRequestRef,
    pub html_url: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub changed_files: Option<u64>,
}

/// Pull request branch reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestRef {
    pub label: String,
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub sha: String,
    pub repo: Option<Repository>,
}

/// GitHub repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub id: u64,
    pub name: String,
    pub full_name: String,
    pub clone_url: String,
    pub ssh_url: String,
}

/// GitHub issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub user: User,
    pub labels: Vec<Label>,
}

/// GitHub label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub name: String,
    pub color: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = GitHubClient::new("test-token").unwrap();
        assert_eq!(client.base_url, "https://api.github.com");
    }

    #[test]
    fn test_client_with_base_url() {
        let client =
            GitHubClient::with_base_url("test-token", "https://github.example.com/api/v3").unwrap();
        assert_eq!(client.base_url, "https://github.example.com/api/v3");
    }
}
