//! Core types for workstream management.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;

use crate::Instance;
use crate::SessionService;

/// Unique identifier for a workstream, derived from the branch name.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkstreamId(pub String);

impl WorkstreamId {
    /// Create a new workstream ID from a branch name.
    pub fn from_branch(branch: &str) -> Self {
        Self(branch.to_string())
    }

    /// Get the display name (strips common prefixes like feature/, bugfix/).
    pub fn display_name(&self) -> &str {
        self.0
            .strip_prefix("feature/")
            .or_else(|| self.0.strip_prefix("bugfix/"))
            .or_else(|| self.0.strip_prefix("hotfix/"))
            .or_else(|| self.0.strip_prefix("refs/heads/"))
            .unwrap_or(&self.0)
    }

    /// Get the raw branch name.
    pub fn branch(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WorkstreamId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

impl From<String> for WorkstreamId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for WorkstreamId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl AsRef<str> for WorkstreamId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A passive workstream - just a git worktree reference.
///
/// This represents a discovered git worktree without any running processes.
/// It can be activated on-demand to become an `ActiveWorkstream`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassiveWorkstream {
    /// The branch name (canonical identifier).
    pub branch: String,
    /// Path to the worktree directory.
    pub path: PathBuf,
    /// Whether this is the main working tree (direct workstream).
    pub is_direct: bool,
    /// Repository root (where .git lives, for secondary worktrees this is the main repo).
    pub repo_root: PathBuf,
    /// When this worktree was discovered/created.
    #[serde(default = "default_system_time")]
    pub discovered_at: std::time::SystemTime,
}

fn default_system_time() -> std::time::SystemTime {
    std::time::SystemTime::now()
}

impl PassiveWorkstream {
    /// Get the workstream ID for this passive workstream.
    pub fn id(&self) -> WorkstreamId {
        WorkstreamId::from_branch(&self.branch)
    }

    /// Convert to WorkstreamInfo.
    pub fn to_info(&self) -> WorkstreamInfo {
        WorkstreamInfo {
            id: self.id(),
            name: self.branch.clone(),
            description: None,
            worktree_path: self.path.clone(),
            status: WorkstreamStatus::Passive,
            is_active: false,
            is_direct: self.is_direct,
            created_at: self.discovered_at,
            last_activity: self.discovered_at,
            connected_clients: 0,
        }
    }
}

/// An active workstream - passive workstream with runtime components.
///
/// This includes a Runner, SessionService, and Instance for AI interactions.
/// Multiple clients can connect to the same active workstream.
pub struct ActiveWorkstream {
    /// The underlying passive workstream.
    pub passive: PassiveWorkstream,
    /// Session service for history persistence.
    pub session_service: Arc<SessionService>,
    /// Instance for project context.
    pub instance: Instance,
    /// Connected client count.
    pub connected_clients: Arc<RwLock<u32>>,
    /// Broadcast channel for updates to clients.
    pub update_broadcast: broadcast::Sender<crate::bus::BusEvent>,
    /// Cancellation token for cleanup.
    pub cancellation_token: CancellationToken,
    /// When this workstream was activated.
    pub activated_at: std::time::SystemTime,
    /// Last activity timestamp.
    pub last_activity: Arc<RwLock<std::time::SystemTime>>,
}

impl ActiveWorkstream {
    /// Get the workstream ID.
    pub fn id(&self) -> WorkstreamId {
        self.passive.id()
    }

    /// Add a client connection.
    pub async fn add_client(&self) -> u32 {
        let mut count = self.connected_clients.write().await;
        *count += 1;
        *self.last_activity.write().await = std::time::SystemTime::now();
        *count
    }

    /// Remove a client connection.
    pub async fn remove_client(&self) -> u32 {
        let mut count = self.connected_clients.write().await;
        if *count > 0 {
            *count -= 1;
        }
        *self.last_activity.write().await = std::time::SystemTime::now();
        *count
    }

    /// Get current client count.
    pub async fn client_count(&self) -> u32 {
        *self.connected_clients.read().await
    }

    /// Subscribe to updates from this workstream.
    pub fn subscribe_updates(&self) -> broadcast::Receiver<crate::bus::BusEvent> {
        self.update_broadcast.subscribe()
    }

    /// Convert to WorkstreamInfo.
    pub async fn to_info(&self) -> WorkstreamInfo {
        let client_count = *self.connected_clients.read().await;
        let last_activity = *self.last_activity.read().await;

        WorkstreamInfo {
            id: self.id(),
            name: self.passive.branch.clone(),
            description: None,
            worktree_path: self.passive.path.clone(),
            status: WorkstreamStatus::Active,
            is_active: true,
            is_direct: self.passive.is_direct,
            created_at: self.passive.discovered_at,
            last_activity,
            connected_clients: client_count,
        }
    }

    /// Shutdown the active workstream.
    pub fn shutdown(&self) {
        self.cancellation_token.cancel();
    }
}

/// Status of a workstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkstreamStatus {
    /// Passive - just a worktree, no running processes.
    Passive,
    /// Active - has Runner, SessionService, etc.
    Active,
    /// Activating - in the process of spinning up.
    Activating,
    /// Deactivating - in the process of shutting down.
    Deactivating,
    /// Error state.
    Error,
}

impl std::fmt::Display for WorkstreamStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Passive => write!(f, "Passive"),
            Self::Active => write!(f, "Active"),
            Self::Activating => write!(f, "Activating"),
            Self::Deactivating => write!(f, "Deactivating"),
            Self::Error => write!(f, "Error"),
        }
    }
}

/// Information about a workstream (for API responses).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkstreamInfo {
    /// Unique identifier (branch name).
    pub id: WorkstreamId,
    /// Display name (may be same as branch).
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Path to the worktree directory.
    pub worktree_path: PathBuf,
    /// Current status.
    pub status: WorkstreamStatus,
    /// Whether this workstream is currently active.
    pub is_active: bool,
    /// Whether this is the main working tree (direct workstream).
    pub is_direct: bool,
    /// When the worktree was created/discovered.
    pub created_at: std::time::SystemTime,
    /// Last activity timestamp.
    pub last_activity: std::time::SystemTime,
    /// Number of connected clients (0 if passive).
    pub connected_clients: u32,
}

/// Events emitted by the WorkstreamService.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkstreamEvent {
    /// A new passive workstream was discovered.
    Discovered { info: WorkstreamInfo },
    /// A workstream was activated.
    Activated { id: WorkstreamId },
    /// A workstream was deactivated.
    Deactivated { id: WorkstreamId },
    /// A new worktree was created.
    WorktreeCreated { info: WorkstreamInfo },
    /// A worktree was deleted.
    WorktreeDeleted { id: WorkstreamId },
    /// A client connected to a workstream.
    ClientConnected { id: WorkstreamId, client_count: u32 },
    /// A client disconnected from a workstream.
    ClientDisconnected { id: WorkstreamId, client_count: u32 },
    /// Workstream list was refreshed.
    Refreshed { count: usize },
}
