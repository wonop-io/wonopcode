//! In-memory storage implementation for testing.

use crate::{Storage, StorageError, StorageResult};
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

/// In-memory storage for testing.
///
/// This stores all data in memory and is not persistent.
pub struct MemoryStorage {
    data: RwLock<HashMap<String, String>>,
}

impl MemoryStorage {
    /// Create a new in-memory storage.
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }

    /// Convert a key slice to a storage key string.
    fn key_to_string(key: &[&str]) -> String {
        key.join("/")
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn read<T: DeserializeOwned + Send>(&self, key: &[&str]) -> StorageResult<Option<T>> {
        let key_str = Self::key_to_string(key);
        let data = self
            .data
            .read()
            .map_err(|e| StorageError::LockPoisoned(e.to_string()))?;

        match data.get(&key_str) {
            Some(json) => {
                let value: T = serde_json::from_str(json)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    async fn write<T: Serialize + Send + Sync>(
        &self,
        key: &[&str],
        value: &T,
    ) -> StorageResult<()> {
        let key_str = Self::key_to_string(key);
        let json = serde_json::to_string(value)?;

        let mut data = self
            .data
            .write()
            .map_err(|e| StorageError::LockPoisoned(e.to_string()))?;
        data.insert(key_str, json);

        Ok(())
    }

    async fn update<T, F>(&self, key: &[&str], editor: F) -> StorageResult<T>
    where
        T: DeserializeOwned + Serialize + Send + Sync + Default,
        F: FnOnce(&mut T) + Send,
    {
        let mut value: T = self.read(key).await?.unwrap_or_default();
        editor(&mut value);
        self.write(key, &value).await?;
        Ok(value)
    }

    async fn remove(&self, key: &[&str]) -> StorageResult<()> {
        let key_str = Self::key_to_string(key);
        let mut data = self
            .data
            .write()
            .map_err(|e| StorageError::LockPoisoned(e.to_string()))?;
        data.remove(&key_str);
        Ok(())
    }

    async fn list(&self, prefix: &[&str]) -> StorageResult<Vec<Vec<String>>> {
        let prefix_str = Self::key_to_string(prefix);
        let prefix_with_sep = if prefix_str.is_empty() {
            String::new()
        } else {
            format!("{prefix_str}/")
        };

        let data = self
            .data
            .read()
            .map_err(|e| StorageError::LockPoisoned(e.to_string()))?;
        let results: Vec<Vec<String>> = data
            .keys()
            .filter(|k| {
                if prefix_str.is_empty() {
                    true
                } else {
                    k.starts_with(&prefix_with_sep) || *k == &prefix_str
                }
            })
            .filter_map(|k| {
                // Only include direct children (one level deep)
                let remainder = if prefix_str.is_empty() {
                    k.as_str()
                } else {
                    k.strip_prefix(&prefix_with_sep)?
                };

                // Skip if there are more path separators (not a direct child)
                if remainder.contains('/') {
                    return None;
                }

                let parts: Vec<String> = k.split('/').map(|s| s.to_string()).collect();
                Some(parts)
            })
            .collect();

        Ok(results)
    }

    async fn exists(&self, key: &[&str]) -> StorageResult<bool> {
        let key_str = Self::key_to_string(key);
        let data = self
            .data
            .read()
            .map_err(|e| StorageError::LockPoisoned(e.to_string()))?;
        Ok(data.contains_key(&key_str))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
    struct TestData {
        name: String,
        value: i32,
    }

    #[tokio::test]
    async fn test_memory_storage() {
        let storage = MemoryStorage::new();

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        // Write
        storage.write(&["test", "data"], &data).await.unwrap();

        // Read
        let read: Option<TestData> = storage.read(&["test", "data"]).await.unwrap();
        assert_eq!(read, Some(data.clone()));

        // Exists
        assert!(storage.exists(&["test", "data"]).await.unwrap());
        assert!(!storage.exists(&["nonexistent"]).await.unwrap());

        // Remove
        storage.remove(&["test", "data"]).await.unwrap();
        assert!(!storage.exists(&["test", "data"]).await.unwrap());
    }

    #[tokio::test]
    async fn test_memory_storage_list() {
        let storage = MemoryStorage::new();

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        storage.write(&["project", "item1"], &data).await.unwrap();
        storage.write(&["project", "item2"], &data).await.unwrap();
        storage.write(&["other", "item"], &data).await.unwrap();

        let items = storage.list(&["project"]).await.unwrap();
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn test_memory_storage_default() {
        let storage = MemoryStorage::default();
        let result: Option<TestData> = storage.read(&["test"]).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_memory_storage_update() {
        let storage = MemoryStorage::new();

        // Update creates default if not exists
        let result: TestData = storage
            .update(&["new", "item"], |data: &mut TestData| {
                data.name = "created".to_string();
                data.value = 100;
            })
            .await
            .unwrap();

        assert_eq!(result.name, "created");
        assert_eq!(result.value, 100);

        // Update modifies existing
        let result: TestData = storage
            .update(&["new", "item"], |data: &mut TestData| {
                data.value = 200;
            })
            .await
            .unwrap();

        assert_eq!(result.value, 200);
    }

    #[tokio::test]
    async fn test_memory_storage_list_empty_prefix() {
        let storage = MemoryStorage::new();

        let data = TestData::default();
        storage.write(&["item1"], &data).await.unwrap();
        storage.write(&["item2"], &data).await.unwrap();

        // List with empty prefix should return top-level items
        let items = storage.list(&[]).await.unwrap();
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn test_memory_storage_list_excludes_nested() {
        let storage = MemoryStorage::new();

        let data = TestData::default();
        storage.write(&["project", "item1"], &data).await.unwrap();
        storage
            .write(&["project", "nested", "item"], &data)
            .await
            .unwrap();

        // List should only include direct children
        let items = storage.list(&["project"]).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0], vec!["project", "item1"]);
    }

    #[tokio::test]
    async fn test_memory_storage_read_nonexistent() {
        let storage = MemoryStorage::new();
        let result: Option<TestData> = storage.read(&["does", "not", "exist"]).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_memory_storage_remove_nonexistent() {
        let storage = MemoryStorage::new();
        // Removing nonexistent key should not error
        storage.remove(&["does", "not", "exist"]).await.unwrap();
    }

    #[tokio::test]
    async fn test_memory_storage_overwrite() {
        let storage = MemoryStorage::new();

        let data1 = TestData {
            name: "first".to_string(),
            value: 1,
        };
        let data2 = TestData {
            name: "second".to_string(),
            value: 2,
        };

        storage.write(&["key"], &data1).await.unwrap();
        storage.write(&["key"], &data2).await.unwrap();

        let result: Option<TestData> = storage.read(&["key"]).await.unwrap();
        assert_eq!(result.unwrap().name, "second");
    }
}
