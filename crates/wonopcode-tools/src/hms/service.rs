//! HMS Service - the main entry point for hierarchical memory operations.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use tokio::fs;
use tracing::debug;

use super::{
    AgentsGenerator, CachedResolver, HmsError, MemoryEntryData, MemoryFile, StorageClass,
};

/// The main HMS service that coordinates memory operations.
pub struct HmsService {
    project_root: PathBuf,
    generator: AgentsGenerator,
}

impl HmsService {
    /// Create a new HMS service for the given project root.
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            project_root: project_root.clone(),
            generator: AgentsGenerator::new(project_root),
        }
    }

    /// Get the project root.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Get the resolver for memory queries.
    pub fn resolver(&self) -> &CachedResolver {
        self.generator.resolver()
    }

    /// Set a memory entry.
    pub async fn set_memory(
        &self,
        target_dir: &Path,
        key: &str,
        data: MemoryEntryData,
        storage: StorageClass,
    ) -> Result<(), HmsError> {
        let path = storage.memory_path(target_dir);
        debug!(?path, key, "Setting memory");

        // Load existing file or create new
        let mut file = if path.exists() {
            let content = fs::read_to_string(&path).await?;
            MemoryFile::parse(&content)?
        } else {
            MemoryFile::new()
        };

        // Set the entry
        file.set(key, data);

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Write back
        let yaml = file.to_yaml()?;
        fs::write(&path, yaml).await?;

        // Invalidate cache
        self.generator.resolver().invalidate(target_dir);

        Ok(())
    }

    /// Delete a memory entry.
    ///
    /// If `storage` is None, searches all storage classes.
    /// Returns true if the key was found and deleted.
    pub async fn delete_memory(
        &self,
        target_dir: &Path,
        key: &str,
        storage: Option<&str>,
    ) -> Result<bool, HmsError> {
        let storage_classes = match storage {
            Some(s) => vec![StorageClass::from_str(s)?],
            None => StorageClass::all_in_priority_order().to_vec(),
        };

        let mut deleted = false;

        for storage_class in storage_classes {
            let path = storage_class.memory_path(target_dir);
            if !path.exists() {
                continue;
            }

            let content = fs::read_to_string(&path).await?;
            let mut file = MemoryFile::parse(&content)?;

            if file.remove(key).is_some() {
                deleted = true;

                // Write back or delete file if empty
                if file.is_empty() {
                    fs::remove_file(&path).await?;
                } else {
                    let yaml = file.to_yaml()?;
                    fs::write(&path, yaml).await?;
                }

                // Invalidate cache
                self.generator.resolver().invalidate(target_dir);

                // If specific storage was requested, stop here
                if storage.is_some() {
                    break;
                }
            }
        }

        Ok(deleted)
    }

    /// Generate AGENTS.md content for a directory.
    pub async fn generate(
        &mut self,
        target_dir: &Path,
    ) -> Result<String, HmsError> {
        self.generator.generate(target_dir, &self.project_root).await
    }

    /// Write AGENTS.md to disk.
    pub async fn write_agents_md(
        &mut self,
        target_dir: &Path,
    ) -> Result<PathBuf, HmsError> {
        self.generator
            .write_agents_md(target_dir, &self.project_root)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hms::Visibility;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_service_set_and_resolve() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let service = HmsService::new(root.to_path_buf());

        // Set a memory
        service
            .set_memory(
                root,
                "test_key",
                MemoryEntryData::new("test_value"),
                StorageClass::Tracked,
            )
            .await
            .unwrap();

        // Verify it exists
        let memories = service.resolver().resolve(root).await.unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].key, "test_key");
    }

    #[tokio::test]
    async fn test_service_delete() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let service = HmsService::new(root.to_path_buf());

        // Set a memory
        service
            .set_memory(
                root,
                "delete_me",
                MemoryEntryData::new("value"),
                StorageClass::Tracked,
            )
            .await
            .unwrap();

        // Delete it
        let deleted = service.delete_memory(root, "delete_me", None).await.unwrap();
        assert!(deleted);

        // Verify it's gone
        let memories = service.resolver().resolve(root).await.unwrap();
        assert!(memories.is_empty());
    }

    #[tokio::test]
    async fn test_service_delete_nonexistent() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let service = HmsService::new(root.to_path_buf());

        let deleted = service
            .delete_memory(root, "nonexistent", None)
            .await
            .unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_service_set_with_visibility() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let subdir = root.join("src");
        fs::create_dir_all(&subdir).await.unwrap();

        let service = HmsService::new(root.to_path_buf());

        // Set a private memory in subdir
        service
            .set_memory(
                &subdir,
                "private_key",
                MemoryEntryData::new("secret").with_visibility(Visibility::Private),
                StorageClass::Tracked,
            )
            .await
            .unwrap();

        // Should be visible in subdir
        let subdir_memories = service.resolver().resolve(&subdir).await.unwrap();
        assert_eq!(subdir_memories.len(), 1);

        // Should NOT be visible at root
        let root_memories = service.resolver().resolve(root).await.unwrap();
        assert!(root_memories.is_empty());
    }

    #[tokio::test]
    async fn test_service_local_storage() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let service = HmsService::new(root.to_path_buf());

        // Set in local storage
        service
            .set_memory(
                root,
                "local_key",
                MemoryEntryData::new("local_value"),
                StorageClass::Local,
            )
            .await
            .unwrap();

        // Verify file location
        assert!(root.join(".wonopcode-local/memory.yaml").exists());
    }
}
