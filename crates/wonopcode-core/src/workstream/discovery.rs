//! Git worktree discovery for passive workstreams.

use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::{debug, info, warn};

use super::types::PassiveWorkstream;
use crate::error::{CoreResult, WorkstreamError};

/// Discover all git worktrees in a repository.
///
/// Returns a list of `PassiveWorkstream` entries, one for each worktree
/// including the main working tree (marked as `is_direct = true`).
pub async fn discover_worktrees(repo_path: &Path) -> CoreResult<Vec<PassiveWorkstream>> {
    let repo_root = get_repo_root(repo_path).await?;

    debug!(repo_root = %repo_root.display(), "Discovering worktrees");

    // Run git worktree list --porcelain
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&repo_root)
        .output()
        .map_err(|e| WorkstreamError::Git(format!("Failed to run git worktree list: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(WorkstreamError::Git(format!("git worktree list failed: {}", stderr)).into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let worktrees = parse_worktree_list(&stdout, &repo_root);

    info!(count = worktrees.len(), "Discovered worktrees");

    Ok(worktrees)
}

/// Get the direct (main) workstream for a path.
///
/// This finds the git repository root and returns it as a PassiveWorkstream
/// with `is_direct = true`.
pub async fn get_direct_workstream(path: &Path) -> CoreResult<PassiveWorkstream> {
    let repo_root = get_repo_root(path).await?;
    let branch = get_current_branch(&repo_root).await?;

    Ok(PassiveWorkstream {
        branch,
        path: repo_root.clone(),
        is_direct: true,
        repo_root,
        discovered_at: std::time::SystemTime::now(),
    })
}

/// Get the git repository root for a path.
pub async fn get_repo_root(path: &Path) -> CoreResult<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .map_err(|e| WorkstreamError::Git(format!("Failed to run git rev-parse: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(WorkstreamError::NotGitRepo(format!("{}: {}", path.display(), stderr)).into());
    }

    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();

    Ok(PathBuf::from(root))
}

/// Get the current branch name for a worktree.
async fn get_current_branch(worktree_path: &Path) -> CoreResult<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(worktree_path)
        .output()
        .map_err(|e| WorkstreamError::Git(format!("Failed to get branch: {}", e)))?;

    if !output.status.success() {
        // May be in detached HEAD state, try to get the commit hash
        let output = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(worktree_path)
            .output()
            .map_err(|e| WorkstreamError::Git(format!("Failed to get HEAD commit: {}", e)))?;

        if output.status.success() {
            let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok(format!("detached-{}", commit));
        }

        return Err(WorkstreamError::Git("Failed to determine branch".to_string()).into());
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Handle detached HEAD
    if branch == "HEAD" {
        let output = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(worktree_path)
            .output()
            .map_err(|e| WorkstreamError::Git(format!("Failed to get HEAD commit: {}", e)))?;

        if output.status.success() {
            let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok(format!("detached-{}", commit));
        }
    }

    Ok(branch)
}

/// Parse the output of `git worktree list --porcelain`.
///
/// Format:
/// ```text
/// worktree /path/to/worktree
/// HEAD <commit>
/// branch refs/heads/branch-name
///
/// worktree /path/to/another
/// HEAD <commit>
/// branch refs/heads/other-branch
/// ```
fn parse_worktree_list(output: &str, repo_root: &Path) -> Vec<PassiveWorkstream> {
    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;
    let mut is_first = true;

    for line in output.lines() {
        if line.starts_with("worktree ") {
            // Save previous worktree if complete
            if let (Some(path), Some(branch)) = (current_path.take(), current_branch.take()) {
                let is_direct = is_first;
                is_first = false;

                worktrees.push(PassiveWorkstream {
                    branch,
                    path,
                    is_direct,
                    repo_root: repo_root.to_path_buf(),
                    discovered_at: std::time::SystemTime::now(),
                });
            }

            // Start new worktree
            let path_str = line.strip_prefix("worktree ").unwrap_or("");
            current_path = Some(PathBuf::from(path_str));
        } else if line.starts_with("branch ") {
            // Extract branch name, stripping refs/heads/ prefix
            let branch = line
                .strip_prefix("branch ")
                .unwrap_or("")
                .strip_prefix("refs/heads/")
                .unwrap_or(line.strip_prefix("branch ").unwrap_or(""))
                .to_string();
            current_branch = Some(branch);
        } else if line.starts_with("detached") {
            // Handle detached HEAD - we'll use a placeholder branch name
            current_branch = Some("detached".to_string());
        }
    }

    // Don't forget the last worktree
    if let (Some(path), Some(branch)) = (current_path, current_branch) {
        let is_direct = is_first;

        worktrees.push(PassiveWorkstream {
            branch,
            path,
            is_direct,
            repo_root: repo_root.to_path_buf(),
            discovered_at: std::time::SystemTime::now(),
        });
    }

    worktrees
}

/// Check if a git branch exists in the repository.
fn branch_exists(repo_root: &Path, branch_name: &str) -> bool {
    let output = Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", branch_name),
        ])
        .current_dir(repo_root)
        .output();

    match output {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Create a new git worktree.
pub async fn create_worktree(
    repo_root: &Path,
    branch_name: &str,
    base_branch: &str,
    worktree_dir: Option<&Path>,
) -> CoreResult<PassiveWorkstream> {
    info!(
        repo_root = %repo_root.display(),
        branch = %branch_name,
        base = %base_branch,
        "Creating new worktree"
    );

    // Determine worktree path
    let worktree_path = if let Some(dir) = worktree_dir {
        dir.join(branch_name)
    } else {
        // Default: create in parent directory with branch name
        repo_root
            .parent()
            .ok_or_else(|| WorkstreamError::Git("Cannot determine worktree directory".to_string()))?
            .join(".worktrees")
            .join(branch_name)
    };

    // Ensure parent directory exists
    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Check if branch already exists (e.g., from a previously closed worktree)
    let branch_already_exists = branch_exists(repo_root, branch_name);

    let output = if branch_already_exists {
        // Branch exists - reuse it by creating worktree without -b flag
        info!(branch = %branch_name, "Branch already exists, reusing for worktree");
        Command::new("git")
            .args([
                "worktree",
                "add",
                worktree_path.to_string_lossy().as_ref(),
                branch_name,
            ])
            .current_dir(repo_root)
            .output()
            .map_err(|e| WorkstreamError::Git(format!("Failed to run git worktree add: {}", e)))?
    } else {
        // Branch doesn't exist - create it from base branch
        Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                branch_name,
                worktree_path.to_string_lossy().as_ref(),
                base_branch,
            ])
            .current_dir(repo_root)
            .output()
            .map_err(|e| WorkstreamError::Git(format!("Failed to run git worktree add: {}", e)))?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(WorkstreamError::Git(format!("git worktree add failed: {}", stderr)).into());
    }

    info!(path = %worktree_path.display(), branch_reused = branch_already_exists, "Worktree created");

    Ok(PassiveWorkstream {
        branch: branch_name.to_string(),
        path: worktree_path,
        is_direct: false,
        repo_root: repo_root.to_path_buf(),
        discovered_at: std::time::SystemTime::now(),
    })
}

/// Delete a git worktree.
pub async fn delete_worktree(repo_root: &Path, worktree_path: &Path) -> CoreResult<()> {
    info!(
        repo_root = %repo_root.display(),
        worktree = %worktree_path.display(),
        "Deleting worktree"
    );

    // Remove the worktree
    let output = Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            worktree_path.to_string_lossy().as_ref(),
        ])
        .current_dir(repo_root)
        .output()
        .map_err(|e| WorkstreamError::Git(format!("Failed to run git worktree remove: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        // If worktree doesn't exist in registry, try to clean up manually
        if stderr.contains("not a working tree") {
            warn!("Worktree not in registry, cleaning up directory if it exists");
            if worktree_path.exists() {
                std::fs::remove_dir_all(worktree_path)?;
            }
        } else {
            return Err(
                WorkstreamError::Git(format!("git worktree remove failed: {}", stderr)).into(),
            );
        }
    }

    // Prune any stale worktree references
    let _ = Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(repo_root)
        .output();

    info!(worktree = %worktree_path.display(), "Worktree deleted");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_worktree_list_single() {
        let output = "worktree /home/user/project\nHEAD abc123\nbranch refs/heads/main\n";
        let repo_root = PathBuf::from("/home/user/project");

        let worktrees = parse_worktree_list(output, &repo_root);

        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].branch, "main");
        assert_eq!(worktrees[0].path, PathBuf::from("/home/user/project"));
        assert!(worktrees[0].is_direct);
    }

    #[test]
    fn test_parse_worktree_list_multiple() {
        let output = "\
worktree /home/user/project
HEAD abc123
branch refs/heads/main

worktree /home/user/.worktrees/feature-foo
HEAD def456
branch refs/heads/feature/foo
";
        let repo_root = PathBuf::from("/home/user/project");

        let worktrees = parse_worktree_list(output, &repo_root);

        assert_eq!(worktrees.len(), 2);

        assert_eq!(worktrees[0].branch, "main");
        assert!(worktrees[0].is_direct);

        assert_eq!(worktrees[1].branch, "feature/foo");
        assert!(!worktrees[1].is_direct);
    }

    #[test]
    fn test_parse_worktree_list_detached() {
        let output = "worktree /home/user/project\nHEAD abc123\ndetached\n";
        let repo_root = PathBuf::from("/home/user/project");

        let worktrees = parse_worktree_list(output, &repo_root);

        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].branch, "detached");
    }
}
