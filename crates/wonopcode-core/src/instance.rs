//! Instance/project state management.
//!
//! An instance represents a wonopcode session in a specific project directory.
//! Each instance has its own isolated state including configuration, event bus,
//! and sessions.
//!
//! # Example
//!
//! ```ignore
//! use wonopcode_core::Instance;
//!
//! // Create an instance for a project
//! let instance = Instance::new("/path/to/project").await?;
//!
//! // Access instance state
//! let config = instance.config();
//! let bus = instance.bus();
//!
//! // Clean up when done
//! instance.dispose().await;
//! ```

use crate::bus::Bus;
use crate::config::Config;
use crate::error::CoreResult;
use crate::project::Project;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use wonopcode_storage::json::JsonStorage;

/// An instance managing state for a project directory.
#[derive(Clone)]
pub struct Instance {
    inner: Arc<InstanceInner>,
}

struct InstanceInner {
    /// Working directory.
    directory: PathBuf,

    /// Project information.
    project: RwLock<Project>,

    /// Configuration.
    config: RwLock<Config>,

    /// Event bus.
    bus: Bus,

    /// Storage backend.
    storage: JsonStorage,

    /// Config file sources.
    config_sources: RwLock<Vec<PathBuf>>,
}

impl Instance {
    /// Create a new instance for a directory.
    pub async fn new(directory: impl AsRef<Path>) -> CoreResult<Self> {
        let directory = directory.as_ref().to_path_buf();

        // Run project discovery and config loading in parallel
        let (project_result, config_result) = tokio::join!(
            Project::from_directory(&directory),
            Config::load(Some(&directory))
        );

        let project = project_result?;
        let (config, config_sources) = config_result?;

        // Create storage
        let storage_dir = Config::data_dir()
            .unwrap_or_else(|| PathBuf::from(".wonopcode"))
            .join("storage");
        let storage = JsonStorage::new(storage_dir);

        // Save/update project in storage
        let project = if let Some(existing) = Project::load(&storage, &project.id).await? {
            let mut existing = existing;
            existing.touch();
            existing.worktree = project.worktree;
            existing
        } else {
            project
        };
        project.save(&storage).await?;

        Ok(Self {
            inner: Arc::new(InstanceInner {
                directory,
                project: RwLock::new(project),
                config: RwLock::new(config),
                bus: Bus::new(),
                storage,
                config_sources: RwLock::new(config_sources),
            }),
        })
    }

    /// Get the working directory.
    pub fn directory(&self) -> &Path {
        &self.inner.directory
    }

    /// Get the project worktree root.
    pub async fn worktree(&self) -> PathBuf {
        self.inner.project.read().await.worktree.clone()
    }

    /// Get the project ID.
    pub async fn project_id(&self) -> String {
        self.inner.project.read().await.id.clone()
    }

    /// Get a copy of the project info.
    pub async fn project(&self) -> Project {
        self.inner.project.read().await.clone()
    }

    /// Get a copy of the configuration.
    pub async fn config(&self) -> Config {
        self.inner.config.read().await.clone()
    }

    /// Update the configuration.
    pub async fn update_config<F>(&self, f: F) -> CoreResult<()>
    where
        F: FnOnce(&mut Config),
    {
        let mut config = self.inner.config.write().await;
        f(&mut config);

        // Persist config changes to the project directory
        if let Err(e) = config.save_partial(Some(&self.inner.directory)).await {
            tracing::warn!("Failed to persist config changes: {}", e);
            // Don't fail the update, just log the error
        }

        Ok(())
    }

    /// Get the event bus.
    pub fn bus(&self) -> &Bus {
        &self.inner.bus
    }

    /// Get the storage backend.
    pub fn storage(&self) -> &JsonStorage {
        &self.inner.storage
    }

    /// Get config file sources.
    pub async fn config_sources(&self) -> Vec<PathBuf> {
        self.inner.config_sources.read().await.clone()
    }

    /// Reload configuration from disk.
    pub async fn reload_config(&self) -> CoreResult<()> {
        let (config, sources) = Config::load(Some(&self.inner.directory)).await?;
        *self.inner.config.write().await = config;
        *self.inner.config_sources.write().await = sources;
        Ok(())
    }

    /// Dispose the instance and clean up resources.
    pub async fn dispose(&self) {
        // Publish disposal event
        self.inner
            .bus
            .publish(crate::bus::InstanceDisposed {
                directory: self.inner.directory.display().to_string(),
            })
            .await;
    }

    /// Create a session repository.
    pub fn session_repo(&self) -> crate::session::SessionRepository {
        crate::session::SessionRepository::new(self.inner.storage.clone(), self.inner.bus.clone())
    }

    /// Create a new session.
    pub async fn create_session(
        &self,
        title: Option<String>,
    ) -> crate::error::CoreResult<crate::session::Session> {
        let project_id = self.project_id().await;
        let directory = self.directory().display().to_string();
        let mut session = crate::session::Session::new(&project_id, &directory);
        if let Some(t) = title {
            session.title = t;
        }
        self.session_repo().create(session).await
    }

    /// Get a session by ID.
    pub async fn get_session(&self, session_id: &str) -> Option<crate::session::Session> {
        let project_id = self.project_id().await;
        self.session_repo().get(&project_id, session_id).await.ok()
    }

    /// List sessions for the current project.
    pub async fn list_sessions(&self) -> Vec<crate::session::Session> {
        let project_id = self.project_id().await;
        self.session_repo()
            .list(&project_id)
            .await
            .unwrap_or_default()
    }

    /// Get the most recent session.
    pub async fn last_session(&self) -> Option<crate::session::Session> {
        let sessions = self.list_sessions().await;
        sessions.into_iter().next() // Sessions are sorted by descending ID
    }
}

/// Global instance registry for managing multiple instances.
pub struct InstanceRegistry {
    instances: RwLock<std::collections::HashMap<PathBuf, Instance>>,
}

impl InstanceRegistry {
    /// Create a new registry.
    pub fn new() -> Self {
        Self {
            instances: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Get or create an instance for a directory.
    pub async fn get_or_create(&self, directory: impl AsRef<Path>) -> CoreResult<Instance> {
        let directory = directory.as_ref().to_path_buf();

        // Check if instance exists
        {
            let instances = self.instances.read().await;
            if let Some(instance) = instances.get(&directory) {
                return Ok(instance.clone());
            }
        }

        // Create new instance
        let instance = Instance::new(&directory).await?;

        // Store in registry
        {
            let mut instances = self.instances.write().await;
            instances.insert(directory, instance.clone());
        }

        Ok(instance)
    }

    /// Dispose an instance.
    pub async fn dispose(&self, directory: impl AsRef<Path>) {
        let directory = directory.as_ref().to_path_buf();
        let mut instances = self.instances.write().await;
        if let Some(instance) = instances.remove(&directory) {
            instance.dispose().await;
        }
    }

    /// Dispose all instances.
    pub async fn dispose_all(&self) {
        let mut instances = self.instances.write().await;
        for (_, instance) in instances.drain() {
            instance.dispose().await;
        }
    }
}

impl Default for InstanceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Most Instance tests require a full storage infrastructure to be set up.
    // The test_instance_registry test works because it uses the default Config::data_dir()
    // which creates storage in a consistent location. For unit tests that need to test
    // Instance methods in isolation, we'd need to mock the storage layer.

    #[tokio::test]
    async fn test_instance_registry() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_path = temp_dir.path();

        let registry = InstanceRegistry::new();

        // Create instance
        let instance1 = registry.get_or_create(test_path).await.unwrap();
        assert_eq!(instance1.directory(), test_path);

        // Get same instance (should return cached)
        let instance2 = registry.get_or_create(test_path).await.unwrap();
        assert_eq!(instance1.directory(), instance2.directory());

        // Dispose
        registry.dispose_all().await;
    }

    #[test]
    fn test_instance_registry_new() {
        let registry = InstanceRegistry::new();
        // Just verify creation doesn't panic
        let _ = registry;
    }

    #[test]
    fn test_instance_registry_default() {
        let registry = InstanceRegistry::default();
        // Just verify default creation doesn't panic
        let _ = registry;
    }

    #[tokio::test]
    async fn test_instance_registry_dispose_nonexistent() {
        let registry = InstanceRegistry::new();
        // Disposing a non-existent path should not panic
        registry.dispose("/nonexistent/path").await;
    }

    #[tokio::test]
    async fn test_instance_registry_dispose_all_empty() {
        let registry = InstanceRegistry::new();
        // Disposing all when empty should not panic
        registry.dispose_all().await;
    }
}
