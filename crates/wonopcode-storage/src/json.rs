//! JSON file-based storage implementation.
//!
//! This storage backend stores each key as a separate JSON file.
//! Keys are mapped to file paths: `["session", "proj_123", "ses_456"]` -> `session/proj_123/ses_456.json`

use crate::{Storage, StorageError, StorageResult};
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::debug;

/// JSON file-based storage.
#[derive(Clone)]
pub struct JsonStorage {
    base_path: PathBuf,
}

impl JsonStorage {
    /// Create a new JSON storage at the given base path.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    /// Get the file path for a key.
    fn key_to_path(&self, key: &[&str]) -> StorageResult<PathBuf> {
        if key.is_empty() {
            return Err(StorageError::invalid_key("Key cannot be empty"));
        }

        // Validate key components (no path traversal)
        for component in key {
            if component.is_empty()
                || component.contains('/')
                || component.contains('\\')
                || *component == "."
                || *component == ".."
            {
                return Err(StorageError::invalid_key(format!(
                    "Invalid key component: {}",
                    component
                )));
            }
        }

        let mut path = self.base_path.clone();
        for component in key {
            path.push(component);
        }
        path.set_extension("json");

        Ok(path)
    }

    /// Get the directory path for a prefix.
    fn prefix_to_dir(&self, prefix: &[&str]) -> PathBuf {
        let mut path = self.base_path.clone();
        for component in prefix {
            path.push(component);
        }
        path
    }
}

#[async_trait]
impl Storage for JsonStorage {
    async fn read<T: DeserializeOwned + Send>(&self, key: &[&str]) -> StorageResult<Option<T>> {
        let path = self.key_to_path(key)?;
        debug!(path = %path.display(), "Reading from storage");

        match fs::read_to_string(&path).await {
            Ok(content) => {
                let value: T = serde_json::from_str(&content)?;
                Ok(Some(value))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    async fn write<T: Serialize + Send + Sync>(
        &self,
        key: &[&str],
        value: &T,
    ) -> StorageResult<()> {
        let path = self.key_to_path(key)?;
        debug!(path = %path.display(), "Writing to storage");

        // Create parent directories
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Serialize to JSON
        let content = serde_json::to_string_pretty(value)?;

        // Write atomically (write to temp file, then rename)
        let temp_path = path.with_extension("json.tmp");
        fs::write(&temp_path, &content).await?;
        fs::rename(&temp_path, &path).await?;

        Ok(())
    }

    async fn update<T, F>(&self, key: &[&str], editor: F) -> StorageResult<T>
    where
        T: DeserializeOwned + Serialize + Send + Sync + Default,
        F: FnOnce(&mut T) + Send,
    {
        // Read current value
        let mut value: T = self.read(key).await?.unwrap_or_default();

        // Apply edit
        editor(&mut value);

        // Write back
        self.write(key, &value).await?;

        Ok(value)
    }

    async fn remove(&self, key: &[&str]) -> StorageResult<()> {
        let path = self.key_to_path(key)?;
        debug!(path = %path.display(), "Removing from storage");

        match fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    async fn list(&self, prefix: &[&str]) -> StorageResult<Vec<Vec<String>>> {
        let dir = self.prefix_to_dir(prefix);
        debug!(path = %dir.display(), "Listing storage");

        let mut results = Vec::new();

        match fs::read_dir(&dir).await {
            Ok(mut entries) => {
                while let Some(entry) = entries.next_entry().await? {
                    let path = entry.path();

                    // Only include .json files
                    if path.extension().is_some_and(|ext| ext == "json") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            let mut key: Vec<String> =
                                prefix.iter().map(|s| s.to_string()).collect();
                            key.push(stem.to_string());
                            results.push(key);
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Directory doesn't exist, return empty list
            }
            Err(e) => return Err(StorageError::Io(e)),
        }

        Ok(results)
    }

    async fn exists(&self, key: &[&str]) -> StorageResult<bool> {
        let path = self.key_to_path(key)?;
        Ok(path.exists())
    }
}

/// Create a storage instance at the default data directory.
pub fn default_storage() -> Option<JsonStorage> {
    wonopcode_util::path::data_dir().map(|p| JsonStorage::new(p.join("data")))
}

/// Create a storage instance at a project-specific directory.
pub fn project_storage(project_root: &Path) -> JsonStorage {
    JsonStorage::new(project_root.join(".wonopcode").join("data"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
    struct TestData {
        name: String,
        value: i32,
    }

    #[tokio::test]
    async fn test_write_and_read() {
        let dir = tempdir().unwrap();
        let storage = JsonStorage::new(dir.path());

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        storage.write(&["test", "data"], &data).await.unwrap();

        let read: Option<TestData> = storage.read(&["test", "data"]).await.unwrap();
        assert_eq!(read, Some(data));
    }

    #[tokio::test]
    async fn test_read_not_found() {
        let dir = tempdir().unwrap();
        let storage = JsonStorage::new(dir.path());

        let read: Option<TestData> = storage.read(&["nonexistent"]).await.unwrap();
        assert_eq!(read, None);
    }

    #[tokio::test]
    async fn test_update() {
        let dir = tempdir().unwrap();
        let storage = JsonStorage::new(dir.path());

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        storage.write(&["test", "data"], &data).await.unwrap();

        let updated: TestData = storage
            .update(&["test", "data"], |d: &mut TestData| {
                d.value = 100;
            })
            .await
            .unwrap();

        assert_eq!(updated.value, 100);

        let read: Option<TestData> = storage.read(&["test", "data"]).await.unwrap();
        assert_eq!(read.unwrap().value, 100);
    }

    #[tokio::test]
    async fn test_remove() {
        let dir = tempdir().unwrap();
        let storage = JsonStorage::new(dir.path());

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        storage.write(&["test", "data"], &data).await.unwrap();
        assert!(storage.exists(&["test", "data"]).await.unwrap());

        storage.remove(&["test", "data"]).await.unwrap();
        assert!(!storage.exists(&["test", "data"]).await.unwrap());
    }

    #[tokio::test]
    async fn test_list() {
        let dir = tempdir().unwrap();
        let storage = JsonStorage::new(dir.path());

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        storage.write(&["project", "item1"], &data).await.unwrap();
        storage.write(&["project", "item2"], &data).await.unwrap();
        storage.write(&["project", "item3"], &data).await.unwrap();

        let items = storage.list(&["project"]).await.unwrap();
        assert_eq!(items.len(), 3);
    }

    #[tokio::test]
    async fn test_invalid_key() {
        let dir = tempdir().unwrap();
        let storage = JsonStorage::new(dir.path());

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        // Empty key
        assert!(storage.write(&[], &data).await.is_err());

        // Path traversal attempt
        assert!(storage
            .write(&["..", "etc", "passwd"], &data)
            .await
            .is_err());

        // Slash in component
        assert!(storage.write(&["path/traversal"], &data).await.is_err());
    }
}
