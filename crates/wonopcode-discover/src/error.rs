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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_info_error_display() {
        let error = DiscoverError::ServiceInfo("invalid service name".to_string());
        assert_eq!(
            format!("{}", error),
            "Service info error: invalid service name"
        );
    }

    #[test]
    fn test_no_servers_found_error_display() {
        let error = DiscoverError::NoServersFound;
        assert_eq!(
            format!("{}", error),
            "No servers found on the local network"
        );
    }

    #[test]
    fn test_service_info_error_debug() {
        let error = DiscoverError::ServiceInfo("test error".to_string());
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("ServiceInfo"));
        assert!(debug_str.contains("test error"));
    }

    #[test]
    fn test_no_servers_found_error_debug() {
        let error = DiscoverError::NoServersFound;
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("NoServersFound"));
    }
}
