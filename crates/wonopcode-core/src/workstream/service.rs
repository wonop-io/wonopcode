//! WorkstreamService - unified management of passive and active workstreams.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::bus::BusEvent;
use crate::error::{CoreResult, WorkstreamError};
use crate::Instance;
use crate::SessionService;

use super::discovery::{create_worktree, delete_worktree, discover_worktrees, get_repo_root};
use super::types::{
    ActiveWorkstream, PassiveWorkstream, WorkstreamEvent, WorkstreamId, WorkstreamInfo,
};

/// Configuration for the WorkstreamService.
#[derive(Debug, Clone)]
pub struct WorkstreamServiceConfig {
    /// Provider for AI models (e.g., "anthropic").
    pub provider: String,
    /// Model ID for AI models (e.g., "claude-sonnet-4-5-20250929").
    pub model_id: String,
    /// Whether to auto-deactivate workstreams when last client disconnects.
    pub auto_deactivate: bool,
    /// Timeout before auto-deactivation (if enabled).
    pub auto_deactivate_timeout: std::time::Duration,
}

impl Default for WorkstreamServiceConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model_id: "claude-sonnet-4-5-20250929".to_string(),
            auto_deactivate: false,
            auto_deactivate_timeout: std::time::Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Manages passive and active workstreams for a repository.
///
/// The service discovers git worktrees as passive workstreams and can
/// activate them on-demand by creating Runner, SessionService, and Instance.
pub struct WorkstreamService {
    /// Repository root path.
    repo_root: PathBuf,
    /// Service configuration.
    config: WorkstreamServiceConfig,
    /// Discovered passive workstreams (branch -> PassiveWorkstream).
    passive_workstreams: Arc<RwLock<HashMap<WorkstreamId, PassiveWorkstream>>>,
    /// Currently active workstreams (branch -> ActiveWorkstream).
    active_workstreams: Arc<RwLock<HashMap<WorkstreamId, Arc<ActiveWorkstream>>>>,
    /// Event broadcaster.
    event_tx: broadcast::Sender<WorkstreamEvent>,
}

impl WorkstreamService {
    /// Create a new WorkstreamService for a repository.
    pub async fn new(repo_path: impl AsRef<Path>) -> CoreResult<Self> {
        Self::with_config(repo_path, WorkstreamServiceConfig::default()).await
    }

    /// Create a new WorkstreamService with custom configuration.
    pub async fn with_config(
        repo_path: impl AsRef<Path>,
        config: WorkstreamServiceConfig,
    ) -> CoreResult<Self> {
        let repo_root = get_repo_root(repo_path.as_ref()).await?;
        let (event_tx, _) = broadcast::channel(256);

        info!(repo_root = %repo_root.display(), "Creating WorkstreamService");

        let service = Self {
            repo_root,
            config,
            passive_workstreams: Arc::new(RwLock::new(HashMap::new())),
            active_workstreams: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
        };

        // Initial discovery
        service.refresh().await?;

        Ok(service)
    }

    /// Refresh the list of passive workstreams from git.
    pub async fn refresh(&self) -> CoreResult<()> {
        info!(repo_root = %self.repo_root.display(), "Refreshing workstream list");

        let worktrees = discover_worktrees(&self.repo_root).await?;
        let mut passive = self.passive_workstreams.write().await;

        // Clear and repopulate
        passive.clear();
        for wt in worktrees {
            let id = wt.id();
            debug!(id = %id, path = %wt.path.display(), is_direct = wt.is_direct, "Discovered worktree");
            passive.insert(id, wt);
        }

        let count = passive.len();
        drop(passive);

        // Emit refresh event
        let _ = self.event_tx.send(WorkstreamEvent::Refreshed { count });

        info!(count = count, "Workstream list refreshed");

        Ok(())
    }

    /// List all workstreams (passive and active).
    pub async fn list(&self) -> Vec<WorkstreamInfo> {
        let passive = self.passive_workstreams.read().await;
        let active = self.active_workstreams.read().await;

        let mut infos = Vec::with_capacity(passive.len());

        for (id, ws) in passive.iter() {
            if let Some(active_ws) = active.get(id) {
                // This workstream is active
                infos.push(active_ws.to_info().await);
            } else {
                // This workstream is passive
                infos.push(ws.to_info());
            }
        }

        // Sort by name
        infos.sort_by(|a, b| a.name.cmp(&b.name));

        infos
    }

    /// Get workstream info by ID.
    pub async fn get(&self, id: &WorkstreamId) -> Option<WorkstreamInfo> {
        // Check if active first
        if let Some(active) = self.active_workstreams.read().await.get(id) {
            return Some(active.to_info().await);
        }

        // Otherwise check passive
        self.passive_workstreams
            .read()
            .await
            .get(id)
            .map(|ws| ws.to_info())
    }

    /// Check if a workstream exists.
    pub async fn exists(&self, id: &WorkstreamId) -> bool {
        self.passive_workstreams.read().await.contains_key(id)
    }

    /// Check if a workstream is currently active.
    pub async fn is_active(&self, id: &WorkstreamId) -> bool {
        self.active_workstreams.read().await.contains_key(id)
    }

    /// Get an active workstream by ID.
    pub async fn get_active(&self, id: &WorkstreamId) -> Option<Arc<ActiveWorkstream>> {
        self.active_workstreams.read().await.get(id).cloned()
    }

    /// Get the direct workstream (main working tree).
    pub async fn direct(&self) -> Option<WorkstreamInfo> {
        let passive = self.passive_workstreams.read().await;
        for (id, ws) in passive.iter() {
            if ws.is_direct {
                // Check if it's active
                if let Some(active) = self.active_workstreams.read().await.get(id) {
                    return Some(active.to_info().await);
                }
                return Some(ws.to_info());
            }
        }
        None
    }

    /// Activate a passive workstream.
    ///
    /// Creates Runner, SessionService, and Instance for the workstream.
    /// If already active, returns the existing ActiveWorkstream.
    pub async fn activate(&self, id: &WorkstreamId) -> CoreResult<Arc<ActiveWorkstream>> {
        // Check if already active
        if let Some(active) = self.active_workstreams.read().await.get(id).cloned() {
            debug!(id = %id, "Workstream already active");
            return Ok(active);
        }

        // Get passive workstream
        let passive = self
            .passive_workstreams
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| WorkstreamError::NotFound(id.to_string()))?;

        info!(id = %id, path = %passive.path.display(), "Activating workstream");

        // Create Instance for the worktree path
        let instance = Instance::new(&passive.path)
            .await
            .map_err(|e| WorkstreamError::Git(format!("Failed to create Instance: {}", e)))?;

        // Create SessionService
        let session_service = Arc::new(
            SessionService::from_instance(&instance, &self.config.model_id, &self.config.provider)
                .await,
        );

        // Create broadcast channel for updates
        let (update_broadcast, _) = broadcast::channel::<BusEvent>(1000);

        // Create cancellation token
        let cancellation_token = CancellationToken::new();

        // Create ActiveWorkstream
        let active = Arc::new(ActiveWorkstream {
            passive,
            session_service,
            instance,
            connected_clients: Arc::new(RwLock::new(0)),
            update_broadcast,
            cancellation_token,
            activated_at: std::time::SystemTime::now(),
            last_activity: Arc::new(RwLock::new(std::time::SystemTime::now())),
        });

        // Store in active map
        self.active_workstreams
            .write()
            .await
            .insert(id.clone(), active.clone());

        // Emit activation event
        let _ = self.event_tx.send(WorkstreamEvent::Activated { id: id.clone() });

        info!(id = %id, "Workstream activated");

        Ok(active)
    }

    /// Deactivate an active workstream.
    ///
    /// Only succeeds if no clients are connected (unless force=true).
    pub async fn deactivate(&self, id: &WorkstreamId, force: bool) -> CoreResult<()> {
        let active = self
            .active_workstreams
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| WorkstreamError::NotActive(id.to_string()))?;

        let client_count = active.client_count().await;
        if client_count > 0 && !force {
            return Err(WorkstreamError::HasClients(id.to_string(), client_count).into());
        }

        info!(id = %id, "Deactivating workstream");

        // Shutdown the active workstream
        active.shutdown();

        // Remove from active map
        self.active_workstreams.write().await.remove(id);

        // Emit deactivation event
        let _ = self.event_tx.send(WorkstreamEvent::Deactivated { id: id.clone() });

        info!(id = %id, "Workstream deactivated");

        Ok(())
    }

    /// Activate the direct workstream (main working tree).
    pub async fn activate_direct(&self) -> CoreResult<Arc<ActiveWorkstream>> {
        // Find the direct workstream
        let direct_id = {
            let passive = self.passive_workstreams.read().await;
            passive
                .iter()
                .find(|(_, ws)| ws.is_direct)
                .map(|(id, _)| id.clone())
                .ok_or_else(|| WorkstreamError::NotFound("direct workstream".to_string()))?
        };

        self.activate(&direct_id).await
    }

    /// Create a new worktree (passive workstream).
    pub async fn create_worktree(
        &self,
        branch_name: &str,
        base_branch: &str,
    ) -> CoreResult<WorkstreamId> {
        info!(branch = %branch_name, base = %base_branch, "Creating new worktree");

        let passive = create_worktree(&self.repo_root, branch_name, base_branch, None).await?;
        let id = passive.id();
        let info = passive.to_info();

        // Add to passive map
        self.passive_workstreams
            .write()
            .await
            .insert(id.clone(), passive);

        // Emit event
        let _ = self.event_tx.send(WorkstreamEvent::WorktreeCreated { info });

        Ok(id)
    }

    /// Delete a worktree (must be passive, not active).
    pub async fn delete_worktree(&self, id: &WorkstreamId) -> CoreResult<()> {
        // Check it's not active
        if self.is_active(id).await {
            return Err(WorkstreamError::NotActive(format!(
                "Cannot delete active workstream {}. Deactivate first.",
                id
            ))
            .into());
        }

        // Get passive workstream
        let passive = self
            .passive_workstreams
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| WorkstreamError::NotFound(id.to_string()))?;

        // Don't allow deleting the direct workstream
        if passive.is_direct {
            return Err(WorkstreamError::CannotDeleteDirect.into());
        }

        info!(id = %id, path = %passive.path.display(), "Deleting worktree");

        // Delete the worktree
        delete_worktree(&self.repo_root, &passive.path).await?;

        // Remove from passive map
        self.passive_workstreams.write().await.remove(id);

        // Emit event
        let _ = self.event_tx.send(WorkstreamEvent::WorktreeDeleted { id: id.clone() });

        Ok(())
    }

    /// Connect a client to a workstream.
    ///
    /// Activates the workstream if it's passive.
    pub async fn connect_client(&self, id: &WorkstreamId) -> CoreResult<Arc<ActiveWorkstream>> {
        // Ensure workstream is active
        let active = self.activate(id).await?;

        // Add client
        let client_count = active.add_client().await;

        // Emit event
        let _ = self.event_tx.send(WorkstreamEvent::ClientConnected {
            id: id.clone(),
            client_count,
        });

        Ok(active)
    }

    /// Disconnect a client from a workstream.
    pub async fn disconnect_client(&self, id: &WorkstreamId) -> CoreResult<()> {
        let active = self
            .get_active(id)
            .await
            .ok_or_else(|| WorkstreamError::NotActive(id.to_string()))?;

        let client_count = active.remove_client().await;

        // Emit event
        let _ = self.event_tx.send(WorkstreamEvent::ClientDisconnected {
            id: id.clone(),
            client_count,
        });

        // Auto-deactivate if configured and no clients
        if self.config.auto_deactivate && client_count == 0 {
            // TODO: Implement timeout-based deactivation
            // For now, deactivate immediately
            if let Err(e) = self.deactivate(id, false).await {
                warn!(id = %id, error = %e, "Failed to auto-deactivate workstream");
            }
        }

        Ok(())
    }

    /// Subscribe to workstream events.
    pub fn subscribe_events(&self) -> broadcast::Receiver<WorkstreamEvent> {
        self.event_tx.subscribe()
    }

    /// Get the repository root path.
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// Get the service configuration.
    pub fn config(&self) -> &WorkstreamServiceConfig {
        &self.config
    }

    /// Shutdown all active workstreams.
    pub async fn shutdown(&self) {
        info!("Shutting down all active workstreams");

        let active_ids: Vec<WorkstreamId> = self
            .active_workstreams
            .read()
            .await
            .keys()
            .cloned()
            .collect();

        for id in active_ids {
            if let Err(e) = self.deactivate(&id, true).await {
                error!(id = %id, error = %e, "Failed to deactivate workstream during shutdown");
            }
        }
    }
}
