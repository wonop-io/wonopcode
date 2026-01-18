//! GitHub event types.
//!
//! TODO: Complete GitHub Actions integration. This module provides types for
//! parsing GitHub webhook events. Currently events are parsed as raw JSON in
//! the handler functions. These types will be used when we implement proper
//! type-safe event handling.
//!
//! Priority: Low - GitHub Actions is an advanced deployment feature.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// GitHub event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    IssueComment,
    PullRequestReviewComment,
    Issues,
    PullRequest,
    WorkflowDispatch,
    Schedule,
    Unknown,
}

impl EventType {
    /// Parse from event name string.
    pub fn from_name(name: &str) -> Self {
        match name {
            "issue_comment" => EventType::IssueComment,
            "pull_request_review_comment" => EventType::PullRequestReviewComment,
            "issues" => EventType::Issues,
            "pull_request" => EventType::PullRequest,
            "workflow_dispatch" => EventType::WorkflowDispatch,
            "schedule" => EventType::Schedule,
            _ => EventType::Unknown,
        }
    }

    /// Get event name string.
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::IssueComment => "issue_comment",
            EventType::PullRequestReviewComment => "pull_request_review_comment",
            EventType::Issues => "issues",
            EventType::PullRequest => "pull_request",
            EventType::WorkflowDispatch => "workflow_dispatch",
            EventType::Schedule => "schedule",
            EventType::Unknown => "unknown",
        }
    }

    /// Check if this is a user-triggered event.
    pub fn is_user_event(&self) -> bool {
        matches!(
            self,
            EventType::IssueComment
                | EventType::PullRequestReviewComment
                | EventType::Issues
                | EventType::PullRequest
        )
    }

    /// Check if this is a scheduled/automated event.
    pub fn is_automated_event(&self) -> bool {
        matches!(self, EventType::WorkflowDispatch | EventType::Schedule)
    }
}

/// GitHub event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubEvent {
    pub action: Option<String>,
    pub repository: Option<EventRepository>,
    pub sender: Option<EventUser>,
    pub issue: Option<EventIssue>,
    pub pull_request: Option<EventPullRequest>,
    pub comment: Option<EventComment>,
    pub inputs: Option<serde_json::Value>,
}

/// Repository in event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRepository {
    pub id: u64,
    pub name: String,
    pub full_name: String,
    pub owner: EventUser,
    pub default_branch: String,
}

/// User in event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventUser {
    pub login: String,
    pub id: u64,
}

/// Issue in event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventIssue {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub user: EventUser,
}

/// Pull request in event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventPullRequest {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub user: EventUser,
    pub head: EventRef,
    pub base: EventRef,
}

/// Branch reference in event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRef {
    pub label: String,
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub sha: String,
}

/// Comment in event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventComment {
    pub id: u64,
    pub body: String,
    pub user: EventUser,
}

impl GitHubEvent {
    /// Parse from JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Get the repository full name (owner/repo).
    pub fn repo_full_name(&self) -> Option<&str> {
        self.repository.as_ref().map(|r| r.full_name.as_str())
    }

    /// Get the issue/PR number.
    pub fn number(&self) -> Option<u64> {
        self.issue
            .as_ref()
            .map(|i| i.number)
            .or_else(|| self.pull_request.as_ref().map(|p| p.number))
    }

    /// Get the comment body if present.
    pub fn comment_body(&self) -> Option<&str> {
        self.comment.as_ref().map(|c| c.body.as_str())
    }

    /// Get the sender username.
    pub fn sender_login(&self) -> Option<&str> {
        self.sender.as_ref().map(|s| s.login.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_from_name() {
        assert_eq!(
            EventType::from_name("issue_comment"),
            EventType::IssueComment
        );
        assert_eq!(EventType::from_name("pull_request"), EventType::PullRequest);
        assert_eq!(EventType::from_name("unknown_event"), EventType::Unknown);
    }

    #[test]
    fn test_event_type_is_user_event() {
        assert!(EventType::IssueComment.is_user_event());
        assert!(EventType::PullRequest.is_user_event());
        assert!(!EventType::Schedule.is_user_event());
    }

    #[test]
    fn test_parse_event() {
        let json = r#"{
            "action": "created",
            "repository": {
                "id": 123,
                "name": "test-repo",
                "full_name": "owner/test-repo",
                "owner": { "login": "owner", "id": 1 },
                "default_branch": "main"
            },
            "sender": { "login": "user", "id": 2 },
            "comment": {
                "id": 456,
                "body": "/wonopcode fix this",
                "user": { "login": "user", "id": 2 }
            }
        }"#;

        let event = GitHubEvent::from_json(json).unwrap();
        assert_eq!(event.action, Some("created".to_string()));
        assert_eq!(event.repo_full_name(), Some("owner/test-repo"));
        assert_eq!(event.comment_body(), Some("/wonopcode fix this"));
    }
}
