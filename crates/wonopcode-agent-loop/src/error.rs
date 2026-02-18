//! Error types for agent loop execution.

use thiserror::Error;
use wonopcode_provider::ProviderError;

/// Errors that can occur during agentic loop execution.
#[derive(Debug, Error)]
pub enum LoopError {
    /// Error from the LLM provider.
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    /// Tool execution failed.
    #[error("Tool execution failed: {0}")]
    ToolExecution(String),

    /// Permission was denied for an operation.
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Doom loop detected (repetitive tool patterns).
    #[error("Doom loop detected: {0}")]
    DoomLoop(String),

    /// Operation was cancelled by user.
    #[error("Cancelled")]
    Cancelled,

    /// Context limit was exceeded.
    #[error("Context limit exceeded")]
    ContextLimitExceeded,

    /// Context overflow detected - compaction is needed.
    /// This is a recoverable error that signals the runner should compact
    /// and retry the operation.
    #[error("Context overflow: compaction needed")]
    ContextOverflow,

    /// Maximum iterations reached.
    #[error("Maximum iterations reached: {0}")]
    MaxIterations(u32),

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl LoopError {
    /// Check if this error is recoverable.
    ///
    /// Some errors (like cancellation) are expected and don't indicate
    /// a problem with the loop implementation.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::ContextLimitExceeded | Self::ContextOverflow
        )
    }

    /// Check if this error indicates the loop should stop.
    pub fn should_stop(&self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::MaxIterations(_) | Self::DoomLoop(_)
        )
    }

    /// Check if this error indicates compaction is needed and retry is possible.
    pub fn needs_compaction(&self) -> bool {
        matches!(self, Self::ContextOverflow)
    }

    /// Create a tool execution error.
    pub fn tool_error(msg: impl Into<String>) -> Self {
        Self::ToolExecution(msg.into())
    }

    /// Create an internal error.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// Create from a provider error, detecting context overflow.
    pub fn from_provider_error(err: ProviderError) -> Self {
        if err.is_context_overflow() {
            Self::ContextOverflow
        } else {
            Self::Provider(err)
        }
    }
}

/// Result type for loop operations.
pub type LoopResult<T> = Result<T, LoopError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_is_recoverable() {
        assert!(LoopError::Cancelled.is_recoverable());
        assert!(LoopError::ContextLimitExceeded.is_recoverable());
        assert!(LoopError::ContextOverflow.is_recoverable());
        assert!(!LoopError::DoomLoop("test".into()).is_recoverable());
        assert!(!LoopError::ToolExecution("test".into()).is_recoverable());
    }

    #[test]
    fn test_error_needs_compaction() {
        assert!(LoopError::ContextOverflow.needs_compaction());
        assert!(!LoopError::Cancelled.needs_compaction());
        assert!(!LoopError::ContextLimitExceeded.needs_compaction());
    }

    #[test]
    fn test_error_should_stop() {
        assert!(LoopError::Cancelled.should_stop());
        assert!(LoopError::MaxIterations(50).should_stop());
        assert!(LoopError::DoomLoop("test".into()).should_stop());
        assert!(!LoopError::ToolExecution("test".into()).should_stop());
    }

    #[test]
    fn test_error_display() {
        let err = LoopError::tool_error("read failed");
        assert_eq!(err.to_string(), "Tool execution failed: read failed");

        let err = LoopError::internal("something broke");
        assert_eq!(err.to_string(), "Internal error: something broke");
    }
}
