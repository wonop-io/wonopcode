//! Provider error types.

use thiserror::Error;

/// Result type for provider operations.
pub type ProviderResult<T> = Result<T, ProviderError>;

/// Errors that can occur during provider operations.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// HTTP request failed.
    #[error("Request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    /// Invalid API response.
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// Model not found.
    #[error("Model not found: {provider}/{model}")]
    ModelNotFound { provider: String, model: String },

    /// Missing API key.
    #[error("Missing API key for provider: {0}")]
    MissingApiKey(String),

    /// Invalid API key.
    #[error("Invalid API key for provider: {0}")]
    InvalidApiKey(String),

    /// Rate limited.
    #[error("Rate limited, retry after {retry_after:?}")]
    RateLimited {
        retry_after: Option<std::time::Duration>,
    },

    /// Context length exceeded.
    #[error("Context length exceeded: {used} > {limit}")]
    ContextLengthExceeded { used: u32, limit: u32 },

    /// Content filtered/blocked.
    #[error("Content filtered: {reason}")]
    ContentFiltered { reason: String },

    /// Stream interrupted.
    #[error("Stream interrupted")]
    StreamInterrupted,

    /// Operation cancelled.
    #[error("Operation cancelled")]
    Cancelled,

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// URL parsing error.
    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    /// IO error (for streaming).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Internal provider error.
    #[error("Provider error: {message}")]
    Internal { message: String },

    /// API error with status code.
    #[error("API error ({status}): {message}")]
    ApiError { status: u16, message: String },
}

impl ProviderError {
    /// Create a model not found error.
    pub fn model_not_found(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self::ModelNotFound {
            provider: provider.into(),
            model: model.into(),
        }
    }

    /// Create a missing API key error.
    pub fn missing_api_key(provider: impl Into<String>) -> Self {
        Self::MissingApiKey(provider.into())
    }

    /// Create an invalid API key error.
    pub fn invalid_api_key(provider: impl Into<String>) -> Self {
        Self::InvalidApiKey(provider.into())
    }

    /// Create an invalid response error.
    pub fn invalid_response(message: impl Into<String>) -> Self {
        Self::InvalidResponse(message.into())
    }

    /// Create an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Create an API error.
    pub fn api_error(status: u16, message: impl Into<String>) -> Self {
        Self::ApiError {
            status,
            message: message.into(),
        }
    }

    /// Check if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ProviderError::RequestFailed(_)
                | ProviderError::RateLimited { .. }
                | ProviderError::StreamInterrupted
        )
    }
}
