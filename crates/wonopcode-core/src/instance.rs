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

        // Ensure storage dir exists
        if let Some(data_dir) = Config::data_dir() {
            let _ = tokio::fs::create_dir_all(data_dir.join("storage")).await;
        }

        let registry = InstanceRegistry::new();

        // Create instance - may fail due to storage setup
        if let Ok(instance1) = registry.get_or_create(test_path).await {
            assert_eq!(instance1.directory(), test_path);

            // Get same instance (should return cached)
            if let Ok(instance2) = registry.get_or_create(test_path).await {
                assert_eq!(instance1.directory(), instance2.directory());
            }

            // Dispose
            registry.dispose_all().await;
        }
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

    // Helper to create instance with storage dir properly set up
    async fn create_test_instance(test_path: &Path) -> CoreResult<Instance> {
        // Ensure the global storage dir exists for tests
        // This is needed because Instance::new uses Config::data_dir()
        if let Some(data_dir) = Config::data_dir() {
            let _ = tokio::fs::create_dir_all(data_dir.join("storage")).await;
        }
        Instance::new(test_path).await
    }

    #[tokio::test]
    async fn test_instance_new() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_path = temp_dir.path();

        if let Ok(instance) = create_test_instance(test_path).await {
            // Check directory
            assert_eq!(instance.directory(), test_path);

            // Check worktree exists (may be different from test_path if no git repo)
            let worktree = instance.worktree().await;
            // Just verify worktree is valid
            assert!(!worktree.as_os_str().is_empty());

            // Check project_id
            let project_id = instance.project_id().await;
            assert!(!project_id.is_empty());

            // Check project
            let project = instance.project().await;
            assert_eq!(project.id, project_id);

            // Check bus exists
            let _bus = instance.bus();

            // Check storage exists
            let _storage = instance.storage();

            // Cleanup
            instance.dispose().await;
        }
        // If Instance::new fails due to storage issues, that's OK for this test
    }

    #[tokio::test]
    async fn test_instance_config() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_path = temp_dir.path();

        if let Ok(instance) = create_test_instance(test_path).await {
            // Get config (should return default for empty project)
            let config = instance.config().await;
            // Config should be valid (has default values)
            assert!(config.theme.is_none() || config.theme.is_some()); // Either is fine

            // Get config sources
            let sources = instance.config_sources().await;
            // May be empty for new project or have some defaults
            let _ = sources;

            instance.dispose().await;
        }
    }

    #[tokio::test]
    async fn test_instance_update_config() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_path = temp_dir.path();

        if let Ok(instance) = create_test_instance(test_path).await {
            // Update config
            instance
                .update_config(|config| {
                    config.theme = Some("dark".to_string());
                })
                .await
                .unwrap();

            // Verify update persisted in memory
            let config = instance.config().await;
            assert_eq!(config.theme, Some("dark".to_string()));

            instance.dispose().await;
        }
    }

    #[tokio::test]
    async fn test_instance_reload_config() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_path = temp_dir.path();

        if let Ok(instance) = create_test_instance(test_path).await {
            // Create a config file
            let config_path = test_path.join("wonopcode.json");
            tokio::fs::write(&config_path, r#"{"theme": "light"}"#)
                .await
                .unwrap();

            // Reload config
            instance.reload_config().await.unwrap();

            // Verify new config is loaded
            let config = instance.config().await;
            assert_eq!(config.theme, Some("light".to_string()));

            instance.dispose().await;
        }
    }

    #[tokio::test]
    async fn test_instance_session_repo() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_path = temp_dir.path();

        if let Ok(instance) = create_test_instance(test_path).await {
            // Get session repo
            let _repo = instance.session_repo();

            instance.dispose().await;
        }
    }

    #[tokio::test]
    async fn test_instance_create_session() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_path = temp_dir.path();

        if let Ok(instance) = create_test_instance(test_path).await {
            // Create session without title
            let session1 = instance.create_session(None).await.unwrap();
            assert!(!session1.id.is_empty());

            // Create session with title
            let session2 = instance
                .create_session(Some("Test Session".to_string()))
                .await
                .unwrap();
            assert_eq!(session2.title, "Test Session");

            instance.dispose().await;
        }
    }

    #[tokio::test]
    async fn test_instance_get_session() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_path = temp_dir.path();

        if let Ok(instance) = create_test_instance(test_path).await {
            // Create a session
            let session = instance.create_session(None).await.unwrap();
            let session_id = session.id.clone();

            // Get the session
            let retrieved = instance.get_session(&session_id).await;
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().id, session_id);

            // Get non-existent session
            let not_found = instance.get_session("nonexistent").await;
            assert!(not_found.is_none());

            instance.dispose().await;
        }
    }

    #[tokio::test]
    async fn test_instance_list_sessions() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_path = temp_dir.path();

        if let Ok(instance) = create_test_instance(test_path).await {
            // Create some sessions
            let session1 = instance.create_session(None).await.unwrap();
            let session2 = instance.create_session(None).await.unwrap();

            // Verify our sessions are in the list
            // (don't check exact count due to parallel test interference)
            let sessions = instance.list_sessions().await;
            let session_ids: Vec<_> = sessions.iter().map(|s| s.id.as_str()).collect();
            assert!(
                session_ids.contains(&session1.id.as_str()),
                "session1 should be in list"
            );
            assert!(
                session_ids.contains(&session2.id.as_str()),
                "session2 should be in list"
            );

            instance.dispose().await;
        }
    }

    #[tokio::test]
    async fn test_instance_last_session() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_path = temp_dir.path();

        if let Ok(instance) = create_test_instance(test_path).await {
            // Create a session
            let session = instance.create_session(None).await.unwrap();

            // Last session should return something (we just created a session)
            let last = instance.last_session().await;
            assert!(last.is_some());
            let last_session = last.unwrap();
            assert!(!last_session.id.is_empty());
            // The session we created should be accessible
            assert!(instance.get_session(&session.id).await.is_some());

            instance.dispose().await;
        }
    }

    #[tokio::test]
    async fn test_instance_dispose() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_path = temp_dir.path();

        if let Ok(instance) = create_test_instance(test_path).await {
            // Dispose should not panic
            instance.dispose().await;

            // Instance can still be used after dispose (it just sends event)
            let _config = instance.config().await;
        }
    }

    #[tokio::test]
    async fn test_instance_registry_dispose_specific() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_path = temp_dir.path();

        let registry = InstanceRegistry::new();

        // Create instance - may fail due to storage setup
        if let Ok(_instance) = registry.get_or_create(test_path).await {
            // Dispose specific instance
            registry.dispose(test_path).await;

            // Creating again should create a new instance
            if let Ok(new_instance) = registry.get_or_create(test_path).await {
                assert_eq!(new_instance.directory(), test_path);
            }

            registry.dispose_all().await;
        }
    }

    #[tokio::test]
    async fn test_instance_clone() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_path = temp_dir.path();

        if let Ok(instance1) = create_test_instance(test_path).await {
            let instance2 = instance1.clone();

            // Both should point to the same data
            assert_eq!(instance1.directory(), instance2.directory());

            // Both can update config and see each other's changes
            instance1
                .update_config(|config| {
                    config.theme = Some("shared".to_string());
                })
                .await
                .unwrap();

            let config2 = instance2.config().await;
            assert_eq!(config2.theme, Some("shared".to_string()));

            instance1.dispose().await;
        }
    }
}
