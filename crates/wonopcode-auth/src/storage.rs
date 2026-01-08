//! Authentication storage implementation.

use crate::error::{AuthError, AuthResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Authentication information for a provider.
///
/// This enum represents the different ways a user can authenticate
/// with a provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AuthInfo {
    /// API key authentication.
    ///
    /// Simple key-based authentication for providers that support it.
    Api {
        /// The API key.
        key: String,
    },

    /// CLI-based authentication.
    ///
    /// Indicates that authentication should be delegated to an external CLI tool
    /// (e.g., Claude Code CLI for Claude Max/Pro subscriptions).
    /// The CLI handles OAuth tokens internally.
    Cli,
}

impl AuthInfo {
    /// Create a new API key auth info.
    pub fn api_key(key: String) -> Self {
        Self::Api { key }
    }

    /// Create a CLI-based auth marker.
    pub fn cli() -> Self {
        Self::Cli
    }

    /// Check if this is API key authentication.
    pub fn is_api_key(&self) -> bool {
        matches!(self, Self::Api { .. })
    }

    /// Check if this is CLI-based authentication.
    pub fn is_cli(&self) -> bool {
        matches!(self, Self::Cli)
    }

    /// Get the API key if this is API key auth.
    pub fn as_api_key(&self) -> Option<&str> {
        match self {
            Self::Api { key } => Some(key),
            Self::Cli => None,
        }
    }
}

/// Secure storage for authentication credentials.
///
/// Provides thread-safe access to stored credentials with automatic
/// file permission management on Unix systems.
pub struct AuthStorage {
    /// Path to the auth file.
    path: PathBuf,
    /// In-memory cache of auth data.
    cache: RwLock<Option<HashMap<String, AuthInfo>>>,
}

impl AuthStorage {
    /// Create a new auth storage using the default path.
    ///
    /// # Errors
    ///
    /// Returns an error if the data directory cannot be determined.
    pub fn new() -> AuthResult<Self> {
        let path = crate::default_auth_path().ok_or(AuthError::NoDataDir)?;
        Ok(Self {
            path,
            cache: RwLock::new(None),
        })
    }

    /// Create auth storage with a custom path.
    ///
    /// Useful for testing or custom configurations.
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            path,
            cache: RwLock::new(None),
        }
    }

    /// Get the path to the auth file.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Get authentication info for a provider.
    ///
    /// Returns `None` if no auth is stored for the provider.
    pub async fn get(&self, provider: &str) -> AuthResult<Option<AuthInfo>> {
        let all = self.all().await?;
        Ok(all.get(provider).cloned())
    }

    /// Set authentication info for a provider.
    ///
    /// This will create the auth file if it doesn't exist.
    pub async fn set(&self, provider: &str, info: AuthInfo) -> AuthResult<()> {
        debug!(provider = %provider, auth_type = ?info.is_cli(), "Setting auth");

        let mut all = self.all().await?;
        all.insert(provider.to_string(), info);
        self.write_all(&all).await?;

        // Invalidate cache
        *self.cache.write().await = None;

        Ok(())
    }

    /// Remove authentication for a provider.
    ///
    /// Returns `true` if auth was removed, `false` if it didn't exist.
    pub async fn remove(&self, provider: &str) -> AuthResult<bool> {
        debug!(provider = %provider, "Removing auth");

        let mut all = self.all().await?;
        let existed = all.remove(provider).is_some();

        if existed {
            self.write_all(&all).await?;
            // Invalidate cache
            *self.cache.write().await = None;
        }

        Ok(existed)
    }

    /// Get all stored authentication info.
    pub async fn all(&self) -> AuthResult<HashMap<String, AuthInfo>> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(data) = &*cache {
                return Ok(data.clone());
            }
        }

        // Read from file
        let data = self.read_all().await?;

        // Update cache
        *self.cache.write().await = Some(data.clone());

        Ok(data)
    }

    /// List all providers with stored auth.
    pub async fn list_providers(&self) -> AuthResult<Vec<String>> {
        let all = self.all().await?;
        Ok(all.keys().cloned().collect())
    }

    /// Check if a provider has stored auth.
    pub async fn has(&self, provider: &str) -> AuthResult<bool> {
        let all = self.all().await?;
        Ok(all.contains_key(provider))
    }

    /// Clear all stored authentication.
    pub async fn clear(&self) -> AuthResult<()> {
        debug!("Clearing all auth");
        self.write_all(&HashMap::new()).await?;
        *self.cache.write().await = None;
        Ok(())
    }

    /// Read all auth data from file.
    async fn read_all(&self) -> AuthResult<HashMap<String, AuthInfo>> {
        if !self.path.exists() {
            return Ok(HashMap::new());
        }

        let content = tokio::fs::read_to_string(&self.path).await?;

        if content.trim().is_empty() {
            return Ok(HashMap::new());
        }

        // Parse as raw JSON first, then validate each entry
        let raw: HashMap<String, serde_json::Value> = serde_json::from_str(&content)?;
        let mut result = HashMap::new();

        for (key, value) in raw {
            match serde_json::from_value::<AuthInfo>(value) {
                Ok(info) => {
                    result.insert(key, info);
                }
                Err(e) => {
                    warn!(provider = %key, error = %e, "Skipping invalid auth entry");
                }
            }
        }

        Ok(result)
    }

    /// Write all auth data to file.
    async fn write_all(&self, data: &HashMap<String, AuthInfo>) -> AuthResult<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Serialize with pretty printing
        let content = serde_json::to_string_pretty(data)?;

        // Write to file
        tokio::fs::write(&self.path, &content).await?;

        // Set restrictive permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            tokio::fs::set_permissions(&self.path, perms)
                .await
                .map_err(|e| {
                    AuthError::Permissions(format!(
                        "Failed to set permissions on {:?}: {}",
                        self.path, e
                    ))
                })?;
        }

        debug!(path = ?self.path, "Wrote auth file");
        Ok(())
    }
}

impl std::fmt::Debug for AuthStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthStorage")
            .field("path", &self.path)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn test_storage() -> (AuthStorage, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("auth.json");
        let storage = AuthStorage::with_path(path);
        (storage, dir)
    }

    #[tokio::test]
    async fn test_set_and_get_api_key() {
        let (storage, _dir) = test_storage().await;

        let auth = AuthInfo::api_key("sk-test-key".to_string());
        storage.set("anthropic", auth.clone()).await.unwrap();

        let retrieved = storage.get("anthropic").await.unwrap();
        assert_eq!(retrieved, Some(auth));
    }

    #[tokio::test]
    async fn test_set_and_get_cli() {
        let (storage, _dir) = test_storage().await;

        let auth = AuthInfo::cli();
        storage.set("anthropic", auth.clone()).await.unwrap();

        let retrieved = storage.get("anthropic").await.unwrap();
        assert_eq!(retrieved, Some(auth));
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let (storage, _dir) = test_storage().await;

        let retrieved = storage.get("nonexistent").await.unwrap();
        assert_eq!(retrieved, None);
    }

    #[tokio::test]
    async fn test_remove() {
        let (storage, _dir) = test_storage().await;

        let auth = AuthInfo::api_key("sk-test-key".to_string());
        storage.set("anthropic", auth).await.unwrap();

        let removed = storage.remove("anthropic").await.unwrap();
        assert!(removed);

        let retrieved = storage.get("anthropic").await.unwrap();
        assert_eq!(retrieved, None);
    }

    #[tokio::test]
    async fn test_remove_nonexistent() {
        let (storage, _dir) = test_storage().await;

        let removed = storage.remove("nonexistent").await.unwrap();
        assert!(!removed);
    }

    #[tokio::test]
    async fn test_all() {
        let (storage, _dir) = test_storage().await;

        storage
            .set("anthropic", AuthInfo::api_key("key1".to_string()))
            .await
            .unwrap();
        storage
            .set("openai", AuthInfo::api_key("key2".to_string()))
            .await
            .unwrap();

        let all = storage.all().await.unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.contains_key("anthropic"));
        assert!(all.contains_key("openai"));
    }

    #[tokio::test]
    async fn test_list_providers() {
        let (storage, _dir) = test_storage().await;

        storage
            .set("anthropic", AuthInfo::api_key("key1".to_string()))
            .await
            .unwrap();
        storage
            .set("openai", AuthInfo::api_key("key2".to_string()))
            .await
            .unwrap();

        let mut providers = storage.list_providers().await.unwrap();
        providers.sort();
        assert_eq!(providers, vec!["anthropic", "openai"]);
    }

    #[tokio::test]
    async fn test_has() {
        let (storage, _dir) = test_storage().await;

        storage
            .set("anthropic", AuthInfo::api_key("key".to_string()))
            .await
            .unwrap();

        assert!(storage.has("anthropic").await.unwrap());
        assert!(!storage.has("openai").await.unwrap());
    }

    #[tokio::test]
    async fn test_clear() {
        let (storage, _dir) = test_storage().await;

        storage
            .set("anthropic", AuthInfo::api_key("key1".to_string()))
            .await
            .unwrap();
        storage
            .set("openai", AuthInfo::api_key("key2".to_string()))
            .await
            .unwrap();

        storage.clear().await.unwrap();

        let all = storage.all().await.unwrap();
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn test_auth_type_checks() {
        let cli = AuthInfo::cli();
        assert!(cli.is_cli());
        assert!(!cli.is_api_key());
        assert!(cli.as_api_key().is_none());

        let api = AuthInfo::api_key("key".to_string());
        assert!(!api.is_cli());
        assert!(api.is_api_key());
        assert_eq!(api.as_api_key(), Some("key"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let (storage, _dir) = test_storage().await;

        storage
            .set("test", AuthInfo::api_key("key".to_string()))
            .await
            .unwrap();

        let metadata = std::fs::metadata(storage.path()).unwrap();
        let mode = metadata.permissions().mode();

        // Check that only owner has read/write (0600)
        assert_eq!(mode & 0o777, 0o600);
    }

    #[tokio::test]
    async fn test_persistence() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("auth.json");

        // Write with one instance
        {
            let storage = AuthStorage::with_path(path.clone());
            storage
                .set("anthropic", AuthInfo::api_key("key".to_string()))
                .await
                .unwrap();
        }

        // Read with new instance
        {
            let storage = AuthStorage::with_path(path);
            let auth = storage.get("anthropic").await.unwrap();
            assert_eq!(auth, Some(AuthInfo::api_key("key".to_string())));
        }
    }

    #[tokio::test]
    async fn test_invalid_json_entry_skipped() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("auth.json");

        // Write invalid JSON directly
        tokio::fs::write(
            &path,
            r#"{
                "valid": {"type": "api", "key": "sk-valid"},
                "invalid": {"type": "unknown", "foo": "bar"}
            }"#,
        )
        .await
        .unwrap();

        let storage = AuthStorage::with_path(path);
        let all = storage.all().await.unwrap();

        // Valid entry should be loaded
        assert!(all.contains_key("valid"));
        // Invalid entry should be skipped
        assert!(!all.contains_key("invalid"));
    }
}
