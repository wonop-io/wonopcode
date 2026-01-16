//! Bash command permission configuration.
//!
//! This module provides configurable permission rules for bash commands.

use crate::wildcard;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Permission decision for a bash command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BashPermission {
    /// Allow the command without asking.
    Allow,
    /// Deny the command.
    Deny,
    /// Ask the user before executing.
    #[default]
    Ask,
}

/// Bash permission configuration.
///
/// Maps command patterns to permission decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BashPermissionConfig {
    /// Single permission for all commands.
    Single(BashPermission),
    /// Pattern-based permissions.
    Patterns(HashMap<String, BashPermission>),
}

impl Default for BashPermissionConfig {
    fn default() -> Self {
        Self::Patterns(default_bash_permissions())
    }
}

impl BashPermissionConfig {
    /// Check permission for a command.
    ///
    /// Returns the permission decision based on the most specific matching pattern.
    pub fn check(&self, command: &str) -> BashPermission {
        match self {
            Self::Single(perm) => *perm,
            Self::Patterns(patterns) => {
                // Collect all patterns and their permissions
                let mut matching: Vec<(&str, BashPermission)> = patterns
                    .iter()
                    .filter(|(pattern, _)| wildcard::matches(pattern, command))
                    .map(|(p, perm)| (p.as_str(), *perm))
                    .collect();

                // Sort by specificity (most specific first)
                matching.sort_by(|(a, _), (b, _)| {
                    wildcard::specificity(b).cmp(&wildcard::specificity(a))
                });

                // Return the most specific match, or Ask if no match
                matching
                    .first()
                    .map(|(_, perm)| *perm)
                    .unwrap_or(BashPermission::Ask)
            }
        }
    }

    /// Check if a command should be denied.
    pub fn is_denied(&self, command: &str) -> bool {
        self.check(command) == BashPermission::Deny
    }

    /// Check if a command requires asking.
    pub fn requires_ask(&self, command: &str) -> bool {
        self.check(command) == BashPermission::Ask
    }

    /// Check if a command is allowed without asking.
    pub fn is_allowed(&self, command: &str) -> bool {
        self.check(command) == BashPermission::Allow
    }
}

/// Default bash permissions for safe read-only commands.
pub fn default_bash_permissions() -> HashMap<String, BashPermission> {
    let mut perms = HashMap::new();

    // Safe read-only commands - always allow
    perms.insert("cat *".to_string(), BashPermission::Allow);
    perms.insert("cut *".to_string(), BashPermission::Allow);
    perms.insert("diff *".to_string(), BashPermission::Allow);
    perms.insert("du *".to_string(), BashPermission::Allow);
    perms.insert("file *".to_string(), BashPermission::Allow);
    perms.insert("head *".to_string(), BashPermission::Allow);
    perms.insert("less *".to_string(), BashPermission::Allow);
    perms.insert("ls *".to_string(), BashPermission::Allow);
    perms.insert("ls".to_string(), BashPermission::Allow);
    perms.insert("more *".to_string(), BashPermission::Allow);
    perms.insert("pwd".to_string(), BashPermission::Allow);
    perms.insert("pwd *".to_string(), BashPermission::Allow);
    perms.insert("stat *".to_string(), BashPermission::Allow);
    perms.insert("tail *".to_string(), BashPermission::Allow);
    perms.insert("wc *".to_string(), BashPermission::Allow);
    perms.insert("whereis *".to_string(), BashPermission::Allow);
    perms.insert("which *".to_string(), BashPermission::Allow);

    // Search commands - always allow
    perms.insert("find *".to_string(), BashPermission::Allow);
    perms.insert("grep *".to_string(), BashPermission::Allow);
    perms.insert("rg *".to_string(), BashPermission::Allow);
    perms.insert("tree *".to_string(), BashPermission::Allow);
    perms.insert("tree".to_string(), BashPermission::Allow);

    // Sort/uniq - safe for reading
    perms.insert("sort *".to_string(), BashPermission::Allow);
    perms.insert("uniq *".to_string(), BashPermission::Allow);

    // Git read-only commands - always allow
    perms.insert("git status".to_string(), BashPermission::Allow);
    perms.insert("git status *".to_string(), BashPermission::Allow);
    perms.insert("git log *".to_string(), BashPermission::Allow);
    perms.insert("git log".to_string(), BashPermission::Allow);
    perms.insert("git diff *".to_string(), BashPermission::Allow);
    perms.insert("git diff".to_string(), BashPermission::Allow);
    perms.insert("git show *".to_string(), BashPermission::Allow);
    perms.insert("git branch".to_string(), BashPermission::Allow);
    perms.insert("git branch -v".to_string(), BashPermission::Allow);
    perms.insert("git branch -a".to_string(), BashPermission::Allow);
    perms.insert("git remote *".to_string(), BashPermission::Allow);
    perms.insert("git rev-parse *".to_string(), BashPermission::Allow);

    // Dangerous find operations - ask
    perms.insert("find * -delete*".to_string(), BashPermission::Ask);
    perms.insert("find * -exec*".to_string(), BashPermission::Ask);
    perms.insert("find * -ok*".to_string(), BashPermission::Ask);

    // Sort/tree with output - ask
    perms.insert("sort --output=*".to_string(), BashPermission::Ask);
    perms.insert("sort -o *".to_string(), BashPermission::Ask);
    perms.insert("tree -o *".to_string(), BashPermission::Ask);

    // Dangerous commands - ask by default
    perms.insert("rm *".to_string(), BashPermission::Ask);
    perms.insert("rmdir *".to_string(), BashPermission::Ask);
    perms.insert("mv *".to_string(), BashPermission::Ask);
    perms.insert("cp *".to_string(), BashPermission::Ask);
    perms.insert("chmod *".to_string(), BashPermission::Ask);
    perms.insert("chown *".to_string(), BashPermission::Ask);

    // Default catch-all - ask for everything else
    perms.insert("*".to_string(), BashPermission::Ask);

    perms
}

/// Read-only bash permissions.
///
/// Allows only safe read-only commands, asks for everything else.
pub fn readonly_bash_permissions() -> HashMap<String, BashPermission> {
    let mut perms = default_bash_permissions();

    // Deny all write operations
    perms.insert("rm *".to_string(), BashPermission::Deny);
    perms.insert("rmdir *".to_string(), BashPermission::Deny);
    perms.insert("mv *".to_string(), BashPermission::Deny);
    perms.insert("cp *".to_string(), BashPermission::Deny);
    perms.insert("chmod *".to_string(), BashPermission::Deny);
    perms.insert("chown *".to_string(), BashPermission::Deny);
    perms.insert("mkdir *".to_string(), BashPermission::Deny);
    perms.insert("touch *".to_string(), BashPermission::Deny);

    // Deny git write operations
    perms.insert("git add *".to_string(), BashPermission::Deny);
    perms.insert("git commit *".to_string(), BashPermission::Deny);
    perms.insert("git push *".to_string(), BashPermission::Deny);
    perms.insert("git merge *".to_string(), BashPermission::Deny);
    perms.insert("git rebase *".to_string(), BashPermission::Deny);
    perms.insert("git reset *".to_string(), BashPermission::Deny);
    perms.insert("git checkout *".to_string(), BashPermission::Deny);

    perms
}

/// Check if a path is external to the given root directory.
pub fn is_external_path(root: &std::path::Path, target: &std::path::Path) -> bool {
    // Canonicalize both paths for comparison
    let root_canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let target_canonical = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());

    !target_canonical.starts_with(&root_canonical)
}

/// Extract path arguments from a bash command for external directory checking.
///
/// Returns paths from commands that might access external directories.
pub fn extract_path_args(command: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let parts: Vec<&str> = command.split_whitespace().collect();

    if parts.is_empty() {
        return paths;
    }

    let cmd = parts[0];

    // Commands that take path arguments
    let path_commands = [
        "cd", "ls", "cat", "head", "tail", "less", "more", "stat", "rm", "rmdir", "mv", "cp",
        "chmod", "chown", "mkdir", "touch", "find", "tree", "du", "file",
    ];

    if path_commands.contains(&cmd) {
        for arg in &parts[1..] {
            // Skip flags
            if arg.starts_with('-') {
                continue;
            }
            // Skip chmod mode arguments like +x, 755
            if cmd == "chmod" && (arg.starts_with('+') || arg.chars().all(|c| c.is_ascii_digit())) {
                continue;
            }
            paths.push((*arg).to_string());
        }
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_permissions() {
        let config = BashPermissionConfig::default();

        // Safe commands should be allowed
        assert_eq!(config.check("ls"), BashPermission::Allow);
        assert_eq!(config.check("git status"), BashPermission::Allow);
        assert_eq!(config.check("git diff --cached"), BashPermission::Allow);
        assert_eq!(config.check("grep -r foo"), BashPermission::Allow);
        assert_eq!(config.check("cat file.txt"), BashPermission::Allow);

        // Dangerous commands should ask
        assert_eq!(config.check("rm -rf /"), BashPermission::Ask);
        assert_eq!(config.check("mv old new"), BashPermission::Ask);
        assert_eq!(config.check("find . -delete"), BashPermission::Ask);
    }

    #[test]
    fn test_single_permission() {
        let config = BashPermissionConfig::Single(BashPermission::Allow);
        assert_eq!(config.check("anything"), BashPermission::Allow);

        let config = BashPermissionConfig::Single(BashPermission::Deny);
        assert_eq!(config.check("anything"), BashPermission::Deny);
    }

    #[test]
    fn test_readonly_permissions() {
        let perms = readonly_bash_permissions();
        let config = BashPermissionConfig::Patterns(perms);

        // Safe commands still allowed
        assert_eq!(config.check("ls"), BashPermission::Allow);
        assert_eq!(config.check("git status"), BashPermission::Allow);

        // Write commands denied
        assert_eq!(config.check("rm file.txt"), BashPermission::Deny);
        assert_eq!(config.check("git add ."), BashPermission::Deny);
    }

    #[test]
    fn test_specificity() {
        let mut perms = HashMap::new();
        perms.insert("*".to_string(), BashPermission::Ask);
        perms.insert("git *".to_string(), BashPermission::Allow);
        perms.insert("git push *".to_string(), BashPermission::Deny);

        let config = BashPermissionConfig::Patterns(perms);

        // Most specific should win
        assert_eq!(config.check("git push origin main"), BashPermission::Deny);
        assert_eq!(config.check("git status"), BashPermission::Allow);
        assert_eq!(config.check("echo hello"), BashPermission::Ask);
    }

    #[test]
    fn test_extract_path_args() {
        assert_eq!(extract_path_args("ls -la /tmp"), vec!["/tmp"]);
        assert_eq!(extract_path_args("cp -r src dest"), vec!["src", "dest"]);
        assert_eq!(extract_path_args("chmod +x script.sh"), vec!["script.sh"]);
        assert_eq!(extract_path_args("chmod 755 script.sh"), vec!["script.sh"]);
        assert_eq!(extract_path_args("rm -rf ./foo"), vec!["./foo"]);
        assert_eq!(extract_path_args("echo hello").len(), 0);
    }

    #[test]
    fn test_is_external_path() {
        let _root = std::path::Path::new("/tmp/project");

        // These would need actual directories to test properly
        // For now, just test the logic with paths that don't need canonicalization
        let _target = std::path::Path::new("/tmp/project/src");
        // This will fail to canonicalize since paths don't exist, but logic is right
        // assert!(!is_external_path(_root, _target));
    }

    #[test]
    fn test_is_external_path_with_real_paths() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let subdir = dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();

        // Subdir is inside root
        assert!(!is_external_path(dir.path(), &subdir));

        // Different temp dir is external
        let other_dir = tempdir().unwrap();
        assert!(is_external_path(dir.path(), other_dir.path()));
    }

    #[test]
    fn test_bash_permission_is_denied() {
        let config = BashPermissionConfig::Single(BashPermission::Deny);
        assert!(config.is_denied("anything"));

        let config2 = BashPermissionConfig::Single(BashPermission::Allow);
        assert!(!config2.is_denied("anything"));
    }

    #[test]
    fn test_bash_permission_requires_ask() {
        let config = BashPermissionConfig::Single(BashPermission::Ask);
        assert!(config.requires_ask("anything"));

        let config2 = BashPermissionConfig::Single(BashPermission::Allow);
        assert!(!config2.requires_ask("anything"));
    }

    #[test]
    fn test_bash_permission_is_allowed() {
        let config = BashPermissionConfig::Single(BashPermission::Allow);
        assert!(config.is_allowed("anything"));

        let config2 = BashPermissionConfig::Single(BashPermission::Deny);
        assert!(!config2.is_allowed("anything"));
    }

    #[test]
    fn test_extract_path_args_cd_command() {
        assert_eq!(extract_path_args("cd /home/user"), vec!["/home/user"]);
        assert_eq!(extract_path_args("cd"), Vec::<String>::new());
    }

    #[test]
    fn test_extract_path_args_mkdir() {
        assert_eq!(extract_path_args("mkdir -p /tmp/new/dir"), vec!["/tmp/new/dir"]);
    }

    #[test]
    fn test_extract_path_args_touch() {
        assert_eq!(extract_path_args("touch file.txt"), vec!["file.txt"]);
    }

    #[test]
    fn test_extract_path_args_tree() {
        assert_eq!(extract_path_args("tree /some/path"), vec!["/some/path"]);
    }

    #[test]
    fn test_extract_path_args_du() {
        assert_eq!(extract_path_args("du -h /var"), vec!["/var"]);
    }

    #[test]
    fn test_extract_path_args_file() {
        assert_eq!(extract_path_args("file binary.exe"), vec!["binary.exe"]);
    }

    #[test]
    fn test_bash_permission_default_is_ask() {
        let perm = BashPermission::default();
        assert_eq!(perm, BashPermission::Ask);
    }

    #[test]
    fn test_bash_permission_serialization() {
        let perm = BashPermission::Allow;
        let json = serde_json::to_string(&perm).unwrap();
        assert_eq!(json, "\"allow\"");

        let parsed: BashPermission = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, BashPermission::Allow);
    }

    #[test]
    fn test_bash_permission_config_serialization() {
        let config = BashPermissionConfig::Single(BashPermission::Deny);
        let json = serde_json::to_string(&config).unwrap();
        let parsed: BashPermissionConfig = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, BashPermissionConfig::Single(BashPermission::Deny)));
    }

    #[test]
    fn test_patterns_check_no_match_returns_ask() {
        let mut perms = HashMap::new();
        perms.insert("specific_command".to_string(), BashPermission::Allow);
        let config = BashPermissionConfig::Patterns(perms);

        // Something that doesn't match any pattern
        assert_eq!(config.check("completely_different"), BashPermission::Ask);
    }
}
