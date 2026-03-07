//! Error types for the Hierarchical Memory System.

use thiserror::Error;

/// Errors that can occur in HMS operations.
#[derive(Debug, Error)]
pub enum HmsError {
    /// Failed to parse memory.yaml file.
    #[error("Failed to parse memory.yaml: {0}")]
    ParseError(String),

    /// Failed to serialize memory data to YAML.
    #[error("Failed to serialize memory.yaml: {0}")]
    SerializeError(String),

    /// IO error during file operations.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Template rendering error.
    #[error("Template error: {0}")]
    TemplateError(String),

    /// Requested key was not found.
    #[error("Key not found: {0}")]
    KeyNotFound(String),

    /// Invalid path provided.
    #[error("Invalid path: {0}")]
    InvalidPath(String),

    /// Path is outside project root.
    #[error("Path is outside project root: {0}")]
    PathOutsideProject(String),

    /// Invalid visibility value.
    #[error("Invalid visibility: {0}")]
    InvalidVisibility(String),

    /// Invalid storage class value.
    #[error("Invalid storage class: {0}")]
    InvalidStorageClass(String),

    /// No template found in directory hierarchy.
    #[error("No AGENTS.TEMPLATE.md found in directory hierarchy")]
    NoTemplateFound,
}

impl From<serde_yaml::Error> for HmsError {
    fn from(err: serde_yaml::Error) -> Self {
        HmsError::ParseError(err.to_string())
    }
}

impl From<tera::Error> for HmsError {
    fn from(err: tera::Error) -> Self {
        HmsError::TemplateError(err.to_string())
    }
}
