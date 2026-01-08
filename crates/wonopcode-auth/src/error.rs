//! Error types for authentication operations.

use thiserror::Error;

/// Errors that can occur during authentication operations.
#[derive(Debug, Error)]
pub enum AuthError {
    /// Failed to read or write auth file.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to serialize or deserialize auth data.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// No authentication found for the specified provider.
    #[error("Not authenticated with provider '{0}'")]
    NotAuthenticated(String),

    /// The stored authentication is invalid or corrupted.
    #[error("Invalid authentication data for provider '{0}'")]
    InvalidAuth(String),

    /// Could not determine the data directory.
    #[error("Could not determine data directory")]
    NoDataDir,

    /// Failed to set file permissions.
    #[error("Failed to set file permissions: {0}")]
    Permissions(String),
}

/// Result type for auth operations.
pub type AuthResult<T> = Result<T, AuthError>;
