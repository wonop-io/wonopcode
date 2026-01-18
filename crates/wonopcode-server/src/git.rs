//! Git operations for the headless server.
//!
//! This module provides git operations (stage, unstage, commit, etc.) using the git2 crate.

use chrono::{DateTime, Utc};
use git2::{Repository, StatusOptions};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Git file status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFileStatus {
    /// Relative path to file.
    pub path: String,
    /// Status flags.
    pub status: GitFileState,
    /// Whether file is staged.
    pub staged: bool,
}

/// Git file state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GitFileState {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

impl std::fmt::Display for GitFileState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitFileState::Modified => write!(f, "M"),
            GitFileState::Added => write!(f, "A"),
            GitFileState::Deleted => write!(f, "D"),
            GitFileState::Renamed => write!(f, "R"),
            GitFileState::Untracked => write!(f, "?"),
            GitFileState::Conflicted => write!(f, "C"),
        }
    }
}

/// Git repository status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatus {
    /// Current branch name.
    pub branch: String,
    /// Remote tracking branch (if any).
    pub upstream: Option<String>,
    /// Commits ahead of upstream.
    pub ahead: usize,
    /// Commits behind upstream.
    pub behind: usize,
    /// Files with changes.
    pub files: Vec<GitFileStatus>,
}

/// A commit in history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitInfo {
    /// Commit hash (short).
    pub id: String,
    /// Full commit hash.
    pub full_id: String,
    /// Commit message (first line).
    pub message: String,
    /// Author name.
    pub author: String,
    /// Author email.
    pub email: String,
    /// Commit timestamp (ISO 8601).
    pub timestamp: String,
}

impl GitCommitInfo {
    fn from_commit(commit: &git2::Commit) -> Self {
        Self {
            id: commit.id().to_string().chars().take(7).collect(),
            full_id: commit.id().to_string(),
            message: commit.summary().unwrap_or("").to_string(),
            author: commit.author().name().unwrap_or("").to_string(),
            email: commit.author().email().unwrap_or("").to_string(),
            timestamp: DateTime::<Utc>::from_timestamp(commit.time().seconds(), 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default(),
        }
    }
}

/// Error type for git operations.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("Git error: {0}")]
    Git(#[from] git2::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Path error: {0}")]
    Path(String),
    #[error("Operation not supported: {0}")]
    NotSupported(String),
}

/// Git operations handler.
pub struct GitOperations {
    working_dir: std::path::PathBuf,
}

impl GitOperations {
    /// Create a new git operations handler for the given working directory.
    pub fn new(working_dir: impl AsRef<Path>) -> Self {
        Self {
            working_dir: working_dir.as_ref().to_path_buf(),
        }
    }

    /// Open the repository for this working directory.
    fn open_repo(&self) -> Result<Repository, GitError> {
        Ok(Repository::discover(&self.working_dir)?)
    }

    /// Get repository status.
    pub fn status(&self) -> Result<GitStatus, GitError> {
        let repo = self.open_repo()?;

        // Get current branch
        let branch = match repo.head() {
            Ok(head) => head
                .shorthand()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "HEAD".to_string()),
            Err(_) => "HEAD".to_string(),
        };

        // Get upstream info
        let (upstream, ahead, behind) = self.get_upstream_info(&repo, &branch)?;

        // Get file statuses
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false)
            .include_unmodified(false);

        let statuses = repo.statuses(Some(&mut opts))?;
        let mut files = Vec::new();

        for entry in statuses.iter() {
            let path = entry.path().unwrap_or("").to_string();
            let status = entry.status();

            // Check staged status
            let staged_status = if status.is_index_new() {
                Some((GitFileState::Added, true))
            } else if status.is_index_modified() {
                Some((GitFileState::Modified, true))
            } else if status.is_index_deleted() {
                Some((GitFileState::Deleted, true))
            } else if status.is_index_renamed() {
                Some((GitFileState::Renamed, true))
            } else {
                None
            };

            // Check unstaged status
            let unstaged_status = if status.is_wt_new() {
                Some((GitFileState::Untracked, false))
            } else if status.is_wt_modified() {
                Some((GitFileState::Modified, false))
            } else if status.is_wt_deleted() {
                Some((GitFileState::Deleted, false))
            } else if status.is_wt_renamed() {
                Some((GitFileState::Renamed, false))
            } else if status.is_conflicted() {
                Some((GitFileState::Conflicted, false))
            } else {
                None
            };

            // Add staged entry if present
            if let Some((state, staged)) = staged_status {
                files.push(GitFileStatus {
                    path: path.clone(),
                    status: state,
                    staged,
                });
            }

            // Add unstaged entry if present
            if let Some((state, staged)) = unstaged_status {
                files.push(GitFileStatus {
                    path,
                    status: state,
                    staged,
                });
            }
        }

        Ok(GitStatus {
            branch,
            upstream,
            ahead,
            behind,
            files,
        })
    }

    /// Get upstream tracking info.
    fn get_upstream_info(
        &self,
        repo: &Repository,
        branch: &str,
    ) -> Result<(Option<String>, usize, usize), GitError> {
        let local_branch = match repo.find_branch(branch, git2::BranchType::Local) {
            Ok(b) => b,
            Err(_) => return Ok((None, 0, 0)),
        };

        let upstream = match local_branch.upstream() {
            Ok(b) => b,
            Err(_) => return Ok((None, 0, 0)),
        };

        let upstream_name = upstream.name()?.map(|s| s.to_string());

        // Get ahead/behind counts
        let local_oid = repo
            .head()?
            .target()
            .ok_or_else(|| git2::Error::from_str("HEAD has no target"))?;
        let upstream_oid = upstream
            .get()
            .target()
            .ok_or_else(|| git2::Error::from_str("Upstream has no target"))?;

        let (ahead, behind) = repo.graph_ahead_behind(local_oid, upstream_oid)?;

        Ok((upstream_name, ahead, behind))
    }

    /// Stage files.
    ///
    /// If paths is empty, stages all modified files.
    pub fn stage(&self, paths: &[String]) -> Result<(), GitError> {
        let repo = self.open_repo()?;
        let mut index = repo.index()?;

        if paths.is_empty() {
            // Stage all
            index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
        } else {
            for path in paths {
                // Validate path is within repo
                let full_path = self.working_dir.join(path);
                if !full_path.starts_with(&self.working_dir) {
                    return Err(GitError::Path(format!(
                        "Path '{path}' is outside repository"
                    )));
                }

                // Check if file was deleted
                if full_path.exists() {
                    index.add_path(Path::new(path))?;
                } else {
                    // For deleted files, update the index
                    index.remove_path(Path::new(path))?;
                }
            }
        }

        index.write()?;
        Ok(())
    }

    /// Unstage files.
    ///
    /// If paths is empty, unstages all staged files.
    pub fn unstage(&self, paths: &[String]) -> Result<(), GitError> {
        let repo = self.open_repo()?;

        // Get HEAD commit
        let head = match repo.head() {
            Ok(h) => Some(h.peel_to_commit()?),
            Err(_) => None, // Empty repository
        };

        if paths.is_empty() {
            // Unstage all - reset index to HEAD
            if let Some(commit) = head {
                repo.reset(commit.as_object(), git2::ResetType::Mixed, None)?;
            } else {
                // Empty repository - clear the entire index
                let mut index = repo.index()?;
                index.clear()?;
                index.write()?;
            }
        } else {
            // Unstage specific files
            if let Some(commit) = head {
                let paths_iter = paths.iter().map(|p| Path::new(p.as_str()));
                repo.reset_default(Some(&commit.into_object()), paths_iter)?;
            } else {
                // Empty repository - remove specific files from index
                let mut index = repo.index()?;
                for path in paths {
                    index.remove_path(Path::new(path))?;
                }
                index.write()?;
            }
        }

        Ok(())
    }

    /// Checkout (discard changes to) files.
    ///
    /// This discards local changes and restores files from HEAD.
    pub fn checkout(&self, paths: &[String]) -> Result<(), GitError> {
        let repo = self.open_repo()?;

        if paths.is_empty() {
            return Err(GitError::Path("Must specify files to checkout".to_string()));
        }

        let mut opts = git2::build::CheckoutBuilder::new();
        opts.force();

        for path in paths {
            // Validate path is within repo
            let full_path = self.working_dir.join(path);
            if !full_path.starts_with(&self.working_dir) {
                return Err(GitError::Path(format!(
                    "Path '{path}' is outside repository"
                )));
            }
            opts.path(path);
        }

        repo.checkout_head(Some(&mut opts))?;
        Ok(())
    }

    /// Create a commit with the given message.
    pub fn commit(&self, message: &str) -> Result<GitCommitInfo, GitError> {
        let repo = self.open_repo()?;
        let sig = repo.signature()?;
        let mut index = repo.index()?;

        // Check if there are staged changes
        let statuses = repo.statuses(None)?;
        let has_staged = statuses.iter().any(|s| {
            let status = s.status();
            status.is_index_new()
                || status.is_index_modified()
                || status.is_index_deleted()
                || status.is_index_renamed()
        });

        if !has_staged {
            return Err(GitError::NotSupported(
                "Nothing to commit - no staged changes".to_string(),
            ));
        }

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;

        // Get parent commit (if not initial commit)
        let parents = match repo.head() {
            Ok(head) => {
                let parent = head.peel_to_commit()?;
                vec![parent]
            }
            Err(_) => vec![], // Initial commit
        };

        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

        let commit_id = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)?;

        let commit = repo.find_commit(commit_id)?;
        Ok(GitCommitInfo::from_commit(&commit))
    }

    /// Get commit history.
    pub fn history(&self, limit: usize) -> Result<Vec<GitCommitInfo>, GitError> {
        let repo = self.open_repo()?;
        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(git2::Sort::TIME)?;

        let mut commits = Vec::new();
        for (i, oid) in revwalk.enumerate() {
            if i >= limit {
                break;
            }
            let commit = repo.find_commit(oid?)?;
            commits.push(GitCommitInfo::from_commit(&commit));
        }
        Ok(commits)
    }

    /// Push to remote.
    pub fn push(
        &self,
        remote_name: Option<&str>,
        branch_name: Option<&str>,
    ) -> Result<(), GitError> {
        let repo = self.open_repo()?;

        let remote_name = remote_name.unwrap_or("origin");
        let branch = match branch_name {
            Some(b) => b.to_string(),
            None => {
                // Get current branch
                repo.head()?
                    .shorthand()
                    .map(|s| s.to_string())
                    .ok_or_else(|| {
                        GitError::NotSupported("Cannot determine current branch".to_string())
                    })?
            }
        };

        let mut remote = repo.find_remote(remote_name)?;
        let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");

        // Configure callbacks for authentication
        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials(|_url, username_from_url, allowed_types| {
            // Try SSH agent first
            if allowed_types.contains(git2::CredentialType::SSH_KEY) {
                return git2::Cred::ssh_key_from_agent(username_from_url.unwrap_or("git"));
            }
            // Fall back to default credentials
            git2::Cred::default()
        });

        let mut push_opts = git2::PushOptions::new();
        push_opts.remote_callbacks(callbacks);

        remote.push(&[&refspec], Some(&mut push_opts))?;
        Ok(())
    }

    /// Pull from remote (fetch + fast-forward merge).
    pub fn pull(
        &self,
        remote_name: Option<&str>,
        branch_name: Option<&str>,
    ) -> Result<(), GitError> {
        let repo = self.open_repo()?;

        let remote_name = remote_name.unwrap_or("origin");
        let branch = match branch_name {
            Some(b) => b.to_string(),
            None => {
                // Get current branch
                repo.head()?
                    .shorthand()
                    .map(|s| s.to_string())
                    .ok_or_else(|| {
                        GitError::NotSupported("Cannot determine current branch".to_string())
                    })?
            }
        };

        // Fetch
        let mut remote = repo.find_remote(remote_name)?;
        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials(|_url, username_from_url, allowed_types| {
            if allowed_types.contains(git2::CredentialType::SSH_KEY) {
                return git2::Cred::ssh_key_from_agent(username_from_url.unwrap_or("git"));
            }
            git2::Cred::default()
        });

        let mut fetch_opts = git2::FetchOptions::new();
        fetch_opts.remote_callbacks(callbacks);
        remote.fetch(&[&branch], Some(&mut fetch_opts), None)?;

        // Get fetch head
        let fetch_head = repo.find_reference("FETCH_HEAD")?;
        let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;

        // Analyze merge
        let (analysis, _) = repo.merge_analysis(&[&fetch_commit])?;

        if analysis.is_up_to_date() {
            // Already up to date
            return Ok(());
        }

        if analysis.is_fast_forward() {
            // Fast-forward merge
            let ref_name = format!("refs/heads/{branch}");
            let mut reference = repo.find_reference(&ref_name)?;
            reference.set_target(fetch_commit.id(), "Fast-forward")?;
            repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
            return Ok(());
        }

        // Regular merge or other situation - not supported via API
        Err(GitError::NotSupported(
            "Merge required - please use git manually to resolve".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_repo() -> (TempDir, Repository) {
        let temp_dir = TempDir::new().unwrap();
        let repo = Repository::init(temp_dir.path()).unwrap();

        // Configure user for commits
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();

        (temp_dir, repo)
    }

    #[test]
    fn test_status_empty_repo() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        let status = ops.status().unwrap();
        assert!(status.files.is_empty());
    }

    #[test]
    fn test_stage_and_unstage() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // Create a file
        fs::write(temp_dir.path().join("test.txt"), "hello").unwrap();

        // Check status - should be untracked
        let status = ops.status().unwrap();
        assert_eq!(status.files.len(), 1);
        assert!(!status.files[0].staged);

        // Stage it
        ops.stage(&["test.txt".to_string()]).unwrap();

        // Check status - should be staged
        let status = ops.status().unwrap();
        assert_eq!(status.files.len(), 1);
        assert!(status.files[0].staged);

        // Unstage it
        ops.unstage(&["test.txt".to_string()]).unwrap();

        // Check status - should be untracked again
        let status = ops.status().unwrap();
        assert_eq!(status.files.len(), 1);
        assert!(!status.files[0].staged);
    }

    #[test]
    fn test_commit() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // Create and stage a file
        fs::write(temp_dir.path().join("test.txt"), "hello").unwrap();
        ops.stage(&["test.txt".to_string()]).unwrap();

        // Commit
        let commit = ops.commit("Initial commit").unwrap();
        assert_eq!(commit.message, "Initial commit");
        assert_eq!(commit.author, "Test User");

        // Check history
        let history = ops.history(10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].message, "Initial commit");
    }

    // === UX-critical tests for git operations ===

    #[test]
    fn user_sees_modified_file_status_after_editing() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // Create and commit initial file
        fs::write(temp_dir.path().join("file.txt"), "initial content").unwrap();
        ops.stage(&["file.txt".to_string()]).unwrap();
        ops.commit("Initial").unwrap();

        // Modify the file
        fs::write(temp_dir.path().join("file.txt"), "modified content").unwrap();

        // User should see modified status
        let status = ops.status().unwrap();
        assert_eq!(status.files.len(), 1);
        assert_eq!(status.files[0].status, GitFileState::Modified);
        assert!(!status.files[0].staged);
    }

    #[test]
    fn user_can_discard_changes_with_checkout() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // Create and commit initial file
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "initial content").unwrap();
        ops.stage(&["file.txt".to_string()]).unwrap();
        ops.commit("Initial").unwrap();

        // Modify the file
        fs::write(&file_path, "unwanted changes").unwrap();
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "unwanted changes");

        // User discards changes
        ops.checkout(&["file.txt".to_string()]).unwrap();

        // File should be restored to committed state
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "initial content");
    }

    #[test]
    fn user_sees_error_when_checkout_called_without_paths() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // User tries to checkout without specifying files
        let result = ops.checkout(&[]);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Must specify files"));
    }

    #[test]
    fn user_sees_commit_history() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // Create first commit
        fs::write(temp_dir.path().join("file1.txt"), "content1").unwrap();
        ops.stage(&["file1.txt".to_string()]).unwrap();
        ops.commit("First commit").unwrap();

        // Create second commit
        fs::write(temp_dir.path().join("file2.txt"), "content2").unwrap();
        ops.stage(&["file2.txt".to_string()]).unwrap();
        ops.commit("Second commit").unwrap();

        // Create third commit
        fs::write(temp_dir.path().join("file3.txt"), "content3").unwrap();
        ops.stage(&["file3.txt".to_string()]).unwrap();
        ops.commit("Third commit").unwrap();

        // User views history - all commits should be present
        let history = ops.history(10).unwrap();
        assert_eq!(history.len(), 3);

        // All commits should be in the history
        let messages: Vec<&str> = history.iter().map(|c| c.message.as_str()).collect();
        assert!(messages.contains(&"First commit"));
        assert!(messages.contains(&"Second commit"));
        assert!(messages.contains(&"Third commit"));
    }

    #[test]
    fn user_history_limit_is_respected() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // Create 5 commits
        for i in 1..=5 {
            fs::write(temp_dir.path().join(format!("file{i}.txt")), "content").unwrap();
            ops.stage(&[format!("file{i}.txt")]).unwrap();
            ops.commit(&format!("Commit {i}")).unwrap();
        }

        // User requests only 2 commits - should get exactly 2
        let history = ops.history(2).unwrap();
        assert_eq!(history.len(), 2);

        // Request all 5
        let history_all = ops.history(10).unwrap();
        assert_eq!(history_all.len(), 5);
    }

    #[test]
    fn user_cannot_commit_without_staged_changes() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // Create an initial commit
        fs::write(temp_dir.path().join("file.txt"), "content").unwrap();
        ops.stage(&["file.txt".to_string()]).unwrap();
        ops.commit("Initial").unwrap();

        // User tries to commit with no staged changes
        let result = ops.commit("Empty commit");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Nothing to commit"));
    }

    #[test]
    fn user_can_stage_all_files_at_once() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // Create multiple files
        fs::write(temp_dir.path().join("file1.txt"), "content1").unwrap();
        fs::write(temp_dir.path().join("file2.txt"), "content2").unwrap();
        fs::write(temp_dir.path().join("file3.txt"), "content3").unwrap();

        // User stages all files
        ops.stage(&[]).unwrap();

        // All files should be staged
        let status = ops.status().unwrap();
        let staged_count = status.files.iter().filter(|f| f.staged).count();
        assert_eq!(staged_count, 3);
    }

    #[test]
    fn user_can_unstage_all_files_at_once() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // Create and stage multiple files
        fs::write(temp_dir.path().join("file1.txt"), "content1").unwrap();
        fs::write(temp_dir.path().join("file2.txt"), "content2").unwrap();
        ops.stage(&[]).unwrap();

        // Verify staged
        let status = ops.status().unwrap();
        assert!(status.files.iter().all(|f| f.staged));

        // User unstages all
        ops.unstage(&[]).unwrap();

        // All files should be unstaged
        let status = ops.status().unwrap();
        assert!(status.files.iter().all(|f| !f.staged));
    }

    #[test]
    fn user_sees_deleted_file_status() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // Create and commit a file
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "content").unwrap();
        ops.stage(&["file.txt".to_string()]).unwrap();
        ops.commit("Initial").unwrap();

        // Delete the file
        fs::remove_file(&file_path).unwrap();

        // User should see deleted status
        let status = ops.status().unwrap();
        assert_eq!(status.files.len(), 1);
        assert_eq!(status.files[0].status, GitFileState::Deleted);
    }

    #[test]
    fn user_can_stage_deleted_file() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // Create and commit a file
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "content").unwrap();
        ops.stage(&["file.txt".to_string()]).unwrap();
        ops.commit("Initial").unwrap();

        // Delete the file
        fs::remove_file(&file_path).unwrap();

        // User stages the deletion
        ops.stage(&["file.txt".to_string()]).unwrap();

        // Deletion should be staged
        let status = ops.status().unwrap();
        assert_eq!(status.files.len(), 1);
        assert!(status.files[0].staged);
        assert_eq!(status.files[0].status, GitFileState::Deleted);
    }

    #[test]
    fn git_file_state_display_shows_correct_symbols() {
        assert_eq!(format!("{}", GitFileState::Modified), "M");
        assert_eq!(format!("{}", GitFileState::Added), "A");
        assert_eq!(format!("{}", GitFileState::Deleted), "D");
        assert_eq!(format!("{}", GitFileState::Renamed), "R");
        assert_eq!(format!("{}", GitFileState::Untracked), "?");
        assert_eq!(format!("{}", GitFileState::Conflicted), "C");
    }

    #[test]
    fn git_file_state_serializes_to_snake_case() {
        let json = serde_json::to_string(&GitFileState::Modified).unwrap();
        assert_eq!(json, r#""modified""#);

        let json = serde_json::to_string(&GitFileState::Untracked).unwrap();
        assert_eq!(json, r#""untracked""#);
    }

    #[test]
    fn git_status_serializes_for_api_response() {
        let status = GitStatus {
            branch: "main".to_string(),
            upstream: Some("origin/main".to_string()),
            ahead: 1,
            behind: 0,
            files: vec![GitFileStatus {
                path: "test.txt".to_string(),
                status: GitFileState::Modified,
                staged: true,
            }],
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"branch\":\"main\""));
        assert!(json.contains("\"upstream\":\"origin/main\""));
        assert!(json.contains("\"ahead\":1"));
        assert!(json.contains("\"files\""));
    }

    #[test]
    fn git_commit_info_serializes_for_history_display() {
        let commit = GitCommitInfo {
            id: "abc1234".to_string(),
            full_id: "abc1234567890abcdef".to_string(),
            message: "Test commit".to_string(),
            author: "Test User".to_string(),
            email: "test@example.com".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&commit).unwrap();
        assert!(json.contains("\"id\":\"abc1234\""));
        assert!(json.contains("\"message\":\"Test commit\""));
        assert!(json.contains("\"author\":\"Test User\""));
    }

    #[test]
    fn git_error_displays_descriptive_message() {
        let err = GitError::Path("invalid path".to_string());
        assert_eq!(err.to_string(), "Path error: invalid path");

        let err = GitError::NotSupported("merge required".to_string());
        assert_eq!(err.to_string(), "Operation not supported: merge required");
    }

    #[test]
    fn commit_info_has_short_and_full_hash() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // Create a commit
        fs::write(temp_dir.path().join("file.txt"), "content").unwrap();
        ops.stage(&["file.txt".to_string()]).unwrap();
        let commit = ops.commit("Test").unwrap();

        // Short hash is 7 characters
        assert_eq!(commit.id.len(), 7);
        // Full hash is 40 characters (SHA-1)
        assert_eq!(commit.full_id.len(), 40);
        // Short hash is prefix of full hash
        assert!(commit.full_id.starts_with(&commit.id));
    }

    #[test]
    fn user_sees_branch_name_in_status() {
        let (temp_dir, _repo) = setup_test_repo();
        let ops = GitOperations::new(temp_dir.path());

        // Create initial commit (needed for branch to exist)
        fs::write(temp_dir.path().join("file.txt"), "content").unwrap();
        ops.stage(&["file.txt".to_string()]).unwrap();
        ops.commit("Initial").unwrap();

        // Check status shows branch
        let status = ops.status().unwrap();
        // Default branch could be "master" or "main" depending on git config
        assert!(!status.branch.is_empty());
    }

    // === Additional git type tests ===

    #[test]
    fn git_file_status_debug() {
        let status = GitFileStatus {
            path: "test.txt".to_string(),
            status: GitFileState::Modified,
            staged: true,
        };
        let debug = format!("{:?}", status);
        assert!(debug.contains("GitFileStatus"));
        assert!(debug.contains("test.txt"));
    }

    #[test]
    fn git_file_status_clone() {
        let status = GitFileStatus {
            path: "file.rs".to_string(),
            status: GitFileState::Added,
            staged: false,
        };
        let cloned = status;
        assert_eq!(cloned.path, "file.rs");
        assert_eq!(cloned.status, GitFileState::Added);
        assert!(!cloned.staged);
    }

    #[test]
    fn git_file_status_serialize() {
        let status = GitFileStatus {
            path: "src/main.rs".to_string(),
            status: GitFileState::Deleted,
            staged: true,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"path\":\"src/main.rs\""));
        assert!(json.contains("\"status\":\"deleted\""));
        assert!(json.contains("\"staged\":true"));
    }

    #[test]
    fn git_file_status_deserialize() {
        let json = r#"{"path": "lib.rs", "status": "renamed", "staged": false}"#;
        let status: GitFileStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status.path, "lib.rs");
        assert_eq!(status.status, GitFileState::Renamed);
        assert!(!status.staged);
    }

    #[test]
    fn git_file_state_copy() {
        let state = GitFileState::Conflicted;
        let copied = state;
        assert_eq!(copied, GitFileState::Conflicted);
    }

    #[test]
    fn git_file_state_eq() {
        assert_eq!(GitFileState::Modified, GitFileState::Modified);
        assert_ne!(GitFileState::Modified, GitFileState::Added);
    }

    #[test]
    fn git_status_debug() {
        let status = GitStatus {
            branch: "main".to_string(),
            upstream: None,
            ahead: 0,
            behind: 0,
            files: vec![],
        };
        let debug = format!("{:?}", status);
        assert!(debug.contains("GitStatus"));
        assert!(debug.contains("main"));
    }

    #[test]
    fn git_status_clone() {
        let status = GitStatus {
            branch: "feature".to_string(),
            upstream: Some("origin/feature".to_string()),
            ahead: 2,
            behind: 1,
            files: vec![GitFileStatus {
                path: "a.txt".to_string(),
                status: GitFileState::Modified,
                staged: true,
            }],
        };
        let cloned = status;
        assert_eq!(cloned.branch, "feature");
        assert_eq!(cloned.ahead, 2);
        assert_eq!(cloned.files.len(), 1);
    }

    #[test]
    fn git_status_deserialize() {
        let json = r#"{
            "branch": "dev",
            "upstream": null,
            "ahead": 0,
            "behind": 0,
            "files": []
        }"#;
        let status: GitStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status.branch, "dev");
        assert!(status.upstream.is_none());
    }

    #[test]
    fn git_commit_info_debug() {
        let info = GitCommitInfo {
            id: "abc1234".to_string(),
            full_id: "abc1234567890".to_string(),
            message: "Test commit".to_string(),
            author: "Test".to_string(),
            email: "test@test.com".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        let debug = format!("{:?}", info);
        assert!(debug.contains("GitCommitInfo"));
    }

    #[test]
    fn git_commit_info_clone() {
        let info = GitCommitInfo {
            id: "xyz".to_string(),
            full_id: "xyz123".to_string(),
            message: "Msg".to_string(),
            author: "Auth".to_string(),
            email: "a@b.com".to_string(),
            timestamp: "2024".to_string(),
        };
        let cloned = info;
        assert_eq!(cloned.id, "xyz");
        assert_eq!(cloned.message, "Msg");
    }

    #[test]
    fn git_commit_info_deserialize() {
        let json = r#"{
            "id": "abc",
            "full_id": "abc123",
            "message": "Initial",
            "author": "Author",
            "email": "a@b.com",
            "timestamp": "2024-01-01"
        }"#;
        let info: GitCommitInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.id, "abc");
        assert_eq!(info.message, "Initial");
    }

    #[test]
    fn git_error_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let git_err = GitError::Io(io_err);
        let msg = git_err.to_string();
        assert!(msg.contains("IO error"));
    }

    #[test]
    fn git_error_path_display() {
        let err = GitError::Path("invalid path".to_string());
        assert_eq!(err.to_string(), "Path error: invalid path");
    }

    #[test]
    fn git_error_not_supported_display() {
        let err = GitError::NotSupported("feature".to_string());
        assert_eq!(err.to_string(), "Operation not supported: feature");
    }

    #[test]
    fn git_operations_new() {
        let ops = GitOperations::new(Path::new("/tmp"));
        // Should create successfully even if path isn't a git repo
        assert!(ops.working_dir.to_str().unwrap().contains("tmp"));
    }

    // === Additional integration-like tests ===

    #[test]
    fn user_sees_renamed_file_display() {
        assert_eq!(format!("{}", GitFileState::Renamed), "R");
    }

    #[test]
    fn user_sees_conflicted_file_display() {
        assert_eq!(format!("{}", GitFileState::Conflicted), "C");
    }

    #[test]
    fn user_sees_added_file_display() {
        assert_eq!(format!("{}", GitFileState::Added), "A");
    }

    #[test]
    fn git_file_state_untracked_serializes() {
        let json = serde_json::to_string(&GitFileState::Untracked).unwrap();
        assert_eq!(json, r#""untracked""#);
    }

    #[test]
    fn git_file_state_conflicted_serializes() {
        let json = serde_json::to_string(&GitFileState::Conflicted).unwrap();
        assert_eq!(json, r#""conflicted""#);
    }

    #[test]
    fn git_file_state_renamed_serializes() {
        let json = serde_json::to_string(&GitFileState::Renamed).unwrap();
        assert_eq!(json, r#""renamed""#);
    }
}
