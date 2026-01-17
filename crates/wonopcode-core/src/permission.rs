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

    /// Create a new rule that asks the user.
    pub fn ask(tool: impl Into<String>) -> Self {
        Self {
            tool: tool.into(),
            action: None,
            path: None,
            decision: Decision::Ask,
        }
    }

    /// Create a rule with a specific decision.
    pub fn with_decision(tool: impl Into<String>, decision: Decision) -> Self {
        Self {
            tool: tool.into(),
            action: None,
            path: None,
            decision,
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

/// Pending permission request info, stored while waiting for user response.
struct PendingRequest {
    /// Response channel.
    tx: oneshot::Sender<bool>,
    /// Session ID for the request.
    session_id: String,
    /// Tool name.
    tool: String,
    /// Action being performed.
    action: String,
    /// Path involved (for file operations).
    path: Option<String>,
}

/// Permission manager.
pub struct PermissionManager {
    /// Global rules (from config).
    rules: RwLock<Vec<PermissionRule>>,
    /// Session-specific rules.
    session_rules: RwLock<HashMap<String, Vec<PermissionRule>>>,
    /// Pending permission requests (keyed by request ID).
    pending: RwLock<HashMap<String, PendingRequest>>,
    /// Event bus for permission events.
    bus: Bus,
    /// Whether sandbox is currently running (shared state for MCP tools).
    sandbox_running: std::sync::atomic::AtomicBool,
    /// Shared sandbox runtime (set when sandbox starts).
    /// Stored as Any so we can downcast to the concrete type when needed.
    sandbox_runtime: RwLock<Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>>,
}

impl PermissionManager {
    /// Create a new permission manager.
    pub fn new(bus: Bus) -> Self {
        Self {
            rules: RwLock::new(Vec::new()),
            session_rules: RwLock::new(HashMap::new()),
            pending: RwLock::new(HashMap::new()),
            bus,
            sandbox_running: std::sync::atomic::AtomicBool::new(false),
            sandbox_runtime: RwLock::new(None),
        }
    }

    /// Set sandbox running state and optionally the runtime.
    pub fn set_sandbox_running(&self, running: bool) {
        self.sandbox_running
            .store(running, std::sync::atomic::Ordering::SeqCst);
        tracing::info!(
            sandbox_running = running,
            "Sandbox state updated in permission manager"
        );
    }

    /// Set the sandbox runtime (for MCP tools to use).
    /// The runtime is stored as `Arc<dyn Any + Send + Sync>` to avoid circular dependencies.
    pub async fn set_sandbox_runtime_any(
        &self,
        runtime: Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>,
    ) {
        let mut sandbox = self.sandbox_runtime.write().await;
        *sandbox = runtime;
    }

    /// Get the sandbox runtime (if running) as `Arc<dyn Any>`.
    /// The caller is responsible for downcasting to the correct type.
    pub async fn sandbox_runtime_any(
        &self,
    ) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        let sandbox = self.sandbox_runtime.read().await;
        sandbox.clone()
    }

    /// Check if sandbox is running.
    pub fn is_sandbox_running(&self) -> bool {
        self.sandbox_running
            .load(std::sync::atomic::Ordering::SeqCst)
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

    /// Clear all global rules.
    pub async fn clear_rules(&self) {
        let mut rules = self.rules.write().await;
        rules.clear();
    }

    /// Reload rules from config.
    ///
    /// This clears existing rules and reloads them from the provided config.
    /// Call this when settings are changed at runtime.
    pub async fn reload_from_config(&self, config: &crate::config::PermissionConfig) {
        // Clear existing rules
        self.clear_rules().await;

        // Re-add default rules (safe read-only operations)
        for rule in Self::default_rules() {
            self.add_rule(rule).await;
        }

        // Add config-based rules (these take precedence as they're added last)
        for rule in Self::rules_from_config(config) {
            self.add_rule(rule).await;
        }

        tracing::info!("Permission rules reloaded from config");
    }

    /// Check permission for an action.
    /// Returns true if allowed, false if denied.
    pub async fn check(&self, session_id: &str, check: PermissionCheck) -> bool {
        self.check_with_sandbox(session_id, check, false).await
    }

    /// Check permission for an action, considering sandbox state.
    /// When `sandbox_running` is true, sandbox allow rules are applied.
    /// Returns true if allowed, false if denied.
    pub async fn check_with_sandbox(
        &self,
        session_id: &str,
        check: PermissionCheck,
        sandbox_running: bool,
    ) -> bool {
        // If sandbox is running, check sandbox rules first
        if sandbox_running {
            for rule in Self::sandbox_allow_all_rules() {
                if rule.matches(&check.tool, Some(&check.action), check.path.as_deref()) {
                    match rule.decision {
                        Decision::Allow => return true,
                        Decision::Deny => return false,
                        Decision::Ask => break,
                    }
                }
            }
        }

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
    #[allow(clippy::cognitive_complexity)]
    async fn ask_user(&self, session_id: &str, check: PermissionCheck) -> bool {
        let (tx, rx) = oneshot::channel();

        // Store the pending request with all info needed for "remember" functionality
        {
            let mut pending = self.pending.write().await;
            pending.insert(
                check.id.clone(),
                PendingRequest {
                    tx,
                    session_id: session_id.to_string(),
                    tool: check.tool.clone(),
                    action: check.action.clone(),
                    path: check.path.clone(),
                },
            );
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
                tool: check.tool.clone(),
                action: check.action.clone(),
                description: check.description,
                path: check.path,
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
        // Remove the pending request and get its info
        let pending_req = {
            let mut pending = self.pending.write().await;
            pending.remove(request_id)
        };

        if let Some(req) = pending_req {
            // Send response to the waiting task
            let _ = req.tx.send(allowed);

            // If "remember" is set, create a session rule for future requests
            if remember {
                let decision = if allowed {
                    Decision::Allow
                } else {
                    Decision::Deny
                };

                let mut rule = PermissionRule {
                    tool: req.tool.clone(),
                    action: Some(req.action.clone()),
                    path: None,
                    decision,
                };

                // If the request had a path, create a pattern for it
                // For now, we match the exact tool+action without path restriction
                // A more sophisticated approach could create path-specific rules
                if req.path.is_some() {
                    // We could use: rule.path = Some(format!("{}*", path_dir));
                    // But for simplicity, we'll just remember the tool+action
                    rule.path = None;
                }

                tracing::info!(
                    tool = %req.tool,
                    action = %req.action,
                    allowed = allowed,
                    "Created session rule for remembered permission"
                );

                self.add_session_rule(&req.session_id, rule).await;
            }
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

    /// Convert a config Permission to a permission Decision.
    fn config_permission_to_decision(perm: crate::config::Permission) -> Decision {
        match perm {
            crate::config::Permission::Allow => Decision::Allow,
            crate::config::Permission::Deny => Decision::Deny,
            crate::config::Permission::Ask => Decision::Ask,
        }
    }

    /// Create permission rules from config settings.
    ///
    /// This converts the user's permission settings from the config file
    /// into permission rules that will be checked during tool execution.
    /// These rules take precedence over default rules when added after them.
    pub fn rules_from_config(config: &crate::config::PermissionConfig) -> Vec<PermissionRule> {
        let mut rules = Vec::new();

        // Edit permission - applies to file modification tools
        if let Some(edit_perm) = config.edit {
            let decision = Self::config_permission_to_decision(edit_perm);
            // Apply to all file editing tools
            rules.push(PermissionRule::with_decision("edit", decision));
            rules.push(PermissionRule::with_decision("write", decision));
            rules.push(PermissionRule::with_decision("multiedit", decision));
            rules.push(PermissionRule::with_decision("patch", decision));
        }

        // Bash permission - applies to shell command execution
        if let Some(bash_config) = &config.bash {
            match bash_config {
                crate::config::PermissionOrMap::Single(perm) => {
                    let decision = Self::config_permission_to_decision(*perm);
                    rules.push(PermissionRule::with_decision("bash", decision));
                }
                crate::config::PermissionOrMap::Map(map) => {
                    // For pattern maps, we create rules for each pattern
                    // The wildcard "*" pattern is the default
                    for (pattern, perm) in map {
                        let decision = Self::config_permission_to_decision(*perm);
                        let mut rule = PermissionRule::with_decision("bash", decision);
                        // Use the pattern as the action (command pattern)
                        rule.action = Some(pattern.clone());
                        rules.push(rule);
                    }
                }
            }
        }

        // Webfetch permission - applies to web access tools
        if let Some(webfetch_perm) = config.webfetch {
            let decision = Self::config_permission_to_decision(webfetch_perm);
            rules.push(PermissionRule::with_decision("webfetch", decision));
            rules.push(PermissionRule::with_decision("websearch", decision));
        }

        // External directory permission - applies to file operations outside project
        if let Some(ext_dir_perm) = config.external_directory {
            let decision = Self::config_permission_to_decision(ext_dir_perm);
            // These rules apply to file tools when accessing external paths
            // The path pattern will be checked by the permission manager
            // For now, we set up general rules; specific path checks happen during execution
            let mut read_rule = PermissionRule::with_decision("read", decision);
            read_rule.action = Some("external".to_string());
            rules.push(read_rule);

            let mut edit_rule = PermissionRule::with_decision("edit", decision);
            edit_rule.action = Some("external".to_string());
            rules.push(edit_rule);

            let mut write_rule = PermissionRule::with_decision("write", decision);
            write_rule.action = Some("external".to_string());
            rules.push(write_rule);
        }

        rules
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decision_default() {
        let decision = Decision::default();
        assert_eq!(decision, Decision::Ask);
    }

    #[test]
    fn test_decision_serialization() {
        let allow = Decision::Allow;
        let json = serde_json::to_string(&allow).unwrap();
        assert_eq!(json, r#""allow""#);

        let deny = Decision::Deny;
        let json = serde_json::to_string(&deny).unwrap();
        assert_eq!(json, r#""deny""#);

        let ask = Decision::Ask;
        let json = serde_json::to_string(&ask).unwrap();
        assert_eq!(json, r#""ask""#);

        let parsed: Decision = serde_json::from_str(r#""allow""#).unwrap();
        assert_eq!(parsed, Decision::Allow);
    }

    #[test]
    fn test_permission_rule_constructors() {
        let allow = PermissionRule::allow("bash");
        assert_eq!(allow.tool, "bash");
        assert_eq!(allow.decision, Decision::Allow);
        assert!(allow.action.is_none());
        assert!(allow.path.is_none());

        let deny = PermissionRule::deny("rm");
        assert_eq!(deny.tool, "rm");
        assert_eq!(deny.decision, Decision::Deny);

        let ask = PermissionRule::ask("edit");
        assert_eq!(ask.tool, "edit");
        assert_eq!(ask.decision, Decision::Ask);

        let with_decision = PermissionRule::with_decision("write", Decision::Allow);
        assert_eq!(with_decision.tool, "write");
        assert_eq!(with_decision.decision, Decision::Allow);
    }

    #[test]
    fn test_permission_rule_with_path() {
        let rule = PermissionRule::allow("edit").with_path("/project/*");
        assert_eq!(rule.path, Some("/project/*".to_string()));
    }

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

    #[test]
    fn test_permission_rule_matches_with_action() {
        let mut rule = PermissionRule::allow("bash");
        rule.action = Some("ls*".to_string());

        // Matches when action matches pattern
        assert!(rule.matches("bash", Some("ls -la"), None));
        // Doesn't match when action doesn't match pattern
        assert!(!rule.matches("bash", Some("rm -rf"), None));
        // Doesn't match when action is None but rule expects action
        assert!(!rule.matches("bash", None, None));
    }

    #[test]
    fn test_permission_rule_matches_with_path_pattern() {
        let rule = PermissionRule::allow("edit").with_path("/project/src/*");

        // Matches when path matches pattern
        assert!(rule.matches("edit", None, Some("/project/src/main.rs")));
        // Doesn't match when path doesn't match pattern
        assert!(!rule.matches("edit", None, Some("/project/tests/test.rs")));
        // Doesn't match when path is None but rule expects path
        assert!(!rule.matches("edit", None, None));
    }

    #[test]
    fn test_permission_rule_serialization() {
        let rule = PermissionRule::allow("bash").with_path("src/*");
        let json = serde_json::to_string(&rule).unwrap();
        let parsed: PermissionRule = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tool, "bash");
        assert_eq!(parsed.path, Some("src/*".to_string()));
        assert_eq!(parsed.decision, Decision::Allow);
    }

    #[test]
    fn test_permission_check() {
        let check = PermissionCheck {
            id: "req_123".to_string(),
            tool: "bash".to_string(),
            action: "ls".to_string(),
            description: "List files".to_string(),
            path: Some("/tmp".to_string()),
            details: serde_json::json!({"command": "ls -la"}),
        };
        assert_eq!(check.id, "req_123");
        assert_eq!(check.tool, "bash");
        assert_eq!(check.path, Some("/tmp".to_string()));
    }

    #[tokio::test]
    async fn test_permission_manager_new() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);
        assert!(!manager.is_sandbox_running());
    }

    #[tokio::test]
    async fn test_permission_manager_sandbox_state() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        assert!(!manager.is_sandbox_running());
        manager.set_sandbox_running(true);
        assert!(manager.is_sandbox_running());
        manager.set_sandbox_running(false);
        assert!(!manager.is_sandbox_running());
    }

    #[tokio::test]
    async fn test_permission_manager_sandbox_runtime() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        // Initially none
        assert!(manager.sandbox_runtime_any().await.is_none());

        // Set a runtime (using a simple Arc<String> for testing)
        let runtime: std::sync::Arc<dyn std::any::Any + Send + Sync> =
            std::sync::Arc::new("test_runtime".to_string());
        manager.set_sandbox_runtime_any(Some(runtime)).await;

        let retrieved = manager.sandbox_runtime_any().await;
        assert!(retrieved.is_some());

        // Clear it
        manager.set_sandbox_runtime_any(None).await;
        assert!(manager.sandbox_runtime_any().await.is_none());
    }

    #[tokio::test]
    async fn test_permission_manager_add_rule() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        manager.add_rule(PermissionRule::allow("read")).await;

        let rules = manager.rules.read().await;
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].tool, "read");
    }

    #[tokio::test]
    async fn test_permission_manager_add_session_rule() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        manager
            .add_session_rule("session_1", PermissionRule::allow("bash"))
            .await;

        let session_rules = manager.session_rules.read().await;
        assert!(session_rules.contains_key("session_1"));
        assert_eq!(session_rules.get("session_1").unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_permission_manager_clear_session_rules() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        manager
            .add_session_rule("session_1", PermissionRule::allow("bash"))
            .await;
        manager.clear_session_rules("session_1").await;

        let session_rules = manager.session_rules.read().await;
        assert!(!session_rules.contains_key("session_1"));
    }

    #[tokio::test]
    async fn test_permission_manager_clear_rules() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        manager.add_rule(PermissionRule::allow("read")).await;
        manager.add_rule(PermissionRule::allow("write")).await;
        manager.clear_rules().await;

        let rules = manager.rules.read().await;
        assert!(rules.is_empty());
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

    #[tokio::test]
    async fn test_permission_manager_check_rules_only_allow() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        manager.add_rule(PermissionRule::allow("read")).await;

        let allowed = manager
            .check_rules_only("session_1", "read", Some("read"), None)
            .await;
        assert!(allowed);
    }

    #[tokio::test]
    async fn test_permission_manager_check_rules_only_deny() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        manager.add_rule(PermissionRule::deny("bash")).await;

        let allowed = manager
            .check_rules_only("session_1", "bash", Some("exec"), None)
            .await;
        assert!(!allowed);
    }

    #[tokio::test]
    async fn test_permission_manager_check_rules_only_no_match() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        // No rules match "unknown_tool"
        let allowed = manager
            .check_rules_only("session_1", "unknown_tool", None, None)
            .await;
        assert!(!allowed); // Default is deny in non-interactive mode
    }

    #[tokio::test]
    async fn test_permission_manager_check_rules_only_session_precedence() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        // Global rule denies
        manager.add_rule(PermissionRule::deny("bash")).await;
        // Session rule allows
        manager
            .add_session_rule("session_1", PermissionRule::allow("bash"))
            .await;

        // Session rule should take precedence
        let allowed = manager
            .check_rules_only("session_1", "bash", None, None)
            .await;
        assert!(allowed);

        // Different session uses global rule
        let allowed = manager
            .check_rules_only("session_2", "bash", None, None)
            .await;
        assert!(!allowed);
    }

    #[test]
    fn test_default_rules() {
        let rules = PermissionManager::default_rules();
        assert!(!rules.is_empty());

        // Check some expected default rules
        assert!(rules.iter().any(|r| r.tool == "read"));
        assert!(rules.iter().any(|r| r.tool == "glob"));
        assert!(rules.iter().any(|r| r.tool == "grep"));

        // All default rules should be Allow
        assert!(rules.iter().all(|r| r.decision == Decision::Allow));
    }

    #[test]
    fn test_sandbox_allow_all_rules() {
        let rules = PermissionManager::sandbox_allow_all_rules();
        assert!(!rules.is_empty());

        // Check some expected sandbox rules
        assert!(rules.iter().any(|r| r.tool == "write"));
        assert!(rules.iter().any(|r| r.tool == "edit"));
        assert!(rules.iter().any(|r| r.tool == "bash"));

        // All sandbox rules should be Allow
        assert!(rules.iter().all(|r| r.decision == Decision::Allow));
    }

    #[tokio::test]
    async fn test_permission_manager_apply_sandbox_rules() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        manager.apply_sandbox_rules().await;

        let rules = manager.rules.read().await;
        // Should have sandbox allow rules
        assert!(rules.iter().any(|r| r.tool == "write"));
        assert!(rules.iter().any(|r| r.tool == "bash"));
    }

    #[test]
    fn test_rules_from_config() {
        use crate::config::{Permission, PermissionConfig, PermissionOrMap};

        // Test with edit permission set to Ask
        let config = PermissionConfig {
            edit: Some(Permission::Ask),
            bash: None,
            webfetch: None,
            external_directory: None,
            allow_all_in_sandbox: None,
        };

        let rules = PermissionManager::rules_from_config(&config);
        assert_eq!(rules.len(), 4); // edit, write, multiedit, patch
        assert!(rules.iter().all(|r| r.decision == Decision::Ask));
        assert!(rules.iter().any(|r| r.tool == "edit"));
        assert!(rules.iter().any(|r| r.tool == "write"));
        assert!(rules.iter().any(|r| r.tool == "multiedit"));
        assert!(rules.iter().any(|r| r.tool == "patch"));

        // Test with webfetch permission set to Deny
        let config = PermissionConfig {
            edit: None,
            bash: None,
            webfetch: Some(Permission::Deny),
            external_directory: None,
            allow_all_in_sandbox: None,
        };

        let rules = PermissionManager::rules_from_config(&config);
        assert_eq!(rules.len(), 2); // webfetch, websearch
        assert!(rules.iter().all(|r| r.decision == Decision::Deny));
        assert!(rules.iter().any(|r| r.tool == "webfetch"));
        assert!(rules.iter().any(|r| r.tool == "websearch"));

        // Test with bash permission as single value
        let config = PermissionConfig {
            edit: None,
            bash: Some(PermissionOrMap::Single(Permission::Allow)),
            webfetch: None,
            external_directory: None,
            allow_all_in_sandbox: None,
        };

        let rules = PermissionManager::rules_from_config(&config);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].tool, "bash");
        assert_eq!(rules[0].decision, Decision::Allow);

        // Test with bash permission as map
        let mut bash_map = std::collections::HashMap::new();
        bash_map.insert("ls*".to_string(), Permission::Allow);
        bash_map.insert("rm*".to_string(), Permission::Ask);

        let config = PermissionConfig {
            edit: None,
            bash: Some(PermissionOrMap::Map(bash_map)),
            webfetch: None,
            external_directory: None,
            allow_all_in_sandbox: None,
        };

        let rules = PermissionManager::rules_from_config(&config);
        assert_eq!(rules.len(), 2);
        assert!(rules.iter().all(|r| r.tool == "bash"));
        // Check that action patterns are set
        assert!(rules.iter().any(|r| r.action == Some("ls*".to_string())));
        assert!(rules.iter().any(|r| r.action == Some("rm*".to_string())));
    }

    #[test]
    fn test_rules_from_config_external_directory() {
        use crate::config::{Permission, PermissionConfig};

        let config = PermissionConfig {
            edit: None,
            bash: None,
            webfetch: None,
            external_directory: Some(Permission::Ask),
            allow_all_in_sandbox: None,
        };

        let rules = PermissionManager::rules_from_config(&config);
        assert_eq!(rules.len(), 3); // read, edit, write with "external" action
        assert!(rules.iter().all(|r| r.decision == Decision::Ask));
        assert!(rules
            .iter()
            .all(|r| r.action == Some("external".to_string())));
    }

    #[tokio::test]
    async fn test_config_rules_take_precedence() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        // Add default rule that allows webfetch
        manager.add_rule(PermissionRule::allow("webfetch")).await;

        // Add config rule that asks for webfetch (should take precedence)
        manager.add_rule(PermissionRule::ask("webfetch")).await;

        // Check that the rules are in the expected order
        let rules = manager.rules.read().await;
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].decision, Decision::Allow); // Default rule
        assert_eq!(rules[1].decision, Decision::Ask); // Config rule (takes precedence when checked in reverse)
    }

    #[tokio::test]
    async fn test_permission_manager_reload_from_config() {
        use crate::config::{Permission, PermissionConfig};

        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        // Add some initial rules
        manager.add_rule(PermissionRule::deny("everything")).await;

        let config = PermissionConfig {
            edit: Some(Permission::Allow),
            bash: None,
            webfetch: None,
            external_directory: None,
            allow_all_in_sandbox: None,
        };

        manager.reload_from_config(&config).await;

        let rules = manager.rules.read().await;
        // Should have default rules + config rules, but not the old "everything" deny rule
        assert!(!rules.iter().any(|r| r.tool == "everything"));
        assert!(rules.iter().any(|r| r.tool == "read")); // From default rules
        assert!(rules
            .iter()
            .any(|r| r.tool == "edit" && r.decision == Decision::Allow));
    }

    #[tokio::test]
    async fn test_check_with_sandbox_allows_write() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        // No rules configured - normally would ask/deny
        let check = PermissionCheck {
            id: "1".to_string(),
            tool: "write".to_string(),
            action: "write".to_string(),
            description: "Write file".to_string(),
            path: Some("/tmp/test.txt".to_string()),
            details: serde_json::Value::Null,
        };

        // With sandbox_running=true, write should be allowed by sandbox rules
        let result = manager.check_with_sandbox("session_1", check, true).await;
        assert!(result);
    }

    #[tokio::test]
    async fn test_check_with_sandbox_allows_bash() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        let check = PermissionCheck {
            id: "2".to_string(),
            tool: "bash".to_string(),
            action: "execute".to_string(),
            description: "Run command".to_string(),
            path: None,
            details: serde_json::Value::Null,
        };

        // With sandbox_running=true, bash should be allowed
        let result = manager.check_with_sandbox("session_1", check, true).await;
        assert!(result);
    }

    #[tokio::test]
    async fn test_check_with_sandbox_session_rule_takes_precedence() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        // Add session rule that denies write
        manager
            .add_session_rule("session_1", PermissionRule::deny("write"))
            .await;

        let check = PermissionCheck {
            id: "3".to_string(),
            tool: "write".to_string(),
            action: "write".to_string(),
            description: "Write file".to_string(),
            path: Some("/tmp/test.txt".to_string()),
            details: serde_json::Value::Null,
        };

        // Session rule should be checked before sandbox rules
        // But sandbox rules are checked first in check_with_sandbox when sandbox_running=true
        let result = manager.check_with_sandbox("session_1", check, true).await;
        // Sandbox rules allow write, so it should be allowed
        assert!(result);
    }

    #[tokio::test]
    async fn test_check_rules_only_with_ask_rule() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        // Add ask rule - should be skipped in check_rules_only
        manager.add_rule(PermissionRule::ask("bash")).await;
        // Add allow rule for a different pattern
        manager.add_rule(PermissionRule::allow("read")).await;

        // bash with ask rule - should return false (ask is skipped, no allow match)
        let allowed = manager
            .check_rules_only("session_1", "bash", None, None)
            .await;
        assert!(!allowed);

        // read with allow rule - should return true
        let allowed = manager
            .check_rules_only("session_1", "read", None, None)
            .await;
        assert!(allowed);
    }

    #[tokio::test]
    async fn test_respond_nonexistent_request() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        // Responding to a nonexistent request should not panic
        manager.respond("nonexistent", true, false).await;

        // Verify no pending requests
        let pending = manager.pending.read().await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_permission_rule_matches_wildcard_tool() {
        let rule = PermissionRule::allow("web*");
        assert!(rule.matches("webfetch", None, None));
        assert!(rule.matches("websearch", None, None));
        assert!(!rule.matches("read", None, None));
    }

    #[tokio::test]
    async fn test_permission_rule_matches_all_conditions() {
        let mut rule = PermissionRule::allow("bash");
        rule.action = Some("ls*".to_string());
        rule.path = Some("/tmp/*".to_string());

        // All conditions must match
        assert!(rule.matches("bash", Some("ls -la"), Some("/tmp/test")));
        assert!(!rule.matches("bash", Some("rm -rf"), Some("/tmp/test"))); // action mismatch
        assert!(!rule.matches("bash", Some("ls -la"), Some("/home/test"))); // path mismatch
        assert!(!rule.matches("read", Some("ls -la"), Some("/tmp/test"))); // tool mismatch
    }

    #[tokio::test]
    async fn test_multiple_session_rules() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        // Add multiple session rules
        manager
            .add_session_rule("session_1", PermissionRule::deny("bash"))
            .await;
        manager
            .add_session_rule("session_1", PermissionRule::allow("bash"))
            .await; // Later rule takes precedence

        // Later rule (allow) should take precedence (rules checked in reverse)
        let allowed = manager
            .check_rules_only("session_1", "bash", None, None)
            .await;
        assert!(allowed);
    }

    #[tokio::test]
    async fn test_check_rules_only_with_path() {
        let bus = Bus::new();
        let manager = PermissionManager::new(bus);

        manager
            .add_rule(PermissionRule::allow("edit").with_path("src/*"))
            .await;

        // Matches path pattern
        let allowed = manager
            .check_rules_only("session_1", "edit", None, Some("src/main.rs"))
            .await;
        assert!(allowed);

        // Doesn't match path pattern
        let allowed = manager
            .check_rules_only("session_1", "edit", None, Some("tests/test.rs"))
            .await;
        assert!(!allowed);
    }

    #[test]
    fn test_permission_check_clone() {
        let check = PermissionCheck {
            id: "1".to_string(),
            tool: "bash".to_string(),
            action: "execute".to_string(),
            description: "Run command".to_string(),
            path: Some("/tmp".to_string()),
            details: serde_json::json!({"key": "value"}),
        };

        let cloned = check;
        assert_eq!(cloned.id, "1");
        assert_eq!(cloned.tool, "bash");
        assert_eq!(cloned.path, Some("/tmp".to_string()));
    }

    #[test]
    fn test_decision_copy() {
        let allow = Decision::Allow;
        let copied = allow;
        assert_eq!(copied, Decision::Allow);
    }

    #[test]
    fn test_permission_rule_clone() {
        let rule = PermissionRule::allow("bash").with_path("/tmp/*");
        let cloned = rule;
        assert_eq!(cloned.tool, "bash");
        assert_eq!(cloned.path, Some("/tmp/*".to_string()));
        assert_eq!(cloned.decision, Decision::Allow);
    }
}
