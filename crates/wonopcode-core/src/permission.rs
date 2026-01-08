//! Permission system for tool execution.
//!
//! This module provides a permission system that controls which tools can be
//! executed and with what parameters. It supports:
//! - Always allow/deny rules
//! - Per-session permissions
//! - Wildcard patterns for paths

use crate::bus::{Bus, PermissionRequest, PermissionResponse};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::{oneshot, RwLock};
use wonopcode_util::wildcard;

/// Permission decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Allow the action.
    Allow,
    /// Deny the action.
    Deny,
    /// Ask the user.
    Ask,
}

impl Default for Decision {
    fn default() -> Self {
        Self::Ask
    }
}

/// A permission rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    /// Tool name pattern (supports wildcards).
    pub tool: String,
    /// Action pattern (supports wildcards).
    #[serde(default)]
    pub action: Option<String>,
    /// Path pattern (supports wildcards).
    #[serde(default)]
    pub path: Option<String>,
    /// Decision for matching requests.
    pub decision: Decision,
}

impl PermissionRule {
    /// Create a new rule that allows a tool.
    pub fn allow(tool: impl Into<String>) -> Self {
        Self {
            tool: tool.into(),
            action: None,
            path: None,
            decision: Decision::Allow,
        }
    }

    /// Create a new rule that denies a tool.
    pub fn deny(tool: impl Into<String>) -> Self {
        Self {
            tool: tool.into(),
            action: None,
            path: None,
            decision: Decision::Deny,
        }
    }

    /// Add a path pattern to the rule.
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Check if this rule matches a request.
    pub fn matches(&self, tool: &str, action: Option<&str>, path: Option<&str>) -> bool {
        // Check tool pattern
        if !wildcard::matches(&self.tool, tool) {
            return false;
        }

        // Check action pattern if specified
        if let Some(ref action_pattern) = self.action {
            if let Some(req_action) = action {
                if !wildcard::matches(action_pattern, req_action) {
                    return false;
                }
            } else {
                return false;
            }
        }

        // Check path pattern if specified
        if let Some(ref path_pattern) = self.path {
            if let Some(req_path) = path {
                if !wildcard::matches(path_pattern, req_path) {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }
}

/// Permission request for a tool action.
#[derive(Debug, Clone)]
pub struct PermissionCheck {
    /// Request ID.
    pub id: String,
    /// Tool name.
    pub tool: String,
    /// Action being performed.
    pub action: String,
    /// Description for the user.
    pub description: String,
    /// Path involved (for file operations).
    pub path: Option<String>,
    /// Additional details.
    pub details: serde_json::Value,
}

/// Permission manager.
pub struct PermissionManager {
    /// Global rules (from config).
    rules: RwLock<Vec<PermissionRule>>,
    /// Session-specific rules.
    session_rules: RwLock<HashMap<String, Vec<PermissionRule>>>,
    /// Pending permission requests.
    pending: RwLock<HashMap<String, oneshot::Sender<bool>>>,
    /// Event bus for permission events.
    bus: Bus,
}

impl PermissionManager {
    /// Create a new permission manager.
    pub fn new(bus: Bus) -> Self {
        Self {
            rules: RwLock::new(Vec::new()),
            session_rules: RwLock::new(HashMap::new()),
            pending: RwLock::new(HashMap::new()),
            bus,
        }
    }

    /// Add a global rule.
    pub async fn add_rule(&self, rule: PermissionRule) {
        let mut rules = self.rules.write().await;
        rules.push(rule);
    }

    /// Add a session-specific rule.
    pub async fn add_session_rule(&self, session_id: &str, rule: PermissionRule) {
        let mut session_rules = self.session_rules.write().await;
        session_rules
            .entry(session_id.to_string())
            .or_default()
            .push(rule);
    }

    /// Clear session-specific rules.
    pub async fn clear_session_rules(&self, session_id: &str) {
        let mut session_rules = self.session_rules.write().await;
        session_rules.remove(session_id);
    }

    /// Check permission for an action.
    /// Returns true if allowed, false if denied.
    pub async fn check(&self, session_id: &str, check: PermissionCheck) -> bool {
        // First check session-specific rules
        let session_rules = self.session_rules.read().await;
        if let Some(rules) = session_rules.get(session_id) {
            for rule in rules.iter().rev() {
                if rule.matches(&check.tool, Some(&check.action), check.path.as_deref()) {
                    match rule.decision {
                        Decision::Allow => return true,
                        Decision::Deny => return false,
                        Decision::Ask => break,
                    }
                }
            }
        }
        drop(session_rules);

        // Then check global rules
        let rules = self.rules.read().await;
        for rule in rules.iter().rev() {
            if rule.matches(&check.tool, Some(&check.action), check.path.as_deref()) {
                match rule.decision {
                    Decision::Allow => return true,
                    Decision::Deny => return false,
                    Decision::Ask => break,
                }
            }
        }
        drop(rules);

        // No matching rule, ask the user
        self.ask_user(session_id, check).await
    }

    /// Check permission using only rules, without prompting the user.
    /// Returns true if explicitly allowed, false if denied or no matching rule.
    /// This is useful for non-interactive contexts like MCP servers.
    pub async fn check_rules_only(
        &self,
        session_id: &str,
        tool: &str,
        action: Option<&str>,
        path: Option<&str>,
    ) -> bool {
        // First check session-specific rules
        let session_rules = self.session_rules.read().await;
        if let Some(rules) = session_rules.get(session_id) {
            for rule in rules.iter().rev() {
                if rule.matches(tool, action, path) {
                    match rule.decision {
                        Decision::Allow => return true,
                        Decision::Deny => return false,
                        Decision::Ask => continue, // Skip "Ask" rules in non-interactive mode
                    }
                }
            }
        }
        drop(session_rules);

        // Then check global rules
        let rules = self.rules.read().await;
        for rule in rules.iter().rev() {
            if rule.matches(tool, action, path) {
                match rule.decision {
                    Decision::Allow => return true,
                    Decision::Deny => return false,
                    Decision::Ask => continue, // Skip "Ask" rules in non-interactive mode
                }
            }
        }
        drop(rules);

        // No matching rule in non-interactive mode means deny
        false
    }

    /// Ask the user for permission.
    async fn ask_user(&self, session_id: &str, check: PermissionCheck) -> bool {
        let (tx, rx) = oneshot::channel();

        // Store the pending request
        {
            let mut pending = self.pending.write().await;
            pending.insert(check.id.clone(), tx);
        }

        // Log that we're waiting for user permission
        tracing::warn!(
            tool = %check.tool,
            action = %check.action,
            "Waiting for user permission (no matching rule found)"
        );

        // Publish permission request event
        self.bus
            .publish(PermissionRequest {
                id: check.id.clone(),
                session_id: session_id.to_string(),
                tool: check.tool,
                action: check.action,
                details: check.details,
            })
            .await;

        // Wait for response (with timeout)
        match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
            Ok(Ok(allowed)) => allowed,
            Ok(Err(_)) => {
                tracing::warn!("Permission request channel closed");
                false
            }
            Err(_) => {
                tracing::warn!("Permission request timed out after 300 seconds");
                false
            }
        }
    }

    /// Respond to a permission request.
    pub async fn respond(&self, request_id: &str, allowed: bool, remember: bool) {
        // Remove the pending request and send response
        let tx = {
            let mut pending = self.pending.write().await;
            pending.remove(request_id)
        };

        if let Some(tx) = tx {
            let _ = tx.send(allowed);
        }

        // Publish response event
        self.bus
            .publish(PermissionResponse {
                id: request_id.to_string(),
                allowed,
                remember,
            })
            .await;
    }

    /// Create default rules for read-only operations.
    ///
    /// These are safe operations that don't modify files or execute commands.
    /// Write operations (edit, bash, etc.) are not included and will trigger
    /// permission prompts unless sandbox rules are applied.
    pub fn default_rules() -> Vec<PermissionRule> {
        vec![
            // Read-only file tools
            PermissionRule::allow("read"),
            PermissionRule::allow("glob"),
            PermissionRule::allow("grep"),
            PermissionRule::allow("list"),
            PermissionRule::allow("todoread"),
            PermissionRule::allow("search"),
            PermissionRule::allow("codesearch"),
            PermissionRule::allow("lsp"),
            PermissionRule::allow("hover"),
            // Web access for docs (read-only)
            PermissionRule::allow("webfetch"),
            PermissionRule::allow("websearch"),
            // Todo tracking (low risk)
            PermissionRule::allow("todowrite"),
            // Plan mode tools (safe, just switch agent mode)
            PermissionRule::allow("enterplanmode"),
            PermissionRule::allow("exitplanmode"),
        ]
    }

    /// Create rules that allow all write operations.
    ///
    /// These rules should only be applied when running in a sandbox
    /// with `allow_all_in_sandbox` enabled, as they permit potentially
    /// dangerous operations like file editing and command execution.
    pub fn sandbox_allow_all_rules() -> Vec<PermissionRule> {
        vec![
            PermissionRule::allow("write"),
            PermissionRule::allow("edit"),
            PermissionRule::allow("multiedit"),
            PermissionRule::allow("patch"),
            PermissionRule::allow("bash"),
            PermissionRule::allow("task"),
            PermissionRule::allow("skill"),
        ]
    }

    /// Apply sandbox rules that allow all operations.
    ///
    /// Call this when sandbox is active and `allow_all_in_sandbox` is true.
    pub async fn apply_sandbox_rules(&self) {
        for rule in Self::sandbox_allow_all_rules() {
            self.add_rule(rule).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_rule_matches() {
        let rule = PermissionRule::allow("bash");
        assert!(rule.matches("bash", None, None));
        assert!(!rule.matches("read", None, None));

        let rule = PermissionRule::allow("*");
        assert!(rule.matches("bash", None, None));
        assert!(rule.matches("read", None, None));

        let rule = PermissionRule::allow("write").with_path("src/*");
        assert!(rule.matches("write", None, Some("src/main.rs")));
        assert!(!rule.matches("write", None, Some("tests/test.rs")));
    }

    #[tokio::test]
    async fn test_permission_manager_rules() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        // Add allow rule
        manager.add_rule(PermissionRule::allow("read")).await;

        // Check permission
        let _check = PermissionCheck {
            id: "1".to_string(),
            tool: "read".to_string(),
            action: "read".to_string(),
            description: "Read file".to_string(),
            path: Some("test.txt".to_string()),
            details: serde_json::Value::Null,
        };

        // This would normally need a responder, but with the allow rule it should pass
        // We can't easily test the full flow without mocking
    }
}
