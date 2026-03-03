//! Git operations for wonopcode
//!
//! Provides common git operations used across different wonopcode components.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

/// A file with git status information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GitFileStatus {
    /// File path relative to repository root
    pub path: String,
    /// Git status code (e.g., "M ", "A ", "D ", "??")
    pub status: String,
}

/// Summary of a file in a diff (for file list)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffFileSummary {
    /// File path relative to repository root
    pub path: String,
    /// Status: "added", "modified", "deleted", "renamed"
    pub status: String,
    /// Number of lines added
    pub additions: usize,
    /// Number of lines deleted
    pub deletions: usize,
}

/// Full diff for a file
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileDiff {
    /// File path
    pub path: String,
    /// Status
    pub status: String,
    /// Lines added
    pub additions: usize,
    /// Lines deleted
    pub deletions: usize,
    /// Diff lines
    pub lines: Vec<DiffLine>,
}

/// A single line in a diff
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffLine {
    /// Type of line
    pub line_type: DiffLineType,
    /// Old file line number
    pub old_line_num: Option<usize>,
    /// New file line number
    pub new_line_num: Option<usize>,
    /// Line content
    pub content: String,
}

/// Type of diff line
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineType {
    /// @@ header line
    Header,
    /// Added line (+)
    Added,
    /// Removed line (-)
    Removed,
    /// Unchanged context line
    Unchanged,
}

/// Get git status for a repository
///
/// Returns a list of files with their status codes.
/// Uses `git status --porcelain` for machine-readable output.
pub fn get_status(worktree_path: &Path) -> Result<Vec<GitFileStatus>> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git status")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<GitFileStatus> = stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let status = line.get(0..2).unwrap_or("??").to_string();
            let path = line.get(3..).unwrap_or("").to_string();
            GitFileStatus { path, status }
        })
        .collect();

    Ok(files)
}

/// Stage (add) files to the git index
///
/// Runs `git add` for each file in the list.
pub fn stage_files(worktree_path: &Path, files: &[String]) -> Result<()> {
    for file in files {
        let output = Command::new("git")
            .args(["add", file])
            .current_dir(worktree_path)
            .output()
            .context("Failed to run git add")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "git add failed for {}: {}",
                file,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    Ok(())
}

/// Commit staged changes with a message
///
/// Runs `git commit -m "<message>"`.
pub fn commit(worktree_path: &Path, message: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git commit")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

/// Push commits to remote
///
/// Runs `git push`. If that fails (e.g., no upstream branch set),
/// tries `git push --set-upstream origin HEAD`.
pub fn push(worktree_path: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["push"])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git push")?;

    if !output.status.success() {
        // Try push with set-upstream
        let output = Command::new("git")
            .args(["push", "--set-upstream", "origin", "HEAD"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to run git push --set-upstream")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "git push failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    Ok(())
}

/// Stage files, commit, and push in one operation
///
/// This is a convenience function that combines stage_files, commit, and push.
pub fn commit_and_push(worktree_path: &Path, files: &[String], message: &str) -> Result<()> {
    stage_files(worktree_path, files)?;
    commit(worktree_path, message)?;
    push(worktree_path)?;
    Ok(())
}

/// Get the base branch for the current branch
///
/// Tries git config first, then falls back to finding merge-base with common branches.
pub fn get_base_branch(worktree_path: &Path) -> Result<String> {
    // Try to get from git config
    let output = Command::new("git")
        .args(["config", "--get", "branch.HEAD.baseBranch"])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git config")?;

    if output.status.success() {
        let base = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !base.is_empty() {
            return Ok(base);
        }
    }

    // Fallback: try common base branches
    for candidate in &["main", "master", "develop"] {
        let output = Command::new("git")
            .args(["merge-base", "HEAD", candidate])
            .current_dir(worktree_path)
            .output()
            .context("Failed to run git merge-base")?;

        if output.status.success() {
            return Ok(candidate.to_string());
        }
    }

    // Last resort
    Ok("main".to_string())
}

/// Get list of files changed compared to base branch
///
/// Returns a summary of each file with addition/deletion counts.
pub fn get_diff_files(worktree_path: &Path, base_branch: &str) -> Result<Vec<DiffFileSummary>> {
    // Get merge base to compare against
    let merge_base_output = Command::new("git")
        .args(["merge-base", "HEAD", base_branch])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git merge-base")?;

    let merge_base = if merge_base_output.status.success() {
        String::from_utf8_lossy(&merge_base_output.stdout)
            .trim()
            .to_string()
    } else {
        base_branch.to_string()
    };

    // Get name-status
    let status_output = Command::new("git")
        .args(["diff", "--name-status", &format!("{merge_base}..HEAD")])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git diff --name-status")?;

    if !status_output.status.success() {
        return Err(anyhow::anyhow!(
            "git diff --name-status failed: {}",
            String::from_utf8_lossy(&status_output.stderr)
        ));
    }

    // Get numstat
    let numstat_output = Command::new("git")
        .args(["diff", "--numstat", &format!("{merge_base}..HEAD")])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git diff --numstat")?;

    if !numstat_output.status.success() {
        return Err(anyhow::anyhow!(
            "git diff --numstat failed: {}",
            String::from_utf8_lossy(&numstat_output.stderr)
        ));
    }

    // Parse results
    let status_text = String::from_utf8_lossy(&status_output.stdout).to_string();
    let status_lines: Vec<_> = status_text.lines().filter(|l| !l.is_empty()).collect();

    let numstat_text = String::from_utf8_lossy(&numstat_output.stdout).to_string();
    let numstat_lines: Vec<_> = numstat_text.lines().filter(|l| !l.is_empty()).collect();

    let mut files = Vec::new();

    for (status_line, numstat_line) in status_lines.iter().zip(numstat_lines.iter()) {
        let status_parts: Vec<_> = status_line.splitn(2, '\t').collect();
        if status_parts.len() < 2 {
            continue;
        }

        let status_code = status_parts[0];
        let path = status_parts[1].to_string();

        let status = match status_code.chars().next() {
            Some('A') => "added",
            Some('M') => "modified",
            Some('D') => "deleted",
            Some('R') => "renamed",
            _ => "modified",
        }
        .to_string();

        let numstat_parts: Vec<_> = numstat_line.splitn(3, '\t').collect();
        let additions = numstat_parts
            .first()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let deletions = numstat_parts
            .get(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        files.push(DiffFileSummary {
            path,
            status,
            additions,
            deletions,
        });
    }

    Ok(files)
}

/// Get list of files changed with custom compare target
///
/// Supports different diff modes:
/// - base=Some("main"), compare=None: main..working_tree (all changes)
/// - base=Some("main"), compare=Some("HEAD"): main..HEAD (branch changes)
/// - base=Some("HEAD"), compare=None: HEAD..working_tree (uncommitted)
pub fn get_diff_files_with_compare(
    worktree_path: &Path,
    base: Option<&str>,
    compare: Option<&str>,
) -> Result<Vec<DiffFileSummary>> {
    // Build diff spec
    let diff_spec = match (base, compare) {
        (Some(b), Some(c)) => format!("{b}..{c}"),
        (Some(b), None) => b.to_string(), // Compare against working tree
        (None, None) => "HEAD".to_string(), // HEAD vs working tree
        _ => return Ok(Vec::new()),
    };

    // Determine if we're comparing against working tree (should include untracked files)
    let include_untracked = compare.is_none();

    // Get name-status
    let status_output = Command::new("git")
        .args(["diff", "--name-status", &diff_spec])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git diff --name-status")?;

    if !status_output.status.success() {
        return Err(anyhow::anyhow!(
            "git diff --name-status failed: {}",
            String::from_utf8_lossy(&status_output.stderr)
        ));
    }

    // Get numstat
    let numstat_output = Command::new("git")
        .args(["diff", "--numstat", &diff_spec])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git diff --numstat")?;

    if !numstat_output.status.success() {
        return Err(anyhow::anyhow!(
            "git diff --numstat failed: {}",
            String::from_utf8_lossy(&numstat_output.stderr)
        ));
    }

    // Parse results
    let status_text = String::from_utf8_lossy(&status_output.stdout).to_string();
    let status_lines: Vec<_> = status_text.lines().filter(|l| !l.is_empty()).collect();

    let numstat_text = String::from_utf8_lossy(&numstat_output.stdout).to_string();
    let numstat_lines: Vec<_> = numstat_text.lines().filter(|l| !l.is_empty()).collect();

    let mut files = Vec::new();

    for (status_line, numstat_line) in status_lines.iter().zip(numstat_lines.iter()) {
        let status_parts: Vec<_> = status_line.splitn(2, '\t').collect();
        if status_parts.len() < 2 {
            continue;
        }

        let status_code = status_parts[0];
        let path = status_parts[1].to_string();

        let status = match status_code.chars().next() {
            Some('A') => "added",
            Some('M') => "modified",
            Some('D') => "deleted",
            Some('R') => "renamed",
            _ => "unknown",
        };

        let numstat_parts: Vec<_> = numstat_line.split_whitespace().collect();
        let additions = numstat_parts
            .first()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let deletions = numstat_parts
            .get(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        files.push(DiffFileSummary {
            path,
            status: status.to_string(),
            additions,
            deletions,
        });
    }

    // If comparing against working tree, also include untracked files
    if include_untracked {
        let untracked_files = get_untracked_files(worktree_path)?;
        for untracked in untracked_files {
            // Count lines in untracked file for additions count
            let line_count = count_file_lines(worktree_path, &untracked).unwrap_or(0);
            files.push(DiffFileSummary {
                path: untracked,
                status: "untracked".to_string(),
                additions: line_count,
                deletions: 0,
            });
        }
    }

    Ok(files)
}

/// Get list of untracked files in the repository
fn get_untracked_files(worktree_path: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git ls-files")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<String> = stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(|s| s.to_string())
        .collect();

    Ok(files)
}

/// Count number of lines in a file
fn count_file_lines(worktree_path: &Path, file_path: &str) -> Result<usize> {
    let full_path = worktree_path.join(file_path);
    let content = std::fs::read_to_string(&full_path)
        .context(format!("Failed to read file: {}", file_path))?;
    Ok(content.lines().count())
}

/// Check if a file is untracked (not in git index)
fn is_file_untracked(worktree_path: &Path, file_path: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["ls-files", "--error-unmatch", file_path])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git ls-files")?;

    // If ls-files --error-unmatch succeeds, the file is tracked
    // If it fails, the file is untracked
    Ok(!output.status.success())
}

/// Get diff for an untracked file (all lines shown as added)
fn get_untracked_file_diff(worktree_path: &Path, file_path: &str) -> Result<FileDiff> {
    let full_path = worktree_path.join(file_path);
    let content = std::fs::read_to_string(&full_path)
        .context(format!("Failed to read untracked file: {}", file_path))?;

    let file_lines: Vec<&str> = content.lines().collect();
    let line_count = file_lines.len();

    // Create diff lines - all lines are "added"
    let mut diff_lines = Vec::new();

    // Add a header line
    diff_lines.push(DiffLine {
        line_type: DiffLineType::Header,
        old_line_num: None,
        new_line_num: None,
        content: format!("@@ -0,0 +1,{} @@ new file", line_count),
    });

    // Add all lines as "added"
    for (idx, line) in file_lines.iter().enumerate() {
        diff_lines.push(DiffLine {
            line_type: DiffLineType::Added,
            old_line_num: None,
            new_line_num: Some(idx + 1),
            content: line.to_string(),
        });
    }

    Ok(FileDiff {
        path: file_path.to_string(),
        status: "untracked".to_string(),
        additions: line_count,
        deletions: 0,
        lines: diff_lines,
    })
}

/// Get full diff for a specific file
///
/// Returns the complete diff with line-by-line information.
pub fn get_file_diff(worktree_path: &Path, file_path: &str, base_branch: &str) -> Result<FileDiff> {
    // Get merge base
    let merge_base_output = Command::new("git")
        .args(["merge-base", "HEAD", base_branch])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git merge-base")?;

    let merge_base = if merge_base_output.status.success() {
        String::from_utf8_lossy(&merge_base_output.stdout)
            .trim()
            .to_string()
    } else {
        base_branch.to_string()
    };

    // Get diff with context
    let output = Command::new("git")
        .args([
            "diff",
            "-U3",
            &format!("{merge_base}..HEAD"),
            "--",
            file_path,
        ])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git diff")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let diff_text = String::from_utf8_lossy(&output.stdout);

    // Parse the diff
    let lines = parse_unified_diff(&diff_text)?;

    // Count additions and deletions
    let additions = lines
        .iter()
        .filter(|l| matches!(l.line_type, DiffLineType::Added))
        .count();
    let deletions = lines
        .iter()
        .filter(|l| matches!(l.line_type, DiffLineType::Removed))
        .count();

    // Determine status
    let status = if lines.is_empty() {
        "unchanged".to_string()
    } else if lines
        .iter()
        .all(|l| matches!(l.line_type, DiffLineType::Added))
    {
        "added".to_string()
    } else if lines
        .iter()
        .all(|l| matches!(l.line_type, DiffLineType::Removed))
    {
        "deleted".to_string()
    } else {
        "modified".to_string()
    };

    Ok(FileDiff {
        path: file_path.to_string(),
        status,
        additions,
        deletions,
        lines,
    })
}

/// Get full diff for a specific file with custom compare target
///
/// Supports different diff modes (same as get_diff_files_with_compare)
pub fn get_file_diff_with_compare(
    worktree_path: &Path,
    file_path: &str,
    base: Option<&str>,
    compare: Option<&str>,
) -> Result<FileDiff> {
    // Check if comparing against working tree (includes untracked files)
    let include_untracked = compare.is_none();

    // Check if file is untracked
    if include_untracked && is_file_untracked(worktree_path, file_path)? {
        return get_untracked_file_diff(worktree_path, file_path);
    }

    // Build diff spec
    let diff_spec = match (base, compare) {
        (Some(b), Some(c)) => format!("{b}..{c}"),
        (Some(b), None) => b.to_string(), // Compare against working tree
        (None, None) => "HEAD".to_string(), // HEAD vs working tree
        _ => {
            return Ok(FileDiff {
                path: file_path.to_string(),
                status: "unchanged".to_string(),
                additions: 0,
                deletions: 0,
                lines: Vec::new(),
            });
        }
    };

    // Get the unified diff for this file
    let diff_output = Command::new("git")
        .args(["diff", "-U3", &diff_spec, "--", file_path])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git diff")?;

    if !diff_output.status.success() {
        return Err(anyhow::anyhow!(
            "git diff failed: {}",
            String::from_utf8_lossy(&diff_output.stderr)
        ));
    }

    let diff_text = String::from_utf8_lossy(&diff_output.stdout);
    let lines = parse_unified_diff(&diff_text)?;

    // Get stats
    let numstat_output = Command::new("git")
        .args(["diff", "--numstat", &diff_spec, "--", file_path])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git diff --numstat")?;

    let mut additions = 0;
    let mut deletions = 0;
    let mut status = "modified".to_string();

    if numstat_output.status.success() {
        let numstat_text = String::from_utf8_lossy(&numstat_output.stdout);
        if let Some(line) = numstat_text.lines().next() {
            let parts: Vec<_> = line.split_whitespace().collect();
            additions = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            deletions = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }

    // Try to detect file status
    let status_output = Command::new("git")
        .args(["diff", "--name-status", &diff_spec, "--", file_path])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git diff --name-status")?;

    if status_output.status.success() {
        let status_text = String::from_utf8_lossy(&status_output.stdout);
        if let Some(line) = status_text.lines().next() {
            let parts: Vec<_> = line.splitn(2, '\t').collect();
            if let Some(status_code) = parts.first() {
                status = match status_code.chars().next() {
                    Some('A') => "added".to_string(),
                    Some('M') => "modified".to_string(),
                    Some('D') => "deleted".to_string(),
                    Some('R') => "renamed".to_string(),
                    _ => "unknown".to_string(),
                };
            }
        }
    }

    Ok(FileDiff {
        path: file_path.to_string(),
        status,
        additions,
        deletions,
        lines,
    })
}

/// Parse unified diff format into DiffLine structures
fn parse_unified_diff(diff_text: &str) -> Result<Vec<DiffLine>> {
    let mut lines = Vec::new();
    let mut old_line_num = 0usize;
    let mut new_line_num = 0usize;

    for line in diff_text.lines() {
        // Skip file headers
        if line.starts_with("diff --git")
            || line.starts_with("index ")
            || line.starts_with("---")
            || line.starts_with("+++")
        {
            continue;
        }

        // Parse @@ headers
        if line.starts_with("@@") {
            // Extract line numbers from @@ -old_start,old_count +new_start,new_count @@
            if let Some(header_part) = line.split("@@").nth(1) {
                let parts: Vec<_> = header_part.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Some(old_part) = parts[0].strip_prefix('-') {
                        if let Some(start) = old_part.split(',').next() {
                            old_line_num = start.parse().unwrap_or(1);
                        }
                    }
                    if let Some(new_part) = parts[1].strip_prefix('+') {
                        if let Some(start) = new_part.split(',').next() {
                            new_line_num = start.parse().unwrap_or(1);
                        }
                    }
                }
            }

            lines.push(DiffLine {
                line_type: DiffLineType::Header,
                old_line_num: None,
                new_line_num: None,
                content: line.to_string(),
            });
            continue;
        }

        // Parse diff lines
        if line.starts_with('+') && !line.starts_with("+++") {
            lines.push(DiffLine {
                line_type: DiffLineType::Added,
                old_line_num: None,
                new_line_num: Some(new_line_num),
                content: line[1..].to_string(),
            });
            new_line_num += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            lines.push(DiffLine {
                line_type: DiffLineType::Removed,
                old_line_num: Some(old_line_num),
                new_line_num: None,
                content: line[1..].to_string(),
            });
            old_line_num += 1;
        } else if let Some(stripped) = line.strip_prefix(' ') {
            lines.push(DiffLine {
                line_type: DiffLineType::Unchanged,
                old_line_num: Some(old_line_num),
                new_line_num: Some(new_line_num),
                content: stripped.to_string(),
            });
            old_line_num += 1;
            new_line_num += 1;
        }
    }

    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_file_status_serialization() {
        let status = GitFileStatus {
            path: "src/main.rs".to_string(),
            status: "M ".to_string(),
        };

        let json = serde_json::to_string(&status).unwrap();
        let deserialized: GitFileStatus = serde_json::from_str(&json).unwrap();

        assert_eq!(status, deserialized);
    }
}
