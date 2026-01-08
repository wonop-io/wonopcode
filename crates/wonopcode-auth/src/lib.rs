//! Authentication storage and management for wonopcode.
//!
//! This crate provides secure storage for authentication credentials,
//! supporting API keys and CLI-based authentication markers.
//!
//! # Authentication Types
//!
//! - **API Key**: Direct API access using a provider's API key
//! - **CLI**: Marker indicating authentication is handled by an external CLI
//!   (e.g., Claude Code CLI for Claude Max/Pro subscriptions)
//!
//! # Storage Location
//!
//! Credentials are stored in a platform-specific data directory:
//! - Linux: `~/.local/share/wonopcode/auth.json`
//! - macOS: `~/Library/Application Support/wonopcode/auth.json`
//! - Windows: `%APPDATA%/wonopcode/auth.json`
//!
//! The file is created with restrictive permissions (0600 on Unix).
//!
//! # Example
//!
//! ```no_run
//! use wonopcode_auth::{AuthStorage, AuthInfo};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let storage = AuthStorage::new()?;
//!     
//!     // Store an API key
//!     storage.set("anthropic", AuthInfo::api_key("sk-ant-...".to_string())).await?;
//!     
//!     // Or mark that CLI auth should be used
//!     storage.set("anthropic", AuthInfo::cli()).await?;
//!     
//!     // Retrieve it later
//!     if let Some(auth) = storage.get("anthropic").await? {
//!         match auth {
//!             AuthInfo::Api { key } => println!("Using API key"),
//!             AuthInfo::Cli => println!("Using CLI authentication"),
//!         }
//!     }
//!     
//!     Ok(())
//! }
//! ```

mod error;
mod storage;

pub use error::{AuthError, AuthResult};
pub use storage::{AuthInfo, AuthStorage};

/// Get the default auth file path for the current platform.
///
/// Returns `None` if the data directory cannot be determined.
pub fn default_auth_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|p| p.join("wonopcode").join("auth.json"))
}

/// Get the current time in milliseconds since Unix epoch.
pub fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
