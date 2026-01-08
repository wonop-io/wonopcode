//! Storage layer for wonopcode.
//!
//! This crate provides a key-value storage abstraction with multiple backends:
//! - JSON file storage (default)
//! - In-memory storage (for testing)

pub mod error;
pub mod json;
pub mod memory;

pub use error::{StorageError, StorageResult};

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

/// A trait for key-value storage backends.
///
/// Keys are represented as path segments, e.g., `["session", "project_id", "session_id"]`.
/// Values are serialized/deserialized as JSON.
#[async_trait]
pub trait Storage: Send + Sync {
    /// Read a value from storage.
    ///
    /// Returns `None` if the key doesn't exist.
    async fn read<T: DeserializeOwned + Send>(&self, key: &[&str]) -> StorageResult<Option<T>>;

    /// Write a value to storage.
    ///
    /// Creates parent directories if necessary.
    async fn write<T: Serialize + Send + Sync>(&self, key: &[&str], value: &T)
        -> StorageResult<()>;

    /// Update a value in storage atomically.
    ///
    /// The editor function is called with the current value (or default if not exists).
    /// The updated value is written back to storage.
    async fn update<T, F>(&self, key: &[&str], editor: F) -> StorageResult<T>
    where
        T: DeserializeOwned + Serialize + Send + Sync + Default,
        F: FnOnce(&mut T) + Send;

    /// Remove a value from storage.
    async fn remove(&self, key: &[&str]) -> StorageResult<()>;

    /// List all keys under a prefix.
    ///
    /// Returns the full key paths for each item.
    async fn list(&self, prefix: &[&str]) -> StorageResult<Vec<Vec<String>>>;

    /// Check if a key exists.
    async fn exists(&self, key: &[&str]) -> StorageResult<bool>;
}
