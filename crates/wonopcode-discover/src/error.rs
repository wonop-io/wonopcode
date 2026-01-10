//! Error types for the discover crate.

use thiserror::Error;

/// Errors that can occur during mDNS operations.
#[derive(Debug, Error)]
pub enum DiscoverError {
    /// Error from the zeroconf library.
    #[error("mDNS error: {0}")]
    Zeroconf(#[from] zeroconf::error::Error),

    /// Error creating service info.
    #[error("Service info error: {0}")]
    ServiceInfo(String),

    /// No servers found.
    #[error("No servers found on the local network")]
    NoServersFound,
}
