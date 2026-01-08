//! Project identification and metadata.
//!
//! A project is identified by its git root (first commit hash) or "global"
//! for non-git directories. Project metadata is stored in the data directory.

use crate::error::CoreResult;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use wonopcode_storage::json::JsonStorage;
use wonopcode_storage::Storage;

/// Project information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Unique project ID (git root commit hash or "global").
    pub id: String,

    /// Git worktree root path.
    pub worktree: PathBuf,

    /// Version control system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vcs: Option<Vcs>,

    /// Project display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Project icon.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<ProjectIcon>,

    /// Timestamps.
    pub time: ProjectTime,
}

/// Version control system type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vcs {
    Git,
}

/// Project icon configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectIcon {
    /// Icon URL or data URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Icon color (hex).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Project timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTime {
    /// When the project was first seen.
    pub created: i64,

    /// When the project was last accessed.
    pub updated: i64,

    /// When the project was initialized (first session).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialized: Option<i64>,
}

impl Project {
    /// Discover project from a directory.
    ///
    /// Walks up from the directory looking for a git repository.
    /// If found, uses the first commit hash as the project ID.
    /// Otherwise, returns a "global" project.
    pub async fn from_directory(directory: &Path) -> CoreResult<Self> {
        // Try to find git repository
        if let Some((worktree, id)) = Self::find_git_project(directory).await? {
            let now = chrono::Utc::now().timestamp_millis();

            return Ok(Project {
                id,
                worktree,
                vcs: Some(Vcs::Git),
                name: None,
                icon: None,
                time: ProjectTime {
                    created: now,
                    updated: now,
                    initialized: None,
                },
            });
        }

        // No git repository - use global project
        let now = chrono::Utc::now().timestamp_millis();
        Ok(Project {
            id: "global".to_string(),
            worktree: PathBuf::from("/"),
            vcs: None,
            name: None,
            icon: None,
            time: ProjectTime {
                created: now,
                updated: now,
                initialized: None,
            },
        })
    }

    /// Find git project info.
    async fn find_git_project(directory: &Path) -> CoreResult<Option<(PathBuf, String)>> {
        // Walk up looking for .git directory
        let mut current = directory.to_path_buf();
        let git_root = loop {
            let git_dir = current.join(".git");
            if git_dir.exists() {
                break current;
            }
            if !current.pop() {
                return Ok(None);
            }
        };

        // Check for cached project ID FIRST (before any git commands)
        // This avoids spawning git subprocesses if we have a cached ID
        let cache_file = git_root.join(".git/wonopcode");
        if let Ok(cached_id) = tokio::fs::read_to_string(&cache_file).await {
            let id = cached_id.trim().to_string();
            if !id.is_empty() {
                // We found a cached ID - use git_root as worktree
                // (This is correct for simple repos; for worktrees, git rev-parse would differ)
                return Ok(Some((git_root, id)));
            }
        }

        // No cache found - need to run git commands
        // Get worktree root (handles git worktrees correctly)
        let output = tokio::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(directory)
            .output()
            .await?;

        if !output.status.success() {
            return Ok(None);
        }

        let worktree = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());

        // Get first commit hash as project ID
        let output = tokio::process::Command::new("git")
            .args(["rev-list", "--max-parents=0", "--all"])
            .current_dir(&worktree)
            .output()
            .await?;

        if !output.status.success() {
            return Ok(None);
        }

        let commits = String::from_utf8_lossy(&output.stdout);
        let id = commits
            .lines()
            .next()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "global".to_string());

        // Cache the project ID for future startup speedup
        if !id.is_empty() && id != "global" {
            let _ = tokio::fs::write(&cache_file, &id).await;
        }

        Ok(Some((worktree, id)))
    }

    /// Load project from storage.
    pub async fn load(storage: &JsonStorage, id: &str) -> CoreResult<Option<Self>> {
        let key = ["project", id];
        Ok(storage.read(&key).await?)
    }

    /// Save project to storage.
    pub async fn save(&self, storage: &JsonStorage) -> CoreResult<()> {
        let key = ["project", &self.id];
        storage.write(&key, self).await?;
        Ok(())
    }

    /// Update the last accessed time.
    pub fn touch(&mut self) {
        self.time.updated = chrono::Utc::now().timestamp_millis();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_serialization() {
        let project = Project {
            id: "abc123".to_string(),
            worktree: PathBuf::from("/home/user/project"),
            vcs: Some(Vcs::Git),
            name: Some("My Project".to_string()),
            icon: None,
            time: ProjectTime {
                created: 1234567890,
                updated: 1234567890,
                initialized: None,
            },
        };

        let json = serde_json::to_string(&project).unwrap();
        let parsed: Project = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, "abc123");
        assert_eq!(parsed.vcs, Some(Vcs::Git));
    }
}
