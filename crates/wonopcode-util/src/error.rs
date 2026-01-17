//! Error handling utilities.
//!
//! This module provides a consistent error handling pattern across wonopcode.

use std::fmt;

/// A type alias for `Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;

/// The main error type for wonopcode utilities.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    message: String,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Invalid input or argument
    InvalidInput,
    /// IO operation failed
    Io,
    /// Serialization/deserialization failed
    Serialization,
    /// Configuration error
    Config,
    /// Not found
    NotFound,
    /// Permission denied
    PermissionDenied,
    /// Internal error
    Internal,
}

impl Error {
    /// Create a new error with the given kind and message.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            source: None,
        }
    }

    /// Create a new error with a source error.
    pub fn with_source<E>(kind: ErrorKind, message: impl Into<String>, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self {
            kind,
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Get the error kind.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Create an invalid input error.
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidInput, message)
    }

    /// Create an IO error.
    pub fn io(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Io, message)
    }

    /// Create a not found error.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message)
    }

    /// Create a permission denied error.
    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::PermissionDenied, message)
    }

    /// Create an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|e| e.as_ref() as _)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::with_source(ErrorKind::Io, err.to_string(), err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::with_source(ErrorKind::Serialization, err.to_string(), err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as StdError;

    #[test]
    fn test_error_display() {
        let err = Error::invalid_input("test error");
        assert_eq!(err.to_string(), "test error");
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn test_error_with_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = Error::with_source(ErrorKind::Io, "failed to read file", io_err);
        assert!(StdError::source(&err).is_some());
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();
        assert_eq!(err.kind(), ErrorKind::Io);
    }

    #[test]
    fn test_error_io() {
        let err = Error::io("disk full");
        assert_eq!(err.kind(), ErrorKind::Io);
        assert_eq!(err.to_string(), "disk full");
    }

    #[test]
    fn test_error_not_found() {
        let err = Error::not_found("file not found");
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn test_error_permission_denied() {
        let err = Error::permission_denied("access denied");
        assert_eq!(err.kind(), ErrorKind::PermissionDenied);
    }

    #[test]
    fn test_error_internal() {
        let err = Error::internal("unexpected state");
        assert_eq!(err.kind(), ErrorKind::Internal);
    }

    #[test]
    fn test_error_from_serde_json() {
        let json_err = serde_json::from_str::<String>("invalid").unwrap_err();
        let err: Error = json_err.into();
        assert_eq!(err.kind(), ErrorKind::Serialization);
    }

    #[test]
    fn test_error_without_source() {
        let err = Error::new(ErrorKind::Config, "invalid config");
        assert!(StdError::source(&err).is_none());
        assert_eq!(err.kind(), ErrorKind::Config);
    }

    #[test]
    fn test_error_kind_equality() {
        assert_eq!(ErrorKind::InvalidInput, ErrorKind::InvalidInput);
        assert_ne!(ErrorKind::InvalidInput, ErrorKind::Io);
    }
}
